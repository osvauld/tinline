package com.osvauld.p2p

import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.tween
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.lerp
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
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
 * Building blocks for the group screens (the .dc.html boards in docs/design/groups).
 * Sizes and colours follow the boards' CSS through the TinColors / TinType tokens.
 */

// ------------------------------------------------------------------ avatars, pills

/** Group avatar (playground Avatar `group`): a circle in surface-2 with the accent-ink people glyph at 46 %. [tint] and [radius] are
 * kept for older callers and ignored; [dim] (a group we left or were removed from) fades it to 72 %. */
@Composable
fun GroupAvatar(tint: Int, size: Dp, dim: Boolean = false, radius: Dp = size / 2, gid: String? = null) {
    val c = Tin.c
    val art = remember(gid, size) { FingerprintCache.ofGroup(gid, size.value) }
    if (art != null) { GroupMandala(art, size, Modifier.alpha(if (dim) 0.72f else 1f)); return }
    Box(Modifier.size(size).alpha(if (dim) 0.72f else 1f).clip(CircleShape).background(c.sf2), contentAlignment = Alignment.Center) {
        Icon(GIcons.Users, null, tint = c.accentInk, modifier = Modifier.size(size * 0.46f))
    }
}

/** A group's mandala (from its group id) in a rounded square, corner radius 28 % of [size]; people are circles. */
@Composable
fun GroupMandala(art: FingerprintDrawing, size: Dp, modifier: Modifier = Modifier) {
    val c = Tin.c
    val (bg, fg) = if (c.dark) art.darkBg to art.darkFg else art.lightBg to art.lightFg
    val gold = c.gold
    Box(modifier.size(size).clip(androidx.compose.foundation.shape.RoundedCornerShape(size * 0.28f)).background(bg)
        .drawBehind { drawFingerprint(art, fg, gold) })
}

/** Person avatar in group surfaces (GroupInfo / MemberSheet use the one Avatar with the person's hue and picture, contact or not). */
@Composable
fun PersonAvatar(name: String, did: String, size: Dp, contact: Boolean = true, isMe: Boolean = false) {
    if (isMe) { SelfAvatar(name, size); return }
    Avatar(name, did, size)
}

/** Three-segment flag meter (playground `.meter`): 6 dp bars, radius 3, gap 6; [filled] in danger-btn, the next one ([yours]) at 45 %. */
@Composable
fun FlagMeter(filled: Int, total: Int = GroupText.FLAGS_TO_REMOVE, yours: Boolean = false, desc: String = pluralStringResource(R.plurals.groupsc_flags_of, total, filled, total)) {
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

/** Per-author name colour in group chats (relay, amber text, teal, pink), stable by DID. */
@Composable
fun authorColor(did: String): Color {
    val c = Tin.c
    return when (did) {
        else -> listOf(c.relay, c.threadText, c.pr, c.onPk)[(did.hashCode() and 0x7fffffff) % 4]
    }
}

// ------------------------------------------------------------------ time words

private fun mdhm() = localFormatter("MMMdHm", "MMM d, HH:mm")
internal fun at(ms: Long) = Instant.ofEpochMilli(ms).atZone(ZoneId.systemDefault())

/** "Oct 10, 14:02" */
fun whenFull(ms: Long): String = at(ms).format(mdhm())

/** "Yesterday, 03:12" / "Today, 09:00" / "Oct 3, 10:00" */
fun dayAndTime(ms: Long, nowMs: Long): String =
    if (ChronoUnit.DAYS.between(at(ms).toLocalDate(), at(nowMs).toLocalDate()) in 0..1) viewerWhen(ms, nowMs) else whenFull(ms)
