package com.osvauld.p2p

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import uniffi.p2pcore.ReceiptTick

/**
 * Receipt ticks drawn as the mark's string (the user's pick from the playground's `?lab=ticks`, preset "crisp", string
 * variant at every size). Geometry and the spring's morph curves are generated from Rust ([TinTicks],
 * crates/tin-icons/src/ticks.rs); this file only picks the visual state and plays the one-shot transition:
 * slack → one (pulled taut), one → two (the second string pulls out of the first), two → read (one pluck while the colour
 * eases to lavender), waiting → not delivered (colour ease only). Nothing loops; reduced motion shows the end state.
 * The desktop's twin is crates/desktop/src/ticks.rs.
 */
enum class TickVis { Slack, One, Two, Read, Bad }

/** The core decides Delivered/Read (`ReceiptTick`, signed receipts only); [notDelivered] = a block stopped it. */
fun tickVisOf(tick: ReceiptTick?, notDelivered: Boolean): TickVis? = when {
    notDelivered -> TickVis.Bad
    tick == ReceiptTick.ONLY_HERE -> TickVis.Slack
    tick == ReceiptTick.WAITING -> TickVis.One
    tick == ReceiptTick.DELIVERED -> TickVis.Two
    tick == ReceiptTick.READ -> TickVis.Read
    else -> null
}

private fun TickVis.shape() = when (this) { TickVis.Slack, TickVis.Bad -> 0; TickVis.One -> 1; TickVis.Two, TickVis.Read -> 2 }
private fun TickVis.tone() = when (this) { TickVis.Read -> 1; TickVis.Bad -> 2; else -> 0 }

/** One frame: string 1 morph, string 2 morph + offset fraction (null = hidden), colour mix. */
private data class TickPose(val k0: Float, val k1: Pair<Float, Float>?, val mix: Float)

private fun restPose(v: TickVis) = when (v.shape()) {
    0 -> TickPose(0f, null, 1f)
    1 -> TickPose(1f, null, 1f)
    else -> TickPose(1f, 1f to 1f, 1f)
}

private fun curve(c: FloatArray, secs: Float): Float? { val i = (secs * TinTicks.FRAME_HZ).toInt(); return if (i < c.size) c[i] else null }

/** The transition from [from] to [to] at [secs]; null once settled. Same rules as the desktop's `ticks::pose`. */
private fun tickPose(from: TickVis, to: TickVis, secs: Float): TickPose? {
    val color = from.tone() != to.tone()
    val cs = PgMotion.OVERLAY_OPEN / 1000f
    val mix = if (color) PgMotion.Out.transform((secs / cs).coerceAtMost(1f)) else 1f
    var p = restPose(to).copy(mix = mix)
    val shaped = when {
        from.shape() == 0 && to.shape() == 1 -> curve(TinTicks.PULL, secs)?.also { p = p.copy(k0 = it) }
        from.shape() == 1 && to.shape() == 2 -> curve(TinTicks.PULL, secs)?.also { p = p.copy(k1 = (0.5f + 0.5f * it) to maxOf(0f, it)) }
        from.shape() == 0 && to.shape() == 2 -> curve(TinTicks.PULL, secs)?.also { p = p.copy(k0 = it, k1 = it to 1f) }
        to == TickVis.Read && from == TickVis.Two -> curve(TinTicks.PLUCK, secs)?.also { p = p.copy(k0 = it, k1 = it to 1f) }
        else -> null
    }
    return if (shaped != null || (color && secs < cs)) p else null
}

/** The slot's width for a glyph [size] tall (two ticks wide, so nothing moves as the state advances). */
fun tickSlotWidth(size: Dp): Dp = size * (TinTicks.VB_W / TinTicks.VB_H)

/**
 * The receipt tick of one message. [key] identifies the message: the glyph animates only when [vis] changes while it is
 * on screen (never on first composition or scroll-in). [bg] is the bubble (the knot's halo). [label] is what a
 * screen reader hears.
 */
@Composable
fun ReceiptTickGlyph(key: String, vis: TickVis, meta: Color, read: Color, bad: Color, bg: Color, label: String, size: Dp = 12.dp) {
    val reduced = reducedMotion()
    var shown by remember(key) { mutableStateOf(vis) }
    var from by remember(key) { mutableStateOf<TickVis?>(null) }
    var secs by remember(key) { mutableFloatStateOf(0f) }
    LaunchedEffect(key, vis) {
        if (shown == vis) return@LaunchedEffect
        val prev = shown
        shown = vis
        if (reduced) { from = null; return@LaunchedEffect }
        from = prev; secs = 0f
        val t0 = withFrameNanos { it }
        while (true) {
            val now = withFrameNanos { it }
            secs = (now - t0) / 1e9f
            if (tickPose(prev, vis, secs) == null) break
        }
        from = null
    }
    val f = from
    val pose = (if (f != null) tickPose(f, vis, secs) else null) ?: restPose(vis)
    fun ink(v: TickVis) = when (v) { TickVis.Read -> read; TickVis.Bad -> bad; else -> meta }
    val color = if (f != null) lerp(ink(f), ink(vis), pose.mix) else ink(vis)
    Canvas(Modifier.size(tickSlotWidth(size), size).semantics { contentDescription = label }) {
        val s = this.size.height / TinTicks.VB_H
        val stroke = Stroke(width = TinTicks.SW * s, cap = StrokeCap.Round, join = StrokeJoin.Round)
        fun path(k: Float, ox: Float) = Path().apply {
            for (i in 0 until TinTicks.N) {
                val sx = TinTicks.SLACK[2 * i]; val sy = TinTicks.SLACK[2 * i + 1]
                val tx = TinTicks.TICK[2 * i]; val ty = TinTicks.TICK[2 * i + 1]
                val x = (sx + (tx - sx) * k + ox - TinTicks.VB_X) * s; val y = (sy + (ty - sy) * k) * s
                if (i == 0) moveTo(x, y) else lineTo(x, y)
            }
        }
        drawPath(path(pose.k0, 0f), color, style = stroke)
        pose.k1?.let { (k, frac) ->
            val p2 = path(k, TinTicks.DX * frac)
            if (TinTicks.GAP > 0f) drawPath(p2, bg, style = Stroke(width = (TinTicks.SW + 2 * TinTicks.GAP) * s, cap = StrokeCap.Round, join = StrokeJoin.Round))
            drawPath(p2, color, style = stroke)
        }
    }
}

/** Screen-reader words per state (VOICE.md: how things are). */
fun tickLabel(v: TickVis, first: String): String = when (v) {
    TickVis.Slack -> "Waiting to send"
    TickVis.One -> "Sent, waiting for $first’s device to confirm"
    TickVis.Two -> "On $first’s device"
    TickVis.Read -> "Read on $first’s device"
    TickVis.Bad -> "Not delivered"
}
