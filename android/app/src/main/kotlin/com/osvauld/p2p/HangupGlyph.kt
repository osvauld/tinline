package com.osvauld.p2p

import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.vector.PathParser

/*
 * End's glyph (crates/tin-icons/src/cut.rs; playground EndGlyph.svelte `xcut`): two strings crossed as a swirl X, one
 * cut where the other crosses it. Pressing End morphs `hangup` into `hangup-go` (the cut string goes slack, its halves
 * fall back from the cut), once, in the switch time with the playground's little overshoot. Both shapes are the same
 * three quadratics, so the morph moves each number in a straight line, as the playground's CSS `d` transition does.
 */

/** How long the call screen stays after my own End, so the glyph is seen (CallScreens.kt). */
const val END_HOLD_MS = 320L

private val GIVE = CubicBezierEasing(0.34f, 1.4f, 0.64f, 1f)
private val NUM = Regex("-?\\d+(?:\\.\\d+)?")

/** The two shapes' numbers, in path order, and the rest shape as a template to put them back into. */
private class Morph(rest: String, go: String) {
    val from = NUM.findAll(rest).map { it.value.toFloat() }.toList()
    val to = NUM.findAll(go).map { it.value.toFloat() }.toList()
    val parts = rest.split(NUM)
    init { check(from.size == to.size) { "hangup and hangup-go differ in shape" } }
    fun at(p: Float): String = buildString {
        for (i in from.indices) { append(parts[i]); append(from[i] + (to[i] - from[i]) * p) }
        append(parts.last())
    }
}

private val morph by lazy { Morph(TinIcons.Hangup.tiers[2][2], TinIcons.HangupGo.tiers[2][2]) }

@Composable
fun HangupGlyph(gone: Boolean, tint: Color, modifier: Modifier = Modifier) {
    val reduced = rememberReducedMotion()
    val p by animateFloatAsState(if (gone) 1f else 0f, if (reduced) snap() else tween(Motion.SWITCH, easing = GIVE), label = "hangup")
    val path = remember(p) { PathParser().parsePathString(morph.at(p)).toPath() }
    Canvas(modifier) {
        scale(size.width / 24f, size.height / 24f, pivot = Offset.Zero) {
            drawPath(path, tint, style = Stroke(1.75f, cap = StrokeCap.Round, join = StrokeJoin.Round))
        }
    }
}
