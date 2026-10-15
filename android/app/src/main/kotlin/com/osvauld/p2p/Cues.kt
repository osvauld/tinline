package com.osvauld.p2p

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import uniffi.p2pcore.toyCuePcm

/*
 * Tinline's toy sounds (sent, received, line, knock). The synthesis lives in the Rust core (crates/core/src/cues.rs, `toy_cue_pcm`),
 * the same samples the desktop plays; this file only plays the PCM through an AudioTrack with the sonification usage, so the media/
 * notification volume and Do Not Disturb decide what is heard. Settings › Sounds › Quiet silences all of it.
 */
object Cues {
    /** Plays the core's toy cue [name] once, off the main thread; it layers over anything playing. */
    private fun play(cue: String) {
        P2pApp.instanceOrNull()?.let { if (AlertPrefs.quiet(it)) return }
        Thread {
            runCatching {
                val pcm = toyCuePcm(cue, 1.0) ?: return@runCatching
                val n = pcm.samples.size
                val track = AudioTrack.Builder()
                    .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
                    .setAudioFormat(AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_16BIT).setSampleRate(pcm.rate.toInt())
                        .setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build())
                    .setTransferMode(AudioTrack.MODE_STATIC).setBufferSizeInBytes(n * 2).build()
                track.write(pcm.samples.toShortArray(), 0, n)
                track.play()
                Thread.sleep((n * 1000L / pcm.rate.toLong()) + 200)
                track.release()
            }
        }.apply { name = "cue"; isDaemon = true }.start()
    }

    /** `sent`: one faint high pluck when I send. */
    fun sent() = play("sent")
    /** `received`: the can, tapped once. */
    fun received() = play("received")
    /** `line`: sign-up Done, a contact added. */
    fun line() = play("line")
    /** `knock`: someone asks to add you. */
    fun knock() = play("knock")

    private var lastReceivedMs = 0L

    /**
     * An incoming message (1:1 or a group post) while the app is in front: the `received` cue once, at most once per ~2 s burst.
     * Follows Settings › Notifications (Messages / Groups switches), Quiet, and a sender I blocked. Like the playground, the open
     * conversation plays it too (the app is in front, so its notification stays silent).
     */
    @Synchronized fun incoming(c: Context, group: Boolean, senderDid: String) {
        if (!AppVisible.resumed || AlertPrefs.quiet(c)) return
        if (!(if (group) AlertPrefs.groups(c) else AlertPrefs.messages(c))) return
        if (P2pApp.get(c).blocks.value.blockedByMe(senderDid)) return
        val now = android.os.SystemClock.elapsedRealtime()
        if (now - lastReceivedMs < 2000) return
        lastReceivedMs = now
        received()
    }
}
