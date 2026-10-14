package com.osvauld.p2p

import androidx.compose.animation.core.InfiniteRepeatableSpec
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.StartOffset
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.keyframes
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * SignalBars.svelte: four rising bars with round ends. Lit bars are [color]; the rest stay as faint ghosts
 * (28 %) so the glyph never changes size. 0 = all ghosts (not connected yet); [wave] = reconnecting, a slow
 * wave runs through the ghosts (1.6 s, bars 160 ms apart), nothing lit. Changes fade over 300 ms.
 * Geometry (24 box): bars 3 wide at x 3.5 / 8.5 / 13.5 / 18.5, heights 6 / 10 / 14 / 18 on y 20, radius 1.5.
 */
@Composable
fun SignalBars(level: Int, color: Color, modifier: Modifier = Modifier, wave: Boolean = false, size: Dp = 24.dp) {
    val ctx = androidx.compose.ui.platform.LocalContext.current
    // Reduced motion (animator scale 0): fades are cuts and the wave doesn't run.
    val reduce = remember(ctx) { android.provider.Settings.Global.getFloat(ctx.contentResolver, android.provider.Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f }
    val alphas = (0 until 4).map { i ->
        animateFloatAsState(if (!wave && i < level) 1f else GHOST, tween(if (reduce) 0 else 300, easing = PgMotion.easeOut), label = "bar$i")
    }
    val waves: List<State<Float>>? = if (wave && !reduce) {
        val t = rememberInfiniteTransition(label = "wave")
        (0 until 4).map { i ->
            t.animateFloat(GHOST, GHOST, InfiniteRepeatableSpec(keyframes {
                durationMillis = 1600
                GHOST at 0 using PgMotion.InOut
                0.7f at 400 using PgMotion.InOut
                GHOST at 960 using LinearEasing
            }, RepeatMode.Restart, StartOffset(i * 160)), label = "w$i")
        }
    } else null
    val h = remember { floatArrayOf(6f, 10f, 14f, 18f) }
    Canvas(modifier.size(size)) {
        val k = this.size.width / 24f
        for (i in 0 until 4) {
            val a = waves?.get(i)?.value ?: alphas[i].value
            drawRoundRect(color.copy(alpha = color.alpha * a), Offset((3.5f + i * 5f) * k, (20f - h[i]) * k),
                Size(3f * k, h[i] * k), CornerRadius(1.5f * k))
        }
    }
}

/** link.svelte.ts ghost opacity. */
private const val GHOST = .28f

/** link.svelte.ts `SIGNAL_WORD`, by strength 1..4. */
fun signalWord(level: Int) = when (level) { 4 -> "Strong"; 3 -> "Good"; 2 -> "Fair"; else -> "Weak" }

/**
 * Loss over the last few seconds, not the whole call, or one bad minute would colour the rest of it.
 * Fed the running `CallStats.received / lost` totals about once a second.
 */
class RecentLoss(private val window: Int = 5) {
    private val seen = ArrayDeque<Pair<Long, Long>>()
    fun push(received: Long, lost: Long): Double {
        val last = seen.lastOrNull()
        if (last != null && (last.first > received || last.second > lost)) seen.clear()
        seen.addLast(received to lost)
        while (seen.size > window + 1) seen.removeFirst()
        val (r0, l0) = seen.first()
        val r = received - r0; val l = lost - l0
        return if (r + l == 0L) 0.0 else l * 100.0 / (r + l)
    }
}

/** The card's loss number: "0", "0.2", "14". */
fun lossText(pct: Double): String {
    val r = Math.round(pct * 10) / 10.0
    return if (r % 1.0 == 0.0 || r >= 10) Math.round(r).toString() else String.format(java.util.Locale.ROOT, "%.1f", r)
}
