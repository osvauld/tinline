package com.osvauld.p2p

import androidx.activity.BackEventCompat
import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext

/** System edge swipe, not a competing pointer gesture: scrolling and the IME keep their Back policy. */
@Composable
internal fun BackSwipe(enabled: Boolean, onBack: () -> Unit, content: @Composable () -> Unit) {
    val progress = remember { Animatable(0f) }
    val reduced = rememberReducedMotion()
    var edge by remember { mutableIntStateOf(BackEventCompat.EDGE_LEFT) }
    // Register before content so a screen's search, sheet or viewer can consume Back first.
    PredictiveBackHandler(enabled = enabled) { events ->
        try {
            events.collect { event ->
                edge = event.swipeEdge
                progress.snapTo(event.progress)
            }
            // The stack changes only on completion, never on a cancelled gesture.
            onBack()
        } catch (cancelled: CancellationException) {
            withContext(NonCancellable) {
                if (reduced) progress.snapTo(0f) else progress.animateTo(0f, tween(160))
            }
            throw cancelled
        } finally {
            withContext(NonCancellable) { progress.snapTo(0f) }
        }
    }
    // Keep only the current route composed: rendering a previous route here could expose
    // recovery secrets or run its navigation/network side effects during a cancelled swipe.
    Box(Modifier.fillMaxSize().graphicsLayer {
        val p = if (reduced) 0f else progress.value
        scaleX = 1f - 0.08f * p
        scaleY = 1f - 0.08f * p
        translationX = (if (edge == BackEventCompat.EDGE_LEFT) 1f else -1f) * 24f * density * p
        shape = RoundedCornerShape((24f * p).dp)
        clip = p > 0f
    }.background(MaterialTheme.colorScheme.background)) { content() }
}
