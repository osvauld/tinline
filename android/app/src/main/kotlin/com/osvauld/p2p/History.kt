package com.osvauld.p2p

import uniffi.p2pcore.CallRecord
import uniffi.p2pcore.Contact
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.time.temporal.ChronoUnit
import java.util.Locale

/** Name to show for a contact: the alias we gave them if any, else what they call themselves. */
fun Contact.display(): String = alias?.takeIf { it.isNotBlank() } ?: name.ifBlank { "Unnamed" }

/** One call in the Recent section / a contact's history, already worded. */
data class RecentCall(
    val did: String, val name: String, val kind: Kind, val whenText: String,
    /** History row title, e.g. "Incoming · 18 min". Falls back to the kind's label. */
    val title: String = "",
    /** History row sub-line, e.g. "Today, 12:31". */
    val whenLong: String = "",
    val direct: Boolean? = null,
    val callId: String = "",
) {
    enum class Kind { Incoming, Outgoing, Missed, Unreached, Away }
}

private val HM = DateTimeFormatter.ofPattern("HH:mm")
private val DM = DateTimeFormatter.ofPattern("d MMM", Locale.getDefault())

private fun zone() = ZoneId.systemDefault()
private fun dayOf(secs: Long) = Instant.ofEpochSecond(secs).atZone(zone()).toLocalDate()
private fun hm(secs: Long) = Instant.ofEpochSecond(secs).atZone(zone()).format(HM)
private fun weekday(d: LocalDate) = d.dayOfWeek.getDisplayName(TextStyle.SHORT, Locale.getDefault())
private fun daysBetween(at: Long, now: Long) = ChronoUnit.DAYS.between(dayOf(at), dayOf(now))

/** Relative "when" for the Recent list: "4 min ago", "12:31", "Yesterday · 08:15", "Mon · 09:12", "12 Sep". */
fun whenShort(at: Long, now: Long): String {
    val ago = now - at
    val days = daysBetween(at, now)
    return when {
        ago < 60 -> "just now"
        ago < 3600 -> "${ago / 60} min ago"
        days <= 0L -> hm(at)
        days == 1L -> "Yesterday · ${hm(at)}"
        days < 7 -> "${weekday(dayOf(at))} · ${hm(at)}"
        else -> dayOf(at).format(DM)
    }
}

/** "Today, 12:31" / "Yesterday, 19:04" / "Mon, 09:12" / "12 Sep, 09:12". */
fun whenLong(at: Long, now: Long): String {
    val days = daysBetween(at, now)
    val day = when {
        days <= 0L -> "Today"
        days == 1L -> "Yesterday"
        days < 7 -> weekday(dayOf(at))
        else -> dayOf(at).format(DM)
    }
    return "$day, ${hm(at)}"
}

/** "42 s", "18 min", "1 h 5 min". */
fun durationText(secs: Long): String = when {
    secs < 60 -> "$secs s"
    secs < 3600 -> "${(secs + 30) / 60} min"
    else -> "${secs / 3600} h ${secs % 3600 / 60} min"
}

fun CallRecord.toRecent(contacts: Map<String, Contact>, now: Long): RecentCall {
    val at = startedAt.toLong()
    val kind = when {
        reason == "unavailable" -> RecentCall.Kind.Away
        missed -> RecentCall.Kind.Missed
        !incoming && reason == "unreachable" -> RecentCall.Kind.Unreached
        incoming -> RecentCall.Kind.Incoming
        else -> RecentCall.Kind.Outgoing
    }
    val label = when (kind) {
        RecentCall.Kind.Missed -> "Missed"
        RecentCall.Kind.Unreached -> "Couldn’t reach"
        RecentCall.Kind.Away -> "While not available"
        RecentCall.Kind.Incoming -> "Incoming"
        RecentCall.Kind.Outgoing -> "Outgoing"
    }
    val secs = durationSecs.toLong()
    val detail = when {
        secs > 0 -> durationText(secs)
        kind == RecentCall.Kind.Outgoing || kind == RecentCall.Kind.Incoming -> when (reason) {
            "declined", "declined_local" -> "declined"
            "cancelled" -> "cancelled"
            "no_answer" -> "no answer"
            "busy" -> "busy"
            else -> null
        }
        else -> null
    }
    val name = contacts[peerDid]?.display() ?: peerName.ifBlank { "Unknown" }
    return RecentCall(
        peerDid, name, kind, "$label · ${whenShort(at, now)}",
        title = if (detail != null) "$label · $detail" else label, whenLong = whenLong(at, now),
        direct = if (secs > 0) direct else null, callId = callId,
    )
}

/** Sub-line under a contact's name on Home, from their newest call (`history` is newest first). */
fun contactSubLine(did: String, history: List<CallRecord>, now: Long): String {
    val r = history.firstOrNull { it.peerDid == did } ?: return "Not called yet"
    val at = r.startedAt.toLong()
    return when {
        r.reason == "unavailable" -> "Last call ${dayWord(at, now)}"
        r.missed -> "Missed call · ${whenShort(at, now)}"
        !r.incoming && r.reason == "unreachable" -> "Couldn’t reach · ${whenShort(at, now)}"
        else -> "Last call ${dayWord(at, now)}"
    }
}

private fun dayWord(at: Long, now: Long): String {
    val days = daysBetween(at, now)
    return when {
        days <= 0L -> "today"
        days == 1L -> "yesterday"
        days < 7 -> dayOf(at).dayOfWeek.getDisplayName(TextStyle.FULL, Locale.getDefault())
        else -> dayOf(at).format(DM)
    }
}

/** Epoch seconds of 08:00 local tomorrow. */
fun tomorrowMorning(now: Long = System.currentTimeMillis() / 1000): Long =
    dayOf(now).plusDays(1).atTime(8, 0).atZone(zone()).toEpochSecond()

fun clockTime(secs: Long): String = hm(secs)
