package com.osvauld.p2p

import android.content.Context
import android.media.AudioAttributes
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
 * Audio focus: a GAIN_TRANSIENT request with the voice-communication usage is held for the call.
 * If another app takes focus (e.g. a GSM call, an alarm) the mic is MUTED until focus comes back;
 * we do not hang up, the user decides.
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
    @Volatile private var focusMuted = false
    private var micThread: Thread? = null
    private var spkThread: Thread? = null
    private var prevMode = AudioManager.MODE_NORMAL
    private var focus: AudioFocusRequest? = null
    private var scoStarted = false

    private val focusListener = AudioManager.OnAudioFocusChangeListener { change ->
        focusMuted = change != AudioManager.AUDIOFOCUS_GAIN
        Log.i(TAG, "audio focus change=$change -> focusMuted=$focusMuted")
    }

    @Synchronized
    fun start(speaker: Boolean) {
        if (running) return
        running = true
        val gen = ++generation
        prevMode = am.mode
        am.mode = AudioManager.MODE_IN_COMMUNICATION
        requestFocus()
        setSpeaker(speaker)
        micThread = Thread({ micLoop(gen) }, "p2p-mic").also { it.start() }
        spkThread = Thread({ speakerLoop(gen) }, "p2p-spk").also { it.start() }
    }

    @Synchronized
    fun stop() {
        if (!running) return
        running = false
        generation++
        // Fully join: the old threads release the AudioRecord/AudioTrack, which a following
        // start() would otherwise race with. The loops wake at least every 20 ms.
        for (t in listOf(micThread, spkThread)) {
            try { t?.join(2000) } catch (_: InterruptedException) { Thread.currentThread().interrupt() }
            if (t?.isAlive == true) Log.w(TAG, "audio thread ${t.name} did not exit in time")
        }
        micThread = null; spkThread = null
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

    /**
     * speaker=true forces the loudspeaker. speaker=false hands routing back to the system, which
     * prefers a connected wired/Bluetooth headset and otherwise uses the earpiece (forcing the
     * earpiece, as before, overrode headsets).
     */
    @Synchronized
    fun setSpeaker(on: Boolean) {
        try {
            if (Build.VERSION.SDK_INT >= 31) {
                if (on) {
                    am.availableCommunicationDevices.firstOrNull { it.type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER }
                        ?.let { am.setCommunicationDevice(it) }
                } else am.clearCommunicationDevice()
            } else {
                @Suppress("DEPRECATION") run {
                    if (on) { stopSco(); am.isSpeakerphoneOn = true }
                    else {
                        am.isSpeakerphoneOn = false
                        val sco = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS).any { it.type == AudioDeviceInfo.TYPE_BLUETOOTH_SCO }
                        if (sco && !scoStarted) { am.startBluetoothSco(); am.isBluetoothScoOn = true; scoStarted = true }
                    }
                }
            }
        } catch (e: Exception) { Log.w(TAG, "setSpeaker failed: $e") }
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
                node().pushMicPcm16(if (filled && !muted && !focusMuted) buf else zeros)
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
