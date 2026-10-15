package com.osvauld.p2p

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.util.Log
import uniffi.p2pcore.callCuePcm

/**
 * Tinline's own call sounds: the core renders the cue (`call_cue_pcm`, the same synthesis the desktop plays) and an AudioTrack plays it
 * with the ringtone usage, so the ring volume, silent and vibrate modes, Do Not Disturb and the system's own routing still decide what
 * is heard. Quiet (Settings > Sounds) and the Calls alert switch are checked by the callers.
 */
object CallSounds {
    const val PALETTES = "classic,slack,triad"
    /** The cues are rendered at the desktop's level, which is quiet; the ring volume is the phone's, so lift it. */
    private const val GAIN = 4.0

    private fun prefs(c: Context) = c.getSharedPreferences("alerts", Context.MODE_PRIVATE)
    fun palette(c: Context): String = prefs(c).getString("palette", "classic")?.takeIf { it in PALETTES.split(',') } ?: "classic"
    fun setPalette(c: Context, key: String) { prefs(c).edit().putString("palette", key).apply() }

    /** Only the loud ringer mode plays a ring; silent and vibrate leave it to the vibration. */
    private fun ringerLoud(c: Context) = (c.getSystemService(Context.AUDIO_SERVICE) as AudioManager).ringerMode == AudioManager.RINGER_MODE_NORMAL

    private fun track(c: Context, cue: String, loop: Boolean): AudioTrack? {
        val pcm = callCuePcm(palette(c), cue, GAIN) ?: return null
        val bytes = pcm.samples.size * 2
        val t = AudioTrack.Builder()
            .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE).setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
            .setAudioFormat(AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_16BIT).setSampleRate(pcm.rate.toInt()).setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build())
            .setTransferMode(AudioTrack.MODE_STATIC).setBufferSizeInBytes(bytes).build()
        t.write(pcm.samples.toShortArray(), 0, pcm.samples.size)
        if (loop && pcm.looped) t.setLoopPoints(0, pcm.samples.size, -1)
        return t
    }

    /** The looping incoming-call ring. [stop] before dropping it. */
    class Ring internal constructor(private val t: AudioTrack) {
        fun stop() { try { t.stop() } catch (_: Exception) {}; try { t.release() } catch (_: Exception) {} }
    }

    /** Starts the ring, or returns null when the ringer is silent or vibrate (or rendering fails). */
    fun startRing(c: Context): Ring? {
        if (!ringerLoud(c)) return null
        return try { track(c, "ring", true)?.also { it.play() }?.let { Ring(it) } } catch (e: Exception) { Log.w("Call", "ring: $e"); null }
    }

    private var preview: AudioTrack? = null

    /** Settings > Sounds previews; follows the ringer like a real call. */
    @Synchronized fun preview(c: Context, cue: String) {
        if (AlertPrefs.quiet(c) || !ringerLoud(c)) return
        try { preview?.stop(); preview?.release() } catch (_: Exception) {}
        preview = try { track(c, cue, false)?.also { it.play() } } catch (e: Exception) { Log.w("Call", "preview: $e"); null }
    }
}
