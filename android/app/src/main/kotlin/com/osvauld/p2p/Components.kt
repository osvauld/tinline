package com.osvauld.p2p

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.rounded.ChevronRight
import androidx.compose.material.icons.rounded.Visibility
import androidx.compose.material.icons.rounded.VisibilityOff
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// ------------------------------------------------------------------ layout

/** Full-screen surface in the page colour, clear of the system bars and the keyboard. */
@Composable
fun Page(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier.fillMaxSize().background(Tin.c.bg).systemBarsPadding().imePadding(),
        content = content,
    )
}

@Composable
fun TopBar(
    title: String? = null, onBack: (() -> Unit)? = null,
    actions: @Composable RowScope.() -> Unit = {},
) {
    Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(start = 4.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        if (onBack != null) IconBtn(Icons.AutoMirrored.Rounded.ArrowBack, "Back", onBack)
        else Spacer(Modifier.width(16.dp))
        Text(title.orEmpty(), Modifier.weight(1f), style = TinType.titleM.copy(fontSize = 20.sp, lineHeight = 26.sp), color = Tin.c.ink,
            maxLines = 1, overflow = TextOverflow.Ellipsis)
        actions()
    }
}

@Composable
fun IconBtn(icon: ImageVector, desc: String, onClick: () -> Unit, tint: Color = Tin.c.ink, enabled: Boolean = true) {
    Box(
        Modifier.size(48.dp).clip(CircleShape).clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = tint, modifier = Modifier.size(24.dp)) }
}

// ------------------------------------------------------------------ buttons

enum class BtnStyle { Filled, Tonal, Outlined, Text, Danger, DangerFilled }

/** Pill button, 52 dp tall by default. */
@Composable
fun TinButton(
    text: String, onClick: () -> Unit, modifier: Modifier = Modifier, style: BtnStyle = BtnStyle.Filled,
    icon: ImageVector? = null, enabled: Boolean = true, height: Dp = 52.dp, fill: Boolean = true,
    textStyle: TextStyle = TinType.button,
) {
    val c = Tin.c
    val (bg, fg) = when (style) {
        BtnStyle.Filled -> if (enabled) c.pr to c.onPr else c.sf3 to c.ink2
        BtnStyle.Tonal -> if (enabled) c.prc to c.onPrc else c.sf3 to c.ink2
        BtnStyle.Outlined, BtnStyle.Text -> Color.Transparent to (if (enabled) c.pr else c.ink2)
        BtnStyle.Danger -> c.erc to c.er
        BtnStyle.DangerFilled -> c.er to c.onEr
    }
    val border = if (style == BtnStyle.Outlined) BorderStroke(1.dp, c.ln2) else null
    Surface(
        onClick = onClick, enabled = enabled, shape = RoundedCornerShape(50), color = bg, contentColor = fg, border = border,
        modifier = (if (fill) modifier.fillMaxWidth() else modifier).heightIn(min = height),
    ) {
        Row(
            Modifier.heightIn(min = height).padding(horizontal = if (style == BtnStyle.Text && !fill) 12.dp else 24.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically,
        ) {
            if (icon != null) Icon(icon, null, Modifier.size(20.dp))
            Text(text, style = textStyle, color = fg, textAlign = TextAlign.Center)
        }
    }
}

/** Round call-style button: accept / end (72 dp) and the 64 dp mute / route controls with a label. */
@Composable
fun RoundBtn(icon: ImageVector, desc: String, bg: Color, fg: Color, onClick: () -> Unit, size: Dp = 72.dp, iconSize: Dp = 30.dp, rotate: Float = 0f) {
    Box(
        Modifier.size(size).clip(CircleShape).background(bg).clickable(role = Role.Button, onClick = onClick).semantics { contentDescription = desc },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, null, tint = fg, modifier = Modifier.size(iconSize).then(if (rotate != 0f) Modifier.rotate(rotate) else Modifier))
    }
}

@Composable
fun LabeledRound(label: String, content: @Composable () -> Unit) {
    Column(Modifier.width(96.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        content()
        Text(label, style = TinType.caption.copy(fontSize = 13.sp), color = Tin.c.ink, textAlign = TextAlign.Center)
    }
}

// ------------------------------------------------------------------ fields

enum class FieldState { Normal, Error }

/** 56 dp text field, radius 8, label above. Border: 1 dp outline, 2 dp primary when focused, 2 dp error. */
@Composable
fun TinField(
    value: String, onChange: (String) -> Unit, label: String, modifier: Modifier = Modifier,
    mono: Boolean = false, password: Boolean = false, state: FieldState = FieldState.Normal,
    enabled: Boolean = true, singleLine: Boolean = true, minHeight: Dp = 56.dp, hint: String? = null,
    placeholder: String? = null, keyboard: KeyboardOptions = KeyboardOptions.Default, actions: KeyboardActions = KeyboardActions.Default,
    trailing: (@Composable () -> Unit)? = null,
) {
    val c = Tin.c
    val src = remember { MutableInteractionSource() }
    val focused by src.collectIsFocusedAsState()
    var shown by remember { mutableStateOf(false) }
    val err = state == FieldState.Error
    val borderColor = when { err -> c.er; focused -> c.pr; else -> c.ln2 }
    val bw = if (err || focused) 2.dp else 1.dp
    val style = (if (mono) TinType.bodyL.copy(fontFamily = PlexMono) else TinType.bodyL).copy(color = c.ink)
    Column(modifier, verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(label, style = TinType.caption.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold), color = if (err) c.er else c.ink2)
        Row(
            Modifier.fillMaxWidth().heightIn(min = minHeight).clip(RoundedCornerShape(8.dp)).background(c.sf)
                .border(bw, borderColor, RoundedCornerShape(8.dp)),
            verticalAlignment = if (singleLine) Alignment.CenterVertically else Alignment.Top,
        ) {
            BasicTextField(
                value, onChange, Modifier.weight(1f).padding(horizontal = 16.dp, vertical = if (singleLine) 0.dp else 12.dp),
                enabled = enabled, singleLine = singleLine, textStyle = style, cursorBrush = SolidColor(c.pr),
                visualTransformation = if (password && !shown) PasswordVisualTransformation() else VisualTransformation.None,
                keyboardOptions = if (password) keyboard.copy(keyboardType = androidx.compose.ui.text.input.KeyboardType.Password, autoCorrectEnabled = false) else keyboard,
                keyboardActions = actions, interactionSource = src,
                decorationBox = { inner ->
                    Box(contentAlignment = Alignment.CenterStart) {
                        if (value.isEmpty() && placeholder != null) Text(placeholder, style = style.copy(color = c.ink2.copy(alpha = 0.7f)))
                        inner()
                    }
                },
            )
            if (password) IconBtn(if (shown) Icons.Rounded.VisibilityOff else Icons.Rounded.Visibility,
                if (shown) "Hide passphrase" else "Show passphrase", { shown = !shown }, tint = c.ink2)
            else trailing?.invoke()
        }
        if (hint != null) Text(hint, style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = if (err) c.er else c.ink2)
    }
}

// ------------------------------------------------------------------ avatar

private fun hashOf(s: String): Int { var h = 0; for (ch in s) h = h * 31 + ch.code; return h and 0x7fffffff }

fun initialsOf(name: String): String {
    // First letter or digit of each word, so "Lena (work)" is "LW", not "L(".
    val parts = name.trim().split(Regex("\\s+")).mapNotNull { w -> w.firstOrNull { it.isLetterOrDigit() } }
    return when {
        parts.isEmpty() -> "?"
        parts.size == 1 -> parts[0].toString().uppercase()
        else -> (parts[0].toString() + parts.last()).uppercase()
    }
}

/** Initials on one of four tinted containers, picked by hashing the contact key (stable across devices). */
@Composable
fun Avatar(name: String, key: String, size: Dp, modifier: Modifier = Modifier, alpha: Float = 1f) {
    val c = Tin.c
    val (bg, fg) = when (hashOf(key) % 4) {
        0 -> c.prc to c.onPrc
        1 -> c.relayC to c.onRelayC
        2 -> c.warn to c.onWarn
        else -> c.pk to c.onPk
    }
    Box(modifier.size(size).clip(CircleShape).background(bg.copy(alpha = alpha)), contentAlignment = Alignment.Center) {
        Text(initialsOf(name), color = fg, style = TinType.titleM.copy(fontSize = (size.value * 0.36f).sp, lineHeight = (size.value * 0.44f).sp,
            fontWeight = if (size.value >= 60) FontWeight.Bold else FontWeight.SemiBold))
    }
}

/** My own avatar is always the teal container. */
@Composable
fun SelfAvatar(name: String, size: Dp, modifier: Modifier = Modifier) {
    val c = Tin.c
    Box(modifier.size(size).clip(CircleShape).background(c.prc), contentAlignment = Alignment.Center) {
        Text(initialsOf(name).take(1), color = c.onPrc, style = TinType.titleM.copy(fontSize = (size.value * 0.36f).sp, lineHeight = (size.value * 0.44f).sp, fontWeight = FontWeight.Bold))
    }
}

/** Me - string - them, used on Adding / Added / Calling screens. */
@Composable
fun PairAvatars(me: String, other: String, otherKey: String?, mine: Dp = 64.dp, theirs: Dp = 96.dp, unknown: Boolean = false, otherAlpha: Float = 1f) {
    val c = Tin.c
    Row(verticalAlignment = Alignment.CenterVertically) {
        SelfAvatar(me, mine)
        Box(Modifier.width(36.dp).height(3.dp).background(c.thread, RoundedCornerShape(2.dp)))
        if (unknown || otherKey == null) Box(Modifier.size(theirs).clip(CircleShape).background(c.sf3), contentAlignment = Alignment.Center) {
            Text("?", color = c.ink2, style = TinType.titleL)
        } else Avatar(other, otherKey, theirs, alpha = otherAlpha)
    }
}

// ------------------------------------------------------------------ progress

/** Onboarding progress: filled dots up to [step] (1-based) joined by amber string, the rest outlined. */
@Composable
fun ProgressDots(step: Int, total: Int = 4) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, top = 4.dp).semantics { contentDescription = "Step $step of $total" },
        verticalAlignment = Alignment.CenterVertically) {
        for (i in 1..total) {
            val done = i <= step
            Box(Modifier.size(10.dp).clip(CircleShape).then(
                if (done) Modifier.background(c.pr) else Modifier.border(2.dp, c.ln2, CircleShape)))
            if (i < total) Box(Modifier.weight(1f).height(2.dp).background(if (i < step) c.thread else c.ln))
        }
    }
}

// ------------------------------------------------------------------ banners, pills, rows

enum class BannerKind { Warn, Error, Neutral }

@Composable
fun Banner(kind: BannerKind, icon: ImageVector, title: String, body: String, action: String? = null, onAction: () -> Unit = {}) {
    val c = Tin.c
    val (bg, fg) = when (kind) {
        BannerKind.Warn -> c.warn to c.onWarn
        BannerKind.Error -> c.erc to c.onErc
        BannerKind.Neutral -> c.sf2 to c.ink
    }
    Row(
        Modifier.padding(horizontal = 16.dp).fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(bg)
            .padding(start = 14.dp, top = 14.dp, bottom = 14.dp, end = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.Top,
    ) {
        Icon(icon, null, tint = fg, modifier = Modifier.size(24.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = TinType.bodyM.copy(fontSize = 15.sp, fontWeight = FontWeight.Bold), color = fg)
            Text(body, style = TinType.bodyM, color = fg)
        }
        if (action != null) Box(
            Modifier.heightIn(min = 40.dp).align(Alignment.CenterVertically).clip(RoundedCornerShape(50)).clickable(role = Role.Button, onClick = onAction).padding(horizontal = 10.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(action, style = TinType.label.copy(fontWeight = FontWeight.Bold, textDecoration = androidx.compose.ui.text.style.TextDecoration.Underline),
                color = if (kind == BannerKind.Neutral) c.pr else fg)
        }
    }
}

enum class PillState { Available, Connecting, Offline, Away }

/** Status pill: Available / Connecting... / Offline / Not available. Tappable only when [onClick] is given. */
@Composable
fun StatusPill(state: PillState, onClick: (() -> Unit)? = null) {
    val c = Tin.c
    val (bg, fg, label) = when (state) {
        PillState.Available -> Triple(c.prc, c.onPrc, "Available")
        PillState.Connecting -> Triple(c.sf3, c.ink, "Connecting…")
        PillState.Offline -> Triple(c.sf3, c.ink, "Offline")
        PillState.Away -> Triple(c.sf3, c.ink, "Not available")
    }
    Surface(
        shape = RoundedCornerShape(50), color = bg, contentColor = fg,
        modifier = Modifier.heightIn(min = 36.dp).let { if (onClick != null) it.clickable(role = Role.Button, onClick = onClick) else it },
    ) {
        Row(Modifier.padding(start = 10.dp, end = 12.dp).heightIn(min = 36.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (state == PillState.Available) Dot(c.pr, 10.dp)
            else if (state == PillState.Connecting) Dot(c.thread, 10.dp)
            else Dot(c.ln2, 10.dp, hollow = true)
            Text(label, style = TinType.label, color = fg)
        }
    }
}

enum class BadgeKind { Direct, Relayed, Warn, Neutral, Mono }

@Composable
fun Badge(text: String, kind: BadgeKind, icon: ImageVector? = null, content: (@Composable () -> Unit)? = null) {
    val c = Tin.c
    val (bg, fg) = when (kind) {
        BadgeKind.Direct -> c.prc to c.onPrc
        BadgeKind.Relayed -> c.relayC to c.onRelayC
        BadgeKind.Warn -> c.warn to c.onWarn
        BadgeKind.Neutral, BadgeKind.Mono -> c.sf2 to c.ink
    }
    Row(
        Modifier.heightIn(min = 30.dp).clip(RoundedCornerShape(50)).background(bg).padding(start = if (icon != null || content != null) 8.dp else 12.dp, end = 12.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (icon != null) Icon(icon, null, tint = fg, modifier = Modifier.size(20.dp))
        content?.invoke()
        Text(text, style = if (kind == BadgeKind.Mono) TinType.label.copy(fontFamily = PlexMono, fontWeight = FontWeight.Medium) else TinType.label.copy(fontSize = 13.sp), color = fg)
    }
}

@Composable
fun SectionLabel(text: String, modifier: Modifier = Modifier, trailing: (@Composable () -> Unit)? = null) {
    Row(modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 18.dp, bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(text, Modifier.weight(1f), style = TinType.caption.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.04.sp), color = Tin.c.ink2)
        trailing?.invoke()
    }
}

/** Settings-style row: icon, title, optional sub-line, trailing slot (chevron by default). */
@Composable
fun ListItem(
    title: String, onClick: (() -> Unit)? = null, sub: String? = null, icon: ImageVector? = null,
    trailing: (@Composable () -> Unit)? = { Icon(Icons.Rounded.ChevronRight, null, tint = Tin.c.ink2) },
    titleColor: Color = Tin.c.ink,
) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 60.dp).let { if (onClick != null) it.clickable(role = Role.Button, onClick = onClick) else it }
            .padding(start = 20.dp, end = 16.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        if (icon != null) Icon(icon, null, tint = Tin.c.ink2)
        Column(Modifier.weight(1f)) {
            Text(title, style = TinType.bodyL, color = titleColor)
            if (sub != null) Text(sub, style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = Tin.c.ink2)
        }
        trailing?.invoke()
    }
}

/** Rounded info strip: hint text with an optional leading icon. */
@Composable
fun InfoCard(text: String, modifier: Modifier = Modifier, icon: ImageVector? = null, kind: BannerKind = BannerKind.Neutral, bold: String? = null) {
    val c = Tin.c
    val (bg, fg) = when (kind) {
        BannerKind.Warn -> c.warn to c.onWarn
        BannerKind.Error -> c.erc to c.onErc
        BannerKind.Neutral -> c.sf2 to c.ink2
    }
    Row(modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(bg).padding(14.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        if (icon != null) Icon(icon, null, tint = fg, modifier = Modifier.size(22.dp))
        Text(buildAnnotatedStringBold(bold, text), style = TinType.bodyM, color = fg)
    }
}

fun buildAnnotatedStringBold(bold: String?, rest: String) = androidx.compose.ui.text.buildAnnotatedString {
    if (bold != null) { pushStyle(androidx.compose.ui.text.SpanStyle(fontWeight = FontWeight.Bold)); append(bold); pop(); append(" ") }
    append(rest)
}

@Composable
fun Hint(text: String, modifier: Modifier = Modifier, color: Color = Tin.c.ink2, align: TextAlign = TextAlign.Start) {
    Text(text, modifier, style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = color, textAlign = align)
}

// ------------------------------------------------------------------ sheets & dialogs

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TinSheet(onDismiss: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    val c = Tin.c
    ModalBottomSheet(
        onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = c.sf, contentColor = c.ink, shape = RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp),
        dragHandle = { Box(Modifier.padding(top = 12.dp).size(36.dp, 4.dp).clip(RoundedCornerShape(2.dp)).background(c.ln2)) },
    ) {
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp).navigationBarsPadding(), content = content)
    }
}

/** Dialog: radius 24, optional round icon, title, body, then Cancel / confirm. */
@Composable
fun TinDialog(
    title: String, onDismiss: () -> Unit, confirm: String, onConfirm: () -> Unit, dismiss: String? = "Cancel",
    destructive: Boolean = false, icon: ImageVector? = null, confirmEnabled: Boolean = true,
    body: @Composable ColumnScope.() -> Unit,
) {
    val c = Tin.c
    androidx.compose.ui.window.Dialog(onDismissRequest = onDismiss) {
        Column(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(24.dp)).background(c.sf).padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            if (icon != null) Box(Modifier.size(44.dp).clip(CircleShape).background(if (destructive) c.erc else c.prc), contentAlignment = Alignment.Center) {
                Icon(icon, null, tint = if (destructive) c.er else c.onPrc)
            }
            Text(title, style = TinType.titleL.copy(fontSize = 22.sp, lineHeight = 28.sp), color = c.ink)
            body()
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)) {
                if (dismiss != null) TinButton(dismiss, onDismiss, style = BtnStyle.Text, fill = false)
                TinButton(confirm, onConfirm, style = if (destructive) BtnStyle.DangerFilled else BtnStyle.Filled, fill = false, enabled = confirmEnabled)
            }
        }
    }
}

@Composable
fun DialogText(text: String) = Text(text, style = TinType.bodyM.copy(fontSize = 15.sp, lineHeight = 22.sp), color = Tin.c.ink2)

/** Rounded content card: white surface with a hairline. */
@Composable
fun CardBox(modifier: Modifier = Modifier, radius: Dp = 14.dp, content: @Composable ColumnScope.() -> Unit) {
    val c = Tin.c
    Column(modifier.clip(RoundedCornerShape(radius)).background(c.sf).border(1.dp, c.ln, RoundedCornerShape(radius)), content = content)
}
