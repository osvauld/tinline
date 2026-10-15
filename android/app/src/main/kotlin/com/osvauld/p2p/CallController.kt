package com.osvauld.p2p

import android.Manifest
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.ToneGenerator
import android.os.Build
import android.os.PowerManager
import android.os.VibrationAttributes
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.telecom.DisconnectCause
import android.util.Log
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.flow.updateAndGet
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import uniffi.p2pcore.CallInfo
import uniffi.p2pcore.CallState
import uniffi.p2pcore.CallStats

data class CallUi(
    val info: CallInfo,
    val state: CallState,
    val muted: Boolean = false,
    /** Where the audio plays and what else is available (from Telecom, or the in-app fallback). */
    val routes: RouteState = RouteState(),
    /** On hold because another call (e.g. a phone call) took over; both directions are silent. */
    val held: Boolean = false,
    val stats: CallStats? = null,
    /** Loss over the last few seconds ([RecentLoss]), for the signal bars. */
    val lossPct: Double? = null,
    val activeSinceMs: Long? = null,
    /** Shown on the call screen when the mic cannot be used (denied permission or init failure). */
    val micProblem: String? = null,
) {
    val speaker: Boolean get() = routes.speaker
}

/** What the "Call ended" / "Couldn't reach" screens show once the call is gone. */
data class EndedUi(
    val peerName: String, val peerDid: String, val reason: String, val incoming: Boolean,
    val wasActive: Boolean, val secs: Long, val direct: Boolean?, val bars: Int?,
    val atMs: Long = System.currentTimeMillis(),
)

/** Thrown by [CallController.place] when the microphone permission is missing. */
class MicPermissionNeeded : Exception("Microphone permission needed to place a call")

/**
 * Single place that reacts to call events: UI state, ringing, notifications, audio, service type.
 * Each call is also registered with Android's Telecom through a [CallSession], which owns routing.
 */
class CallController(private val app: P2pApp) {
    private val _ui = MutableStateFlow<CallUi?>(null)
    val ui: StateFlow<CallUi?> = _ui
    private val _ended = MutableStateFlow<EndedUi?>(null)
    /** Set when a call ends (after [ui] goes null); cleared when the next call starts. */
    val ended: StateFlow<EndedUi?> = _ended
    private val audio by lazy { AudioEngine(app, { app.node }, ::micUnavailable) }
    @Volatile private var ringtone: CallSounds.Ring? = null
    @Volatile private var statsJob: Job? = null
    @Volatile private var ringTimeout: Job? = null
    @Volatile private var everActive = false

    // A second call that arrived during an active one: it rings quietly over the call screen.
    private val _waiting = MutableStateFlow<CallInfo?>(null)
    val waiting: StateFlow<CallInfo?> = _waiting
    @Volatile private var waitingTone: Job? = null
    /** Set by [endAndAnswer]: the call that takes over when the current one ends. */
    @Volatile private var takeover: CallInfo? = null
    /** Whether the call screen is on top; if not, the waiting call also gets a notification. */
    @Volatile var callScreenShown = false

    /** Debug screen gallery only: a made-up call to draw the strip with (never called by the app). */
    fun showForGallery(u: CallUi?) { _ui.value = u }
    private var proximity: PowerManager.WakeLock? = null

    /** The current call's Telecom registration and audio; replaced per call. */
    @Volatile private var session: CallSession? = null
    @Volatile private var sessionWatch: Job? = null
    /** Set when this side ends the call (a [DisconnectCause] code), so Telecom gets the right cause. */
    @Volatile private var localEnd: Int? = null

    // Events for a call we are still placing arrive before node.call() has returned its id and
    // before _ui is set; they are held here and replayed (the desktop keeps the same `early` list).
    private val lock = Any()
    private val early = mutableListOf<Pair<String, CallState>>()
    @Volatile private var placing = false

    private fun micUnavailable() {
        val msg = if (Perms.granted(app, Manifest.permission.RECORD_AUDIO)) "Microphone unavailable - you can't be heard"
        else "Microphone permission needed - you can't be heard"
        _ui.update { it?.copy(micProblem = msg) }
    }

    fun onIncoming(call: CallInfo) {
        if (_waiting.value?.callId == call.callId) clearWaiting()  // it was promoted: an ordinary ring now
        val cur = _ui.value
        if (cur != null && cur.state is CallState.Active && cur.info.callId != call.callId) {
            testLog("waiting id=${call.callId} from=${call.peerName.ifBlank { call.peerDid }}")
            _waiting.value = call
            startWaitingTone()
            if (!callScreenShown) Notifications.waiting(app, call)
            return
        }
        testLog("incoming id=${call.callId} from=${call.peerName.ifBlank { call.peerDid }}")
        everActive = false; _ended.value = null
        app.video.reset()
        _ui.value = CallUi(call, CallState.Ringing)
        newSession(call)
        CoreService.ensureRunning(app)
        // The app is open and unlocked: the home header (RingHead) is the incoming call, so the system heads-up stays away.
        // It is posted the moment the app leaves the front while the phone is still ringing ([appLeftFront]).
        if (!(AppVisible.resumed && app.lockState.value == uniffi.p2pcore.LockState.UNLOCKED)) Notifications.incoming(app, call)
        startRinging()
        ringTimeout?.cancel()
        ringTimeout = app.scope.launch {
            // The core ends an unanswered call itself after 60 s (no_answer); this is only the backstop.
            delay(RING_TIMEOUT_MS + 5_000)
            val c = _ui.value
            if (c != null && c.info.callId == call.callId && c.state !is CallState.Active) {
                // Nobody answered: stop ringing for good. Ended then posts the "Missed call" note.
                stopRinging(); Notifications.cancelIncoming(app)
                try { app.node.decline(call.callId) } catch (e: Exception) { Log.w("Call", "ring timeout decline: $e") }
            }
        }
    }

    fun onState(callId: String, state: CallState) {
        testLog("state id=$callId state=${describe(state)}")
        val w = _waiting.value
        if (w != null && w.callId == callId) {
            if (state is CallState.Ended) {
                // Unanswered (or the caller gave up): a missed call like any other.
                clearWaiting()
                if (isMissedReason(state.reason, true)) Notifications.missed(app, w.peerName)
            }
            return
        }
        synchronized(lock) {
            val c = _ui.value
            if (c == null || c.info.callId != callId) {
                if (placing) early.add(callId to state)
                return
            }
        }
        when (state) {
            is CallState.Active -> {
                everActive = true
                ringTimeout?.cancel()
                stopRinging(); Notifications.cancelIncoming(app)
                val now = System.currentTimeMillis()
                val cur = _ui.updateAndGet { u -> if (u?.info?.callId == callId) u.copy(state = state, activeSinceMs = now) else u }
                    ?: return
                inCallService(cur.info)
                audio.muted = cur.muted
                session?.connected() ?: audio.start()
                updateProximity(cur.routes.earpiece)
                startStats()
            }
            is CallState.Ended -> {
                ringTimeout?.cancel()
                stopRinging(); Notifications.cancelIncoming(app)
                statsJob?.cancel(); updateProximity(false)
                app.video.reset()
                val cur = _ui.value
                endSession(cur?.takeIf { it.info.callId == callId }, state.reason)
                if (cur != null && cur.info.callId == callId) {
                    val reason = state.reason
                    if (!everActive && isMissedReason(reason, cur.info.incoming)) Notifications.missed(app, cur.info.peerName)
                    val s = cur.stats
                    val next = takeover; takeover = null
                    val loss = cur.lossPct
                    // Our outgoing call yielded to their simultaneous call: no ended screen, theirs rings.
                    if (next == null && classifyEnd(reason) != EndKind.Superseded) _ended.value = EndedUi(
                        cur.info.peerName, cur.info.peerDid, reason, cur.info.incoming, everActive,
                        cur.activeSinceMs?.let { (System.currentTimeMillis() - it) / 1000 } ?: 0L,
                        s?.direct, if (s != null && loss != null) qualityBars(s.rttMs.toInt(), loss) else null,
                    )
                    // End & answer: straight on to the new call, no "call ended" screen in between.
                    _ui.update { u -> if (u?.info?.callId == callId) next?.let { CallUi(it, CallState.Ringing) } else u }
                    if (next != null) newSession(next)
                }
                CoreService.ensureRunning(app, CoreService.ACTION_IDLE)
            }
            else -> _ui.update { u -> if (u?.info?.callId == callId) u.copy(state = state) else u }
        }
    }

    private fun inCallService(info: CallInfo) =
        CoreService.ensureRunning(app, CoreService.ACTION_IN_CALL, info.peerName.ifBlank { "Call" })

    fun place(did: String): Result<CallInfo> = runCatching {
        if (!Perms.granted(app, Manifest.permission.RECORD_AUDIO)) throw MicPermissionNeeded()
        app.video.reset()
        synchronized(lock) { early.clear(); placing = true }
        val info = try { app.node.call(did) } catch (e: Throwable) {
            synchronized(lock) { placing = false; early.clear() }
            throw e
        }
        everActive = false; _ended.value = null
        val replay = synchronized(lock) {
            _ui.value = CallUi(info, CallState.Dialing)
            newSession(info)
            placing = false
            early.filter { it.first == info.callId }.also { early.clear() }
        }
        replay.forEach { (id, st) -> onState(id, st) }
        val i = Intent(app, CallActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try { app.startActivity(i) } catch (e: Exception) { Log.w("Call", "cannot open call screen: $e") }
        info
    }

    fun answer() {
        val c = _ui.value ?: return
        ringTimeout?.cancel()
        stopRinging(); Notifications.cancelIncoming(app)
        // Raise the service to microphone type while still allowed (user-initiated).
        inCallService(c.info)
        session?.answering()
        app.scope.launch { try { app.node.answer(c.info.callId) } catch (e: Exception) { Log.w("Call", "answer: $e") } }
    }

    fun decline() {
        val c = _ui.value ?: return
        ringTimeout?.cancel()
        stopRinging(); Notifications.cancelIncoming(app)
        localEnd = DisconnectCause.REJECTED
        app.scope.launch { try { app.node.decline(c.info.callId) } catch (e: Exception) { Log.w("Call", "decline: $e") } }
    }

    /** Banner action: refuse the second call; its caller hears "busy" and the current call goes on. */
    fun declineWaiting() {
        val w = _waiting.value ?: return
        clearWaiting()
        app.scope.launch { try { app.node.decline(w.callId) } catch (e: Exception) { Log.w("Call", "decline waiting: $e") } }
    }

    /** Banner action: hang up the current call and answer the second one. */
    fun endAndAnswer() {
        val w = _waiting.value ?: return
        clearWaiting()
        takeover = w
        localEnd = DisconnectCause.LOCAL
        inCallService(w)
        everActive = false
        app.scope.launch {
            try { app.node.endAndAnswer(w.callId) } catch (e: Exception) { takeover = null; Log.w("Call", "end and answer: $e") }
        }
    }

    private fun clearWaiting() {
        waitingTone?.cancel(); waitingTone = null
        Notifications.cancelWaiting(app)
        _waiting.value = null
    }

    /** The quiet in-call "call waiting" beep, on the voice-call stream so it does not disturb the call audio. */
    private fun startWaitingTone() {
        waitingTone?.cancel()
        if (AlertPrefs.quiet(app)) return   // Settings › Sounds › Quiet
        waitingTone = app.scope.launch {
            val tg = try { ToneGenerator(AudioManager.STREAM_VOICE_CALL, 35) } catch (e: Exception) { Log.w("Call", "tone: $e"); null }
            try {
                while (isActive) {
                    tg?.startTone(ToneGenerator.TONE_SUP_CALL_WAITING, 400)
                    delay(4_000)
                }
            } finally { tg?.release() }
        }
    }

    fun hangup() {
        val c = _ui.value ?: return
        localEnd = DisconnectCause.LOCAL
        app.scope.launch { try { app.node.hangup(c.info.callId) } catch (e: Exception) { Log.w("Call", "hangup: $e") } }
    }

    fun toggleMute() {
        val n = _ui.updateAndGet { it?.copy(muted = !it.muted) } ?: return
        audio.muted = n.muted
    }

    /** The system microphone mute changed (a car or headset mute button): follow it. */
    private fun systemMute(callId: String, m: Boolean) {
        val n = _ui.updateAndGet { if (it?.info?.callId == callId) it.copy(muted = m) else it } ?: return
        if (n.info.callId == callId) audio.muted = m
    }

    /** Answer/hang-up from outside the app (headset button, car, watch). */
    private fun systemAnswer(callId: String) {
        val c = _ui.value ?: return
        if (c.info.callId == callId && c.info.incoming && c.state is CallState.Ringing) answer()
    }

    private fun systemDisconnect(callId: String) {
        val c = _ui.value ?: return
        if (c.info.callId != callId) return
        if (c.info.incoming && c.state is CallState.Ringing) decline() else hangup()
    }

    /** Registers [info] with Telecom; the routes and hold state it reports feed [ui]. */
    private fun newSession(info: CallInfo) {
        session?.end(DisconnectCause.LOCAL)
        sessionWatch?.cancel()
        localEnd = null
        val id = info.callId
        val s = CallSession(
            app.scope, app, audio, info.peerName.ifBlank { "Tinline call" }, info.incoming, preferSpeaker = false,
            onAnswer = { systemAnswer(id) }, onDisconnect = { systemDisconnect(id) }, onSystemMute = { systemMute(id, it) },
        )
        session = s
        sessionWatch = app.scope.launch {
            launch { s.held.collect { h -> _ui.update { if (it?.info?.callId == id) it.copy(held = h) else it } } }
            s.routes.collect { st ->
                val n = _ui.updateAndGet { if (it?.info?.callId == id) it.copy(routes = st) else it }
                if (n?.info?.callId == id && n.state is CallState.Active) updateProximity(st.earpiece)
            }
        }
    }

    private fun endSession(cur: CallUi?, reason: String) {
        val s = session ?: run { audio.stop(); return }
        session = null
        sessionWatch?.cancel(); sessionWatch = null
        val cause = localEnd ?: when {
            cur != null && !everActive && cur.info.incoming && isMissedReason(reason, true) -> DisconnectCause.MISSED
            else -> DisconnectCause.REMOTE
        }
        localEnd = null
        s.end(cause)
    }

    /** The call screen's remote-video renderer while it shows video; read by the stats log. */
    @Volatile var renderer: RemoteRenderer? = null

    /** Video calls are held away from the ear: the camera going on moves the earpiece to the speaker (headsets stay). */
    fun speakerForVideo() { session?.speakerIfEarpiece() }

    /** Earpiece/headset <-> loudspeaker. */
    fun toggleSpeaker() { session?.toggleSpeaker() }

    /** Play the call through [route] (one of [CallUi.routes]). */
    fun selectRoute(route: AudioRoute) { session?.select(route) }

    /** Screen off against the ear while a call is active on the earpiece. */
    @Synchronized
    private fun updateProximity(want: Boolean) {
        try {
            if (want) {
                val pm = app.getSystemService(Context.POWER_SERVICE) as PowerManager
                if (proximity == null && pm.isWakeLockLevelSupported(PowerManager.PROXIMITY_SCREEN_OFF_WAKE_LOCK)) {
                    proximity = pm.newWakeLock(PowerManager.PROXIMITY_SCREEN_OFF_WAKE_LOCK, "p2p:proximity")
                }
                proximity?.let { if (!it.isHeld) it.acquire(4 * 3600_000L) }
            } else proximity?.let { if (it.isHeld) it.release() }
        } catch (e: Exception) { Log.w("Call", "proximity: $e") }
    }

    fun statsLine(s: CallStats): String =
        "stats secs=${s.secs} direct=${s.direct} rtt=${s.rttMs} sent=${s.sent} recv=${s.received} lost=${s.lost} " +
            "recovered=${s.recovered} concealed=${s.concealed} buf=${s.bufferedMs} freq=${"%.1f".format(s.rxFreqHz)} rms=${"%.0f".format(s.rxRms)}"

    fun logStats() {
        val s = app.node.callStats()
        testLog(if (s == null) "stats none" else statsLine(s))
    }

    private fun startStats() {
        statsJob?.cancel()
        statsJob = app.scope.launch {
            val recent = RecentLoss()
            while (isActive) {
                delay(1000)
                val s = try { app.node.callStats() } catch (_: Exception) { null } ?: continue
                testLog(statsLine(s))
                if (app.video.info.value?.negotiated == true) testLog(app.video.statsLine(renderer))
                val loss = recent.push(s.received.toLong(), s.lost.toLong())
                _ui.update { it?.copy(stats = s, lossPct = loss) }
            }
        }
    }

    /** MainActivity paused (home, screen off, another app): a call still ringing gets its notification and full-screen intent now. */
    fun appLeftFront() {
        val c = _ui.value ?: return
        if (ringing && c.info.incoming && c.state is CallState.Ringing && !callScreenShown) Notifications.incoming(app, c.info)
    }

    @Volatile private var ringing = false

    private fun startRinging() {
        stopRinging()
        ringing = true
        if (!AlertPrefs.calls(app)) return   // Settings › Notifications › Calls off: no ringing
        if (!AlertPrefs.quiet(app)) ringtone = CallSounds.startRing(app)
        try {
            // Vibrate unless the phone is fully silent; the ringtone itself follows the ring volume.
            val mode = (app.getSystemService(Context.AUDIO_SERVICE) as AudioManager).ringerMode
            if (mode != AudioManager.RINGER_MODE_SILENT) {
                val effect = VibrationEffect.createWaveform(longArrayOf(0, 800, 800), 0)
                if (Build.VERSION.SDK_INT >= 33) {
                    vibrator().vibrate(effect, VibrationAttributes.createForUsage(VibrationAttributes.USAGE_RINGTONE))
                } else {
                    @Suppress("DEPRECATION")
                    vibrator().vibrate(effect, AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE).build())
                }
            }
        } catch (e: Exception) { Log.w("Call", "vibrate: $e") }
    }

    private fun vibrator(): Vibrator =
        if (Build.VERSION.SDK_INT >= 31)
            (app.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as VibratorManager).defaultVibrator
        else @Suppress("DEPRECATION") (app.getSystemService(Context.VIBRATOR_SERVICE) as Vibrator)

    private fun stopRinging() {
        ringing = false
        ringtone?.stop()
        ringtone = null
        try { vibrator().cancel() } catch (_: Exception) {}
    }

    companion object {
        const val RING_TIMEOUT_MS = 60_000L
        fun describe(s: CallState): String = when (s) {
            is CallState.Ended -> "Ended(${s.reason})"
            is CallState.Dialing -> "Dialing"
            is CallState.Ringing -> "Ringing"
            is CallState.Active -> "Active"
        }
    }
}
