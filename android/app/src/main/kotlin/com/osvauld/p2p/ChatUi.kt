package com.osvauld.p2p

import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Done
import androidx.compose.material.icons.rounded.DoneAll
import androidx.compose.material.icons.rounded.Schedule
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import uniffi.p2pcore.Attachment
import uniffi.p2pcore.AttachmentKind
import uniffi.p2pcore.DeliveryState
import uniffi.p2pcore.Message
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.time.temporal.ChronoUnit
import java.util.Locale

/** Honest ticks: one = saved on this phone, two = their phone has it, clock = waiting until you are both online. */
enum class Tick { One, Two, Clock }

/**
 * The tick for a message we sent. A clock only when we know sending cannot happen now: the link is
 * [Link.Offline], or we have no signal at all and this phone is offline or the message has sat a while.
 */
fun tickOf(m: Message, link: Link, online: Boolean, nowMs: Long): Tick = when {
    m.delivery == DeliveryState.DELIVERED -> Tick.Two
    link == Link.Offline -> Tick.Clock
    link == Link.Unknown && (!online || nowMs - m.at.toLong() > 30_000) -> Tick.Clock
    else -> Tick.One
}

@Composable
fun TickIcon(t: Tick, tint: Color, size: Dp = 15.dp) {
    val (icon, desc) = when (t) {
        Tick.One -> Icons.Rounded.Done to "Saved on your phone"
        Tick.Two -> Icons.Rounded.DoneAll to "On their phone"
        Tick.Clock -> Icons.Rounded.Schedule to "Waiting until you are both online"
    }
    Icon(icon, desc, tint = tint, modifier = Modifier.size(size))
}

private val HM = DateTimeFormatter.ofPattern("HH:mm")
private val DM = DateTimeFormatter.ofPattern("d MMM", Locale.getDefault())
private fun zone() = ZoneId.systemDefault()
private fun dateOf(ms: Long) = Instant.ofEpochMilli(ms).atZone(zone()).toLocalDate()

fun msClock(ms: Long): String = Instant.ofEpochMilli(ms).atZone(zone()).format(HM)

private fun daysAgo(ms: Long, nowMs: Long) = ChronoUnit.DAYS.between(dateOf(ms), dateOf(nowMs))

/** Chat list time: "12:41", "Yesterday", "Mon", "12 Sep". */
fun listTime(ms: Long, nowMs: Long): String = when (val d = daysAgo(ms, nowMs)) {
    0L -> msClock(ms)
    1L -> "Yesterday"
    in 2..6 -> dateOf(ms).dayOfWeek.getDisplayName(TextStyle.SHORT, Locale.getDefault())
    else -> if (d < 0) msClock(ms) else dateOf(ms).format(DM)
}

/** Day pill in a conversation: "Today", "Yesterday", "Monday", "12 Sep". */
fun dayLabel(ms: Long, nowMs: Long): String = when (val d = daysAgo(ms, nowMs)) {
    0L -> "Today"
    1L -> "Yesterday"
    in 2..6 -> dateOf(ms).dayOfWeek.getDisplayName(TextStyle.FULL, Locale.getDefault())
    else -> if (d < 0) "Today" else dateOf(ms).format(DM)
}

fun sameDay(a: Long, b: Long): Boolean = dateOf(a) == dateOf(b)

/** The viewer's top line: "Today, 16:02". */
fun viewerWhen(ms: Long, nowMs: Long): String = "${dayLabel(ms, nowMs)}, ${msClock(ms)}"

/** "2.4 MB", "184 MB", "812 KB". */
fun sizeText(bytes: Long): String = when {
    bytes >= 1_000_000_000 -> String.format(Locale.US, "%.1f GB", bytes / 1e9)
    bytes >= 100_000_000 -> "${bytes / 1_000_000} MB"
    bytes >= 1_000_000 -> String.format(Locale.US, "%.1f MB", bytes / 1e6)
    bytes >= 1000 -> "${bytes / 1000} KB"
    else -> "$bytes B"
}

/** "PDF", "MP4", ... from the name's extension, else the MIME subtype. */
fun typeLabel(a: Attachment): String =
    a.name.substringAfterLast('.', "").takeIf { it.length in 1..5 }?.uppercase() ?: a.mime.substringAfter('/', "").uppercase().take(5)

fun Attachment.isImage() = kind == AttachmentKind.FILE && mime.startsWith("image/")
fun Attachment.isVideo() = kind == AttachmentKind.FILE && mime.startsWith("video/")

/** First word of a name, for "Arjun deleted this message". */
fun firstName(name: String) = name.trim().substringBefore(' ').ifBlank { name }

/** One row of the Chats tab, already worded. */
data class ChatRowUi(
    val did: String, val name: String, val preview: String, val time: String,
    val mine: Boolean, val tick: Tick, val unread: Int,
)
