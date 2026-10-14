package com.osvauld.p2p

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import kotlin.math.PI
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.sqrt

/*
 * Generated UI sounds, no sample files: the playground synth (lib/fx/audio.ts, SIGNUP-HANDOFF.md § 6d), the same recipe as the
 * desktop's crates/desktop/src/cues.rs. Wired: `line` (sign-up Done, a contact added, both sides) and `knock` (someone asks to add
 * you). Tok = the can: five inharmonic partials of a small struck tin (higher ones die sooner) plus a 4 ms tap. Pluck = Karplus-Strong
 * (a softened noise burst in a delay one period long, averaged each pass, -60 dB over the note), detuned up to ±4 cents per play;
 * sine = a sine and its octave at 18 %. The mix goes through a 5 kHz low-pass at 0.5 × 0.25 of full scale.
 * Meant to become one shared Rust synth for both apps.
 */
object Cues {
    private const val RATE = 48_000

    private enum class Inst { Sine, Pluck, Tok }
    private class Note(val f: Double, val at: Double, val dur: Double, val peak: Double, val atk: Double, val inst: Inst)

    /** `line`: D5 then G5 (on the two tugs), a soft G4 under the second. */
    private val LINE = listOf(
        Note(587.0, 0.0, 1.4, 0.55, 0.003, Inst.Pluck),
        Note(784.0, LINE_GAP_S, 1.8, 0.55, 0.003, Inst.Pluck),
        Note(392.0, LINE_GAP_S, 1.6, 0.12, 0.06, Inst.Sine),
    )

    /** `knock`: someone asks to add you. Two tin taps, the second a little lower. */
    private val KNOCK = listOf(
        Note(640.0, 0.0, 0.35, 0.34, 0.002, Inst.Tok),
        Note(600.0, 0.17, 0.45, 0.3, 0.002, Inst.Tok),
    )

    /** audio.ts tokBuf: (ratio, amplitude, decay share) per partial, random phase, then a 4 ms tap of softened noise. */
    private fun tok(f: Double, dur: Double, rnd: java.util.Random): DoubleArray {
        val n = ceil(RATE * dur).toInt(); val y = DoubleArray(n)
        val parts = arrayOf(doubleArrayOf(1.0, 1.0, 1.0), doubleArrayOf(1.51, .55, .55), doubleArrayOf(2.03, .35, .4), doubleArrayOf(2.47, .22, .28), doubleArrayOf(3.31, .12, .18))
        for (p in parts) {
            val w = 2 * PI * f * p[0] / RATE; val k = 1 / (RATE * dur * p[2] / 6.9); val ph = rnd.nextDouble() * 6.28
            for (i in 0 until n) y[i] += p[1] * kotlin.math.exp(-i * k) * sin(w * i + ph)
        }
        var lp = 0.0; val tap = (RATE * 0.004).toInt()
        for (i in 0 until minOf(tap, n)) { lp += 0.3 * (rnd.nextDouble() * 2 - 1 - lp); y[i] += 0.6 * lp * (1 - i.toDouble() / tap) }
        var mx = 0.0; for (v in y) mx = max(mx, kotlin.math.abs(v))
        if (mx > 0) for (i in y.indices) y[i] /= mx
        val fade = minOf(256, n); for (i in 0 until fade) y[n - 1 - i] *= i.toDouble() / fade
        return y
    }

    private fun pluck(f: Double, dur: Double, rnd: java.util.Random): DoubleArray {
        val n = ceil(RATE * dur).toInt(); val y = DoubleArray(n)
        val period = RATE / f - 0.5; val m = floor(period).toInt(); val g = period - m
        val decay = 0.001.pow(1 / (f * dur)); var lp = 0.0
        for (i in 0 until minOf(m + 2, n)) { lp += 0.45 * (rnd.nextDouble() * 2 - 1 - lp); y[i] = lp }
        for (i in m + 2 until n) y[i] = decay * 0.5 * ((1 - g) * (y[i - m] + y[i - m - 1]) + g * (y[i - m - 1] + y[i - m - 2]))
        var mx = 0.0; for (v in y) mx = max(mx, kotlin.math.abs(v))
        if (mx > 0) for (i in y.indices) y[i] /= mx
        val fade = minOf(256, n); for (i in 0 until fade) y[n - 1 - i] *= i.toDouble() / fade
        return y
    }

    private fun render(notes: List<Note>): ShortArray {
        val rnd = java.util.Random()
        val end = notes.maxOf { it.at + it.dur } + 0.05
        val out = DoubleArray(ceil(end * RATE).toInt())
        for (nt in notes) {
            val t0 = (nt.at * RATE).toInt()
            if (nt.inst != Inst.Sine) {
                val human = 2.0.pow((rnd.nextDouble() - 0.5) * 8 / 1200)
                val b = if (nt.inst == Inst.Pluck) pluck(nt.f * human, nt.dur, rnd) else tok(nt.f * human, nt.dur, rnd)
                for (i in b.indices) { val t = i.toDouble() / RATE; val env = if (t < nt.atk) nt.peak * t / nt.atk else nt.peak; if (t0 + i < out.size) out[t0 + i] += b[i] * env }
            } else {
                val len = ((nt.dur + 0.05) * RATE).toInt()
                for (i in 0 until len) {
                    val t = i.toDouble() / RATE
                    val env = when { t < nt.atk -> nt.peak * t / nt.atk; t < nt.dur -> nt.peak * (0.0001 / nt.peak).pow((t - nt.atk) / (nt.dur - nt.atk)); else -> 0.0 }
                    val ph = 2 * PI * nt.f * t
                    if (t0 + i < out.size) out[t0 + i] += env * (sin(ph) + 0.18 * sin(2 * ph))
                }
            }
        }
        // RBJ low-pass, Q = 1/√2 (Web Audio's default)
        val w0 = 2 * PI * 5000 / RATE; val cs = kotlin.math.cos(w0); val alpha = sin(w0) / (2 * (1 / sqrt(2.0)))
        val a0 = 1 + alpha; val b0 = (1 - cs) / 2 / a0; val b1 = (1 - cs) / a0; val a1 = -2 * cs / a0; val a2 = (1 - alpha) / a0
        var x1 = 0.0; var x2 = 0.0; var y1 = 0.0; var y2 = 0.0
        return ShortArray(out.size) { i ->
            val x0 = out[i]; val y0 = b0 * x0 + b1 * x1 + b0 * x2 - a1 * y1 - a2 * y2
            x2 = x1; x1 = x0; y2 = y1; y1 = y0
            ((y0 * 0.125).coerceIn(-1.0, 1.0) * 32767).toInt().toShort()
        }
    }

    /** `sent`: one faint high pluck when I send (about -13 dB vs Connected). */
    private val SENT = listOf(Note(1175.0, 0.0, 0.5, 0.18, 0.003, Inst.Pluck))

    /** `received`: the can, tapped once, when a message arrives while the app is open. */
    private val RECEIVED = listOf(Note(880.0, 0.0, 0.45, 0.32, 0.002, Inst.Tok))

    fun sent() = play(SENT)
    fun received() = play(RECEIVED)

    /** Plays the `line` cue once, off the main thread; it layers over anything playing. */
    fun line() = play(LINE)

    /** Plays the `knock` cue (a contact request arrived) once. */
    fun knock() = play(KNOCK)

    private fun play(notes: List<Note>) {
        // Settings › Sounds › Quiet: no sounds from Tinline.
        P2pApp.instanceOrNull()?.let { if (AlertPrefs.quiet(it)) return }
        Thread {
            runCatching {
                val pcm = render(notes)
                val track = AudioTrack.Builder()
                    .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
                    .setAudioFormat(AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_16BIT).setSampleRate(RATE)
                        .setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build())
                    .setTransferMode(AudioTrack.MODE_STATIC).setBufferSizeInBytes(pcm.size * 2).build()
                track.write(pcm, 0, pcm.size)
                track.play()
                Thread.sleep((pcm.size * 1000L / RATE) + 200)
                track.release()
            }
        }.apply { name = "cue"; isDaemon = true }.start()
    }
}
