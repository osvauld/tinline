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
 * eases to lavender), waiting → not delivered (colour ease only): those rules are core's (`p2pcore::receipts::ticks`).
 * Nothing loops; reduced motion shows the end state.
 * The desktop's twin is crates/desktop/src/ticks.rs.
 */
typealias TickVis = uniffi.p2pcore.TickVis

/** The core decides the mark (`ReceiptTick`, signed receipts only; [notDelivered] = a block stopped it): `tick_vis`. */
fun tickVisOf(tick: ReceiptTick?, notDelivered: Boolean): TickVis? = uniffi.p2pcore.tickVis(tick, notDelivered)

/** One frame: string 1 morph, string 2 morph + offset fraction (null = hidden), colour mix. */
private data class TickPose(val k0: Float, val k1: Pair<Float, Float>?, val mix: Float)

private fun fromCore(p: uniffi.p2pcore.TickPose, mix: Float) = TickPose(p.k0, if (p.second) p.k1 to p.k1Offset else null, mix)

private fun restPose(v: TickVis) = fromCore(uniffi.p2pcore.tickPose(v, v, null), 1f)

private fun curve(c: FloatArray, secs: Float): Float? { val i = (secs * TinTicks.FRAME_HZ).toInt(); return if (i < c.size) c[i] else null }

/**
 * The transition from [from] to [to] at [secs]; null once settled. The rule (which curve, whether the ink eases, the shape
 * per sample) is core's (`tick_transition` / `tick_pose`, shared with the desktop); the curves, colour ease and clock are ours.
 */
private fun tickPose(from: TickVis, to: TickVis, secs: Float): TickPose? {
    val tr = uniffi.p2pcore.tickTransition(from, to)
    val cs = PgMotion.OVERLAY_OPEN / 1000f
    val mix = if (tr.colour) PgMotion.Out.transform((secs / cs).coerceAtMost(1f)) else 1f
    val x = when (tr.curve) {
        uniffi.p2pcore.TickCurve.PULL -> curve(TinTicks.PULL, secs)
        uniffi.p2pcore.TickCurve.PLUCK -> curve(TinTicks.PLUCK, secs)
        uniffi.p2pcore.TickCurve.NONE -> null
    }
    return if (x != null || (tr.colour && secs < cs)) fromCore(uniffi.p2pcore.tickPose(from, to, x), mix) else null
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
    fun ink(v: TickVis) = when (v) { TickVis.READ -> read; TickVis.BAD -> bad; else -> meta }
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
    TickVis.SLACK -> appString(R.string.chat_tick_slack)
    TickVis.ONE -> appString(R.string.chat_tick_one, first)
    TickVis.TWO -> appString(R.string.chat_tick_two, first)
    TickVis.READ -> appString(R.string.chat_tick_read, first)
    TickVis.BAD -> appString(R.string.chat_tick_bad)
}
