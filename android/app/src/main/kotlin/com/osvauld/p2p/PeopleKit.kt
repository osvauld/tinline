package com.osvauld.p2p

import android.provider.Settings
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.Easing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp

/*
 * The people / gate / settings area's port of the playground's furniture (docs/design/playground/src):
 * gate/StepShell, gate/Btn, gate/Field, gate/Notice, groups/Btn, lib/ui/Switch, t-chip, profile/profile.css (pf-*),
 * people/PassField, people/PersonRow and chat/Layer (dialog = bottom sheet on Android). Values are the playground's
 * computed CSS values (tokens.css, motion.ts); names say which CSS they come from.
 */

// ------------------------------------------------------------------ tokens (tokens.css) missing from TinColors

// Shared colour extensions live in Theme.kt.
/** Layer scrim: rgb(10 16 14 / .5). */
val PgScrim = Color(10, 16, 14).copy(alpha = 0.5f)

// ------------------------------------------------------------------ type (tokens.css type scale, Figtree incl. 650)

@OptIn(ExperimentalTextApi::class)
val PgFont = FontFamily(listOf(400, 500, 600, 650, 700).map { w ->
    Font(R.font.figtree_variable, FontWeight(w), variationSettings = FontVariation.Settings(FontVariation.weight(w)))
})
val W650 = FontWeight(650)

object PgType {
    private fun t(size: Int, lh: Int, w: FontWeight = FontWeight.Normal, ls: TextUnit = TextUnit.Unspecified) =
        TextStyle(fontFamily = PgFont, fontSize = size.sp, lineHeight = lh.sp, fontWeight = w, letterSpacing = ls, lineHeightStyle = CssLineHeight)
    /** gate h1 (StepShell): 26/32 700, -.01em. */
    val h1 = t(26, 32, FontWeight.Bold, (-0.01).em)
    /** Welcome / Done centred h1: 30/36. */
    val h1Center = t(30, 36, FontWeight.Bold, (-0.01).em)
    /** Pages' h1 (LinkDevice, PersonProfile): 22/28 650. */
    val h1Page = t(22, 28, W650)
    val lead = t(15, 22)
    val title = t(17, 24, W650)
    val barTitle = t(17, 24, FontWeight.SemiBold)
    val body = t(15, 21)
    val bodyB = t(15, 20, FontWeight.SemiBold)
    val sub = t(13, 18)
    val sub19 = t(13, 19)
    val meta = t(11, 14)
    val label = t(13, 18, W650)
    val btn = t(15, 20, W650)
    val btnGroups = t(15, 20, FontWeight.SemiBold)
    val secHead = t(12, 16, W650, 0.06.em)
    val mono = TextStyle(fontFamily = PlexMono, fontSize = 15.sp, lineHeight = 20.sp)
}

// ------------------------------------------------------------------ motion (motion.ts)

// Shared PgMotion tokens live in ChatGlyphs.kt.

/** System "remove animations" (animator scale 0) is the app's reduced-motion switch, as in Mark.kt. */
@Composable
fun rememberReducedMotion(): Boolean {
    val ctx = LocalContext.current
    return remember { Settings.Global.getFloat(ctx.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f }
}

/** app.css .press: scale .97 while pressed (100 ms in, 140 ms back, ease-out). */
@Composable
fun Modifier.press(src: MutableInteractionSource, enabled: Boolean = true): Modifier {
    val pressed by src.collectIsPressedAsState()
    val reduced = rememberReducedMotion()
    val on = pressed && enabled && !reduced
    val s by animateFloatAsState(if (on) 0.97f else 1f, tween(if (on) PgMotion.PRESS else PgMotion.RELEASE, easing = PgMotion.easeOut), label = "press")
    return this.graphicsLayer { scaleX = s; scaleY = s }
}

// ------------------------------------------------------------------ small pieces

@Composable
fun PgSpinner(size: Dp, color: Color, stroke: Dp = 2.dp) {
    val reduced = rememberReducedMotion()
    val rot = if (reduced) 0f else rememberInfiniteTransition(label = "spin")
        .animateFloat(0f, 360f, infiniteRepeatable(tween(800, easing = LinearEasing)), label = "spin").value
    Box(Modifier.size(size).rotate(rot).drawBehind {
        val w = stroke.toPx()
        drawArc(color, -90f, 270f, false, topLeft = Offset(w / 2, w / 2),
            size = androidx.compose.ui.geometry.Size(this.size.width - w, this.size.height - w), style = Stroke(w, cap = StrokeCap.Butt))
    })
}

/** lib/ui/IconButton: 44 round, icon 22, ink. */
@Composable
fun PgIconBtn(icon: ImageVector, label: String, onClick: () -> Unit, tint: Color = Tin.c.ink, size: Dp = 44.dp, iconSize: Dp = 22.dp, enabled: Boolean = true,
              modifier: Modifier = Modifier) {
    val src = remember { MutableInteractionSource() }
    Box(
        modifier.size(size).press(src, enabled).clip(CircleShape)
            .clickable(src, androidx.compose.material3.ripple(), enabled = enabled, role = Role.Button, onClick = onClick)
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, null, tint = tint.copy(alpha = if (enabled) 1f else 0.45f), modifier = Modifier.size(iconSize)) }
}

// ------------------------------------------------------------------ buttons

/** gate/Btn.svelte. Primary / secondary / text; 48 tall, r-ctl 14, 650 15 px; busy shows a 16 px spinner. */
enum class GKind { Primary, Secondary, Text }

@Composable
fun GBtn(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, kind: GKind = GKind.Primary, enabled: Boolean = true, busy: Boolean = false, fill: Boolean = true) {
    val c = Tin.c
    val src = remember { MutableInteractionSource() }
    val on = enabled && !busy
    val shape = SmoothCornerShape(14.dp)
    val (bg, fg) = when (kind) {
        GKind.Primary -> when { busy -> c.accent.copy(alpha = 0.85f) to c.onAccent; on -> c.accent to c.onAccent; else -> c.sf2 to c.ink2 }
        GKind.Secondary, GKind.Text -> Color.Transparent to c.accentInk
    }
    val dim = if (kind != GKind.Primary && !on) 0.5f else 1f
    Row(
        (if (fill) modifier.fillMaxWidth() else modifier.widthIn(min = 44.dp)).heightIn(min = 48.dp).press(src, on).graphicsLayer { alpha = dim }
            .clip(shape).background(bg)
            .then(if (kind == GKind.Secondary) Modifier.border(1.dp, c.ln, shape) else Modifier)
            .clickable(src, androidx.compose.material3.ripple(color = fg), enabled = on, role = Role.Button, onClick = onClick)
            .padding(horizontal = 20.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically,
    ) {
        if (busy) PgSpinner(16.dp, fg)
        Text(text, style = PgType.btn, color = fg, textAlign = TextAlign.Center)
    }
}

/** groups/Btn.svelte: the pill used in pages and dialogs. 44 tall, r 22, padding 0 18, 600 15 px. */
enum class PVariant { Primary, Outline, Quiet, Danger, DangerQuiet }

@Composable
fun PBtn(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, variant: PVariant = PVariant.Outline, enabled: Boolean = true,
         icon: ImageVector? = null, height: Dp = 44.dp, padH: Dp = 18.dp, colorOverride: Color? = null, minWidth: Dp = 44.dp) {
    val c = Tin.c
    val src = remember { MutableInteractionSource() }
    val shape = RoundedCornerShape(50)
    val (bg, fg, line) = when (variant) {
        PVariant.Primary -> Triple(c.accent, c.onAccent, Color.Transparent)
        PVariant.Outline -> Triple(c.sf, c.ink, c.ln)
        PVariant.Quiet -> Triple(Color.Transparent, c.ink, Color.Transparent)
        PVariant.Danger -> Triple(c.dangerBtn, Color.White, Color.Transparent)
        PVariant.DangerQuiet -> Triple(Color.Transparent, c.er, Color.Transparent)
    }
    val ink = colorOverride ?: fg
    Row(
        modifier.widthIn(min = minWidth).heightIn(min = height).press(src, enabled).graphicsLayer { alpha = if (enabled) 1f else 0.45f }
            .clip(shape).background(if (colorOverride != null && variant == PVariant.Outline) Color.Transparent else bg)
            .border(1.dp, if (colorOverride != null && variant == PVariant.Outline) colorOverride else line, shape)
            .clickable(src, androidx.compose.material3.ripple(color = ink), enabled = enabled, role = Role.Button, onClick = onClick)
            .padding(horizontal = padH),
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) Icon(icon, null, tint = ink, modifier = Modifier.size(18.dp))
        Text(text, style = PgType.btnGroups, color = ink, maxLines = 1)
    }
}

/** .pf-acts: buttons at the end, gap 8. */
@Composable
fun PfActs(modifier: Modifier = Modifier, top: Dp = 18.dp, content: @Composable RowScope.() -> Unit) =
    Row(modifier.fillMaxWidth().padding(top = top), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End), verticalAlignment = Alignment.CenterVertically, content = content)

// ------------------------------------------------------------------ switch, chip, radio

/** lib/ui/Switch.svelte: 44 x 26 track (line / accent), 20 knob inset 3 with the lift shadow, 120 ms ease-out. */
@Composable
fun PgSwitch(checked: Boolean, onChange: (Boolean) -> Unit, label: String, enabled: Boolean = true) {
    val c = Tin.c
    val reduced = rememberReducedMotion()
    val d = if (reduced) 0 else PgMotion.SWITCH
    val track by animateColorAsState(if (checked) c.accent else c.ln, tween(d, easing = PgMotion.easeOut), label = "track")
    val x by animateDpAsState(if (checked) 18.dp else 0.dp, tween(d, easing = PgMotion.easeOut), label = "knob")
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
    val stateWord = stringResource(if (checked) R.string.people_on else R.string.people_off)
    // 44 px touch target around the 26 px track.
    Box(
        Modifier.size(44.dp, 44.dp).clickable(enabled = enabled, role = Role.Switch, indication = null, interactionSource = remember { MutableInteractionSource() }) { onChange(!checked) }
            .semantics { contentDescription = label; stateDescription = stateWord },
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.size(44.dp, 26.dp).graphicsLayer { alpha = if (enabled) 1f else 0.55f }.clip(RoundedCornerShape(13.dp)).background(track)) {
            Box(Modifier.padding(3.dp).offset(x = if (rtl) -x else x).size(20.dp).depth(Depth.Lift, CircleShape).clip(CircleShape).background(c.sf))
        }
    }
}

/** tokens.css .t-chip: 32 min (callers pass 36/44), r16, line border, 13 px 600; pressed = accent-soft, accent-ink. */
@Composable
fun PgChip(text: String, pressed: Boolean, onClick: () -> Unit, minHeight: Dp = 32.dp, padH: Dp = 14.dp, icon: ImageVector? = null, mono: Boolean = false) {
    val c = Tin.c
    val src = remember { MutableInteractionSource() }
    val reduced = rememberReducedMotion()
    val bg by animateColorAsState(if (pressed) c.accentSoft else Color.Transparent, tween(if (reduced) 0 else PgMotion.SWITCH), label = "chip")
    val shape = RoundedCornerShape(minHeight / 2)
    val selectedWord = stringResource(R.string.people_selected)
    Row(
        Modifier.heightIn(min = minHeight).press(src).clip(shape).background(bg)
            .border(1.dp, if (pressed) Color.Transparent else c.ln, shape)
            .clickable(src, androidx.compose.material3.ripple(), role = Role.Button, onClick = onClick)
            .semantics { if (pressed) stateDescription = selectedWord }
            .padding(horizontal = padH),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (icon != null) Icon(icon, null, Modifier.size(16.dp), tint = if (pressed) c.accentInk else c.ink)
        Text(text, style = if (mono) PgType.mono.copy(fontSize = 14.sp, fontWeight = FontWeight.SemiBold) else PgType.sub.copy(fontWeight = FontWeight.SemiBold),
            color = if (pressed) c.accentInk else c.ink)
    }
}

/** Sounds.svelte .radio: 22 ring (2 px line / accent), 12 dot scaling in. */
@Composable
fun PgRadio(on: Boolean) {
    val c = Tin.c
    val reduced = rememberReducedMotion()
    val s by animateFloatAsState(if (on) 1f else 0f, tween(if (reduced) 0 else PgMotion.SWITCH, easing = PgMotion.easeOut), label = "radio")
    val ring by animateColorAsState(if (on) c.accent else c.ln, tween(if (reduced) 0 else PgMotion.SWITCH), label = "ring")
    Box(Modifier.size(22.dp).border(2.dp, ring, CircleShape), contentAlignment = Alignment.Center) {
        Box(Modifier.size(12.dp).graphicsLayer { scaleX = s; scaleY = s }.clip(CircleShape).background(c.accent))
    }
}

/** A row of equal segments on surface-2 (Availability .seg, AddContact .tabs); the selected one is a lifted surface. */
@Composable
fun PgSegmented(options: List<String>, selected: Int, onSelect: (Int) -> Unit, height: Dp = 44.dp, outerRadius: Dp = 16.dp, innerRadius: Dp = 12.dp,
                pad: Dp = 4.dp, gap: Dp = 4.dp, textStyle: TextStyle = PgType.bodyB, label: String = "") {
    val c = Tin.c
    val reduced = rememberReducedMotion()
    val selectedWord = stringResource(R.string.people_selected)
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(outerRadius)).background(c.sf2).padding(pad).semantics { if (label.isNotEmpty()) contentDescription = label },
        horizontalArrangement = Arrangement.spacedBy(gap),
    ) {
        options.forEachIndexed { i, o ->
            val on = i == selected
            val bg by animateColorAsState(if (on) c.sf else c.sf2.copy(alpha = 0f), tween(if (reduced) 0 else PgMotion.SWITCH, easing = PgMotion.easeOut), label = "seg")
            val fg by animateColorAsState(if (on) c.ink else c.ink2, tween(if (reduced) 0 else PgMotion.SWITCH, easing = PgMotion.easeOut), label = "segfg")
            val shape = RoundedCornerShape(innerRadius)
            val src = remember { MutableInteractionSource() }
            Box(
                Modifier.weight(1f).heightIn(min = height).press(src).then(if (on) Modifier.depth(Depth.Lift, shape) else Modifier)
                    .clip(shape).background(bg).then(if (on) Modifier.border(1.dp, c.hairline, shape) else Modifier)
                    .clickable(src, null, role = Role.Tab) { onSelect(i) }
                    .semantics { stateDescription = if (on) selectedWord else "" },
                contentAlignment = Alignment.Center,
            ) { Text(o, style = textStyle, color = fg) }
        }
    }
}

// ------------------------------------------------------------------ fields

/**
 * gate/Field.svelte: label (13 px 650 ink) over a 48 tall input (r14, line border, surface). Focus: a 2 px accent-ink outline
 * 1 px outside, border hidden. Error: danger border. [reserveLines] keeps the message area at that many 18 px lines, so a
 * message appearing moves nothing below.
 */
@Composable
fun GField(
    value: String, onChange: (String) -> Unit, label: String, modifier: Modifier = Modifier,
    secret: Boolean = false, mono: Boolean = false, error: String? = null, hint: String? = null, reserveLines: Float = 0f,
    enabled: Boolean = true, placeholder: String = "", keyboard: KeyboardOptions = KeyboardOptions.Default, onDone: (() -> Unit)? = null,
    focus: FocusRequester? = null, extra: (@Composable () -> Unit)? = null,
) {
    val c = Tin.c
    val src = remember { MutableInteractionSource() }
    val focused by src.collectIsFocusedAsState()
    var shown by remember { mutableStateOf(false) }
    val shape = SmoothCornerShape(14.dp)
    val style = (if (mono) PgType.mono.copy(fontSize = 15.sp, lineHeight = 21.sp) else PgType.body).copy(color = c.ink)
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(label, style = PgType.label, color = c.ink)
        run {
            Row(
                Modifier.fillMaxWidth().heightIn(min = 48.dp).focusRing(focused, c.accentInk, 14.dp).clip(shape).background(c.sf)
                    .border(1.dp, when { focused -> Color.Transparent; error != null -> c.er; else -> c.ln }, shape)
                    .graphicsLayer { alpha = if (enabled) 1f else 0.6f },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                BasicTextField(
                    value, onChange, Modifier.weight(1f).padding(start = 14.dp, end = if (secret) 4.dp else 14.dp).let { if (focus != null) it.focusRequester(focus) else it },
                    enabled = enabled, singleLine = true, textStyle = style, cursorBrush = SolidColor(c.accentInk),
                    visualTransformation = if (secret && !shown) PasswordVisualTransformation() else VisualTransformation.None,
                    keyboardOptions = if (secret) keyboard.copy(keyboardType = KeyboardType.Password, autoCorrectEnabled = false) else keyboard,
                    keyboardActions = KeyboardActions(onDone = { onDone?.invoke() }, onGo = { onDone?.invoke() }, onNext = { onDone?.invoke() }),
                    interactionSource = src,
                    decorationBox = { inner ->
                        Box(contentAlignment = Alignment.CenterStart) {
                            if (value.isEmpty() && placeholder.isNotEmpty()) Text(placeholder, style = style.copy(color = c.ink2.copy(alpha = 0.8f)))
                            inner()
                        }
                    },
                )
                if (secret) PgIconBtn(if (shown) PgIcons.Eyeoff else PgIcons.Eye,
                    if (shown) stringResource(R.string.people_hide_field, label.lowercase()) else stringResource(R.string.people_show_field, label.lowercase()), { shown = !shown }, tint = c.ink2, iconSize = 20.dp)
            }
        }
        val msg: @Composable () -> Unit = {
            if (error != null) Text(error, Modifier.semantics { liveRegion = LiveRegionMode.Assertive }, style = PgType.sub.copy(fontWeight = FontWeight.SemiBold), color = c.er)
            if (hint != null) Text(hint, style = PgType.sub, color = c.ink2)
            extra?.invoke()
        }
        if (reserveLines > 0f) Column(Modifier.fillMaxWidth().heightIn(min = (reserveLines * 18).dp), verticalArrangement = Arrangement.spacedBy(2.dp)) { msg() }
        else if (error != null || hint != null || extra != null) Column(verticalArrangement = Arrangement.spacedBy(2.dp)) { msg() }
    }
}

/**
 * people/PassField.svelte (pages and dialogs): label 13 px 600, a 44 tall input with the eye inside, focus = accent border plus a
 * 1 px accent ring, and a message line that is always there (19 px), so an error never moves the buttons.
 */
@Composable
fun PassField(value: String, onChange: (String) -> Unit, label: String = stringResource(R.string.people_passphrase), error: String = "", hint: String = "", enabled: Boolean = true,
              focus: FocusRequester? = null, onDone: (() -> Unit)? = null, secret: Boolean = true, mono: Boolean = false, placeholder: String = "",
              keyboard: KeyboardOptions = KeyboardOptions.Default, msgLines: Int = 1) {
    val c = Tin.c
    val src = remember { MutableInteractionSource() }
    val focused by src.collectIsFocusedAsState()
    var shown by remember { mutableStateOf(false) }
    val shape = SmoothCornerShape(14.dp)
    val bad = error.isNotEmpty()
    val style = (if (mono) PgType.mono.copy(fontWeight = FontWeight.Medium, letterSpacing = 0.04.em) else PgType.body).copy(color = c.ink)
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(label, style = PgType.sub.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
        Row(
            Modifier.fillMaxWidth().heightIn(min = 44.dp).clip(shape).background(c.sf)
                .border(if (focused && !bad) 2.dp else 1.dp, when { bad -> c.er; focused -> c.accent; else -> c.ln }, shape),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            BasicTextField(
                value, onChange, Modifier.weight(1f).padding(start = 14.dp, end = if (secret) 0.dp else 14.dp).let { if (focus != null) it.focusRequester(focus) else it },
                enabled = enabled, singleLine = true, textStyle = style, cursorBrush = SolidColor(c.accentInk), interactionSource = src,
                visualTransformation = if (secret && !shown) PasswordVisualTransformation() else VisualTransformation.None,
                keyboardOptions = if (secret) keyboard.copy(keyboardType = KeyboardType.Password, autoCorrectEnabled = false, imeAction = ImeAction.Done) else keyboard,
                keyboardActions = KeyboardActions(onDone = { onDone?.invoke() }, onGo = { onDone?.invoke() }),
                decorationBox = { inner ->
                    Box(contentAlignment = Alignment.CenterStart) {
                        if (value.isEmpty() && placeholder.isNotEmpty()) Text(placeholder, style = style.copy(color = c.ink2.copy(alpha = 0.8f)))
                        inner()
                    }
                },
            )
            if (secret) PgIconBtn(if (shown) PgIcons.Eyeoff else PgIcons.Eye, if (shown) stringResource(R.string.people_hide_passphrase) else stringResource(R.string.people_show_passphrase),
                { shown = !shown }, tint = c.ink2, iconSize = 20.dp)
        }
        Text(error.ifEmpty { hint }, Modifier.fillMaxWidth().heightIn(min = (19 * msgLines).dp).semantics { if (bad) liveRegion = LiveRegionMode.Assertive },
            style = PgType.sub19, color = if (bad) c.er else c.ink2)
    }
}

// ------------------------------------------------------------------ notices

/** gate/Notice.svelte: 12/14 padding, r14, 13/19; info = surface-2 ink, warn = warn-bg, error = danger-bg. */
enum class NKind { Info, Warn, Error }

@Composable
fun GNotice(text: AnnotatedString, kind: NKind = NKind.Info, icon: ImageVector? = null, modifier: Modifier = Modifier) {
    val c = Tin.c
    val (bg, fg) = when (kind) { NKind.Info -> c.sf2 to c.ink; NKind.Warn -> c.warn to c.onWarn; NKind.Error -> c.erc to c.er }
    Row(
        modifier.fillMaxWidth().clip(SmoothCornerShape(14.dp)).background(bg).padding(horizontal = 14.dp, vertical = 12.dp)
            .semantics { if (kind == NKind.Error) liveRegion = LiveRegionMode.Assertive },
        horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.Top,
    ) {
        Icon(icon ?: if (kind == NKind.Info) PgIcons.Info else PgIcons.Warning, null, Modifier.padding(top = 1.dp).size(20.dp), tint = fg)
        Text(text, style = PgType.sub19, color = fg)
    }
}

@Composable
fun GNotice(text: String, kind: NKind = NKind.Info, icon: ImageVector? = null, modifier: Modifier = Modifier) = GNotice(AnnotatedString(text), kind, icon, modifier)

/** Icon disc (accent-soft / accent-ink), used by Failed, Perms, Terms, Choose. */
@Composable
fun IconDisc(icon: ImageVector, size: Dp, iconSize: Dp, danger: Boolean = false) {
    val c = Tin.c
    Box(Modifier.size(size).clip(CircleShape).background(if (danger) c.erc else c.accentSoft), contentAlignment = Alignment.Center) {
        Icon(icon, null, Modifier.size(iconSize), tint = if (danger) c.er else c.accentInk)
    }
}

// ------------------------------------------------------------------ gate frame (Gate.svelte + StepShell.svelte)

/**
 * The gate header: a 44 back slot, then the progress segments (14 x 6, r3, gap 4; line / accent) and "n of m" at the end.
 * Without progress or back it shrinks to 24 (Android `.head.empty`).
 */
@Composable
fun GateHead(onBack: (() -> Unit)?, progress: Pair<Int, Int>?, stepLabel: String = "") {
    val c = Tin.c
    val reduced = rememberReducedMotion()
    val ctx = androidx.compose.ui.platform.LocalContext.current
    // Light/dark right in sign-up (Gate.svelte): the first screens are where someone decides if this feels like theirs.
    val theme: @Composable () -> Unit = {
        PgIconBtn(if (c.dark) PgIcons.Sun else PgIcons.Moon, if (c.dark) stringResource(R.string.people_switch_light) else stringResource(R.string.people_switch_dark),
            { Appearance.set(ctx, !c.dark) }, tint = c.ink2, iconSize = 20.dp, size = 40.dp, modifier = Modifier.offset(x = 10.dp))
    }
    // Android `.head.empty`: no back and no progress; only the theme switch, at the end.
    if (onBack == null && progress == null) {
        Row(Modifier.fillMaxWidth().heightIn(min = 40.dp).padding(start = 8.dp, end = 20.dp), horizontalArrangement = Arrangement.End) { theme() }
        return
    }
    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(start = 8.dp, end = 20.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Box(Modifier.width(44.dp)) { if (onBack != null) PgIconBtn(PgIcons.Back, stringResource(R.string.common_back), onBack) }
        Spacer(Modifier.weight(1f))
        val progressDesc = when {
            progress == null -> ""
            stepLabel.isNotEmpty() -> stringResource(R.string.people_setup_progress_label, progress.first, progress.second, stepLabel)
            else -> stringResource(R.string.people_setup_progress, progress.first, progress.second)
        }
        if (progress != null) Row(
            Modifier.semantics { contentDescription = progressDesc },
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                for (i in 0 until progress.second) {
                    val col by animateColorAsState(if (i < progress.first) c.accent else c.ln, tween(if (reduced) 0 else PgMotion.SWITCH, easing = PgMotion.easeOut), label = "seg")
                    Box(Modifier.size(14.dp, 6.dp).clip(RoundedCornerShape(3.dp)).background(col))
                }
            }
            // step 0 (Welcome, Choose): the count keeps its width, hidden, so "1 of 6" appears in place
            Text(stringResource(R.string.people_n_of_m, progress.first, progress.second), Modifier.widthIn(min = 42.dp).graphicsLayer { alpha = if (progress.first == 0) 0f else 1f },
                style = PgType.sub.copy(fontFeatureSettings = "tnum"), color = c.ink2)
        }
        theme()
    }
}

/**
 * gate/Btn.svelte `next` on Android: Continue is a 56 round accent button with an arrow at the end of the footer; the label stays
 * the accessible name, and busy swaps the arrow for a spinner so the size never changes. The arrow mirrors in RTL.
 */
@Composable
fun GNext(label: String, onClick: () -> Unit, enabled: Boolean = true, busy: Boolean = false) {
    val c = Tin.c
    val src = remember { MutableInteractionSource() }
    val on = enabled && !busy
    val (bg, fg) = when { busy -> c.accent.copy(alpha = 0.85f) to c.onAccent; on -> c.accent to c.onAccent; else -> c.sf2 to c.ink2 }
    Box(
        Modifier.size(56.dp).press(src, on).clip(CircleShape).background(bg)
            .clickable(src, androidx.compose.material3.ripple(color = fg), enabled = on, role = Role.Button, onClick = onClick)
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        if (busy) PgSpinner(20.dp, fg)
        else Icon(PgIcons.Next, null, Modifier.size(24.dp), tint = fg)
    }
}

/** The footer row when Continue is the round arrow: an optional secondary at the start, the arrow at the end. */
@Composable
fun GNextRow(next: @Composable () -> Unit, start: (@Composable () -> Unit)? = null) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
        if (start != null) start() else Spacer(Modifier.width(1.dp))
        next()
    }
}

/**
 * gate/StepShell.svelte on Android: heading (26/32 bold), optional lead (15/22 muted), body (gap 16, padding 8 24 16) that scrolls,
 * footer (padding 12 24 24, gap 8, full-width buttons). [center] centres the body; with [anchor] it hangs from a fixed point
 * (min(14 dvh, 96 px) from the top) so an added line never re-centres the rest.
 */
@Composable
fun GateStep(
    title: String, lead: String? = null, center: Boolean = false, anchor: Boolean = false, bigTitle: Boolean = false,
    hero: (@Composable () -> Unit)? = null, foot: (@Composable ColumnScope.() -> Unit)? = null,
    /** Replaces the plain title text (Unlock: "Welcome back, Maya ⌄", where the nickname is the switcher). */
    titleSlot: (@Composable () -> Unit)? = null,
    /** The body fills the height instead of scrolling (Welcome's mark, Choose's options and Terms' consent line sit at the bottom). */
    fill: Boolean = false,
    body: @Composable ColumnScope.() -> Unit = {},
) {
    val c = Tin.c
    BoxWithConstraints(Modifier.fillMaxSize()) {
        // Page already follows the animated IME inset. Derive the anchor from that same inset, not
        // an open/closed switch (which used to snap the heading on the very first keyboard frame).
        val density = androidx.compose.ui.platform.LocalDensity.current
        val imeHeight = with(density) {
            (WindowInsets.ime.getBottom(this) - WindowInsets.systemBars.getBottom(this)).coerceAtLeast(0).toDp()
        }
        val anchorTop = (minOf((maxHeight + imeHeight) * 0.14f, 96.dp) - imeHeight).coerceAtLeast(0.dp)
        Column(Modifier.fillMaxSize()) {
            Column(
                Modifier.weight(1f).fillMaxWidth().let { if (fill) it else it.verticalScroll(rememberScrollState()) }
                    .padding(start = 24.dp, end = 24.dp, top = if (center && anchor) 8.dp + anchorTop else 8.dp, bottom = 16.dp)
                    .let { if ((center && !anchor) || fill) it.fillMaxHeight() else it },
                verticalArrangement = if (center && !anchor) Arrangement.spacedBy(16.dp, Alignment.CenterVertically) else Arrangement.spacedBy(16.dp),
                horizontalAlignment = if (center) Alignment.CenterHorizontally else Alignment.Start,
            ) {
                hero?.invoke()
                if (titleSlot != null) titleSlot()
                else Text(title, Modifier.fillMaxWidth().semantics { heading() }, style = if (bigTitle) PgType.h1Center else PgType.h1, color = c.ink,
                    textAlign = if (center) TextAlign.Center else TextAlign.Start)
                if (lead != null) Text(lead, Modifier.fillMaxWidth(), style = PgType.lead, color = c.ink2, textAlign = if (center) TextAlign.Center else TextAlign.Start)
                body()
            }
            if (foot != null) Column(Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, top = 12.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp), content = foot)
        }
    }
}

/** Both panes exist during a transition; only the destination may request initial focus. */
internal val LocalGatePaneActive = compositionLocalOf { true }

/** The stage (Gate.svelte .stage): the step slides 36 px in the inline direction and fades; enter 200 / exit 140, cubic-out. */
@Composable
fun <T> GateStage(step: T, dir: Int, content: @Composable (T) -> Unit) {
    val reduced = rememberReducedMotion()
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
    Page {
        AnimatedContent(step, Modifier.fillMaxSize(), transitionSpec = {
            val transition = if (reduced) fadeIn(tween(0)) togetherWith fadeOut(tween(0))
            else {
                val sign = (if (rtl) -1 else 1) * dir
                val px = 36
                (fadeIn(tween(PgMotion.ENTER, easing = PgMotion.cubicOut)) + androidx.compose.animation.slideInHorizontally(tween(PgMotion.ENTER, easing = PgMotion.cubicOut)) { (sign * px * sysDensity).toInt() }) togetherWith
                    (fadeOut(tween(PgMotion.EXIT, easing = PgMotion.cubicOut)) + androidx.compose.animation.slideOutHorizontally(tween(PgMotion.EXIT, easing = PgMotion.cubicOut)) { (-sign * px * sysDensity).toInt() })
            }
            // IME insets own the vertical resize; don't put AnimatedContent's size spring on top of it.
            transition.using(null)
        }, label = "gate") { s ->
            CompositionLocalProvider(LocalGatePaneActive provides (s == step)) {
                Box(Modifier.fillMaxSize()) { content(s) }
            }
        }
    }
}

private val sysDensity: Float get() = android.content.res.Resources.getSystem().displayMetrics.density

// ------------------------------------------------------------------ pages (profile/profile.css)

/** .pf-bar: 60 tall, surface, hairline below; back button then the title (17 px 600). */
@Composable
fun PfBar(title: String, onBack: () -> Unit, actions: @Composable RowScope.() -> Unit = {}) {
    val c = Tin.c
    Column(Modifier.fillMaxWidth().background(c.sf)) {
        Row(Modifier.fillMaxWidth().heightIn(min = 60.dp).padding(start = 8.dp, end = 12.dp, top = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            PgIconBtn(PgIcons.Back, stringResource(R.string.common_back), onBack, modifier = Modifier.mirrorRtl())
            Text(title, Modifier.weight(1f).semantics { heading() }, style = PgType.barTitle, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
            actions()
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(c.hairline))
    }
}

/** .pf: the page (bg), bar, and a scrolling column (max 560, padding-bottom 40). */
@Composable
fun PfPage(title: String, onBack: () -> Unit, scroll: Boolean = true, bottomPad: Dp = 40.dp, overlay: @Composable BoxScope.() -> Unit = {}, content: @Composable ColumnScope.() -> Unit) {
    Page {
        PfBar(title, onBack)
        Box(Modifier.weight(1f).fillMaxWidth()) {
            Column(
                Modifier.fillMaxSize().let { if (scroll) it.verticalScroll(rememberScrollState()) else it }.padding(bottom = bottomPad),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) { Column(Modifier.widthIn(max = 560.dp).fillMaxWidth(), content = content) }
            overlay()
        }
    }
}

/** .pf-sec: padding 0 16, 24 above (16 for the first); optional uppercase heading (12 px 650, .06em, muted). */
@Composable
fun PfSec(title: String? = null, first: Boolean = false, top: Dp? = null, content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = top ?: if (first) 16.dp else 24.dp)) {
        if (title != null) Text(title.uppercase(), Modifier.padding(start = 4.dp, end = 4.dp, bottom = 8.dp).semantics { heading() }, style = PgType.secHead, color = Tin.c.ink2)
        content()
    }
}

/** Collects a card's rows so the card can draw the hairline between them (.pf-card > * + *). */
class PfRows {
    internal val rows = mutableListOf<Pair<Boolean, @Composable () -> Unit>>()
    /** A row with the hairline above it (`.pf-card > * + *`), e.g. a FieldRow. */
    fun row(r: @Composable () -> Unit) { rows += true to r }
    /** A `.pf-act` row: profile.css's later `.pf-act { border: 0 }` removes that hairline, so action rows run together. */
    fun act(r: @Composable () -> Unit) { rows += false to r }
}

/** .pf-card: r14 surface with an inset hairline; rows separated by hairlines. */
@Composable
fun PfCard(modifier: Modifier = Modifier, build: PfRows.() -> Unit) {
    val c = Tin.c
    val rows = PfRows().apply(build).rows
    // profile.css .pf-card is a CSS border-radius (circular), not a shape.ts path.
    val shape = RoundedCornerShape(14.dp)
    Column(modifier.fillMaxWidth().clip(shape).background(c.sf).border(1.dp, c.hairline, shape)) {
        rows.forEachIndexed { i, (sep, r) ->
            if (i > 0 && sep) Box(Modifier.fillMaxWidth().height(1.dp).background(c.hairline))
            r()
        }
    }
}

/** .pf-note: 10 above, 4 inline, 13/19 muted. */
@Composable
fun PfNote(text: String, modifier: Modifier = Modifier, align: TextAlign = TextAlign.Start, top: Dp = 10.dp) =
    Text(text, modifier.fillMaxWidth().padding(start = 4.dp, end = 4.dp, top = top), style = PgType.sub19, color = Tin.c.ink2, textAlign = align)

/** .pf-act: a 60 tall row, icon (muted 20) - title (600, one line) / sub (13/18 muted) - trailing. Clickable when [onClick] is set. */
@Composable
fun PfAct(
    title: String, onClick: (() -> Unit)? = null, icon: ImageVector? = null, sub: String? = null, danger: Boolean = false, chevron: Boolean = false,
    enabled: Boolean = true, subColor: Color? = null, subBold: Boolean = false, minHeight: Dp = 60.dp, leading: (@Composable () -> Unit)? = null,
    titleStyle: TextStyle = PgType.bodyB, gap: Dp = 14.dp, padV: Dp = 8.dp, subMaxLines: Int = Int.MAX_VALUE, padH: Dp = 16.dp,
    shape: androidx.compose.ui.graphics.Shape? = null, role: Role = Role.Button, plain: Boolean = false, extraLines: (@Composable ColumnScope.() -> Unit)? = null,
    subReserve: List<String> = emptyList(), titleMaxLines: Int = 1,
    trailing: (@Composable () -> Unit)? = null,
) {
    val c = Tin.c
    val src = remember { MutableInteractionSource() }
    Row(
        Modifier.fillMaxWidth().heightIn(min = minHeight).graphicsLayer { alpha = if (enabled) 1f else 0.55f }
            .let { if (onClick != null && !plain) it.press(src, enabled) else it }
            .let { if (shape != null) it.clip(shape) else it }
            .let { if (onClick != null) it.clickable(src, if (plain) null else androidx.compose.material3.ripple(), enabled = enabled, role = role, onClick = onClick) else it }
            .padding(horizontal = padH, vertical = padV),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(gap),
    ) {
        if (leading != null) leading()
        else if (icon != null) Icon(icon, null, Modifier.size(20.dp), tint = if (danger) c.er else c.ink2)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(title, style = titleStyle, color = if (danger) c.er else c.ink, maxLines = titleMaxLines, overflow = TextOverflow.Ellipsis)
            if (!sub.isNullOrEmpty()) {
                val subStyle = PgType.sub.copy(fontWeight = if (subBold) FontWeight.SemiBold else FontWeight.Normal, fontFeatureSettings = "tnum")
                // No layout shift: [subReserve] lists every text this sub can switch to. They are laid out invisibly
                // underneath, so the line keeps the height of the tallest variant at any width or font scale.
                Box {
                    subReserve.forEach { alt -> if (alt != sub) Text(alt, Modifier.alpha(0f).clearAndSetSemantics {}, style = subStyle, maxLines = subMaxLines) }
                    Text(sub, style = subStyle, color = subColor ?: c.ink2, maxLines = subMaxLines, overflow = TextOverflow.Ellipsis)
                }
            }
            extraLines?.invoke(this)
        }
        trailing?.invoke()
        if (chevron) Icon(PgIcons.Chevr, null, Modifier.size(18.dp).mirrorRtl(), tint = c.ink2)
    }
}

/** .pf-strip: an inline status (surface-2, r14, 13/19). */
@Composable
fun PfStrip(text: String, icon: ImageVector, modifier: Modifier = Modifier, floating: Boolean = false) {
    val c = Tin.c
    val shape = RoundedCornerShape(14.dp)
    Row(
        modifier.fillMaxWidth().then(if (floating) Modifier.depth(Depth.Pop, shape) else Modifier).clip(shape)
            .background(if (floating) c.sfRaised else c.sf2).then(if (floating) Modifier.border(1.dp, c.hairline, shape) else Modifier)
            .padding(horizontal = 14.dp, vertical = 10.dp).semantics { liveRegion = LiveRegionMode.Polite },
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Icon(icon, null, Modifier.size(16.dp), tint = c.ink2)
        Text(text, style = PgType.sub19, color = c.ink)
    }
}

/** .pf-err: danger-bg block, r12, 13/19. */
@Composable
fun PfErr(text: String, modifier: Modifier = Modifier) {
    val c = Tin.c
    Text(text, modifier.fillMaxWidth().padding(top = 10.dp).clip(RoundedCornerShape(12.dp)).background(c.erc).padding(horizontal = 12.dp, vertical = 10.dp)
        .semantics { liveRegion = LiveRegionMode.Assertive }, style = PgType.sub19, color = c.er)
}

/** A line that is always there and cross-fades its text (reserved space: nothing moves). */
@Composable
fun ReservedLine(text: String, modifier: Modifier = Modifier, style: TextStyle = PgType.sub19, color: Color = Tin.c.ink2, align: TextAlign = TextAlign.Start, lines: Int = 1) {
    val reduced = rememberReducedMotion()
    val lh = style.lineHeight.value
    AnimatedContent(text, modifier.fillMaxWidth().heightIn(min = (lh * lines).dp), transitionSpec = {
        fadeIn(tween(if (reduced) 0 else PgMotion.CROSSFADE, easing = PgMotion.easeOut)) togetherWith fadeOut(tween(if (reduced) 0 else PgMotion.CROSSFADE, easing = PgMotion.easeOut))
    }, label = "line") { t -> Text(t, Modifier.fillMaxWidth().semantics { liveRegion = LiveRegionMode.Polite }, style = style, color = color, textAlign = align) }
}

// ------------------------------------------------------------------ dialogs (chat/Layer.svelte, dialog = bottom sheet on Android)

/**
 * A Layer dialog on Android: a bottom sheet (surface-raised, r24 top, sheet shadow), the 28 x 4 notch, the playground scrim,
 * content padded as .pf-dlg (20 20 16).
 */

/** .pf-dlg h2 (17 px 650, 24) and paragraphs (13/19 muted, 8 above; lead = 15/21 ink). */
@Composable
fun DlgTitle(text: String) = Text(text, Modifier.padding(bottom = 4.dp).semantics { heading() }, style = PgType.title, color = Tin.c.ink)
@Composable
fun DlgP(text: String, lead: Boolean = false, modifier: Modifier = Modifier) =
    Text(text, modifier.fillMaxWidth().padding(top = 8.dp), style = if (lead) PgType.body else PgType.sub19, color = if (lead) Tin.c.ink else Tin.c.ink2)

// ------------------------------------------------------------------ people

/** people/PersonRow.svelte: 64 tall, 44 avatar, gap 12, name (600, 21) over a one-line sub that always holds its 18 px. */
@Composable
fun PersonRowPg(name: String, key: String, sub: String, onClick: (() -> Unit)? = null, pad: Dp = 8.dp, dim: Boolean = false, end: (@Composable () -> Unit)? = null) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(horizontal = pad), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(
            Modifier.weight(1f).heightIn(min = 64.dp).clip(SmoothCornerShape(14.dp)).let { if (onClick != null) it.clickable(role = Role.Button, onClick = onClick) else it },
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Avatar(name, key, 44.dp, alpha = if (dim) 0.6f else 1f)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                Text(name, style = PgType.bodyB.copy(lineHeight = 21.sp), color = c.ink.copy(alpha = if (dim) 0.6f else 1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(sub, Modifier.heightIn(min = 18.dp), style = PgType.sub, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        end?.invoke()
    }
}

/** CSS `outline: 2px solid; outline-offset: 1px` (gate/Field focus): a ring drawn 1..3 px outside the shape, taking no layout space. */
fun Modifier.focusRing(on: Boolean, color: Color, radius: Dp): Modifier = if (!on) this else this.drawBehind {
    val grow = 2.dp.toPx()
    val sh = SmoothCornerShape(radius + 2.dp)
    val o = sh.createOutline(androidx.compose.ui.geometry.Size(size.width + grow * 2, size.height + grow * 2), layoutDirection, this)
    val path = (o as? androidx.compose.ui.graphics.Outline.Generic)?.path ?: return@drawBehind
    translate(-grow, -grow) { drawPath(path, color, style = Stroke(2.dp.toPx())) }
}
