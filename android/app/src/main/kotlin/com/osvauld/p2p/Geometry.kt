package com.osvauld.p2p

import androidx.compose.runtime.Immutable
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin
import kotlin.math.sqrt
import kotlin.math.tan

/**
 * Corner geometry, a port of docs/design/playground/src/lib/geom/shape.ts (see GEOMETRY.md there). Keep the two identical:
 * [SmoothGeom.svg] must produce the strings shape.ts produces (GeometryTest compares them).
 * Lines and cubic Béziers only, Figma-style corner smoothing; one-line bubbles degrade to a pill automatically.
 */
object SmoothGeom {
    /** Default corner smoothing (`--smooth` in tokens.css). */
    const val SMOOTH = 0.6

    /** One path command: 'M' / 'L' (1 point) or 'C' (3 points) or 'Z' (none). Coordinates are x0,y0,x1,y1,... */
    class Seg(val op: Char, val p: DoubleArray)

    private const val RAD = Math.PI / 180

    class Corner(val r: Double, val s: Double, val p: Double, val a: Double, val b: Double, val c: Double, val d: Double, val arc: Double, val alpha: Double, val h: Double)

    private val NONE = Corner(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0)

    fun cornerParams(radius: Double, smoothing: Double, budget: Double = Double.POSITIVE_INFINITY): Corner {
        var r = max(0.0, radius)
        var s = min(1.0, max(0.0, smoothing))
        if (r == 0.0) return NONE
        if ((1 + s) * r > budget) {
            s = min(s, max(0.0, budget / r - 1))
            if (r > budget) { r = budget; s = 0.0 }
        }
        val p = (1 + s) * r
        val arcMeasure = 90 * (1 - s)
        val arc = sin((arcMeasure / 2) * RAD) * r * sqrt(2.0)
        val alpha = (45 * s) * RAD
        val p3p4 = r * tan(((90 - arcMeasure) / 4) * RAD)
        val c = p3p4 * cos(alpha)
        val d = c * tan(alpha)
        val b = (p - arc - c - d) / 3
        val a = 2 * b
        val h = (4.0 / 3) * tan((arcMeasure / 4) * RAD) * r
        return Corner(r, s, p, a, b, c, d, arc, alpha, h)
    }

    private fun budgets(w: Double, h: Double, rad: DoubleArray, s: Double): DoubleArray {
        val (tl, tr, br, bl) = rad.toList()
        fun ext(r: Double) = (1 + s) * r
        fun side(len: Double, ra: Double, rb: Double): DoubleArray {
            val need = ext(ra) + ext(rb)
            if (need <= len || need == 0.0) return doubleArrayOf(Double.POSITIVE_INFINITY, Double.POSITIVE_INFINITY)
            return doubleArrayOf(len * ra / (ra + rb), len * rb / (ra + rb))
        }
        val top = side(w, tl, tr); val right = side(h, tr, br); val bottom = side(w, br, bl); val left = side(h, bl, tl)
        return doubleArrayOf(min(top[0], left[1]), min(top[1], right[0]), min(right[1], bottom[0]), min(bottom[1], left[0]))
    }

    private fun corner(out: MutableList<Seg>, sx: Double, sy: Double, inX: Double, inY: Double, outX: Double, outY: Double, k: Corner) {
        fun pt(u: Double, v: Double) = doubleArrayOf(sx + inX * u + outX * v, sy + inY * u + outY * v)
        val e1u = k.a + k.b + k.c; val e1v = k.d
        val e2u = e1u + k.arc; val e2v = e1v + k.arc
        if (k.a + k.b + k.c + k.d > 1e-9) out.add(Seg('C', pt(k.a, 0.0) + pt(k.a + k.b, 0.0) + pt(e1u, e1v)))
        if (k.arc > 1e-9) {
            val ca = cos(k.alpha); val sa = sin(k.alpha)
            out.add(Seg('C', pt(e1u + k.h * ca, e1v + k.h * sa) + pt(e2u - k.h * sa, e2v - k.h * ca) + pt(e2u, e2v)))
        }
        if (k.a + k.b + k.c + k.d > 1e-9) out.add(Seg('C', pt(e2u + k.d, e2v + k.c) + pt(e2u + k.d, e2v + k.b + k.c) + pt(k.p, k.p)))
    }

    private fun r2(n: Double): Double = floor(n * 100 + 0.5) / 100 + 0.0

    /** Radii are physical: topLeft, topRight, bottomRight, bottomLeft. */
    fun roundedRect(w0: Double, h0: Double, radii: DoubleArray, smoothing: Double = SMOOTH, x: Double = 0.0, y: Double = 0.0): List<Seg> {
        val w = max(0.0, w0); val h = max(0.0, h0)
        val sm = min(1.0, max(0.0, smoothing))
        val bud = budgets(w, h, radii, sm)
        val k = Array(4) { cornerParams(min(radii[it], min(w / 2, h / 2)), smoothing, bud[it]) }
        val (tl, tr, br, bl) = k.toList()
        val out = ArrayList<Seg>()
        out.add(Seg('M', doubleArrayOf(x + tl.p, y)))
        var cx = x + tl.p; var cy = y
        fun line(lx: Double, ly: Double) {
            if (r2(lx) != r2(cx) || r2(ly) != r2(cy)) out.add(Seg('L', doubleArrayOf(lx, ly)))
            cx = lx; cy = ly
        }
        line(x + w - tr.p, y)
        if (tr.r != 0.0) { corner(out, x + w - tr.p, y, 1.0, 0.0, 0.0, 1.0, tr); cx = x + w; cy = y + tr.p }
        line(x + w, y + h - br.p)
        if (br.r != 0.0) { corner(out, x + w, y + h - br.p, 0.0, 1.0, -1.0, 0.0, br); cx = x + w - br.p; cy = y + h }
        line(x + bl.p, y + h)
        if (bl.r != 0.0) { corner(out, x + bl.p, y + h, -1.0, 0.0, 0.0, -1.0, bl); cx = x; cy = y + h - bl.p }
        line(x, y + tl.p)
        if (tl.r != 0.0) { corner(out, x, y + tl.p, 0.0, -1.0, 1.0, 0.0, tl); cx = x + tl.p; cy = y }
        out.add(Seg('Z', DoubleArray(0)))
        return out
    }

    /** Same shape shrunk by [inset] (negative = grown), radius - inset, same smoothing, kept in the original box's space. */
    fun inset(w: Double, h: Double, radii: DoubleArray, smoothing: Double, inset: Double): List<Seg> =
        roundedRect(w - 2 * inset, h - 2 * inset, DoubleArray(4) { max(0.0, radii[it] - inset) }, smoothing, inset, inset)

    /** The SVG path string, rounded to 0.01 exactly like shape.ts. */
    fun svg(segs: List<Seg>): String = buildString {
        for (s in segs) {
            append(s.op)
            if (s.op == 'Z') continue
            for (i in s.p.indices step 2) {
                if (i > 0) append(' ')
                append(fmt(s.p[i])); append(' '); append(fmt(s.p[i + 1]))
            }
        }
    }

    private fun fmt(n: Double): String {
        val v = r2(n)
        return if (v == floor(v) && kotlin.math.abs(v) < 1e15) v.toLong().toString() else v.toString()
    }

    /** Bubble corner radii [tl, tr, br, bl]; [right] = the side facing the screen edge the stack sits against. */
    fun bubbleRadii(pos: BubblePos, right: Boolean, outer: Double = 18.0, tight: Double = 6.0): DoubleArray {
        val top = if (pos == BubblePos.Mid || pos == BubblePos.Last) tight else outer
        val bot = if (pos == BubblePos.Mid || pos == BubblePos.First) tight else outer
        return if (right) doubleArrayOf(outer, top, bot, outer) else doubleArrayOf(top, outer, outer, bot)
    }

    fun toPath(segs: List<Seg>): Path = Path().also { path ->
        for (s in segs) {
            val p = s.p
            when (s.op) {
                'M' -> path.moveTo(p[0].toFloat(), p[1].toFloat())
                'L' -> path.lineTo(p[0].toFloat(), p[1].toFloat())
                'C' -> path.cubicTo(p[0].toFloat(), p[1].toFloat(), p[2].toFloat(), p[3].toFloat(), p[4].toFloat(), p[5].toFloat())
                else -> path.close()
            }
        }
    }
}

/** Position in a run of same-sender messages. */
enum class BubblePos { Solo, First, Mid, Last }

/** A rounded rectangle with Figma-style corner smoothing. start/end follow the layout direction. */
@Immutable
class SmoothCornerShape(
    private val topStart: Dp = 0.dp, private val topEnd: Dp = 0.dp, private val bottomEnd: Dp = 0.dp, private val bottomStart: Dp = 0.dp,
    private val smoothing: Float = SmoothGeom.SMOOTH.toFloat(),
) : Shape {
    constructor(all: Dp, smoothing: Float = SmoothGeom.SMOOTH.toFloat()) : this(all, all, all, all, smoothing)

    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline {
        val ts = with(density) { topStart.toPx() }.toDouble(); val te = with(density) { topEnd.toPx() }.toDouble()
        val be = with(density) { bottomEnd.toPx() }.toDouble(); val bs = with(density) { bottomStart.toPx() }.toDouble()
        val radii = if (layoutDirection == LayoutDirection.Ltr) doubleArrayOf(ts, te, be, bs) else doubleArrayOf(te, ts, bs, be)
        val segs = SmoothGeom.roundedRect(size.width.toDouble(), size.height.toDouble(), radii, smoothing.toDouble())
        return Outline.Generic(SmoothGeom.toPath(segs))
    }

    override fun equals(other: Any?) = other is SmoothCornerShape && topStart == other.topStart && topEnd == other.topEnd &&
        bottomEnd == other.bottomEnd && bottomStart == other.bottomStart && smoothing == other.smoothing
    override fun hashCode() = ((topStart.hashCode() * 31 + topEnd.hashCode()) * 31 + bottomEnd.hashCode()) * 31 + bottomStart.hashCode() + smoothing.hashCode()
}

/** Bubble outline for a message at [pos] in its run. [mine]: stack against the end edge, else the start edge (mirrors in RTL). */
fun bubbleShape(pos: BubblePos, mine: Boolean): SmoothCornerShape {
    val o = 18.dp; val t = 6.dp
    val top = if (pos == BubblePos.Mid || pos == BubblePos.Last) t else o
    val bot = if (pos == BubblePos.Mid || pos == BubblePos.First) t else o
    return if (mine) SmoothCornerShape(o, top, bot, o) else SmoothCornerShape(top, o, o, bot)
}
