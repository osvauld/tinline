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
 * Building blocks for the PLACEHOLDER groups / privacy screens (the .dc.html boards in docs/design/groups).
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

/** Group avatar (playground Avatar `group`): a circle in surface-2 with the accent-ink people glyph at 46 %. [tint] and [radius] are
 * kept for older callers and ignored; [dim] (a group we left or were removed from) fades it to 72 %. */
@Composable
fun GroupAvatar(tint: Int, size: Dp, dim: Boolean = false, radius: Dp = size / 2) {
    val c = Tin.c
    Box(Modifier.size(size).alpha(if (dim) 0.72f else 1f).clip(CircleShape).background(c.sf2), contentAlignment = Alignment.Center) {
        Icon(GIcons.Users, null, tint = c.accentInk, modifier = Modifier.size(size * 0.46f))
    }
}

/**
 * Person avatar. Contacts get the board's tints (Arjun relay, Meera teal, Sam/Priya amber); people who are
 * only fellow group members (not contacts) are neutral grey, as on the boards.
 */
@Composable
fun PersonAvatar(name: String, did: String, size: Dp, contact: Boolean = true, isMe: Boolean = false) {
    val c = Tin.c
    val (bg, fg) = when {
        isMe || !contact -> c.sf2 to c.ink2
        did == FakeGroupsPrivacy.ARJUN -> c.relayC to c.onRelayC
        did == FakeGroupsPrivacy.MEERA -> c.prc to c.onPrc
        did == FakeGroupsPrivacy.SAM || did == FakeGroupsPrivacy.PRIYA -> c.warn to c.onWarn
        else -> null to null
    }
    if (bg == null || fg == null) { Avatar(name, did, size); return }
    Box(Modifier.size(size).clip(CircleShape).background(bg), contentAlignment = Alignment.Center) {
        Text(if (isMe) "YO" else initialsOf(name), color = fg, style = TinType.titleM.copy(fontSize = (size.value * 0.36f).sp, lineHeight = (size.value * 0.44f).sp, fontWeight = FontWeight.Bold))
    }
}

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

/** Three-segment flag meter (playground `.meter`): 6 dp bars, radius 3, gap 6; [filled] in danger-btn, the next one ([yours]) at 45 %. */
@Composable
fun FlagMeter(filled: Int, total: Int = FakeGroupsPrivacy.FLAGS_TO_REMOVE, yours: Boolean = false, desc: String = "$filled of $total flags") {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().semantics { contentDescription = desc }, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        repeat(total) { i ->
            val col = when {
                i < filled -> c.dangerBtn
                yours && i == filled -> lerp(c.ln, c.dangerBtn, 0.45f)
                else -> c.ln
            }
            val anim by animateColorAsState(col, tween(GMotion.ENTER, easing = GMotion.EaseOut), label = "meter")
            Box(Modifier.weight(1f).height(6.dp).clip(RoundedCornerShape(3.dp)).background(anim))
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
        IconBtn(Icons.AutoMirrored.Rounded.ArrowBack, "Back", onBack)
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

/** Board `.sys`: centred grey pill line. */
@Composable
fun ColumnScope.SysLine(text: String, onClick: (() -> Unit)? = null) {
    val c = Tin.c
    Box(
        Modifier.align(Alignment.CenterHorizontally).widthIn(max = 320.dp).clip(RoundedCornerShape(50)).background(c.sf2)
            .let { if (onClick != null) it.heightIn(min = 48.dp).clickable(role = Role.Button, onClick = onClick) else it }
            .padding(horizontal = 12.dp, vertical = 4.dp),
        contentAlignment = Alignment.Center,
    ) { Text(text, style = TinType.caption.copy(fontWeight = FontWeight.Normal), color = c.ink2, textAlign = TextAlign.Center) }
}

/** Board `.comp`: optional attach, rounded field, round send. */
@Composable
fun Composer(draft: String, onDraft: (String) -> Unit, placeholder: String, onSend: () -> Unit, onAttach: (() -> Unit)? = null) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        if (onAttach != null) IconBtn(Icons.Rounded.AttachFile, "Attach", onAttach)
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
            Icon(Icons.AutoMirrored.Rounded.ArrowForward, null, tint = if (draft.isBlank()) c.ink2 else c.onPr)
        }
    }
}

/** Replaces the composer: board's bordered footer with a centred hint and optional buttons. */
@Composable
fun BlockedFooter(hint: String, buttons: (@Composable RowScope.() -> Unit)? = null) {
    val c = Tin.c
    Column(Modifier.fillMaxWidth().border(width = 1.dp, color = c.ln, shape = RoundedCornerShape(0.dp)).padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Hint(hint, Modifier.fillMaxWidth(), align = TextAlign.Center)
        if (buttons != null) Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp), content = buttons)
    }
}

/** Board's tinted notice card (erc for removal / blocked, warn for lost contact). */
@Composable
fun NoticeCard(bg: Color, fg: Color, title: String, body: String, modifier: Modifier = Modifier, extra: @Composable ColumnScope.() -> Unit = {}) {
    Column(modifier.fillMaxWidth().padding(top = 8.dp).clip(SmoothCornerShape(20.dp)).background(bg).padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, style = TinType.bodyL.copy(fontWeight = FontWeight.Bold), color = fg, modifier = Modifier.semantics { heading() })
        Text(body, style = TinType.bodyM, color = fg)
        extra()
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

/** Per-author name colour in group chats (relay, amber text, teal, pink), stable by DID. */
@Composable
fun authorColor(did: String): Color {
    val c = Tin.c
    return when (did) {
        FakeGroupsPrivacy.SAM -> c.relay
        FakeGroupsPrivacy.PRIYA -> c.threadText
        FakeGroupsPrivacy.LEO -> c.pr
        else -> listOf(c.relay, c.threadText, c.pr, c.onPk)[(did.hashCode() and 0x7fffffff) % 4]
    }
}

// ------------------------------------------------------------------ time words

private val MDHM = DateTimeFormatter.ofPattern("MMM d, HH:mm", Locale.getDefault())
private val MD = DateTimeFormatter.ofPattern("MMM d", Locale.getDefault())
private fun at(ms: Long) = Instant.ofEpochMilli(ms).atZone(ZoneId.systemDefault())

/** "Oct 10, 14:02" */
fun whenFull(ms: Long): String = at(ms).format(MDHM)
/** "Oct 10" */
fun whenDate(ms: Long): String = at(ms).format(MD)

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

/** "Yesterday, 03:12" / "Today, 09:00" / "Oct 3, 10:00" */
fun dayAndTime(ms: Long, nowMs: Long): String =
    if (ChronoUnit.DAYS.between(at(ms).toLocalDate(), at(nowMs).toLocalDate()) in 0..1) viewerWhen(ms, nowMs) else whenFull(ms)

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
