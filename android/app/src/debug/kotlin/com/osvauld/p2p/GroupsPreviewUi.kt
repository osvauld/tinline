package com.osvauld.p2p

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.tween
import androidx.compose.runtime.getValue
import androidx.compose.ui.graphics.lerp
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.ArrowForward
import androidx.compose.material.icons.outlined.Groups
import androidx.compose.material.icons.rounded.AttachFile
import androidx.compose.material.icons.rounded.Science
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit
import java.util.Locale

/*
 * Building blocks for the PLACEHOLDER groups / privacy screens (debug builds only; moved out of main by the UI retirement) (the .dc.html boards in docs/design/groups).
 * Sizes and colours follow the boards' CSS through the TinColors / TinType tokens.
 */

// ------------------------------------------------------------------ page + placeholder marker

/** [Page] with the "placeholder data" strip on top, so a preview screen is never mistaken for a working one. */
@Composable
fun PreviewPage(content: @Composable ColumnScope.() -> Unit) = Page {
    PlaceholderStrip()
    content()
}

@Composable
fun PlaceholderStrip() {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().background(c.warn).padding(horizontal = 16.dp, vertical = 4.dp)
            .semantics(mergeDescendants = true) { contentDescription = "Preview with placeholder data. Nothing here is sent." },
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Icon(Icons.Rounded.Science, null, tint = c.onWarn, modifier = Modifier.size(14.dp))
        Text("Preview · placeholder data, nothing is sent", style = TinType.caption, color = c.onWarn, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

// ------------------------------------------------------------------ avatars, pills

@Composable
fun RolePill(role: GroupRole) {
    val c = Tin.c
    val (bg, fg) = when (role) {
        GroupRole.Owner -> c.prc to c.onPrc
        GroupRole.Admin -> c.relayC to c.onRelayC
        GroupRole.Host -> c.warn to c.onWarn
    }
    SmallPill(role.name, bg, fg)
}

/** Board `.pill`: 22 dp, 12 sp semibold. */
@Composable
fun SmallPill(text: String, bg: Color, fg: Color) {
    Box(Modifier.heightIn(min = 22.dp).clip(RoundedCornerShape(50)).background(bg).padding(horizontal = 8.dp), contentAlignment = Alignment.Center) {
        Text(text, style = TinType.caption.copy(fontWeight = FontWeight.SemiBold), color = fg)
    }
}

/** Filter chip (All / People / Groups): 32 dp pill inside a 48 dp touch target. */
@Composable
fun FilterChipPill(label: String, selected: Boolean, onClick: () -> Unit) {
    val c = Tin.c
    Box(
        Modifier.heightIn(min = 48.dp).clip(RoundedCornerShape(50)).clickable(role = Role.Tab, onClick = onClick).semantics { this.selected = selected },
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.height(32.dp).clip(RoundedCornerShape(50)).background(if (selected) c.prc else c.sf2).padding(horizontal = 14.dp), contentAlignment = Alignment.Center) {
            Text(label, style = TinType.label, color = if (selected) c.onPrc else c.ink2)
        }
    }
}

/** Board `.seg`: a pill split into options; the chosen one is teal-tinted. */
@Composable
fun <T> Segmented(options: List<T>, selected: T, label: (T) -> String, onSelect: (T) -> Unit, desc: String) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(50)).border(1.dp, c.ln2, RoundedCornerShape(50)).semantics { contentDescription = desc }) {
        options.forEachIndexed { i, o ->
            val on = o == selected
            Box(
                Modifier.weight(1f).heightIn(min = 48.dp).background(if (on) c.prc else Color.Transparent)
                    .clickable(role = Role.RadioButton, onClick = { onSelect(o) }).semantics { this.selected = on },
                contentAlignment = Alignment.Center,
            ) { Text(label(o), style = TinType.label, color = if (on) c.onPrc else c.ink2) }
            if (i < options.lastIndex) Box(Modifier.width(1.dp).heightIn(min = 48.dp).background(c.ln2))
        }
    }
}

// ------------------------------------------------------------------ conversation pieces

/** Header of a 1:1 or group chat: back, avatar, title and an optional coloured sub-line. */
@Composable
fun ChatHeader(
    title: String, onBack: () -> Unit, sub: String? = null, subColor: Color = Tin.c.ink2,
    avatar: (@Composable () -> Unit)? = null, onTitle: (() -> Unit)? = null, actions: @Composable RowScope.() -> Unit = {},
) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(start = 4.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        IconBtn(PgIcons.Back, "Back", onBack)
        Row(
            Modifier.weight(1f).heightIn(min = 48.dp).clip(SmoothCornerShape(12.dp))
                .let { if (onTitle != null) it.clickable(role = Role.Button, onClickLabel = "Open details", onClick = onTitle) else it }
                .padding(end = 4.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            avatar?.invoke()
            Column(Modifier.weight(1f)) {
                Text(title, style = TinType.titleM, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (sub != null) Text(sub, style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = subColor, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        actions()
    }
}

/** Board `.msg`: max 300 dp, radius 18 with a 6 dp tail corner. */
@Composable
fun ColumnScope.Bubble(
    text: String, outgoing: Boolean, author: String? = null, authorColor: Color = Tin.c.pr,
    tick: String? = null, tickColor: Color? = null, greyed: Boolean = false, faded: Boolean = false,
    tickIcon: (@Composable () -> Unit)? = null,
) {
    val c = Tin.c
    val shape = bubbleShape(BubblePos.First, outgoing)
    val bg = when { greyed -> c.sf2; outgoing -> c.prc; else -> c.bubIn }
    val fg = when { greyed -> c.ink2; outgoing -> c.onPrc; else -> c.ink }
    Column(
        Modifier.align(if (outgoing) Alignment.End else Alignment.Start).widthIn(max = 300.dp).alpha(if (faded) 0.7f else 1f)
            .clip(shape).background(bg).let { if (!outgoing && !greyed) it.border(1.dp, c.bubInLine, shape) else it }
            .padding(horizontal = 12.dp, vertical = 8.dp)
            .semantics(mergeDescendants = true) {},
    ) {
        if (author != null) Text(author, style = TinType.caption.copy(fontWeight = FontWeight.Bold), color = authorColor)
        Text(text, style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 21.sp), color = fg)
        if (tick != null) Row(Modifier.align(Alignment.End).padding(top = 2.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(3.dp)) {
            tickIcon?.invoke()
            Text(tick, style = TinType.caption.copy(fontSize = 11.sp, lineHeight = 14.sp), color = tickColor ?: if (outgoing) c.onPrc else c.ink2)
        }
    }
}

/** Board `.comp`: optional attach, rounded field, round send. */
@Composable
fun Composer(draft: String, onDraft: (String) -> Unit, placeholder: String, onSend: () -> Unit, onAttach: (() -> Unit)? = null) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        if (onAttach != null) IconBtn(PgIcons.Clip, "Attach", onAttach)
        BasicTextField(
            draft, onDraft, Modifier.weight(1f).heightIn(min = 48.dp).semantics { contentDescription = placeholder },
            textStyle = TinType.bodyL.copy(fontSize = 15.sp, color = c.ink), cursorBrush = SolidColor(c.pr),
            decorationBox = { inner ->
                Box(Modifier.heightIn(min = 48.dp).clip(SmoothCornerShape(24.dp)).background(c.sf).border(1.dp, c.ln, SmoothCornerShape(24.dp)).padding(horizontal = 16.dp, vertical = 12.dp),
                    contentAlignment = Alignment.CenterStart) {
                    if (draft.isEmpty()) Text(placeholder, style = TinType.bodyL.copy(fontSize = 15.sp), color = c.ink2)
                    inner()
                }
            },
        )
        Box(Modifier.size(48.dp).clip(CircleShape).background(if (draft.isBlank()) c.sf3 else c.pr)
            .clickable(enabled = draft.isNotBlank(), role = Role.Button, onClick = onSend).semantics { contentDescription = "Send" },
            contentAlignment = Alignment.Center) {
            Icon(PgIcons.Next, null, tint = if (draft.isBlank()) c.ink2 else c.onPr)
        }
    }
}

/** Sheet menu row (board `.mi`): icon, title, optional sub-line, 56 dp minimum. */
@Composable
fun SheetItem(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, sub: String? = null, color: Color = Tin.c.ink, onClick: () -> Unit) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 24.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(icon, null, tint = if (color == c.ink) c.ink2 else color)
        Column(Modifier.weight(1f)) {
            Text(title, style = TinType.bodyL, color = color)
            if (sub != null) Text(sub, style = TinType.bodyM, color = if (color == c.ink) c.ink2 else color)
        }
    }
}

// ------------------------------------------------------------------ time words

/** "2 h ago" today, else "Oct 3". */
fun agoOrDate(ms: Long, nowMs: Long): String {
    val d = nowMs - ms
    return when {
        d < 60_000 -> "Just now"
        d < 3_600_000 -> "${d / 60_000} min ago"
        d < 24 * 3_600_000 && sameDay(ms, nowMs) -> "${d / 3_600_000} h ago"
        else -> whenDate(ms)
    }
}

/** "2 h", "3 days" for durations. */
fun durationWords(ms: Long): String = when {
    ms < 3_600_000 -> "${(ms / 60_000).coerceAtLeast(1)} min"
    ms < 48 * 3_600_000 -> "${ms / 3_600_000} h"
    else -> "${ms / (24 * 3_600_000)} days"
}

/** "In 5 h", "Tomorrow", "In 2 days" */
fun inWords(ms: Long, nowMs: Long): String {
    val days = ChronoUnit.DAYS.between(at(nowMs).toLocalDate(), at(ms).toLocalDate())
    return when {
        ms <= nowMs -> "Due now"
        days == 0L -> "In ${((ms - nowMs) / 3_600_000).coerceAtLeast(1)} h"
        days == 1L -> "Tomorrow"
        else -> "In $days days"
    }
}

/**
 * Extended FAB that shrinks to its icon while the list is scrolled ([wide] false), so it never sits on
 * top of the last row's text. The label stays as the content description when shrunk.
 */
@Composable
fun ShrinkFab(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, wide: Boolean, modifier: Modifier = Modifier, onClick: () -> Unit) {
    val c = Tin.c
    Row(
        modifier.padding(end = 20.dp, bottom = 20.dp).depth(Depth.Lift, SmoothCornerShape(18.dp)).clip(SmoothCornerShape(18.dp)).background(c.prc)
            .clickable(role = Role.Button, onClick = onClick).semantics(mergeDescendants = true) {}
            .heightIn(min = 56.dp).widthIn(min = 56.dp).animateContentSize().padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
    ) {
        Icon(icon, if (wide) null else label, tint = c.onPrc)
        if (wide) Text(label, style = TinType.button.copy(fontSize = 15.sp), color = c.onPrc, maxLines = 1)
    }
}


/** "Oct 10" */
fun whenDate(ms: Long): String = at(ms).format(DateTimeFormatter.ofPattern("MMM d", Locale.getDefault()))
