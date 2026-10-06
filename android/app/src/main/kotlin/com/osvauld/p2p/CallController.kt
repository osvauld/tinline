package com.osvauld.p2p

import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.Ringtone
import android.media.RingtoneManager
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.util.Log
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import uniffi.p2pcore.CallInfo
import uniffi.p2pcore.CallState
import uniffi.p2pcore.CallStats

data class CallUi(
    val info: CallInfo,
    val state: CallState,
    val muted: Boolean = false,
    val speaker: Boolean = false,
    val stats: CallStats? = null,
    val activeSinceMs: Long? = null,
)

/** Single place that reacts to call events: UI state, ringing, notifications, audio, service type. */
class CallController(private val app: P2pApp) {
    private val _ui = MutableStateFlow<CallUi?>(null)
    val ui: StateFlow<CallUi?> = _ui
    private val audio by lazy { AudioEngine(app, app.node) }
    private var ringtone: Ringtone? = null
    private var statsJob: Job? = null
    private var everActive = false

    fun onIncoming(call: CallInfo) {
        testLog("incoming id=${call.callId} from=${call.peerName.ifBlank { call.peerDid }}")
        everActive = false
        _ui.value = CallUi(call, CallState.Ringing)
        CoreService.ensureRunning(app)
        Notifications.incoming(app, call)
        startRinging()
    }

    fun onState(callId: String, state: CallState) {
        testLog("state id=$callId state=${describe(state)}")
        val cur = _ui.value
        if (cur == null || cur.info.callId != callId) return
        when (state) {
            is CallState.Active -> {
                everActive = true
                stopRinging(); Notifications.cancelIncoming(app)
                _ui.value = cur.copy(state = state, activeSinceMs = System.currentTimeMillis())
                inCallService(cur.info)
                audio.muted = cur.muted
                audio.start(cur.speaker)
                startStats()
            }
            is CallState.Ended -> {
                stopRinging(); Notifications.cancelIncoming(app)
                statsJob?.cancel(); audio.stop()
                if (cur.info.incoming && !everActive) Notifications.missed(app, cur.info.peerName)
                _ui.value = null
                CoreService.ensureRunning(app, CoreService.ACTION_IDLE)
            }
            else -> _ui.value = cur.copy(state = state)
        }
    }

    private fun inCallService(info: CallInfo) =
        CoreService.ensureRunning(app, CoreService.ACTION_IN_CALL, info.peerName.ifBlank { "Call" })

    fun place(did: String): Result<CallInfo> = runCatching {
        val info = app.node.call(did)
        everActive = false
        _ui.value = CallUi(info, CallState.Dialing)
        val i = Intent(app, CallActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try { app.startActivity(i) } catch (e: Exception) { Log.w("Call", "cannot open call screen: $e") }
        info
    }

    fun answer() {
        val c = _ui.value ?: return
        stopRinging(); Notifications.cancelIncoming(app)
        // Raise the service to microphone type while still allowed (user-initiated).
        inCallService(c.info)
        app.scope.launch { try { app.node.answer(c.info.callId) } catch (e: Exception) { Log.w("Call", "answer: $e") } }
    }

    fun decline() {
        val c = _ui.value ?: return
        stopRinging(); Notifications.cancelIncoming(app)
        app.scope.launch { try { app.node.decline(c.info.callId) } catch (e: Exception) { Log.w("Call", "decline: $e") } }
    }

    fun hangup() {
        val c = _ui.value ?: return
        app.scope.launch { try { app.node.hangup(c.info.callId) } catch (e: Exception) { Log.w("Call", "hangup: $e") } }
    }

    fun toggleMute() {
        val c = _ui.value ?: return
        audio.muted = !c.muted
        _ui.value = c.copy(muted = !c.muted)
    }

    fun toggleSpeaker() {
        val c = _ui.value ?: return
        audio.setSpeaker(!c.speaker)
        _ui.value = c.copy(speaker = !c.speaker)
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
            while (isActive) {
                delay(1000)
                val s = try { app.node.callStats() } catch (_: Exception) { null } ?: continue
                testLog(statsLine(s))
                _ui.value = _ui.value?.copy(stats = s)
            }
        }
    }

    private fun startRinging() {
        stopRinging()
        try {
            val uri = RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE)
            ringtone = RingtoneManager.getRingtone(app, uri)?.apply {
                audioAttributes = AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build()
                isLooping = true
                play()
            }
        } catch (e: Exception) { Log.w("Call", "ringtone: $e") }
        try {
            val pattern = longArrayOf(0, 800, 800)
            vibrator().vibrate(VibrationEffect.createWaveform(pattern, 0))
        } catch (e: Exception) { Log.w("Call", "vibrate: $e") }
    }

    private fun vibrator(): Vibrator =
        if (Build.VERSION.SDK_INT >= 31)
            (app.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as VibratorManager).defaultVibrator
        else @Suppress("DEPRECATION") (app.getSystemService(Context.VIBRATOR_SERVICE) as Vibrator)

    private fun stopRinging() {
        try { ringtone?.stop() } catch (_: Exception) {}
        ringtone = null
        try { vibrator().cancel() } catch (_: Exception) {}
    }

    companion object {
        fun describe(s: CallState): String = when (s) {
            is CallState.Ended -> "Ended(${s.reason})"
            is CallState.Dialing -> "Dialing"
            is CallState.Ringing -> "Ringing"
            is CallState.Active -> "Active"
        }
    }
}
