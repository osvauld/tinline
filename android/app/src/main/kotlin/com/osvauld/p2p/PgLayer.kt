package com.osvauld.p2p

import android.view.WindowManager
import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Easing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider

/** svelte/easing cubicOut and cubicIn, exactly (transitions.ts uses these, not the CSS beziers). */
val SvelteCubicOut = Easing { t -> val f = t - 1f; f * f * f + 1f }
val SvelteCubicIn = Easing { t -> t * t * t }

/**
 * chat/Layer.svelte, variant `dialog` (or `sheet`) on Android: a bottom sheet inside the app.
 * Scrim rgb(10 16 14 / .5); the sheet is full width up to 520, surface-raised, CSS radius 24 on top (the playground does not
 * smooth sheets), the sheet shadow, a 28 x 4 notch (8 above, 2 below), at most the frame height minus 32.
 * In (transitions.ts layerIn): 200 ms cubicOut, the sheet rises 24 px while its opacity goes .4 -> 1, the scrim fades in.
 * Out (layerOut): 140 ms cubicIn, sinks 24 px and fades to 0. Reduced motion: instant.
 * Back and a tap on the scrim close. [value] non-null opens it; the last value stays drawn while it animates out.
 */
@Composable
fun <T : Any> PgLayer(value: T?, onClose: () -> Unit, label: String, content: @Composable ColumnScope.(T) -> Unit) {
    var shown by remember { mutableStateOf(value) }
    if (value != null) shown = value
    val current = shown ?: return
    val reduced = rememberReducedMotion()
    val t = remember { Animatable(0f) }
    LaunchedEffect(value != null) {
        if (value != null) {
            if (reduced) t.snapTo(1f) else t.animateTo(1f, tween(PgMotion.ENTER, easing = SvelteCubicOut))
        } else {
            if (reduced) t.snapTo(0f) else t.animateTo(0f, tween(PgMotion.EXIT, easing = SvelteCubicIn))
            shown = null
        }
    }
    val closing = value == null
    // The dialog window doesn't receive Compose window insets here, so the host's are measured and applied inside.
    val density = LocalDensity.current
    val top = with(density) { WindowInsets.statusBars.getTop(this).toDp() }
    val nav = with(density) { WindowInsets.navigationBars.getBottom(this).toDp() }
    val ime = with(density) { WindowInsets.ime.getBottom(this).toDp() }
    Dialog(onDismissRequest = { if (!closing) onClose() }, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        val view = LocalView.current
        SideEffect {
            (view.parent as? DialogWindowProvider)?.window?.let { w ->
                w.setDimAmount(0f); w.setWindowAnimations(0)
                // The layer covers the whole app, system bars included (the scrim dims behind them); insets are applied inside.
                if (android.os.Build.VERSION.SDK_INT >= 30) w.attributes = w.attributes.also { it.fitInsetsTypes = 0; it.fitInsetsSides = 0 }
                w.setLayout(WindowManager.LayoutParams.MATCH_PARENT, WindowManager.LayoutParams.MATCH_PARENT)
            }
        }
        BackHandler(enabled = !closing, onBack = onClose)
        val rise = with(LocalDensity.current) { 24.dp.toPx() }
        val shape = RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp)
        val c = Tin.c
        Box(Modifier.fillMaxSize()) {
            Box(Modifier.fillMaxSize().graphicsLayer { alpha = t.value }.background(PgScrim)
                .clickable(remember { MutableInteractionSource() }, indication = null) { if (!closing) onClose() })
          // The keyboard lifts the whole sheet; without it, the sheet's own padding clears the navigation bar.
          Box(Modifier.fillMaxSize().padding(bottom = ime), contentAlignment = Alignment.BottomCenter) {
            Column(
                Modifier.padding(top = top + 32.dp).widthIn(max = 520.dp).fillMaxWidth()
                    .graphicsLayer {
                        translationY = (1f - t.value) * rise
                        alpha = if (closing) t.value else .4f + .6f * t.value
                    }
                    .depth(Depth.Sheet, shape).clip(shape).background(c.sfRaised)
                    .clickable(remember { MutableInteractionSource() }, indication = null) {}
                    .semantics { paneTitle = label; contentDescription = label }
                    .verticalScroll(rememberScrollState()).padding(bottom = if (ime > 0.dp) 0.dp else nav),
            ) {
                Box(Modifier.padding(top = 8.dp, bottom = 2.dp).size(28.dp, 4.dp).clip(RoundedCornerShape(2.dp)).background(c.ln).align(Alignment.CenterHorizontally))
                content(current)
            }
          }
        }
    }
}

/** Convenience for a boolean-driven layer. */
@Composable
fun PgLayer(open: Boolean, onClose: () -> Unit, label: String, content: @Composable ColumnScope.() -> Unit) =
    PgLayer(if (open) Unit else null, onClose, label) { content() }

/** Negative horizontal margin (CSS `margin-inline: -n`), used by lists that bleed into a dialog's padding. */
fun Modifier.bleedX(n: androidx.compose.ui.unit.Dp) = this.layout { m, cs ->
    val extra = n.roundToPx() * 2
    val p = m.measure(cs.copy(maxWidth = cs.maxWidth + extra, minWidth = (cs.minWidth + extra).coerceAtMost(cs.maxWidth + extra)))
    layout(p.width - extra, p.height) { p.place(-extra / 2, 0) }
}

/** `[dir='rtl'] … svg { transform: scaleX(-1) }`: directional glyphs (back, chevron) point the other way in right-to-left. */
@Composable
fun Modifier.mirrorRtl(): Modifier =
    if (androidx.compose.ui.platform.LocalLayoutDirection.current == androidx.compose.ui.unit.LayoutDirection.Rtl) this.graphicsLayer { scaleX = -1f } else this

/**
 * CSS line boxes: the half-leading sits equally above and below every line, the first and last included. Compose's default trims
 * the first line's top and the last line's bottom, which makes every text box shorter than the playground's by the leading.
 */
val CssLineHeight = androidx.compose.ui.text.style.LineHeightStyle(
    androidx.compose.ui.text.style.LineHeightStyle.Alignment.Center, androidx.compose.ui.text.style.LineHeightStyle.Trim.None)
