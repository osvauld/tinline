package com.osvauld.p2p

import uniffi.p2pcore.Attachment
import uniffi.p2pcore.AttachmentKind
import uniffi.p2pcore.DeliveryState
import uniffi.p2pcore.Message
import uniffi.p2pcore.ReceiptTick
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit
import java.util.Locale

/**
 * Honest status of a message we sent, for words and the chat list ("Waiting"). The bubble draws the receipt tick
 * itself (ReceiptTicks.kt). [Two] and [Read] come only from the contact's signed receipts (core `ReceiptTick`);
 * [One] = sent or saved here, not confirmed yet; [Clock] = it cannot go out now (waiting until you are both online).
 */
enum class Tick { One, Two, Read, Clock }

/**
 * The tick for a message we sent. [receipt] is the core's decision; the link only says whether a message still on
 * this phone can go out now (a clock when the link is [Link.Offline], or with no signal at all and this phone
 * offline or the message has sat a while).
 */
fun tickOf(m: Message, receipt: ReceiptTick?, link: Link, online: Boolean, nowMs: Long): Tick = when {
    receipt == ReceiptTick.READ -> Tick.Read
    receipt == ReceiptTick.DELIVERED -> Tick.Two
    m.delivery == DeliveryState.PENDING && link == Link.Offline -> Tick.Clock
    m.delivery == DeliveryState.PENDING && link == Link.Unknown && (!online || nowMs - m.at.toLong() > 30_000) -> Tick.Clock
    else -> Tick.One
}


private val HM = DateTimeFormatter.ofPattern("HH:mm")
private fun zone() = ZoneId.systemDefault()
private fun dateOf(ms: Long) = Instant.ofEpochMilli(ms).atZone(zone()).toLocalDate()

fun msClock(ms: Long): String = Instant.ofEpochMilli(ms).atZone(zone()).format(HM)

private fun daysAgo(ms: Long, nowMs: Long) = ChronoUnit.DAYS.between(dateOf(ms), dateOf(nowMs))

/** Chat list time: "12:41", "Yesterday", "Monday", "12 Sep" (`uniffi.present.listTime`). */
fun listTime(ms: Long, nowMs: Long): String =
    uniffi.present.listTime(ms / 1000, nowMs / 1000, zone().rules.getOffset(Instant.ofEpochMilli(nowMs)).totalSeconds)

/** Day pill in a conversation (Conversation.svelte `dayLabel`): "Today", "Yesterday", else "Thursday 8 Oct" (with the year
 *  when it isn't this year). */
fun dayLabel(ms: Long, nowMs: Long): String = when (daysAgo(ms, nowMs)) {
    0L -> appString(R.string.chat_day_today)
    1L -> appString(R.string.chat_day_yesterday)
    else -> if (daysAgo(ms, nowMs) < 0) appString(R.string.chat_day_today) else dateOf(ms).let { d ->
        d.format(if (d.year == dateOf(nowMs).year) DAY_DM else DAY_DMY)
    }
}
private val DAY_DM = DateTimeFormatter.ofPattern("EEEE d MMM", Locale.getDefault())
private val DAY_DMY = DateTimeFormatter.ofPattern("EEEE d MMM yyyy", Locale.getDefault())

fun sameDay(a: Long, b: Long): Boolean = dateOf(a) == dateOf(b)

/** The viewer's top line: "Today, 16:02". */
fun viewerWhen(ms: Long, nowMs: Long): String = "${dayLabel(ms, nowMs)}, ${msClock(ms)}"

/** "2.4 MB", "34 MB", "812 KB" (`uniffi.present.bytes`). */
fun sizeText(bytes: Long): String = uniffi.present.bytes(bytes.toULong())

/** "PDF", "MP4", ... from the name's extension, else the MIME subtype. */
fun typeLabel(a: Attachment): String =
    a.name.substringAfterLast('.', "").takeIf { it.length in 1..5 }?.uppercase() ?: a.mime.substringAfter('/', "").uppercase().take(5)

fun Attachment.isImage() = kind == AttachmentKind.FILE && mime.startsWith("image/")
fun Attachment.isVideo() = kind == AttachmentKind.FILE && mime.startsWith("video/")

/** First word of a name, for "Arjun deleted this message". */
fun firstName(name: String) = name.trim().substringBefore(' ').ifBlank { name }

/** One row of the Chats tab, already worded. */
/** Subtitle of the "You" conversation (saved items) in the list and its header. */
fun selfSub(): String = appString(R.string.chat_self_sub)

data class ChatRowUi(
    val did: String, val name: String, val preview: String, val time: String,
    val mine: Boolean, val tick: Tick, val unread: Int,
    /** The preview is a block state ("Blocked" / "Blocked you"), shown in red. */
    val alert: Boolean = false,
    /** A group conversation (the Chats tab lists 1:1 and groups together); [did] is then the group id. */
    val group: Boolean = false,
    /** Last activity, unix ms: the list's sort key. */
    val atMs: Long = 0,
    /** My last message's tick (core `chat_list_marks`); null until read. [markSlot] keeps its room from the start. */
    val mark: TickVis? = null,
    val markSlot: Boolean = false,
    /** Identifies the marked message (the glyph animates only when the same message's state changes). */
    val markKey: String = "",
    /** The last message's id: a mark shows only if it is for exactly this message. */
    val lastId: String? = null,
)
