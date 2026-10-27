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
import uniffi.p2pcore.VideoInfo

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
    /** I answered (or ended the other call to answer this one): in call from the tap, before Active. */
    val answered: Boolean = false,
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

    /** The current call is a video call now ([Routes.isVideo]): loudspeaker, no proximity wake lock. */
    @Volatile private var videoNow = false

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
        val msg = appString(if (Perms.granted(app, Manifest.permission.RECORD_AUDIO)) R.string.call_mic_unavailable else R.string.call_mic_permission)
        _ui.update { it?.copy(micProblem = msg) }
    }

    fun onIncoming(call: CallInfo) {
        if (_waiting.value?.callId == call.callId) clearWaiting()  // it was promoted: an ordinary ring now
        val cur = _ui.value
        if (cur != null && cur.state is CallState.Active && cur.info.callId != call.callId) {
            testLog("waiting id=${call.callId} from=${call.peerName.ifBlank { call.peerDid }}")
            CallTrace.add("waiting ${shortId(call.callId)}")
            _waiting.value = call
            startWaitingTone()
            if (!callScreenShown) Notifications.waiting(app, call)
            return
        }
        testLog("incoming id=${call.callId} from=${call.peerName.ifBlank { call.peerDid }}")
        CallTrace.add("incoming ${shortId(call.callId)}${if (cur != null) " over ${shortId(cur.info.callId)} ${CallNotif.of(cur)}" else ""}")
        everActive = false; _ended.value = null
        app.video.reset()
        // A call wake already rang for this contact (docs/push.md): this call takes the ring over instead of ringing twice.
        // Answered on the wake screen: in call from now on, the core has accepted it as it arrived.
        val wake = WakeRing.match(call.peerDid)
        _ui.value = CallUi(call, CallState.Ringing, answered = wake.answeredEarly)
        WakeRing.consumed(call.peerDid)
        newSession(call)
        CoreService.ensureRunning(app)
        if (wake.answeredEarly) {
            CallTrace.add("wake ring answered early ${shortId(call.callId)}")
            stopRinging(); Notifications.cancelIncoming(app)
            inCallService()
            session?.answering()
            return
        }
        if (wake.matched && AppVisible.resumed && AppVisible.ringHeadUp) Notifications.cancelIncoming(app)
        // The app is open and unlocked on Home: the home header (RingHead) is the incoming call, so the system heads-up stays
        // away. On any other screen there is no RingHead, so the notification is posted as usual.
        // It is posted the moment the app leaves the front while the phone is still ringing ([appLeftFront]).
        if (!(AppVisible.resumed && AppVisible.ringHeadUp && app.lockState.value == uniffi.p2pcore.LockState.UNLOCKED)) Notifications.incoming(app, call)
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
        CallTrace.add("state ${shortId(callId)} ${describe(state)}")
        val w = _waiting.value
        if (w != null && w.callId == callId) {
            if (state is CallState.Ended) {
                // Unanswered (or the caller gave up): a missed call like any other.
                clearWaiting()
                if (uniffi.present.isMissedReason(state.reason, true, false)) Notifications.missed(app, w.peerName)
            }
            return
        }
        synchronized(lock) {
            val c = _ui.value
            if (c == null || c.info.callId != callId) {
                if (placing) early.add(callId to state)
                else CallTrace.add("state ${shortId(callId)} ignored: current ${shortId(c?.info?.callId)}")
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
                inCallService()
                audio.muted = cur.muted
                session?.connected() ?: audio.start()
                updateProximity(Routes.proximityWanted(cur.routes, videoNow))
                startStats()
            }
            is CallState.Ended -> try {
                ringTimeout?.cancel()
                stopRinging(); Notifications.cancelIncoming(app)
                statsJob?.cancel(); updateProximity(false)
                app.video.reset()
                val cur = _ui.value
                endSession(cur?.takeIf { it.info.callId == callId }, state.reason)
                if (cur != null && cur.info.callId == callId) {
                    val reason = state.reason
                    if (!everActive && uniffi.present.isMissedReason(reason, cur.info.incoming, everActive)) Notifications.missed(app, AccountLabels.ringName(cur.info.peerName, cur.info.account?.name))
                    val s = cur.stats
                    val next = takeover; takeover = null
                    val loss = cur.lossPct
                    // Our outgoing call yielded to their simultaneous call: no ended screen, theirs rings.
                    if (next == null && uniffi.present.classifyEnd(reason) != uniffi.present.EndClass.SUPERSEDED) _ended.value = EndedUi(
                        cur.info.peerName, cur.info.peerDid, reason, cur.info.incoming, everActive,
                        cur.activeSinceMs?.let { (System.currentTimeMillis() - it) / 1000 } ?: 0L,
                        s?.direct, if (s != null && loss != null) qualityBars(s.rttMs.toInt(), loss) else null,
                    )
                    // End & answer: straight on to the new call, no "call ended" screen in between.
                    _ui.update { u -> if (u?.info?.callId == callId) next?.let { CallUi(it, CallState.Ringing, answered = true) } else u }
                    if (next != null) newSession(next)
                }
            } finally {
                // Whatever threw above, the call is over: never leave it in [ui] (the notification is
                // derived from it), and have the ongoing notification re-derive.
                if (_ui.value?.info?.callId == callId) {
                    CallTrace.add("ended ${shortId(callId)}: ui still set, cleared")
                    _ui.update { u -> if (u?.info?.callId == callId) null else u }
                }
                CoreService.ensureRunning(app, CoreService.ACTION_IDLE)
            }
            else -> _ui.update { u -> if (u?.info?.callId == callId) u.copy(state = state) else u }
        }
    }

    /** Pokes the service while the app may still take the microphone type; it derives the rest from [ui]. */
    private fun inCallService() = CoreService.ensureRunning(app, CoreService.ACTION_IN_CALL)

    fun place(did: String): Result<CallInfo> = runCatching {
        // One press of the video button, one video call: never carried to a later call.
        val video = app.video.wantCamera
        app.video.reset()
        if (!Perms.granted(app, Manifest.permission.RECORD_AUDIO)) throw MicPermissionNeeded()
        // The video button: a video call, my camera on from the start (the core drops frames until video runs, so the
        // camera is warm by then). Without the CAMERA permission it still rings as one, with my camera off until the
        // old way asks for it once video runs (wantCamera, for this call only).
        val camera = video && Perms.granted(app, Manifest.permission.CAMERA)
        synchronized(lock) { early.clear(); placing = true }
        val info = try { if (video) app.node.videoCall(did) else app.node.call(did) } catch (e: Throwable) {
            synchronized(lock) { placing = false; early.clear() }
            throw e
        }
        if (info.video && !camera) { runCatching { app.node.setCamera(false) }; app.video.wantCamera = true }
        everActive = false; _ended.value = null
        val replay = synchronized(lock) {
            _ui.value = CallUi(info, CallState.Dialing)
            newSession(info)
            placing = false
            early.filter { it.first == info.callId }.also { early.clear() }
        }
        replay.forEach { (id, st) -> onState(id, st) }
        watchDialing(info.callId)
        val i = Intent(app, CallActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try { app.startActivity(i) } catch (e: Exception) { Log.w("Call", "cannot open call screen: $e") }
        info
    }

    fun answer() {
        val c = _ui.value ?: return
        ringTimeout?.cancel()
        stopRinging(); Notifications.cancelIncoming(app)
        CallTrace.add("answer ${shortId(c.info.callId)}")
        // Raise the service to microphone type while still allowed (user-initiated): marked
        // answered first, so the service derives "in call" from this poke on.
        _ui.update { u -> if (u?.info?.callId == c.info.callId) u.copy(answered = true) else u }
        inCallService()
        session?.answering()
        app.scope.launch {
            try { app.node.answer(c.info.callId) } catch (e: Exception) {
                Log.w("Call", "answer: $e")
                CallTrace.add("answer ${shortId(c.info.callId)} failed (${e.javaClass.simpleName})")
                // Not answered after all: no ongoing notification for a call that is only ringing.
                _ui.update { u -> if (u?.info?.callId == c.info.callId && u.state !is CallState.Active) u.copy(answered = false) else u }
            }
        }
    }

    fun decline() {
        val c = _ui.value ?: return
        ringTimeout?.cancel()
        stopRinging(); Notifications.cancelIncoming(app)
        CallTrace.add("decline ${shortId(c.info.callId)}")
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
        CallTrace.add("end and answer ${shortId(w.callId)}")
        // The current call is still Active (in call); its Ended hands over to [w] marked answered.
        inCallService()
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

    /** When I last pressed End (ms): the call screen plays End's glyph and stays [END_HOLD_MS] after the call goes. */
    val endTapMs = MutableStateFlow(0L)

    fun hangup() {
        val c = _ui.value ?: run { CallTrace.add("hangup: no call"); return }
        CallTrace.add("hangup ${shortId(c.info.callId)}")
        endTapMs.value = System.currentTimeMillis()
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
        videoNow = info.video
        val s = CallSession(
            app.scope, app, audio, AccountLabels.ringName(info.peerName.ifBlank { appString(R.string.call_default_name) }, info.account?.name), info.incoming,
            // A video call (either side pressed Video) is a Telecom video call on the loudspeaker; a headset still wins.
            video = info.video, preferSpeaker = info.video,
            onAnswer = { systemAnswer(id) }, onDisconnect = { systemDisconnect(id) }, onSystemMute = { systemMute(id, it) },
        )
        session = s
        sessionWatch = app.scope.launch {
            launch { s.held.collect { h -> _ui.update { if (it?.info?.callId == id) it.copy(held = h) else it } } }
            s.routes.collect { st ->
                val n = _ui.updateAndGet { if (it?.info?.callId == id) it.copy(routes = st) else it }
                if (n?.info?.callId == id && n.state is CallState.Active) updateProximity(Routes.proximityWanted(st, videoNow))
            }
        }
    }

    private fun endSession(cur: CallUi?, reason: String) {
        val s = session ?: run { audio.stop(); return }
        session = null
        sessionWatch?.cancel(); sessionWatch = null
        val cause = localEnd ?: when {
            cur != null && !everActive && cur.info.incoming && uniffi.present.isMissedReason(reason, true, false) -> DisconnectCause.MISSED
            else -> DisconnectCause.REMOTE
        }
        localEnd = null
        s.end(cause)
    }

    /** The call screen's remote-video renderer while it shows video; read by the stats log. */
    @Volatile var renderer: RemoteRenderer? = null

    /** Video calls are held away from the ear: the camera going on moves the earpiece to the speaker (headsets stay). */
    fun speakerForVideo() { session?.speakerIfEarpiece() }

    /**
     * The core's video state changed ([CallVideo.onVideoState]). A call that becomes a video call (either camera on,
     * e.g. a voice call turned video mid-call) moves from the earpiece to the loudspeaker, once; headsets and
     * Bluetooth stay. While it is one, no proximity wake lock (the screen must not go dark under the picture).
     */
    fun onVideo(v: VideoInfo) {
        val u = _ui.value ?: return
        if (u.info.callId != v.callId) return
        // A VideoInfo can come while it rings (my camera on for "Video"): not decided until negotiated or active.
        val decided = v.negotiated || u.state is CallState.Active
        val now = Routes.isVideo(u.info.video, decided, v.negotiated, v.localCamera, v.remoteCamera)
        val was = videoNow
        videoNow = now
        if (Routes.becameVideo(was, now)) {
            testLog("video_call id=${v.callId} speaker")
            session?.speakerIfEarpiece()
        }
        if (u.state is CallState.Active) updateProximity(Routes.proximityWanted(u.routes, now))
    }

    /** "Video" on a ringing video call: my camera on (the core takes it while ringing), then answer. */
    fun answerWithVideo() {
        val c = _ui.value ?: return
        if (!c.info.incoming || c.state !is CallState.Ringing) return
        app.video.setCamera(true)
        answer()
    }

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

    fun statsLine(s: CallStats): String = uniffi.present.audioStatsLine(uniffi.present.AudioStats(
        s.secs, s.direct, s.rttMs, s.sent, s.received, s.lost, s.recovered, s.concealed, s.bufferedMs,
        s.rxFreqHz.toDouble(), s.rxRms.toDouble(),
    ))

    fun logStats() {
        val s = app.node.callStats()
        testLog(if (s == null) "stats none" else statsLine(s))
    }

    /** While our call is still Dialing, reads `waking` (the callee's phone is being woken) twice a second. */
    private fun watchDialing(callId: String) {
        app.scope.launch {
            while (isActive) {
                val u = _ui.value
                if (u?.info?.callId != callId || u.state !is CallState.Dialing) break
                val s = try { app.node.callStats() } catch (_: Exception) { null }
                if (s != null && s.callId == callId) _ui.update { if (it?.info?.callId == callId && it.state is CallState.Dialing) it.copy(stats = s) else it }
                delay(500)
            }
        }
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

    /** MainActivity paused (home, screen off, another app) or Home's app bar left the screen: a call still ringing gets its
     *  notification and full-screen intent now. */
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

    /** A call wake rings (WakeRing): the same ringtone and vibration as a call; the real call restarts it as its own. */
    internal fun ringForWake() = startRinging()
    internal fun stopRingForWake() = stopRinging()

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
