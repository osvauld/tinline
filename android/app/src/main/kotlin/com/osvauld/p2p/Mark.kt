package com.osvauld.p2p

import android.provider.Settings
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlin.math.abs
import kotlin.math.asinh
import kotlin.math.ceil
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.max
import kotlin.math.min
import kotlin.math.round
import kotlin.math.sin
import kotlin.math.sqrt

/*
 * The brand mark D, ported from docs/design/playground/src/lib/brand/{tinmark,tiers}.js.
 * Same constants, same model: a fixed-length quadratic Bezier string between two cans, damped springs that
 * retarget mid-flight (never keyframes), string length conserved. Geometry is in the 24-unit model space,
 * cropped per optical-size tier. Change the numbers there first, then here.
 */

enum class MarkState { Slack, Ringing, Taut }

/** App-wide fx bus: a sent message makes every visible live mark pluck, quietly (playground: fx 'sent'). */
/** In-app palette of Mark.svelte: can = --accent (dark theme #117e6b, not the mint text colour pr), mouth always #9fd6c8, string = --gold (Tin.c.thread). */
object MarkColors { val CanDark = Color(0xFF117E6B); val Mouth = Color(0xFF9FD6C8) }

object TinFx {
    val sent = MutableSharedFlow<Unit>(extraBufferCapacity = 1)
    fun messageSent() { sent.tryEmit(Unit) }
}

private object MarkDef {
    const val C = 7.5; const val B = 5.5; const val THETA = 30.6; const val MU = 0.62; const val L = 3.0; const val S0 = 3.5
    const val STROKE = 1.5; const val PHI0 = 12.0; const val T = 1.6; const val A = 0.5; const val PLUCK_A = 0.32; const val PLUCK_SWING = 150.0
    const val F = 7.0; const val TAU = 0.11; const val CONNECT_HZ = 3.0; const val END_ZETA = 1.6; const val END_MS = 260.0
}

private fun rad(d: Double) = d * Math.PI / 180

/** Offset of the `line` cue's second note (s): the two tugs land this far apart (Cues.kt LINE). */
const val LINE_GAP_S = 0.12

/**
 * Per-use spring parameters of the one model (tinmark.js DEF and overrides, MARK-MATH.md "Connect", 2026-10-13). Defaults are
 * the everyday pull: it arrives with speed ([hitOver]), the cans bounce back off the tight string ([recoil] = restitution), and an
 * undamped [settleHz] pull carries them out again until a bounce is below [recoilMin]. [tugs] > 0 tugs exactly that many times.
 */
data class MarkSpring(
    val hitOver: Double = 1.6, val recoil: Double = 0.5, val recoilMin: Double = 1.2, val settleHz: Double = 2.0,
    val windup: Double = 0.0, val windupMs: Double = 180.0, val tugs: Int = 0,
) {
    companion object {
        /** The old dead stop (recoil 0). */
        val DeadStop = MarkSpring(hitOver = 1.1, recoil = 0.0)
        /** springs.ts LINE_SPRING: two tugs on the rhythm of the `line` cue (sign-up Done, the first contact). */
        val Line = MarkSpring(tugs = 2, recoil = 0.95, recoilMin = 0.0, settleHz = 1 / (2 * LINE_GAP_S))
    }
}

private fun lenExact(w: Double, s0: Double): Double {
    val s = abs(s0); if (s < 1e-9) return w
    return 0.5 * sqrt(w * w + 16 * s * s) + (w * w / (8 * s)) * asinh(4 * s / w)
}

private fun sOfW(w: Double, ell: Double): Double {
    if (w >= ell) return 0.0
    val sa = sqrt(3 * w * (ell - w) / 8)
    var lo = 0.0; var hi = max(sa * 1.5, 1e-6)
    while (lenExact(w, hi) < ell) hi *= 2
    repeat(40) { val m = (lo + hi) / 2; if (lenExact(w, m) < ell) lo = m else hi = m }
    return (lo + hi) / 2
}

/** Tier parameters in model units (tiers.js tierModel). */
internal class MarkTier(
    val c: Double, val ko: Double, val a: Double, val b: Double, val len: Double, val mu: Double, val sw: Double, val s0: Double,
    val vb: Double, val vbSize: Double, val sagOnly: Boolean, val sT: Double, val hasMouth: Boolean, val flat: Boolean,
) {
    val w0 = 24 - 2 * c
    val ell = lenExact(w0, s0)
    val dt = max(0.0, (ell - w0) / 2)
}

private class Small(val c: Double, val a: Double, val b: Double, val l: Double, val mu: Double, val flat: Boolean, val ko: Double, val sw: Double, val s: Double, val sT: Double?)

private val SMALL = mapOf(
    12 to Small(3.5, 1.5, 4.0, 1.5, 0.5, true, 0.75, 1.5, 2.5, 0.5),
    14 to Small(4.5, 1.5, 5.0, 2.0, 0.5, true, 0.75, 1.5, 3.0, 0.5),
    16 to Small(5.0, 2.0, 5.0, 2.0, 0.5, true, 1.25, 1.75, 3.0, 0.5),
    20 to Small(6.0, 2.5, 6.0, 2.5, 0.5, true, 1.25, 1.75, 3.5, 0.75),
    24 to Small(7.5, 3.0, 6.0, 3.0, 0.6, false, 0.0, 1.75, 3.5, null),
    28 to Small(8.5, 3.5, 7.0, 3.5, 0.6, false, 0.0, 1.75, 4.0, null),
)
private val TIER_SIZES = intArrayOf(12, 14, 16, 20, 24, 28, 32, 40, 48, 64, 96, 128, 256, 512, 1024)

private fun pickTier(px: Double): Int {
    var best = TIER_SIZES[0]
    for (t in TIER_SIZES) if (abs(t - px) < abs(best - px) || (abs(t - px) == abs(best - px) && t < best)) best = t
    return if (px > 1024) 1024 else best
}

private fun half(v: Double) = round(v * 2) / 2

private fun tierFor(px: Double): MarkTier {
    val bx = pickTier(px)
    val sm = SMALL[bx]
    val u: Double; val c: Double; val a: Double; val b: Double; val l: Double; val mu: Double; val sw: Double; val s: Double
    val sT: Double?; val ko: Double; val flat: Boolean
    if (sm != null) {
        u = 1.0; c = sm.c; a = sm.a; b = sm.b; l = sm.l; mu = sm.mu; sw = sm.sw; s = sm.s; sT = sm.sT; ko = sm.ko; flat = sm.flat
    } else {
        u = bx / 24.0
        val exact = bx >= 96
        fun sn(v: Double, f: (Double) -> Double) = if (exact) v else f(v)
        b = sn(MarkDef.B * u) { round(it) }
        a = sn(MarkDef.B * sin(rad(MarkDef.THETA)) * u) { half(it) }
        c = sn(MarkDef.C * u) { half(it) }; l = sn(MarkDef.L * u) { half(it) }; mu = MarkDef.MU
        sw = max(1.5, sn(MarkDef.STROKE * u) { half(it) }); s = MarkDef.S0 * u; sT = null; ko = 0.0; flat = false
    }
    val half0 = bx / (2 * u)
    return MarkTier(
        c = 12 - (bx / 2.0 - c) / u, ko = ko / u, a = a / u, b = b / u, len = l / u, mu = mu, sw = sw / u, s0 = s / u,
        vb = 12 - half0, vbSize = bx / u, sagOnly = sT != null, sT = (sT ?: 0.0) / u, hasMouth = mu != 0.0, flat = flat,
    )
}

/** The spring model (tinmark.js attach). Pure state: step() advances it, no clocks or UI in here. */
internal class MarkModel(private val p: MarkTier, var spring: MarkSpring = MarkSpring()) {
    var d = 0.0; private var v = 0.0; var phi = 0.0; private var phv = 0.0
    private var tugsNow = 0; private var v0 = 0.0; private var wind = 0.0
    /** Every tug so far (the widget reports each new one); [tight]: the string went tight since the last slack. */
    var tugCount = 0; var tight = false
    private var ring = false; private var ringPhase = 0.0
    private var taut = false; private var hit = false
    private var tw = -1.0; private var twA = 0.0
    private var swayT = MarkDef.T; private var swayAmp = MarkDef.PHI0
    fun setSway(period: Double, amp: Double) { swayT = period; swayAmp = amp }

    private val endOmega: Double = run {
        val z = MarkDef.END_ZETA; val k = z + sqrt(z * z - 1); val m = z - sqrt(z * z - 1)
        val thr = 0.01 / max(p.dt, 1e-6)
        fun x(u: Double) = (k * exp(-m * u) - m * exp(-k * u)) / (k - m)
        var lo = 0.0; var hi = 200.0
        repeat(60) { val mid = (lo + hi) / 2; if (x(mid) > thr) lo = mid else hi = mid }
        hi / (MarkDef.END_MS / 1000)
    }
    private val connectW = 2 * Math.PI * MarkDef.CONNECT_HZ

    val active: Boolean get() =
        ring || tw >= 0 || wind > 0 || (if (taut) d < p.dt || v != 0.0 else d >= 0.01) || abs(phi) > rad(0.2) || abs(phv) > rad(2.0)

    private fun sag(): Double =
        if (p.sagOnly) p.s0 + (p.sT - p.s0) * (if (p.dt > 0) min(1.0, d / p.dt) else 0.0)
        else sOfW(24 - 2 * (p.c - d), p.ell)

    private fun twang(): Double =
        if (tw < 0) 0.0 else (if (twA != 0.0) twA else MarkDef.A) * exp(-tw / MarkDef.TAU) * sin(2 * Math.PI * MarkDef.F * tw)

    val sagNow: Double get() = sag() + twang()
    /** Outward can displacement; small tiers keep the cans still. */
    val delta: Double get() = if (p.sagOnly) 0.0 else d

    fun step(h: Double) {
        val sp = spring
        val z: Double; val w: Double; val tgt: Double
        when {
            wind > 0 -> { wind -= h; z = 1.0; w = 2 * Math.PI / (sp.windupMs / 1000) * 1.2; tgt = -sp.windup }
            taut && hit -> { z = 0.0; w = 2 * Math.PI * sp.settleHz; tgt = p.dt }   // the string pulls the recoiling cans back out
            taut -> { z = 1.0; w = connectW; tgt = p.dt * sp.hitOver }
            else -> { z = MarkDef.END_ZETA; w = endOmega; tgt = 0.0 }
        }
        v += (-2 * z * w * v - w * w * (d - tgt)) * h; d += v * h
        if (taut && d >= p.dt && v <= 0) d = p.dt   // resting at taut is not a tug (tinmark.js re-plucks here every frame)
        else if (taut && d >= p.dt) {
            // The string goes tight and tugs the cans back: velocity reflected (restitution [recoil]); each bounce re-plucks.
            val vin = v; d = p.dt; tugsNow++
            val last = sp.tugs > 0 && tugsNow >= sp.tugs
            v = if (!last && sp.recoil > 0 && vin * sp.recoil >= sp.recoilMin) -vin * sp.recoil else 0.0
            if (!hit) { hit = true; tw = 0.0; twA = 0.0 }
            else if (sp.tugs > 0) { tw = 0.0; twA = MarkDef.A * 0.8 }
            else if (vin > sp.recoilMin) { tw = 0.0; twA = min(MarkDef.A, MarkDef.A * vin / (if (v0 > 0) v0 else vin)) * 0.6 }
            tugCount++; tight = true
            if (v0 == 0.0) v0 = vin
        }
        if (d < -sp.windup) { d = -sp.windup; if (v < 0) v = 0.0 }
        if (d < 0 && wind <= 0 && !taut) { d = 0.0; if (v < 0) v = 0.0 }
        if (ring) {
            ringPhase += 2 * Math.PI / swayT * h
            val tgtPhi = rad(swayAmp) * sin(ringPhase); val k = 1 - exp(-h / 0.05)
            val old = phi; phi += (tgtPhi - phi) * k; phv = (phi - old) / h
        } else {
            val zz = if (taut) 1.0 else 0.5; val ww = if (taut) connectW else 2 * Math.PI * 3.5
            phv += (-2 * zz * ww * phv - ww * ww * phi) * h; phi += phv * h
        }
        if (tw >= 0) { tw += h; if ((if (twA != 0.0) twA else MarkDef.A) * exp(-tw / MarkDef.TAU) < 0.03) tw = -1.0 }
    }

    /** Advance by real [dt] seconds in 1/480 s sub-steps; returns false once everything has settled. */
    fun advance(dt0: Double): Boolean {
        val dt = min(dt0, 0.05)
        val n = max(1, ceil(dt / (1.0 / 480)).toInt()); val h = dt / n
        repeat(n) { step(h) }
        if (!active) { finish(); return false }
        return true
    }

    private fun finish() { if (!taut) { d = 0.0; v = 0.0 }; phi = 0.0; phv = 0.0; tw = -1.0 }

    /** Reduced motion: jump to the end pose. */
    fun snap(toTaut: Boolean) {
        val fresh = toTaut && !hit
        ring = false; taut = toTaut; d = if (toTaut) p.dt else 0.0; v = 0.0; phi = 0.0; phv = 0.0; tw = -1.0; hit = toTaut; wind = 0.0; tight = toTaut
        if (fresh) tugCount++
    }

    /** Mount pose: taut without counting a tug (a mark that mounts taut doesn't replay it). */
    fun set(state: MarkState) { snap(state == MarkState.Taut); tugCount = 0 }

    fun ring(on: Boolean) { ring = on; if (on) { taut = false; hit = false } }
    fun connect() {
        ring = false; taut = true; hit = hit && d >= p.dt; v0 = 0.0; tugsNow = 0
        wind = if (spring.windup > 0 && d < 0.01) spring.windupMs / 1000 else 0.0
    }
    fun end() { ring = false; taut = false; hit = false; tight = false; wind = 0.0; tw = -1.0; if (abs(phi) < rad(1.0)) phv += rad(242.0) }
    fun pluck() {
        if (taut && d >= p.dt - 1e-9) { if (tw < 0) { twA = MarkDef.PLUCK_A; tw = 0.0 } }
        else if (abs(phi) < rad(1.0) && abs(phv) < rad(20.0)) phv += rad(MarkDef.PLUCK_SWING)
    }
}

private fun reducedMotion(c: android.content.Context): Boolean =
    Settings.Global.getFloat(c.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f

/** Draws one can in device pixels (no canvas transform: Skia flattens curves before scaling, which polylines tiny model-space paths). [m] mirrors about x = 12. */
private fun DrawScope.drawCan(p: MarkTier, cx: Double, k: Float, can: Color, mouth: Color, m: Boolean) {
    fun X(v: Double): Float = ((if (m) 24 - v else v) - p.vb).toFloat() * k
    fun Y(v: Double): Float = (v - p.vb).toFloat() * k
    val ym = 12 - p.b; val yp = 12 + p.b
    val ax = (p.a * k).toFloat(); val bx = (p.b * k).toFloat()
    // body: flat top and bottom behind the rim, half-ellipse back (left in the unmirrored frame)
    val back = cx - p.len
    val body = Path().apply {
        moveTo(X(cx), Y(ym)); lineTo(X(back), Y(ym))
        val rx = X(back); val ry = Y(12.0)
        if (m) arcTo(Rect(rx - ax, ry - bx, rx + ax, ry + bx), 270f, 180f, false)
        else arcTo(Rect(rx - ax, ry - bx, rx + ax, ry + bx), 270f, -180f, false)
        lineTo(X(cx), Y(yp)); close()
    }
    drawPath(body, can)
    drawOval(can, Offset(X(cx) - ax, Y(12.0) - bx), androidx.compose.ui.geometry.Size(2 * ax, 2 * bx))
    if (p.hasMouth) {
        val mu = p.mu.toFloat()
        drawOval(mouth, Offset(X(cx) - mu * ax, Y(12.0) - mu * bx), androidx.compose.ui.geometry.Size(2 * mu * ax, 2 * mu * bx))
    }
}

/**
 * The brand mark: two cans, one string. [can]/[string]/[mouth] are the palette (mouth defaults to the playground's
 * hole colour for the can colour). With [live] the mark animates: [state] changes retarget the springs, and a sent
 * message plucks it (TinFx). [size] picks the optical tier; call sites keep working unchanged (static slack pose).
 */
@Composable
fun TinMark(
    size: Dp, can: Color = if (Tin.c.dark) MarkColors.CanDark else Tin.c.pr, string: Color = Tin.c.thread, modifier: Modifier = Modifier,
    mouth: Color? = null, state: MarkState = MarkState.Slack, live: Boolean = false,
    /** Ringing sway: period (s) and amplitude (deg); lock the period to the cue that is playing. */
    sway: Sway? = null, dim: Boolean = false, description: String? = null,
    /** Spring parameters for this use (Done's two tugs); calls keep the default. */
    spring: MarkSpring = MarkSpring(),
    /** Sign-up and first-screen marks: a slack line is faded gold (gold 55 % into --line) and warms to gold on the first tug. */
    lineColor: Boolean = false,
    /** Each tug of the string going tight (1, 2, …): sign-up Done plays the `line` cue on the first. */
    onTug: ((Int) -> Unit)? = null,
    // No glow (decided 2026-10-13): dark-mode depth is the rim only, sign-up included.
) {
    val ctx = LocalContext.current
    val density = androidx.compose.ui.platform.LocalDensity.current
    val tier = remember(size, density) { tierFor(with(density) { size.value.toDouble() }) }
    val model = remember(tier) { MarkModel(tier, spring) }
    model.spring = spring
    val told = remember(model) { intArrayOf(0) }
    var tight by remember(model) { mutableStateOf(false) }
    val warm by androidx.compose.animation.core.animateFloatAsState(if (tight) 1f else 0f,
        androidx.compose.animation.core.tween(if (tight && !reducedMotion(ctx)) PgMotion.SWITCH else 0, easing = PgMotion.easeOut), label = "line")
    var frame by remember { mutableIntStateOf(0) }
    var kick by remember { mutableIntStateOf(0) }
    val reduced = reducedMotion(ctx)
    val hole = mouth ?: MarkColors.Mouth
    val slackStr = androidx.compose.ui.graphics.lerp(string, Tin.c.ln, 0.45f)
    val str = when { dim -> Tin.c.ink2; lineColor -> androidx.compose.ui.graphics.lerp(slackStr, string, warm); else -> string }
    // report tugs and the line's warmth after every model change
    fun report() {
        if (model.tugCount > told[0]) { val n = model.tugCount; told[0] = n; onTug?.invoke(n) }
        tight = model.tight
    }
    LaunchedEffect(model, sway) { if (sway != null) model.setSway(sway.period, sway.amp) }

    // state -> springs. The first composition sets the pose; later changes retarget mid-flight.
    val appliedBox = remember(model) { arrayOfNulls<MarkState>(1) }
    LaunchedEffect(model, state, live, reduced) {
        if (!live || reduced) {
            val first = appliedBox[0] == null
            if (first) model.set(state) else model.snap(state == MarkState.Taut)
            appliedBox[0] = state
            if (first) told[0] = model.tugCount
            report(); frame++; return@LaunchedEffect
        }
        val prev = appliedBox[0]
        if (prev == null) {
            model.set(if (state == MarkState.Taut) MarkState.Taut else MarkState.Slack)
            if (state == MarkState.Ringing) model.ring(true)
            told[0] = model.tugCount
            tight = model.tight
        } else if (prev != state) when (state) {
            MarkState.Taut -> model.connect()
            MarkState.Ringing -> model.ring(true)
            MarkState.Slack -> { model.ring(false); model.end() }
        }
        appliedBox[0] = state
        kick++
    }
    if (live && !reduced) {
        LaunchedEffect(model) { TinFx.sent.collect { model.pluck(); kick++ } }
        LaunchedEffect(model, kick) {
            if (!model.active) return@LaunchedEffect
            var last = androidx.compose.runtime.withFrameNanos { it }
            while (true) {
                val now = androidx.compose.runtime.withFrameNanos { it }
                val more = model.advance((now - last) / 1e9); last = now
                report()
                frame++
                if (!more) break
            }
        }
    }

    val sem = if (description != null) Modifier.semantics { contentDescription = description } else Modifier
    fun DrawScope.paint(can: Color, hole: Color, str: Color, w: Float = this.size.width) {
        val k = w / tier.vbSize.toFloat()
        val cx = tier.c - model.delta
        drawCan(tier, cx, k, can, hole, false)
        drawCan(tier, cx, k, can, hole, true)
        val x1 = cx + tier.ko; val x2 = 24 - x1; val sg = model.sagNow; val phi = model.phi
        fun X(v: Double) = (v - tier.vb).toFloat() * k
        val path = Path().apply {
            moveTo(X(x1), X(12.0))
            quadraticTo(X(12 + 2 * sg * sin(phi)), X(12 + 2 * sg * cos(phi)), X(x2), X(12.0))
        }
        drawPath(path, str, style = Stroke(tier.sw.toFloat() * k, cap = StrokeCap.Round, join = StrokeJoin.Round))
    }
    androidx.compose.foundation.layout.Box(modifier.size(size)) {
        Canvas(Modifier.size(size).then(if (dim) Modifier.alpha(0.45f) else Modifier).then(sem)) {
            frame // observe
            paint(can, hole, str)
        }
    }
}

/** Ringing sway: period in seconds, amplitude in degrees. */
data class Sway(val period: Double, val amp: Double)

/** What the app-bar mark shows (status.svelte.ts MarkLook). */
data class MarkLook(val state: MarkState, val sway: Sway? = null, val dim: Boolean = false, @androidx.annotation.StringRes val labelRes: Int) {
    /** The spoken description, resolved when read (so JVM tests that only look at [state] never need resources). */
    val label: String get() = appString(labelRes)
}

enum class CallPhase { None, Incoming, Outgoing, Connecting, Active, Reconnecting }
enum class GroupCallPhase { None, Starting, Connecting, Active, Reconnecting }
enum class ConnState { Offline, Connecting, Online }

/**
 * Port of statusLook(): call phase wins, then group call phase, then the network connection.
 * Sway periods are locked to the cue patterns: ringtone every 3.2 s (two swings per ring), ringback every 4 s.
 */
fun statusLook(call: CallPhase = CallPhase.None, gcall: GroupCallPhase = GroupCallPhase.None, conn: ConnState): MarkLook {
    when (call) {
        CallPhase.Incoming -> return MarkLook(MarkState.Ringing, Sway(1.6, 12.0), labelRes = R.string.mark_incoming_call)
        CallPhase.Outgoing -> return MarkLook(MarkState.Ringing, Sway(2.0, 9.0), labelRes = R.string.mark_calling)
        CallPhase.Connecting -> return MarkLook(MarkState.Ringing, Sway(1.2, 6.0), labelRes = R.string.mark_connecting)
        CallPhase.Active -> return MarkLook(MarkState.Taut, labelRes = R.string.mark_in_call)
        CallPhase.Reconnecting -> return MarkLook(MarkState.Ringing, Sway(2.0, 4.0), labelRes = R.string.mark_reconnecting)
        CallPhase.None -> {}
    }
    when (gcall) {
        GroupCallPhase.Starting -> return MarkLook(MarkState.Ringing, Sway(2.0, 9.0), labelRes = R.string.mark_starting_group_call)
        GroupCallPhase.Connecting -> return MarkLook(MarkState.Ringing, Sway(1.2, 6.0), labelRes = R.string.mark_connecting)
        GroupCallPhase.Active -> return MarkLook(MarkState.Taut, labelRes = R.string.mark_in_group_call)
        GroupCallPhase.Reconnecting -> return MarkLook(MarkState.Ringing, Sway(2.0, 4.0), labelRes = R.string.mark_reconnecting)
        GroupCallPhase.None -> {}
    }
    return when (conn) {
        ConnState.Offline -> MarkLook(MarkState.Slack, dim = true, labelRes = R.string.mark_no_connection)
        ConnState.Connecting -> MarkLook(MarkState.Ringing, Sway(3.2, 5.0), labelRes = R.string.mark_connecting)
        ConnState.Online -> MarkLook(MarkState.Taut, labelRes = R.string.mark_connected)
    }
}

/** A model at a rendered size (tests). */
internal fun markModelFor(px: Double, spring: MarkSpring = MarkSpring()) = MarkModel(tierFor(px), spring)
