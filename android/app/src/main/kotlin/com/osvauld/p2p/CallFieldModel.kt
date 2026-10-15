package com.osvauld.p2p

import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.cbrt
import kotlin.math.exp
import kotlin.math.floor
import kotlin.math.hypot
import kotlin.math.ln
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.tanh

/*
 * The call screen's colour field model: five soft, hue-separated masses on Lissajous paths that swell and warm
 * with the call's cue notes. A line-for-line port of crates/desktop/src/call_field.rs (itself a numerical port of
 * the playground's lib/calls/field/model.ts): same constants, same state table, same OKLab mixing, same
 * reduced-motion crossfade.
 *
 * Non-UI logic normally lives in Rust (shared across clients). This is per-frame animation maths for drawing only,
 * already duplicated on desktop with golden parity, so a Kotlin port guarded by the same golden file
 * (crates/desktop/src/call_field_golden.txt, see CallFieldTest) is accepted here instead of a new FFI surface.
 *
 * The model is pure: time is passed in (ms), no clocks, no audio, no microphone. The connected field breathes with a
 * simulated voice (makeVoice bursts), as on desktop.
 */

internal object FieldMath {
    const val F_MIN = 196.0
    const val F_MAX = 1568.0
    const val K = 5
    fun clamp(x: Double, a: Double, b: Double) = max(a, min(b, x))
    fun smooth(x: Double): Double { val c = clamp(x, 0.0, 1.0); return c * c * (3.0 - 2.0 * c) }
    fun lerp(a: Double, b: Double, s: Double) = a + (b - a) * s
    /** JS Math.round (half up). */
    fun jsRound(x: Double) = floor(x + 0.5)

    private fun s2l(v: Double): Double { val c = v / 255.0; return if (c <= 0.04045) c / 12.92 else ((c + 0.055) / 1.055).pow(2.4) }
    private fun l2s(v: Double): Double {
        val c = clamp(v, 0.0, 1.0)
        return 255.0 * if (c <= 0.0031308) 12.92 * c else 1.055 * c.pow(1.0 / 2.4) - 0.055
    }
    fun rgbToLab(rgb: DoubleArray): DoubleArray {
        val r = s2l(rgb[0]); val g = s2l(rgb[1]); val b = s2l(rgb[2])
        val l = cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
        val m = cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
        val s = cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
        return doubleArrayOf(
            0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
        )
    }
    /** OKLab to sRGB 0..255 (used by the below-API-33 gradient fallback). */
    fun labToRgb(c: DoubleArray): DoubleArray {
        val l = (c[0] + 0.3963377774 * c[1] + 0.2158037573 * c[2]).pow(3)
        val m = (c[0] - 0.1055613458 * c[1] - 0.0638541728 * c[2]).pow(3)
        val s = (c[0] - 0.0894841775 * c[1] - 1.2914855480 * c[2]).pow(3)
        return doubleArrayOf(
            l2s(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
            l2s(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
            l2s(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
        )
    }
    /** OKLab mix; chroma is interpolated separately so a teal-to-gold mid-mix does not go grey/olive. */
    fun mixLab(a: DoubleArray, b: DoubleArray, s: Double): DoubleArray {
        val l = lerp(a[0], b[0], s); val aa = lerp(a[1], b[1], s); val bb = lerp(a[2], b[2], s)
        val c0 = hypot(a[1], a[2]); val c1 = hypot(b[1], b[2]); val cm = hypot(aa, bb)
        val ct = lerp(c0, c1, s)
        val k = if (cm > 1e-5) ct / cm else 1.0
        return doubleArrayOf(l, aa * k, bb * k)
    }
}

internal enum class FieldTheme { Light, Dark }

private class FLabs(val bg: DoubleArray, val teal: DoubleArray, val gold: DoubleArray)
private fun fLabs(t: FieldTheme): FLabs = when (t) {
    FieldTheme.Light -> FLabs(FieldMath.rgbToLab(doubleArrayOf(245.0, 246.0, 243.0)), FieldMath.rgbToLab(doubleArrayOf(110.0, 210.0, 186.0)), FieldMath.rgbToLab(doubleArrayOf(248.0, 206.0, 104.0)))
    FieldTheme.Dark -> FLabs(FieldMath.rgbToLab(doubleArrayOf(15.0, 21.0, 19.0)), FieldMath.rgbToLab(doubleArrayOf(20.0, 184.0, 160.0)), FieldMath.rgbToLab(doubleArrayOf(236.0, 126.0, 64.0)))
}

internal object FieldTune {
    const val OMEGA = 0.09
    const val SIGMA0 = 0.26
    const val GAMMA = 0.5
    const val A_NORM = 0.55
    const val SPILL = 1.6
    const val COLOUR_MS = 150.0
    const val CAP_LIGHT = 0.76
    const val CAP_DARK = 0.74
    const val WARP_SCALE = 1.9
    const val NOISE_RATE = 0.35
    const val DT_MAX = 100.0
    const val XF_MS = 180.0
    const val PULL_AMT = 0.65
    const val CONNECT_HIT_S = 0.12
    const val MARK_HIT_MS = 217.0
    /** hx, hy, ax, ay, wx, wy, px, py */
    val LJ = arrayOf(
        doubleArrayOf(-0.55, -0.50, 0.30, 0.34, 1.0000, 1.3247, 0.3, 1.9),
        doubleArrayOf(0.62, -0.20, 0.34, 0.28, 0.7071, 1.6180, 2.2, 0.7),
        doubleArrayOf(-0.30, 0.10, 0.28, 0.38, 1.4142, 0.8660, 4.1, 3.3),
        doubleArrayOf(0.50, 0.45, 0.36, 0.30, 1.1180, 1.3591, 1.0, 5.0),
        doubleArrayOf(-0.15, 0.80, 0.32, 0.32, 0.9069, 1.7321, 5.5, 2.4),
    )
}

/** Notes are: freq Hz, at s, dur s, peak, attack s. */
internal class FieldPattern(val periodMs: Double, val notes: Array<DoubleArray>)

private val PAT_RING = FieldPattern(3200.0, arrayOf(doubleArrayOf(784.0, 0.0, 1.0, 0.5, 0.02), doubleArrayOf(988.0, 0.18, 1.0, 0.45, 0.02), doubleArrayOf(1175.0, 0.36, 1.0, 0.4, 0.02), doubleArrayOf(1568.0, 0.7, 1.4, 0.3, 0.02)))
private val PAT_BACK = FieldPattern(4000.0, arrayOf(doubleArrayOf(392.0, 0.0, 1.2, 0.4, 0.18), doubleArrayOf(494.0, 0.0, 1.2, 0.35, 0.18)))
private val PAT_CONNECTED = FieldPattern(0.0, arrayOf(doubleArrayOf(523.0, 0.0, 0.5, 0.45, 0.02), doubleArrayOf(784.0, 0.12, 0.7, 0.45, 0.02)))
private val PAT_ENDED = FieldPattern(0.0, arrayOf(doubleArrayOf(784.0, 0.0, 0.45, 0.4, 0.02), doubleArrayOf(523.0, 0.12, 0.7, 0.4, 0.02)))
private val PAT_FAILED = FieldPattern(0.0, arrayOf(doubleArrayOf(247.0, 0.0, 0.5, 0.45, 0.02), doubleArrayOf(196.0, 0.24, 0.8, 0.45, 0.02)))
private val PAT_PULSE = FieldPattern(0.0, arrayOf(doubleArrayOf(330.0, 0.0, 0.6, 0.35, 0.02)))
private val PAT_WAITING = FieldPattern(3000.0, arrayOf(doubleArrayOf(784.0, 0.0, 0.16, 0.22, 0.012), doubleArrayOf(784.0, 0.22, 0.2, 0.22, 0.012)))

private fun noteLevel(n: DoubleArray, tau: Double): Double {
    val dur = n[2]; val peak = n[3]; val atk = n[4]
    if (tau <= 0.0 || tau >= dur) return 0.0
    if (tau < atk) return peak * tau / atk
    return peak * (0.0001 / peak).pow((tau - atk) / (dur - atk))
}
private fun pitchU(f: Double) = FieldMath.clamp(ln(f / FieldMath.F_MIN) / ln(FieldMath.F_MAX / FieldMath.F_MIN), 0.0, 1.0)

private class ActiveNote(val n: DoubleArray, val s: Double)
private fun activeNotes(p: FieldPattern, tauMs: Double, looped: Boolean): List<ActiveNote> {
    val out = ArrayList<ActiveNote>(4)
    if (tauMs < 0.0) return out
    val tau = tauMs / 1000.0
    val per = p.periodMs / 1000.0
    var maxEnd = -Double.MAX_VALUE
    for (n in p.notes) maxEnd = max(maxEnd, n[1] + n[2])
    val lp = looped && per != 0.0
    val kHi = if (lp) floor(tau / per).toLong() else 0L
    val kLo = if (lp) max(floor((tau - maxEnd) / per).toLong(), 0L) else 0L
    for (k in kLo..kHi) for (n in p.notes) {
        val s = tau - k * per - n[1]
        if (s > 0.0 && s < n[2]) out.add(ActiveNote(n, s))
    }
    return out
}

/** Mulberry32, bit-exact with the JS rng(). */
internal class FieldRng(private var a: Int) {
    fun next(): Double {
        a += 0x6D2B79F5
        val s = a
        var t = (s xor (s ushr 15)) * (1 or s)
        t = (t + (t xor (t ushr 7)) * (61 or t)) xor t
        return ((t xor (t ushr 14)).toLong() and 0xFFFFFFFFL).toDouble() / 4294967296.0
    }
}

private class Burst(val s: Double, val e: Double, val amp: Double, val ph: Double)
/** Simulated speech bursts (model.ts makeVoice). */
private class Voice(seed: Int) {
    private val r = FieldRng(seed)
    private val bursts = ArrayList<Burst>()
    private var t = 600.0
    fun at(ms: Double): Double {
        while (t < ms + 6000.0) {
            val d = 700.0 + r.next() * 1800.0
            val g = 500.0 + r.next() * 1500.0
            val amp = 0.55 + r.next() * 0.45
            val ph = r.next() * 6.28
            bursts.add(Burst(t, t + d, amp, ph))
            t += d + g
        }
        var v = 0.0
        val n = bursts.size
        var i = n - 1
        while (i >= 0 && i > n - 40) {
            val b = bursts[i]
            i -= 1
            if (ms < b.s) continue
            val up = FieldMath.smooth((ms - b.s) / 160.0)
            val rel = if (ms > b.e) exp(-(ms - b.e) / 260.0) else 1.0
            val syl = 0.78 + 0.22 * sin(ms / 1000.0 * 2.0 * PI * 3.6 + b.ph) * sin(ms / 1000.0 * 2.0 * PI * 0.9 + 1.0)
            v = max(v, b.amp * up * rel * syl)
            if (ms > b.e + 2000.0) break
        }
        return v
    }
}

internal enum class CallFieldState {
    Calm, Incoming, Outgoing, Connecting, Active, Waiting, Reconnecting, Ended, Failed;

    /** The cue pattern this state's notes come from (STATE_CUE in cues.ts). */
    internal fun pattern(): FieldPattern? = when (this) {
        Incoming -> PAT_RING
        Outgoing -> PAT_BACK
        Active -> PAT_CONNECTED
        Waiting -> PAT_WAITING
        Reconnecting -> PAT_PULSE
        Ended -> PAT_ENDED
        Failed -> PAT_FAILED
        Calm, Connecting -> null
    }
    /** How long after the state change the cue's first note sounds: connect waits so its landing note meets the string going taut. */
    fun cueDelayMs(reduced: Boolean): Double =
        if (this == Active && !reduced) max(FieldTune.MARK_HIT_MS - FieldTune.CONNECT_HIT_S * 1000.0, 0.0) else 0.0
    internal fun cfg(): FieldCfg = STATES[ordinal]
}

private const val NE = 9
private const val WARP = 2
private const val PULL = 3
private const val SPREAD = 4
private const val CHROMA = 5

/** Eased fields e, in this order: v, a0, warp, pull, spread, chroma, kappa, swell, ak. */
internal class FieldCfg(
    val cue: Boolean, val looped: Boolean, val e: DoubleArray, val bias: Double, val blend: Double, val voice: Boolean,
    /** delay, dur (ms) */
    val fade: DoubleArray?, tcs: Array<Pair<Int, Double>> = emptyArray(),
) {
    /** Per-field time-constant overrides (ms); 0 = use blend. */
    val tc = DoubleArray(NE).also { for ((i, v) in tcs) it[i] = v }
}

private val STATES: Array<FieldCfg> = arrayOf(
    FieldCfg(false, false, doubleArrayOf(0.50, 0.8, 0.08, 0.0, 0.0, 1.0, 1.0, 0.30, 0.45), 0.35, 900.0, false, null),
    FieldCfg(true, true, doubleArrayOf(1.00, 0.9, 0.12, 0.0, 0.0, 1.0, 2.6, 0.38, 1.0), 0.74, 300.0, false, null),
    FieldCfg(true, true, doubleArrayOf(0.42, 0.85, 0.12, 0.0, 0.0, 1.0, 2.2, 0.38, 1.1), 0.08, 450.0, false, null),
    FieldCfg(false, false, doubleArrayOf(0.80, 0.85, 0.012, 1.0, 0.0, 1.0, 1.6, 0.30, 0.7), 0.12, 350.0, false, null, arrayOf(WARP to 330.0, PULL to 520.0)),
    FieldCfg(true, false, doubleArrayOf(0.30, 0.6, 0.07, 0.0, 0.0, 1.0, 1.0, 0.30, 0.6), 0.30, 1200.0, true, null, arrayOf(PULL to 1400.0)),
    FieldCfg(true, true, doubleArrayOf(0.34, 0.65, 0.07, 0.0, 0.0, 1.0, 1.2, 0.32, 0.8), 0.52, 900.0, true, null),
    FieldCfg(true, false, doubleArrayOf(0.22, 0.55, 0.05, 0.5, 0.0, 0.45, 1.2, 0.30, 0.8), 0.20, 700.0, false, null, arrayOf(PULL to 900.0, CHROMA to 500.0)),
    FieldCfg(true, false, doubleArrayOf(0.60, 0.8, 0.10, 0.0, 0.55, 1.0, 1.6, 0.30, 0.6), 0.40, 300.0, false, doubleArrayOf(0.0, 300.0), arrayOf(SPREAD to 320.0)),
    FieldCfg(true, false, doubleArrayOf(0.30, 0.7, 0.05, 0.0, 0.0, 0.0, 1.2, 0.30, 0.6), 0.20, 500.0, false, doubleArrayOf(350.0, 650.0), arrayOf(CHROMA to 160.0)),
)

internal class FieldLobe(val x: Double, val y: Double, val sigma: Double, val alpha: Double, val lab: DoubleArray)
internal class FieldFrame(
    val tau: Double, val warp: Double, val cap: Double, val bg: DoubleArray, val lobes: List<FieldLobe>,
    /** False once an end/failed fade has finished: nothing moves any more. */
    val animating: Boolean,
    /** Reduced motion: when the next note lands (model ms), so a sleeping loop knows when to wake. */
    val wakeAt: Double?,
)

private class Cur(val id: Long, val state: CallFieldState, val t0: Double, val c0: Double, val pat: FieldPattern?)
private class Prev(val st: Cur, val at: Double, val ms: Double)
private class Disp(val alpha: DoubleArray, val u: DoubleArray, val chroma: Double)
private class Xf(val t0: Double, val from: Disp, val to: Disp)
private data class OnsetKey(val k: Long, val i: Int)
private class Onset(val t: Double, val key: OnsetKey, val freq: Double)

internal class CallField(private var theme: FieldTheme, reduced: Boolean) {
    private val K = FieldMath.K
    private var labs = fLabs(theme)
    var reduced = reduced; private set
    private var muteG = 1.0
    private var muted = false
    private val voice = Voice(7)
    /** w, h, avatar x, avatar y (px) */
    private var view = doubleArrayOf(390.0, 844.0, 195.0, 178.0)
    private var cur: Cur? = null
    private val prevs = ArrayList<Prev>()
    private var id = 0L
    private var s: DoubleArray? = null
    private var lu = DoubleArray(K) { 0.5 }
    private val tu = DoubleArray(K) { 0.5 }
    private val key = arrayOfNulls<Pair<Long, Long>>(K)
    private var tau = 0.0
    private var lastT: Double? = null
    private var fadeU = 1.0
    private var hasFrame = false
    private var xf: Xf? = null
    private var rKey: OnsetKey? = null
    private var rNoteU: Double? = null
    private var disp = Disp(DoubleArray(K), DoubleArray(K) { 0.5 }, 1.0)

    val state: CallFieldState? get() = cur?.state
    fun setView(w: Double, h: Double, avx: Double, avy: Double) { view = doubleArrayOf(w, h, avx, avy) }
    fun setMute(m: Boolean) { muted = m }
    fun setTheme(t: FieldTheme) { if (t != theme) { theme = t; labs = fLabs(t) } }
    fun setReduced(r: Boolean) {
        if (r == reduced) return
        reduced = r
        if (r) { rKey = null; rNoteU = null; xf = null }
        else cur?.let { lu = disp.u.copyOf(); s = it.state.cfg().e.copyOf() }
    }

    private fun paletteU(cfg: FieldCfg, i: Int) = FieldMath.clamp(cfg.bias + (i - 2.0) * 0.07, 0.0, 1.0)
    private fun curve(u: Double): Double {
        val x = FieldMath.smooth(FieldMath.clamp((u - 0.2) / 0.75, 0.0, 1.0))
        return if (theme == FieldTheme.Dark) FieldMath.smooth(FieldMath.smooth(x)) else x
    }
    private fun colourAt(u: Double, chroma: Double): DoubleArray {
        val c = FieldMath.mixLab(labs.teal, labs.gold, curve(u))
        return doubleArrayOf(c[0], c[1] * chroma, c[2] * chroma)
    }
    private fun halfExt(): DoubleArray { val sh = min(view[0], view[1]); return doubleArrayOf(view[0] / sh / 2.0, view[1] / sh / 2.0) }
    private fun avatarPt(): DoubleArray { val sh = min(view[0], view[1]); return doubleArrayOf((view[2] - view[0] / 2.0) / sh, (view[3] - view[1] / 2.0) / sh) }
    private fun lobePos(i: Int, tau: Double, h: DoubleArray, pull: Double, spread: Double): DoubleArray {
        val j = FieldTune.LJ[i]
        val sx = max(h[0] / 0.5, 1.0); val sy = max(h[1] / 0.5, 1.0)
        var x = j[0] * h[0] + j[2] * sx * sin(j[4] * FieldTune.OMEGA * tau + j[6])
        var y = j[1] * h[1] + j[3] * sy * sin(j[5] * FieldTune.OMEGA * tau + j[7])
        if (pull > 0.0) {
            val a = avatarPt(); val g = pull * FieldTune.PULL_AMT
            x = FieldMath.lerp(x, a[0], g); y = FieldMath.lerp(y, a[1], g)
        }
        if (spread != 0.0) { x *= 1.0 + spread; y *= 1.0 + spread }
        return doubleArrayOf(x, y)
    }
    private fun cap() = if (theme == FieldTheme.Dark) FieldTune.CAP_DARK else FieldTune.CAP_LIGHT

    /** [cueStart]: absolute model time (ms) of the cue's first note; null anchors it on the state change. */
    fun setState(state: CallFieldState, t: Double, cueStart: Double? = null) {
        val cfg = state.cfg()
        cur?.let { prevs.add(Prev(it, t, 220.0)) }
        id += 1
        cur = Cur(id, state, t, cueStart ?: t, state.pattern())
        if (s == null) s = cfg.e.copyOf()
        for (i in 0 until K) {
            tu[i] = paletteU(cfg, i)
            key[i] = null
            if (!hasFrame) lu[i] = tu[i]
        }
        if (lastT == null) lastT = t
        rKey = null; rNoteU = null
        if (reduced) startXf(t)
    }

    // ---- reduced motion: static gradient, per-note colour crossfade ----
    private fun staticTarget(noteU: Double?): Disp {
        val cfg = cur!!.state.cfg()
        val a = if (cfg.fade != null) 0.0 else cfg.e[1]
        val u = DoubleArray(K) { i -> val p = paletteU(cfg, i); if (noteU == null) p else FieldMath.lerp(p, noteU, 0.75) }
        return Disp(DoubleArray(K) { a }, u, cfg.e[CHROMA])
    }
    private fun startXf(t0: Double) { xf = Xf(t0, disp, staticTarget(rNoteU)) }

    /** Latest onset <= t and next onset > t of a cue started at c0. */
    private fun scanOnsets(p: FieldPattern?, looped: Boolean, c0: Double, t: Double): Pair<Onset?, Double?> {
        if (p == null) return null to null
        val per = p.periodMs
        val lp = looped && per != 0.0
        val lo = if (lp) floor((t - c0) / per).toLong() else 0L
        var latest: Onset? = null
        var next: Double? = null
        val hi = if (lp) lo + 1 else 0L
        for (k in max(lo - 1, 0L)..hi) for ((i, n) in p.notes.withIndex()) {
            val ot = c0 + k * per + n[1] * 1000.0
            if (ot <= t) {
                if (latest == null || ot >= latest.t) latest = Onset(ot, OnsetKey(k, i), n[0])
            } else if (next == null || ot < next) next = ot
        }
        return latest to next
    }
    private fun reducedFrame(t: Double): FieldFrame {
        val cu = cur!!
        val cfg = cu.state.cfg()
        val (lt, next) = scanOnsets(cu.pat, cfg.looped, cu.c0, t)
        if (cfg.cue && lt != null && cu.pat != null && lt.key != rKey) {
            rKey = lt.key
            rNoteU = pitchU(lt.freq)
            startXf(max(lt.t, t - FieldTune.XF_MS))
        }
        if (xf == null) startXf(t - FieldTune.XF_MS)
        val x = xf!!
        val e = FieldMath.smooth((t - x.t0) / FieldTune.XF_MS)
        val alpha = DoubleArray(K) { FieldMath.lerp(x.from.alpha[it], x.to.alpha[it], e) }
        val u = DoubleArray(K) { FieldMath.lerp(x.from.u[it], x.to.u[it], e) }
        val chroma = FieldMath.lerp(x.from.chroma, x.to.chroma, e)
        disp = Disp(alpha, u, chroma)
        val h = halfExt()
        val lobes = List(K) { i ->
            val p = lobePos(i, 7.3, h, 0.0, 0.0)
            FieldLobe(p[0], p[1], FieldTune.SIGMA0, alpha[i], colourAt(u[i], chroma))
        }
        val animating = e < 1.0
        return FieldFrame(7.3, 0.0, cap(), labs.bg, lobes, animating, if (animating) null else next)
    }

    private fun addNotes(st: Cur, t: Double, k: Double, lraw: DoubleArray, mark: Array<Pair<Long, Double>?>?) {
        val cfg = st.state.cfg()
        val pat = st.pat ?: return
        if (!cfg.cue || k <= 0.0) return
        for (an in activeNotes(pat, t - st.c0, cfg.looped)) {
            val n = an.n
            val e = noteLevel(n, an.s)
            val pk = n[3]
            val lv = pk * (e / pk).pow(FieldTune.GAMMA) * k
            val uu = pitchU(n[0]) * K
            for (i in 0 until K) {
                val w = max(1.0 - abs(uu - (i + 0.5)) / FieldTune.SPILL, 0.0)
                if (w <= 0.0) continue
                lraw[i] += lv * w
                if (mark != null && w >= 0.3) {
                    val onset = FieldMath.jsRound(t - an.s * 1000.0).toLong()
                    val p = mark[i]
                    if (p == null || onset >= p.first) mark[i] = onset to n[0]
                }
            }
        }
    }

    /** Advance to model time [t] (ms) and return the frame to draw. */
    fun update(t: Double): FieldFrame {
        val cu = cur ?: error("setState first")
        val l0 = lastT
        val dt = if (l0 == null) 0.0 else FieldMath.clamp(t - l0, 0.0, FieldTune.DT_MAX)
        lastT = t
        muteG += ((if (muted) 0.0 else 1.0) - muteG) * (1.0 - exp(-dt / 120.0))
        if (reduced) { val f = reducedFrame(t); hasFrame = true; return f }
        val cfg = cu.state.cfg()
        val sv = s ?: cfg.e.copyOf().also { s = it }
        for (k in 0 until NE) {
            val tc = (if (cfg.tc[k] != 0.0) cfg.tc[k] else cfg.blend) / 3.2
            sv[k] += (cfg.e[k] - sv[k]) * (1.0 - exp(-dt / tc))
        }
        val lraw = DoubleArray(K)
        val mark = arrayOfNulls<Pair<Long, Double>>(K)
        addNotes(cu, t, 1.0, lraw, mark)
        prevs.removeAll { t - it.at >= it.ms }
        for (p in prevs.toList()) addNotes(p.st, t, 1.0 - FieldMath.smooth((t - p.at) / p.ms), lraw, null)
        val l = DoubleArray(K) { tanh(lraw[it] / FieldTune.A_NORM) }
        val e = l.sum()
        val ctc = max(FieldTune.COLOUR_MS, cfg.blend / 4.0)
        for (i in 0 until K) {
            mark[i]?.let { m ->
                val k2 = cu.id to m.first
                if (key[i] != k2) { key[i] = k2; tu[i] = pitchU(m.second) }
            }
            lu[i] += (tu[i] - lu[i]) * (1.0 - exp(-dt / ctc))
        }
        val since = t - cu.t0
        val vo = if (cfg.voice) voice.at(since) * muteG else 0.0
        tau += dt / 1000.0 * sv[0] * (1.0 + sv[6] * e + 0.5 * vo)
        val fd = cfg.fade
        val inFade = fd != null && since >= fd[0]
        val step = if (inFade) -dt / fd[1] else dt / 400.0
        fadeU = FieldMath.clamp(fadeU + step, 0.0, 1.0)
        val fade = FieldMath.smooth(fadeU)
        val h = halfExt()
        val lobes = List(K) { i ->
            val p = lobePos(i, tau, h, sv[PULL], sv[SPREAD])
            val sigma = FieldTune.SIGMA0 * (1.0 + sv[7] * l[i] + 0.06 * vo) * (1.0 - 0.3 * sv[PULL])
            val alpha = (sv[1] + 0.07 * vo + sv[8] * l[i]) * fade
            FieldLobe(p[0], p[1], sigma, alpha, colourAt(lu[i], sv[CHROMA]))
        }
        disp = Disp(DoubleArray(K) { lobes[it].alpha }, lu.copyOf(), sv[CHROMA])
        val animating = !(cfg.fade != null && fadeU <= 0.0)
        hasFrame = true
        return FieldFrame(tau, sv[WARP], cap(), labs.bg, lobes, animating, null)
    }
}
