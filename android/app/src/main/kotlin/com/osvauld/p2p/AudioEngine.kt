package com.osvauld.p2p

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import android.media.audiofx.AcousticEchoCanceler
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
 */
class AudioEngine(private val ctx: Context, private val node: Node) {
    private val am = ctx.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    @Volatile private var running = false
    @Volatile var muted = false
    private var micThread: Thread? = null
    private var spkThread: Thread? = null
    private var prevMode = AudioManager.MODE_NORMAL
    private val effects = mutableListOf<android.media.audiofx.AudioEffect>()

    @Synchronized
    fun start(speaker: Boolean) {
        if (running) return
        running = true
        prevMode = am.mode
        am.mode = AudioManager.MODE_IN_COMMUNICATION
        setSpeaker(speaker)
        micThread = Thread(::micLoop, "p2p-mic").also { it.start() }
        spkThread = Thread(::speakerLoop, "p2p-spk").also { it.start() }
    }

    @Synchronized
    fun stop() {
        if (!running) return
        running = false
        micThread?.join(500); spkThread?.join(500)
        micThread = null; spkThread = null
        try { if (Build.VERSION.SDK_INT >= 31) am.clearCommunicationDevice() else am.isSpeakerphoneOn = false } catch (_: Exception) {}
        am.mode = prevMode
    }

    fun setSpeaker(on: Boolean) {
        try {
            if (Build.VERSION.SDK_INT >= 31) {
                if (on) {
                    am.availableCommunicationDevices.firstOrNull { it.type == android.media.AudioDeviceInfo.TYPE_BUILTIN_SPEAKER }
                        ?.let { am.setCommunicationDevice(it) }
                } else {
                    val ear = am.availableCommunicationDevices.firstOrNull { it.type == android.media.AudioDeviceInfo.TYPE_BUILTIN_EARPIECE }
                    if (ear != null) am.setCommunicationDevice(ear) else am.clearCommunicationDevice()
                }
            } else {
                @Suppress("DEPRECATION") run { am.isSpeakerphoneOn = on }
            }
        } catch (e: Exception) { Log.w(TAG, "setSpeaker failed: $e") }
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

    private fun micLoop() {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
        var rec: AudioRecord? = null
        try {
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
        } catch (e: Throwable) {
            Log.w(TAG, "mic unavailable, sending silence: $e")
            testLog("audio mic unavailable: $e")
            try { rec?.release() } catch (_: Exception) {}
            rec = null
        }
        val buf = ByteArray(FRAME_BYTES)
        val zeros = ByteArray(FRAME_BYTES)
        val pacer = Pacer()
        while (running) {
            try {
                var filled = false
                if (rec != null) {
                    var off = 0
                    while (off < FRAME_BYTES && running) {
                        val n = rec.read(buf, off, FRAME_BYTES - off)
                        if (n <= 0) break
                        off += n
                    }
                    filled = off == FRAME_BYTES
                }
                node.pushMicPcm16(if (filled && !muted) buf else zeros)
                pacer.tick()
            } catch (e: InterruptedException) { break } catch (e: Throwable) { Log.w(TAG, "mic loop: $e") }
        }
        try { rec?.stop() } catch (_: Exception) {}
        effects.forEach { try { it.release() } catch (_: Exception) {} }
        effects.clear()
        try { rec?.release() } catch (_: Exception) {}
    }

    private fun speakerLoop() {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
        var track: AudioTrack? = null
        try {
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
        } catch (e: Throwable) {
            Log.w(TAG, "speaker unavailable, discarding audio: $e")
            testLog("audio speaker unavailable: $e")
            try { track?.release() } catch (_: Exception) {}
            track = null
        }
        val pacer = Pacer()
        while (running) {
            try {
                val pcm = node.pullSpeakerPcm16()
                if (track != null) track.write(pcm, 0, pcm.size)
                // The device write paces a real loop; the pacer only matters when it returns early.
                pacer.tick()
            } catch (e: InterruptedException) { break } catch (e: Throwable) { Log.w(TAG, "speaker loop: $e") }
        }
        try { track?.stop() } catch (_: Exception) {}
        try { track?.release() } catch (_: Exception) {}
    }

    companion object {
        const val TAG = "AudioEngine"
        const val RATE = 48000
        const val FRAME_BYTES = 1920
        const val FRAME_NS = 20_000_000L
    }
}
