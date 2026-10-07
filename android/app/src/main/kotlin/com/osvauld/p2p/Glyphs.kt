package com.osvauld.p2p

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** Tinline's own glyphs (the rest of the icons are Material Symbols Rounded via material-icons-extended). */
private fun vec(name: String, build: ImageVector.Builder.() -> Unit): ImageVector =
    ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply(build).build()

private fun ImageVector.Builder.line(d: String, w: Float = 1.75f) = addPath(
    PathParser().parsePathString(d).toNodes(), fill = null, stroke = SolidColor(Color.Black),
    strokeLineWidth = w, strokeLineCap = androidx.compose.ui.graphics.StrokeCap.Round,
    strokeLineJoin = androidx.compose.ui.graphics.StrokeJoin.Round,
)

private fun ImageVector.Builder.solid(d: String) = addPath(PathParser().parsePathString(d).toNodes(), fill = SolidColor(Color.Black))

object Glyphs {
    private const val DOT_L = "M1.5,12a2.5,2.5 0 1 0 5,0a2.5,2.5 0 1 0 -5,0z"
    private const val DOT_R = "M17.5,12a2.5,2.5 0 1 0 5,0a2.5,2.5 0 1 0 -5,0z"

    /** Direct = a solid line between two points: the tin-can string. */
    val Direct: ImageVector = vec("Direct") {
        solid(DOT_L); solid(DOT_R); line("M6.5,12h11")
    }

    /** Relayed = the line passes through a locked box: the relay forwards sealed packets. */
    val Relayed: ImageVector = vec("Relayed") {
        solid(DOT_L); solid(DOT_R); line("M6.5,12h3M14.5,12h3")
        line("M10.7,9h2.6a1.2,1.2 0 0 1 1.2,1.2v3.6a1.2,1.2 0 0 1 -1.2,1.2h-2.6a1.2,1.2 0 0 1 -1.2,-1.2v-3.6a1.2,1.2 0 0 1 1.2,-1.2z")
    }

    /** The two cans and the string, single colour (monochrome app icon / small icon). */
    val Cans: ImageVector = vec("Cans") {
        solid("M3,8h3a1.5,1.5 0 0 1 1.5,1.5v5a1.5,1.5 0 0 1 -1.5,1.5h-3a1.5,1.5 0 0 1 -1.5,-1.5v-5a1.5,1.5 0 0 1 1.5,-1.5z")
        solid("M18,8h3a1.5,1.5 0 0 1 1.5,1.5v5a1.5,1.5 0 0 1 -1.5,1.5h-3a1.5,1.5 0 0 1 -1.5,-1.5v-5a1.5,1.5 0 0 1 1.5,-1.5z")
        line("M8.5,12Q12,14.5 15.5,12")
    }
}

/** The brand mark: teal cans joined by an amber string. 24-grid scaled to [size]. */
@Composable
fun TinMark(size: Dp, can: Color = Tin.c.pr, string: Color = Tin.c.thread, modifier: Modifier = Modifier) {
    Canvas(modifier.size(size)) {
        val s = this.size.width / 24f
        val r = CornerRadius(1.5f * s)
        drawRoundRect(can, Offset(1.5f * s, 8f * s), Size(6f * s, 8f * s), r)
        drawRoundRect(can, Offset(16.5f * s, 8f * s), Size(6f * s, 8f * s), r)
        val p = Path().apply { moveTo(8.5f * s, 12f * s); quadraticTo(12f * s, 14.5f * s, 15.5f * s, 12f * s) }
        drawPath(p, string, style = Stroke(1.75f * s, cap = StrokeCap.Round))
    }
}

/** Welcome / empty-state illustration: two cans far apart, the string sagging between them. */
@Composable
fun StringIllustration(modifier: Modifier = Modifier, height: Dp = 180.dp) {
    val c = Tin.c
    Canvas(modifier.fillMaxWidth().height(height)) {
        val w = size.width; val h = size.height
        val cy = h * 0.5f
        val canW = 54.dp.toPx(); val canH = 70.dp.toPx()
        fun can(x: Float, mouthRight: Boolean) {
            drawRoundRect(c.prc, Offset(x, cy - canH / 2), Size(canW, canH), CornerRadius(12.dp.toPx()))
            drawRoundRect(c.pr, Offset(x, cy - canH / 2), Size(canW, canH), CornerRadius(12.dp.toPx()), style = Stroke(3.dp.toPx()))
            val mx = if (mouthRight) x + canW - 8.dp.toPx() else x + 8.dp.toPx()
            drawOval(c.pr.copy(alpha = 0.35f), Offset(mx - 7.dp.toPx(), cy - 22.dp.toPx()), Size(14.dp.toPx(), 44.dp.toPx()))
        }
        val lx = w * 0.12f; val rx = w * 0.88f - canW
        val p = Path().apply {
            moveTo(lx + canW - 8.dp.toPx(), cy)
            cubicTo(w * 0.38f, cy + h * 0.30f, w * 0.62f, cy + h * 0.30f, rx + 8.dp.toPx(), cy)
        }
        drawPath(p, c.thread, style = Stroke(4.dp.toPx(), cap = StrokeCap.Round))
        can(lx, true); can(rx, false)
    }
}

/** Four bars from RTT + loss; [bars] filled in teal. */
@Composable
fun QualityBars(bars: Int, modifier: Modifier = Modifier, warn: Boolean = false) {
    val c = Tin.c
    val on = if (warn) c.threadText else c.pr
    val off = if (warn) c.onWarn.copy(alpha = 0.25f) else c.ln2.copy(alpha = 0.5f)
    Canvas(modifier.size(18.dp)) {
        val u = size.width / 20f
        val xs = floatArrayOf(2f, 7f, 12f, 17f); val tops = floatArrayOf(13f, 10f, 6f, 2f)
        for (i in 0..3) drawRoundRect(
            if (i < bars) on else off, Offset(xs[i] * u, tops[i] * u), Size(3f * u, (18f - tops[i]) * u), CornerRadius(1f * u),
        )
    }
}

@Composable
fun Dot(color: Color, size: Dp = 8.dp, hollow: Boolean = false) {
    Canvas(Modifier.size(size)) {
        if (hollow) drawCircle(color, radius = this.size.width / 2 - 1.dp.toPx(), style = Stroke(2.dp.toPx()))
        else drawCircle(color)
    }
}

/** Little helper for places that want an empty box of a fixed size. */
@Composable
fun Gap(w: Dp = 0.dp, h: Dp = 0.dp) = Box(Modifier.size(w, h))
