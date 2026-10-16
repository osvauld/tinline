package com.osvauld.p2p

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
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
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.layout.layout
import androidx.compose.ui.input.key.*
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.ceil
import kotlin.math.max

/*
 * The conversation's building blocks, ported from the playground (screens/chat/MessageRow.svelte, Composer.svelte,
 * Layer.svelte, Conversation.svelte). Shared by the 1:1 conversation and, through these public parts, by group chats.
 * Values are the playground's computed ones; the comments name the CSS they come from.
 */

/** --fs-body / --lh-body. */
val PgBodyText = TextStyle(fontFamily = Figtree, fontSize = 15.sp, lineHeight = 21.sp)
/** .meta: 11 px, line-height 1, tabular digits. */
private val MetaText = TextStyle(fontFamily = Figtree, fontSize = 11.sp, lineHeight = 11.sp, fontFeatureSettings = "tnum")

/** Spacing between rows (MessageRow.svelte `.m`): 10 dp above a run, 2 dp inside one. */
fun gapAbove(pos: BubblePos): Dp = if (pos == BubblePos.Mid || pos == BubblePos.Last) 2.dp else 10.dp

/** My message's receipt mark ([ReceiptTicks.kt]): the state, the message it belongs to (it animates only when that
 *  message's state changes) and what a screen reader hears. [reserve] with no [vis] keeps the slot while it loads. */
class MetaReceipt(val vis: TickVis?, val key: String, val label: String, val reserve: Boolean = true)

/** The inline time and status of a bubble: [edited] time [tick]. */
class BubbleMeta(val time: String, val edited: Boolean = false, val receipt: MetaReceipt? = null)

@Composable
fun PgMeta(meta: BubbleMeta, color: Color, modifier: Modifier = Modifier) {
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(3.dp), verticalAlignment = Alignment.CenterVertically) {
        if (meta.edited) Text("edited", style = MetaText, color = color, maxLines = 1)
        Text(meta.time, style = MetaText, color = color, maxLines = 1)
        // The tick keeps one slot (two ticks wide) whatever the state, so a status change never narrows the bubble.
        meta.receipt?.let { r ->
            if (r.vis != null || r.reserve) Box(Modifier.size(tickSlotWidth(12.dp), 12.dp), contentAlignment = Alignment.Center) {
                r.vis?.let { ReceiptTickGlyph(r.key, it, color, Tin.c.tickRead, Tin.c.tickBad, Tin.c.prc, r.label) }
            }
        }
    }
}

/**
 * Text with the meta floated at the end of its last line (`.meta { float: right; margin: 10px -2px -2px 10px }`):
 * inline when it fits beside the last line, else on a row of its own that adds 19 dp. A bubble whose text wraps, or whose
 * one line plus meta does not fit, takes the full width, as CSS shrink-to-fit does.
 */
@Composable
fun PgTextWithMeta(text: AnnotatedString, color: Color, meta: @Composable () -> Unit, style: TextStyle = PgBodyText) {
    val measurer = rememberTextMeasurer()
    SubcomposeLayout { cs ->
        val maxW = if (cs.hasBoundedWidth) cs.maxWidth else Constraints.Infinity
        val m = subcompose("meta", meta).single().measure(Constraints())
        val r = measurer.measure(text, style = style, constraints = Constraints(maxWidth = maxW))
        val box = m.width + 8.dp.roundToPx() // 10 left margin, -2 right margin
        val one = r.lineCount == 1
        val textW = ceil(r.size.width.toFloat()).toInt()
        val lastRight = ceil(r.getLineRight(r.lineCount - 1)).toInt()
        val natural = when {
            one && textW + box <= maxW -> textW + box
            maxW == Constraints.Infinity -> textW + box
            else -> maxW
        }
        val w = max(natural, cs.minWidth)
        val inline = lastRight + box <= w
        val body = subcompose("body") { androidx.compose.material3.Text(text, style = style, color = color) }.single().measure(Constraints(maxWidth = w))
        val h = if (inline) body.height else body.height + 19.dp.roundToPx()
        val my = if (inline) body.height - m.height else body.height + 21.dp.roundToPx() - m.height
        layout(w, h) {
            body.placeRelative(0, 0)
            m.placeRelative(w - m.width + 2.dp.roundToPx(), my)
        }
    }
}

/**
 * The bubble surface (MessageRow.svelte `.bub` + `.skin`): Bézier outline by run position, sent = --bub-out, received =
 * --bub-in with a 1 dp --bub-in-line stroke, deleted = a hairline outline only. 7/12 padding plus the 1 px border = 8/13.
 * [held] scales to .97 (the long-press sheet is open for it).
 */
@Composable
fun PgBubble(
    pos: BubblePos, mine: Boolean, modifier: Modifier = Modifier, gone: Boolean = false, held: Boolean = false,
    padding: PaddingValues = PaddingValues(horizontal = 13.dp, vertical = 8.dp), content: @Composable BoxScope.() -> Unit,
) {
    val c = Tin.c
    val shape = bubbleShape(pos, mine)
    val s by animateFloatAsState(if (held) .97f else 1f, tween(PgMotion.RELEASE, easing = PgMotion.Out), label = "held")
    val bg = when { gone -> Color.Transparent; mine -> c.prc; else -> c.bubIn }
    val line = when { gone -> c.hairline; mine -> null; else -> c.bubInLine }
    Box(
        modifier.graphicsLayer { scaleX = s; scaleY = s }.clip(shape).background(bg).let { if (line != null) it.border(1.dp, line, shape) else it }.padding(padding),
        content = content,
    )
}

/** Bubble ink and meta colours. */
@Composable fun bubbleInk(mine: Boolean) = if (mine) Tin.c.onPrc else Tin.c.ink
@Composable fun bubbleMetaColor(mine: Boolean) = if (mine) Tin.c.bubMetaOut else Tin.c.bubMetaIn

/** A text message: the bubble with its text and inline meta, or, for a few emojis only, big emoji with no surface. */
@Composable
fun PgTextBubble(
    text: AnnotatedString, pos: BubblePos, mine: Boolean, meta: BubbleMeta, modifier: Modifier = Modifier, emoji: Boolean = false,
    held: Boolean = false, sender: String? = null, senderColor: Color = Tin.c.accentInk, quote: (@Composable () -> Unit)? = null,
) {
    val metaColor = bubbleMetaColor(mine)
    if (emoji) {
        // .bub.emoji: no surface, 34/42, the meta below at the end, muted.
        Column(modifier.padding(horizontal = 2.dp), horizontalAlignment = Alignment.End) {
            Text(text, style = PgBodyText.copy(fontSize = 34.sp, lineHeight = 42.sp), color = bubbleInk(mine))
            PgMeta(meta, Tin.c.ink2)
        }
        return
    }
    val ink = bubbleInk(mine)
    val body: @Composable () -> Unit = { PgTextWithMeta(text, ink, { PgMeta(meta, metaColor) }) }
    PgBubble(pos, mine, modifier, held = held) {
        if (sender == null && quote == null) body()
        else PgStack(listOfNotNull(
            sender?.let { { Text(it, Modifier.padding(bottom = 1.dp), style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Bold),
                color = senderColor, maxLines = 1, overflow = TextOverflow.Ellipsis) } },
            quote, body,
        ))
    }
}

/**
 * A column whose width is its widest child (CSS shrink-to-fit with block children), every child then laid out at that
 * width: a quote spans the bubble, and the meta floats to the bubble's end. Children are composed twice (probe, final).
 */
@Composable
fun PgStack(children: List<@Composable () -> Unit>) {
    SubcomposeLayout { cs ->
        val loose = Constraints(maxWidth = cs.maxWidth)
        val w = children.mapIndexed { i, ch -> subcompose("p$i", ch).maxOfOrNull { it.measure(loose).width } ?: 0 }.maxOrNull()?.coerceAtMost(cs.maxWidth) ?: 0
        val ps = children.mapIndexed { i, ch -> subcompose("f$i", ch).map { it.measure(Constraints(minWidth = w, maxWidth = w)) } }.flatten()
        layout(w, ps.sumOf { it.height }) {
            var y = 0
            ps.forEach { it.placeRelative(0, y); y += it.height }
        }
    }
}

/** "You deleted this message", italic and muted inside a hairline outline. */
@Composable
fun PgDeletedBubble(label: String, pos: BubblePos, mine: Boolean, time: String, modifier: Modifier = Modifier) {
    PgBubble(pos, mine, modifier, gone = true) {
        PgTextWithMeta(AnnotatedString(label), Tin.c.ink2, { PgMeta(BubbleMeta(time), Tin.c.ink2) }, PgBodyText.copy(fontStyle = FontStyle.Italic))
    }
}

/** The quote of the message replied to (`.quote`): surface-2, 3 dp accent bar at the start, smooth 12 corners. */
@Composable
fun PgQuote(who: String?, whoColor: Color, text: String, onClick: (() -> Unit)?) {
    val c = Tin.c
    val shape = SmoothCornerShape(12.dp)
    Column(
        Modifier.padding(top = 2.dp, bottom = 6.dp).heightIn(min = 44.dp).clip(shape).background(c.sf2).startBar(c.accentFill, 3.dp)
            .let { if (onClick != null) it.clickable(role = Role.Button, onClickLabel = "Jump to message", onClick = onClick) else it }
            .padding(start = 13.dp, end = 10.dp, top = 5.dp, bottom = 6.dp),
    ) {
        if (who != null) Text(who, style = PgBodyText.copy(fontSize = 12.sp, lineHeight = 18.sp, fontWeight = FontWeight.Bold), color = whoColor, maxLines = 1)
        Text(text, style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp), color = c.ink2, maxLines = 2, overflow = TextOverflow.Ellipsis)
    }
}

/** A bar along the start edge, clipped by whatever shape the element has (CSS border-inline-start under a radius). */
fun Modifier.startBar(color: Color, width: Dp) = drawBehind {
    val w = width.toPx()
    val x = if (layoutDirection == androidx.compose.ui.unit.LayoutDirection.Ltr) 0f else size.width - w
    drawRect(color, Offset(x, 0f), Size(w, size.height))
}

/** Day separator (`.day`): a centred pill, surface with an inset hairline, 11 semibold muted. */
@Composable
fun PgDayPill(label: String, modifier: Modifier = Modifier) {
    val c = Tin.c
    Box(modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
        Text(label, Modifier.clip(RoundedCornerShape(50)).background(c.sf).border(1.dp, c.hairline, RoundedCornerShape(50)).padding(horizontal = 10.dp, vertical = 3.dp),
            style = PgBodyText.copy(fontSize = 11.sp, lineHeight = 16.sp, fontWeight = FontWeight.SemiBold), color = c.ink2, maxLines = 1)
    }
}

/** "2 new messages" (`.unread`): accent-ink label between two thin lines. */
@Composable
fun PgUnreadDivider(n: Int, modifier: Modifier = Modifier) {
    val c = Tin.c
    val line = c.accentInk.copy(alpha = .35f)
    val label = "$n new ${if (n == 1) "message" else "messages"}"
    Row(modifier.fillMaxWidth().padding(top = 12.dp, bottom = 6.dp).semantics(mergeDescendants = true) { contentDescription = label },
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(Modifier.weight(1f).height(1.dp).background(line))
        Text(label, style = PgBodyText.copy(fontSize = 11.sp, lineHeight = 16.sp, fontWeight = FontWeight.Bold), color = c.accentInk)
        Box(Modifier.weight(1f).height(1.dp).background(line))
    }
}

/** The status strip over the top of the list (`.strip`); `warn` is the gold "waiting for a host" look. */
@Composable
fun PgStrip(text: String, icon: ImageVector, warn: Boolean = false, modifier: Modifier = Modifier) {
    val c = Tin.c
    Column(modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth().background(if (warn) c.warn else c.sf2).padding(horizontal = 16.dp, vertical = 7.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(icon, null, tint = if (warn) c.onWarn else c.ink2, modifier = Modifier.size(16.dp))
            Text(text, style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp), color = if (warn) c.onWarn else c.ink2)
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(c.hairline))
    }
}

/** The "scroll to latest" button (`.tobottom`): 44 dp, raised surface, lift depth, a count of new messages. */
@Composable
fun PgToBottom(below: Int, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val c = Tin.c
    Box(modifier.size(44.dp)) {
        Box(
            Modifier.fillMaxSize().depth(Depth.Lift, CircleShape).clip(CircleShape).background(c.sfRaised)
                .pgPress(onClick, if (below > 0) "Scroll to latest, $below new" else "Scroll to latest"),
            contentAlignment = Alignment.Center,
        ) { Icon(Pg.Chev, null, tint = c.ink, modifier = Modifier.size(22.dp)) }
        // .t-badge at top/end -4, its 2 dp canvas ring outside it.
        if (below > 0) Box(Modifier.align(Alignment.TopEnd).offset(6.dp, (-6).dp).clip(RoundedCornerShape(11.dp)).background(c.canvas).padding(2.dp)) {
            Box(Modifier.heightIn(min = 18.dp).widthIn(min = 18.dp).clip(RoundedCornerShape(9.dp)).background(c.accentFill).padding(horizontal = 5.dp), contentAlignment = Alignment.Center) {
                Text("$below", style = MetaText.copy(fontWeight = FontWeight.Bold), color = c.onAccentFill)
            }
        }
    }
}

// ------------------------------------------------------------------ composer

/** The context chip over the field (reply / edit). */
class ComposerCtx(val title: String, val text: String, val cancelLabel: String)

/**
 * Composer (Composer.svelte): [attach] [field] [send], 6 dp apart, bottom-aligned; the field is a smooth 22 dp pill on the
 * surface with a hairline that turns accent with a soft glow on focus; it grows with the text up to 140 dp. The send button
 * is the accent circle, dimmed to .38 while there is nothing to send; [trailing] replaces it (the voice button) then.
 */
@Composable
fun PgComposer(
    draft: String, onDraft: (String) -> Unit, placeholder: String, ctx: ComposerCtx?, editing: Boolean,
    onCancelCtx: () -> Unit, onSend: () -> Unit, onAttach: (() -> Unit)?, focus: FocusRequester = remember { FocusRequester() },
    field: (@Composable (Modifier) -> Unit)? = null, trailing: (@Composable () -> Unit)? = null,
) {
    val c = Tin.c
    val empty = draft.isBlank()
    Column(Modifier.fillMaxWidth().padding(start = 10.dp, end = 10.dp, top = 8.dp, bottom = 10.dp)) {
        if (ctx != null) Row {
            if (onAttach != null) Spacer(Modifier.width(50.dp))
            PgCtxChip(ctx, onCancelCtx, Modifier.weight(1f).padding(bottom = 6.dp))
            Spacer(Modifier.width(50.dp))
        }
        Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            if (onAttach != null) PgIconBtn(Pg.Clip, "Attach a file", onAttach, tint = c.ink2, enabled = !editing, dimAlpha = .38f)
            if (field != null) field(Modifier.weight(1f)) else PgField(draft, onDraft, placeholder, focus, Modifier.weight(1f).onPreviewKeyEvent { e ->
                if (e.type == KeyEventType.KeyDown && (e.key == Key.Enter || e.key == Key.NumPadEnter) && (e.isCtrlPressed || e.isMetaPressed)) { if (draft.isNotBlank()) onSend(); true }
                else if (e.type == KeyEventType.KeyDown && e.key == Key.Escape && ctx != null) { onCancelCtx(); true } else false
            })
            if (empty && !editing && trailing != null) trailing()
            else PgSendButton(editing, empty, onSend)
        }
    }
}

@Composable
fun PgSendButton(editing: Boolean, empty: Boolean, onSend: () -> Unit) {
    val c = Tin.c
    // The same 48 dp slot as the mic button it replaces, so the composer row keeps its height on the first letter.
    Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
        Box(
            Modifier.size(44.dp).alpha(if (empty) .38f else 1f).clip(CircleShape).background(c.accentFill)
                .let { if (empty) it.semantics { contentDescription = if (editing) "Save edit" else "Send" } else it.pgPress(onSend, if (editing) "Save edit" else "Send") },
            contentAlignment = Alignment.Center,
        ) {
            Icon(if (editing) Pg.Check else Pg.Send, null, tint = c.onAccentFill, modifier = Modifier.size(20.dp).padding(end = if (editing) 0.dp else 2.dp))
        }
    }
}

/** The message field: transparent text on a smooth-cornered surface, focus = accent hairline + 3 dp glow outside. */
@Composable
fun PgField(draft: String, onDraft: (String) -> Unit, placeholder: String, focus: FocusRequester, modifier: Modifier = Modifier) {
    val c = Tin.c
    var focused by remember { mutableStateOf(false) }
    val shape = SmoothCornerShape(22.dp)
    val line by animateColorAsState(if (focused) c.accentFill else c.hairline, tween(PgMotion.SWITCH, easing = PgMotion.Out), label = "fieldLine")
    val glow by animateFloatAsState(if (focused) 1f else 0f, tween(PgMotion.SWITCH, easing = PgMotion.Out), label = "fieldGlow")
    val glowColor = c.accentInk.copy(alpha = .14f)
    val density = LocalDensity.current
    Box(
        modifier.heightIn(min = 44.dp).drawWithContent {
            // .ring .glow: a 3 dp stroke centred 1.5 dp outside the field's edge, following the same smooth outline.
            if (glow > 0f) {
                val g = with(density) { 1.5.dp.toPx() }
                val ring = SmoothCornerShape(23.5.dp).createOutline(Size(size.width + g * 2, size.height + g * 2), layoutDirection, this)
                val p = (ring as androidx.compose.ui.graphics.Outline.Generic).path
                translate(-g, -g) { drawPath(p, glowColor.copy(alpha = glowColor.alpha * glow), style = Stroke(g * 2)) }
            }
            drawContent()
        }.clip(shape).background(c.sf).border(1.dp, line, shape),
    ) {
        BasicTextField(
            draft, onDraft,
            Modifier.fillMaxWidth().heightIn(max = 140.dp).focusRequester(focus).onFocusChanged { focused = it.isFocused }
                .semantics { contentDescription = placeholder }.padding(horizontal = 16.dp, vertical = 11.dp),
            textStyle = PgBodyText.copy(lineHeight = 22.sp, color = c.ink), cursorBrush = SolidColor(c.pr),
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
            decorationBox = { inner ->
                Box {
                    if (draft.isEmpty()) Text(placeholder, style = PgBodyText.copy(lineHeight = 22.sp), color = c.ink2, maxLines = 1)
                    inner()
                }
            },
        )
    }
}

/** Reply / edit chip (`.ctx`): surface-2, accent start bar, title in accent-ink, the text muted, a cancel button. */
@Composable
fun PgCtxChip(ctx: ComposerCtx, onCancel: () -> Unit, modifier: Modifier = Modifier) {
    val c = Tin.c
    Row(
        modifier.clip(SmoothCornerShape(12.dp)).background(c.sf2).startBar(c.accentFill, 3.dp).padding(start = 13.dp, end = 2.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(ctx.title, style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.SemiBold), color = c.accentInk, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(ctx.text, style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp), color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        PgIconBtn(Pg.X, ctx.cancelLabel, onCancel, Modifier.layoutNegativeV(4.dp), tint = c.ink2, iconSize = 18.dp)
    }
}

/** CSS `margin-block: -4px`: measure normally, report 2 x [d] less height, so the chip stays 4 + 36 + 4 tall. */
private fun Modifier.layoutNegativeV(d: Dp) = this.layout { m, cs ->
    val p = m.measure(cs.copy(minHeight = 0, maxHeight = Constraints.Infinity))
    val dy = d.roundToPx()
    layout(p.width, max(0, p.height - 2 * dy)) { p.placeRelative(0, -dy) }
}

// ------------------------------------------------------------------ sheets (Layer.svelte on Android: bottom sheet)

/** A bottom sheet as the playground's Layer: raised surface, 24 dp top corners, a 28 x 4 notch, the scrim at 50 %. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PgSheet(onDismiss: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    val c = Tin.c
    ModalBottomSheet(
        onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = c.sfRaised, contentColor = c.ink, shape = SmoothCornerShape(topStart = 24.dp, topEnd = 24.dp),
        scrimColor = Color(0x800A100E), tonalElevation = 0.dp,
        dragHandle = { Box(Modifier.padding(top = 8.dp, bottom = 2.dp).size(28.dp, 4.dp).clip(RoundedCornerShape(2.dp)).background(c.ln)) },
    ) {
        Column(Modifier.navigationBarsPadding(), content = content)
    }
}

/** A menu row (`.mi`): 52 dp, 24 dp side padding, a 20 dp muted icon 16 dp before the label. */
@Composable
fun PgMenuItem(icon: ImageVector, label: String, danger: Boolean = false, onClick: () -> Unit) {
    val c = Tin.c
    val fg = if (danger) c.er else c.ink
    Row(
        Modifier.fillMaxWidth().heightIn(min = 52.dp).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 24.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(icon, null, tint = if (danger) c.er else c.ink2, modifier = Modifier.size(20.dp))
        Text(label, style = PgBodyText, color = fg)
    }
}

/** The dialog body inside a sheet (`.dlg`): 20/20/16 padding, a 17/24 title. */
@Composable
fun PgDialogBody(title: String, content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)) {
        Text(title, Modifier.padding(bottom = 4.dp), style = PgBodyText.copy(fontSize = 17.sp, lineHeight = 24.sp, fontWeight = FontWeight.SemiBold), color = Tin.c.ink)
        content()
    }
}

@Composable
fun PgDialogText(text: String) = Text(text, Modifier.padding(vertical = 6.dp), style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 19.sp), color = Tin.c.ink2)

enum class PgBtn { Primary, Quiet, Danger, QuietDanger }

/** `.btn`: 44 dp pill, 18 dp side padding, 15 semibold. */
@Composable
fun PgButton(label: String, kind: PgBtn, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val c = Tin.c
    val dangerBtn = if (c.dark) Color(0xFFC73A30) else Color(0xFFA12B23)
    val (bg, fg) = when (kind) {
        PgBtn.Primary -> c.accentFill to c.onAccentFill
        PgBtn.Danger -> dangerBtn to Color.White
        PgBtn.Quiet -> Color.Transparent to c.ink
        PgBtn.QuietDanger -> Color.Transparent to c.er
    }
    Box(modifier.heightIn(min = 44.dp).clip(RoundedCornerShape(22.dp)).background(bg).pgPress(onClick).padding(horizontal = 18.dp), contentAlignment = Alignment.Center) {
        Text(label, style = PgBodyText.copy(fontWeight = FontWeight.SemiBold), color = fg, maxLines = 1)
    }
}

/** Android stacks dialog actions full width, last one on top (`.acts.stack`: column-reverse). Pass them in reading order. */
@Composable
fun PgActions(vararg buttons: @Composable (Modifier) -> Unit) {
    Column(Modifier.fillMaxWidth().padding(top = 18.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        buttons.reversed().forEach { it(Modifier.fillMaxWidth()) }
    }
}

/** Fades [content] in and out with the cross-fade token (strips, notes). */
@Composable
fun PgFade(visible: Boolean, modifier: Modifier = Modifier, content: @Composable () -> Unit) =
    AnimatedVisibility(visible, modifier, enter = fadeIn(tween(PgMotion.CROSSFADE, easing = PgMotion.Out)), exit = fadeOut(tween(PgMotion.CROSSFADE, easing = PgMotion.Out))) { content() }
