package com.osvauld.p2p

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import android.media.audiofx.AcousticEchoCanceler
import android.media.audiofx.AudioEffect
import android.media.audiofx.AutomaticGainControl
import android.media.audiofx.NoiseSuppressor
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.Process
import android.util.Log
import uniffi.p2pcore.Node

/**
 * Bridges the device audio to the core: mic -> pushMicPcm16, pullSpeakerPcm16 -> speaker.
 * Both loops are paced by a 20 ms clock so they behave identically when no audio device exists
 * (emulator): the call stays up, silence goes out, and received frames are still drained.
 *
 * [node] is a supplier because the app can replace its Node (restore over a locked vault); the
 * engine must never hold on to an old one. Each start() gets a new generation; threads of an older
 * generation exit on their own, so a late stop()/start() pair can never leave two engines running.
 *
 * Routing, audio mode and focus normally belong to Telecom ([CallSession]). Only when the call
 * could not be registered there does [takeRouting] make the engine do it itself: communication
 * mode, a GAIN_TRANSIENT voice-communication focus request (another app taking focus, e.g. a GSM
 * call, MUTES the mic until it comes back; we do not hang up, the user decides), and the route
 * list in [onRoutes] (communication devices on API 31+, speakerphone/SCO below).
 */
class AudioEngine(
    private val ctx: Context,
    private val node: () -> Node,
    private val onMicUnavailable: () -> Unit = {},
) {
    private val am = ctx.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    @Volatile private var running = false
    @Volatile private var generation = 0
    @Volatile var muted = false
    /** Telecom put the call on hold (another call took over): send and play silence. */
    @Volatile var held = false
    @Volatile private var focusMuted = false
    private var micThread: Thread? = null
    private var spkThread: Thread? = null

    // Fallback routing ([takeRouting]); untouched while Telecom owns the call.
    /** Fallback route changes; set by the owner before [takeRouting]. */
    @Volatile var onRoutes: (RouteState) -> Unit = {}
    private var owns = false
    private var preferSpeaker = false
    private var prevMode = AudioManager.MODE_NORMAL
    private var focus: AudioFocusRequest? = null
    private var scoStarted = false
    /** API < 31: the route we last applied (there is no getter for it). */
    private var legacyCurrent = AudioRoute.Kind.Earpiece
    private val main = Handler(Looper.getMainLooper())

    private val focusListener = AudioManager.OnAudioFocusChangeListener { change ->
        focusMuted = change != AudioManager.AUDIOFOCUS_GAIN
        Log.i(TAG, "audio focus change=$change -> focusMuted=$focusMuted")
    }

    private val deviceCallback = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) {
            // A headset connected mid-call: move to it, as a phone call would.
            synchronized(this@AudioEngine) {
                if (!owns) return
                val headset = added.mapNotNull { routeOf(it) }
                    .firstOrNull { it.kind == AudioRoute.Kind.Bluetooth || it.kind == AudioRoute.Kind.Wired }
                if (headset != null) applyRoute(Routes.pickDefault(routes().available, preferSpeaker))
                else publish()
            }
        }
        override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) {
            synchronized(this@AudioEngine) {
                if (!owns) return
                val st = routes()
                if (st.current == null || st.available.none { it.key == st.current.key }) applyRoute(Routes.pickDefault(st.available, preferSpeaker))
                else publish()
            }
        }
    }

    // Lazy: the listener type only exists on API 31+.
    private val commListener by lazy { AudioManager.OnCommunicationDeviceChangedListener { synchronized(this) { if (owns) publish() } } }

    /** Starts the mic and speaker loops. Routing stays with Telecom unless [takeRouting] is called. */
    @Synchronized
    fun start() {
        if (running) return
        running = true
        val gen = ++generation
        micThread = Thread({ micLoop(gen) }, "p2p-mic").also { it.start() }
        spkThread = Thread({ speakerLoop(gen) }, "p2p-spk").also { it.start() }
    }

    @Synchronized
    fun stop() {
        if (running) {
            running = false
            generation++
            // Fully join: the old threads release the AudioRecord/AudioTrack, which a following
            // start() would otherwise race with. The loops wake at least every 20 ms.
            for (t in listOf(micThread, spkThread)) {
                try { t?.join(2000) } catch (_: InterruptedException) { Thread.currentThread().interrupt() }
                if (t?.isAlive == true) Log.w(TAG, "audio thread ${t.name} did not exit in time")
            }
            micThread = null; spkThread = null
        }
        held = false
        releaseRouting()
    }

    /**
     * Telecom is not handling this call: take communication mode, focus and routing ourselves and
     * start on [Routes.pickDefault]. Idempotent; undone by [stop].
     */
    @Synchronized
    fun takeRouting(preferSpeaker: Boolean) {
        if (owns) return
        owns = true
        this.preferSpeaker = preferSpeaker
        Log.i(TAG, "taking routing (no Telecom), preferSpeaker=$preferSpeaker")
        prevMode = am.mode
        am.mode = AudioManager.MODE_IN_COMMUNICATION
        requestFocus()
        try {
            am.registerAudioDeviceCallback(deviceCallback, main)
            if (Build.VERSION.SDK_INT >= 31) am.addOnCommunicationDeviceChangedListener(ctx.mainExecutor, commListener)
        } catch (e: Exception) { Log.w(TAG, "device listeners: $e") }
        applyRoute(Routes.pickDefault(routes().available, preferSpeaker))
    }

    @Synchronized
    private fun releaseRouting() {
        if (!owns) return
        owns = false
        try {
            am.unregisterAudioDeviceCallback(deviceCallback)
            if (Build.VERSION.SDK_INT >= 31) am.removeOnCommunicationDeviceChangedListener(commListener)
        } catch (_: Exception) {}
        try {
            if (Build.VERSION.SDK_INT >= 31) am.clearCommunicationDevice()
            else {
                @Suppress("DEPRECATION") run { am.isSpeakerphoneOn = false }
                stopSco()
            }
        } catch (_: Exception) {}
        abandonFocus()
        focusMuted = false
        am.mode = prevMode
        legacyCurrent = AudioRoute.Kind.Earpiece
    }

    /** Fallback routing only: play through [route] (one of the last published routes). */
    @Synchronized
    fun selectRoute(route: AudioRoute) {
        if (!owns) return
        applyRoute(routes().available.firstOrNull { it.key == route.key })
    }

    private fun requestFocus() {
        try {
            val r = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
                .setAudioAttributes(AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build())
                .setOnAudioFocusChangeListener(focusListener)
                .build()
            focus = r
            am.requestAudioFocus(r)
        } catch (e: Exception) { Log.w(TAG, "audio focus request failed: $e") }
    }

    private fun abandonFocus() {
        try { focus?.let { am.abandonAudioFocusRequest(it) } } catch (_: Exception) {}
        focus = null
    }

    private fun routeOf(d: AudioDeviceInfo): AudioRoute? {
        val kind = when (d.type) {
            AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> AudioRoute.Kind.Earpiece
            AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> AudioRoute.Kind.Speaker
            AudioDeviceInfo.TYPE_BLUETOOTH_SCO, AudioDeviceInfo.TYPE_HEARING_AID,
            AudioDeviceInfo.TYPE_BLE_HEADSET, AudioDeviceInfo.TYPE_BLE_SPEAKER -> AudioRoute.Kind.Bluetooth
            AudioDeviceInfo.TYPE_WIRED_HEADSET, AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
            AudioDeviceInfo.TYPE_USB_HEADSET -> AudioRoute.Kind.Wired
            else -> return null
        }
        val name = when (kind) {
            AudioRoute.Kind.Bluetooth, AudioRoute.Kind.Wired -> d.productName?.toString().orEmpty()
            else -> ""
        }
        return AudioRoute(kind, name, "dev:${d.id}")
    }

    /** The fallback's current routes. */
    private fun routes(): RouteState = try {
        if (Build.VERSION.SDK_INT >= 31) {
            val avail = am.availableCommunicationDevices.mapNotNull { routeOf(it) }.distinctBy { it.key }
            RouteState(am.communicationDevice?.let { routeOf(it) }, avail)
        } else {
            // Below 31 a wired headset simply replaces the earpiece; Bluetooth needs SCO.
            val sco = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS).firstOrNull { it.type == AudioDeviceInfo.TYPE_BLUETOOTH_SCO }
            val avail = listOfNotNull(
                AudioRoute(AudioRoute.Kind.Earpiece, "", "earpiece"),
                AudioRoute(AudioRoute.Kind.Speaker, "", "speaker"),
                sco?.let { AudioRoute(AudioRoute.Kind.Bluetooth, it.productName?.toString().orEmpty(), "sco") },
            )
            RouteState(avail.firstOrNull { it.kind == legacyCurrent } ?: avail.first(), avail)
        }
    } catch (e: Exception) { Log.w(TAG, "routes: $e"); RouteState() }

    private fun publish() {
        val st = routes()
        try { onRoutes(st) } catch (e: Exception) { Log.w(TAG, "onRoutes: $e") }
    }

    private fun applyRoute(route: AudioRoute?) {
        try {
            if (route != null) {
                if (Build.VERSION.SDK_INT >= 31) {
                    val id = route.key.removePrefix("dev:").toIntOrNull()
                    am.availableCommunicationDevices.firstOrNull { it.id == id }?.let { am.setCommunicationDevice(it) }
                } else @Suppress("DEPRECATION") {
                    when (route.kind) {
                        AudioRoute.Kind.Speaker -> { stopSco(); am.isSpeakerphoneOn = true }
                        AudioRoute.Kind.Bluetooth -> {
                            am.isSpeakerphoneOn = false
                            if (!scoStarted) { am.startBluetoothSco(); am.isBluetoothScoOn = true; scoStarted = true }
                        }
                        else -> { stopSco(); am.isSpeakerphoneOn = false }
                    }
                    legacyCurrent = route.kind
                }
                Log.i(TAG, "route -> ${route.kind} ${route.name}")
            }
        } catch (e: Exception) { Log.w(TAG, "applyRoute failed: $e") }
        publish()
    }

    @Suppress("DEPRECATION")
    private fun stopSco() {
        if (!scoStarted) return
        try { am.isBluetoothScoOn = false; am.stopBluetoothSco() } catch (_: Exception) {}
        scoStarted = false
    }

    /** Sleeps until the next 20 ms slot; resyncs if we fell far behind (a blocking device read). */
    private class Pacer {
        var next = System.nanoTime()
        fun tick() {
            next += FRAME_NS
            val now = System.nanoTime()
            val wait = next - now
            if (wait > 0) Thread.sleep(wait / 1_000_000, (wait % 1_000_000).toInt())
            else if (-wait > 200_000_000L) next = now
        }
    }

    private fun live(gen: Int) = running && generation == gen

    /** An opened mic plus its audio effects; release() frees all of it. */
    private class Mic(val rec: AudioRecord, val effects: List<AudioEffect>) {
        fun release() {
            try { rec.stop() } catch (_: Exception) {}
            effects.forEach { try { it.release() } catch (_: Exception) {} }
            try { rec.release() } catch (_: Exception) {}
        }
    }

    private fun openMic(): Mic? {
        var rec: AudioRecord? = null
        val effects = mutableListOf<AudioEffect>()
        return try {
            val min = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
            rec = AudioRecord(
                MediaRecorder.AudioSource.VOICE_COMMUNICATION, RATE, AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT, maxOf(min, FRAME_BYTES * 8),
            )
            if (rec.state != AudioRecord.STATE_INITIALIZED) throw IllegalStateException("AudioRecord not initialized")
            val s = rec.audioSessionId
            if (AcousticEchoCanceler.isAvailable()) AcousticEchoCanceler.create(s)?.also { it.enabled = true; effects += it }
            if (NoiseSuppressor.isAvailable()) NoiseSuppressor.create(s)?.also { it.enabled = true; effects += it }
            if (AutomaticGainControl.isAvailable()) AutomaticGainControl.create(s)?.also { it.enabled = true; effects += it }
            rec.startRecording()
            Mic(rec, effects)
        } catch (e: Throwable) {
            Log.w(TAG, "mic unavailable, sending silence: $e")
            testLog("audio mic unavailable: $e")
            effects.forEach { try { it.release() } catch (_: Exception) {} }
            try { rec?.release() } catch (_: Exception) {}
            null
        }
    }

    private fun micLoop(gen: Int) {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
        var mic = openMic()
        if (mic == null) onMicUnavailable()
        val buf = ByteArray(FRAME_BYTES)
        val zeros = ByteArray(FRAME_BYTES)
        val pacer = Pacer()
        var errors = 0
        while (live(gen)) {
            try {
                var filled = false
                val m = mic
                if (m != null) {
                    var off = 0
                    while (off < FRAME_BYTES && live(gen)) {
                        val n = m.rec.read(buf, off, FRAME_BYTES - off)
                        if (n <= 0) { if (n < 0) errors++; break }
                        off += n
                        errors = 0
                    }
                    filled = off == FRAME_BYTES
                    if (errors >= MAX_ERRORS) {
                        Log.w(TAG, "mic read failing, recreating AudioRecord")
                        m.release(); mic = openMic(); errors = 0
                        if (mic == null) onMicUnavailable()
                    }
                }
                node().pushMicPcm16(if (filled && !muted && !focusMuted && !held) buf else zeros)
                pacer.tick()
            } catch (e: InterruptedException) { break } catch (e: Throwable) {
                Log.w(TAG, "mic loop: $e")
                try { Thread.sleep(20) } catch (_: InterruptedException) { break }
            }
        }
        mic?.release()
    }

    private fun openTrack(): AudioTrack? {
        var track: AudioTrack? = null
        return try {
            val min = AudioTrack.getMinBufferSize(RATE, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_16BIT)
            track = AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build())
                .setAudioFormat(
                    AudioFormat.Builder().setSampleRate(RATE).setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                        .setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
                .setTransferMode(AudioTrack.MODE_STREAM)
                .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                .setBufferSizeInBytes(maxOf(min, FRAME_BYTES * 6))
                .build()
            if (track.state != AudioTrack.STATE_INITIALIZED) throw IllegalStateException("AudioTrack not initialized")
            track.play()
            track
        } catch (e: Throwable) {
            Log.w(TAG, "speaker unavailable, discarding audio: $e")
            testLog("audio speaker unavailable: $e")
            try { track?.release() } catch (_: Exception) {}
            null
        }
    }

    private fun speakerLoop(gen: Int) {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
        var track = openTrack()
        val pacer = Pacer()
        var errors = 0
        while (live(gen)) {
            try {
                val pcm = node().pullSpeakerPcm16()
                val t = track
                if (t != null) {
                    // On hold the frames are still drained, but the other call has the speaker.
                    if (held) pcm.fill(0)
                    val n = t.write(pcm, 0, pcm.size)
                    if (n < 0) errors++ else errors = 0
                    if (errors >= MAX_ERRORS) {
                        Log.w(TAG, "speaker write failing, recreating AudioTrack")
                        try { t.stop() } catch (_: Exception) {}
                        try { t.release() } catch (_: Exception) {}
                        track = openTrack(); errors = 0
                    }
                }
                // The device write paces a real loop; the pacer only matters when it returns early.
                pacer.tick()
            } catch (e: InterruptedException) { break } catch (e: Throwable) {
                Log.w(TAG, "speaker loop: $e")
                try { Thread.sleep(20) } catch (_: InterruptedException) { break }
            }
        }
        try { track?.stop() } catch (_: Exception) {}
        try { track?.release() } catch (_: Exception) {}
    }

    companion object {
        const val TAG = "AudioEngine"
        const val RATE = 48000
        const val FRAME_BYTES = 1920
        const val FRAME_NS = 20_000_000L
        const val MAX_ERRORS = 10
    }
}
