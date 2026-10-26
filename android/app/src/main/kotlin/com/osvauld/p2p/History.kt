package com.osvauld.p2p

import uniffi.p2pcore.CallRecord
import uniffi.p2pcore.Contact
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.time.temporal.ChronoUnit
import java.util.Locale

/** Name to show for a contact: the alias we gave them if any, else what they call themselves. */
fun Contact.display(): String = uniffi.present.displayName(alias, name)

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
private fun dm() = localFormatter("dMMM", "d MMM")

private fun zone() = ZoneId.systemDefault()
private fun dayOf(secs: Long) = Instant.ofEpochSecond(secs).atZone(zone()).toLocalDate()
private fun hm(secs: Long) = Instant.ofEpochSecond(secs).atZone(zone()).format(HM)
private fun daysBetween(at: Long, now: Long) = ChronoUnit.DAYS.between(dayOf(at), dayOf(now))

/** UTC offset (seconds east) at [at]: the one zone fact `uniffi.present` needs for the time words. */
private fun utcOffset(at: Long) = zone().rules.getOffset(Instant.ofEpochSecond(at)).totalSeconds

/** Relative "when" for the Recent list: "4 min ago", "12:31", "Yesterday · 08:15", "Mon · 09:12", "12 Sep". */
fun whenShort(at: Long, now: Long): String = uniffi.present.whenShort(at, now, utcOffset(now))

/** "Today, 12:31" / "Yesterday, 19:04" / "Mon, 09:12" / "12 Sep, 09:12". */
fun whenLong(at: Long, now: Long): String = uniffi.present.whenLong(at, now, utcOffset(now))

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
        RecentCall.Kind.Missed -> appString(R.string.history_kind_missed)
        RecentCall.Kind.Unreached -> appString(R.string.history_kind_unreached)
        RecentCall.Kind.Away -> appString(R.string.history_kind_away)
        RecentCall.Kind.Incoming -> appString(R.string.history_kind_incoming)
        RecentCall.Kind.Outgoing -> appString(R.string.history_kind_outgoing)
    }
    val secs = durationSecs.toLong()
    val detail = when {
        secs > 0 -> uniffi.present.durationText(secs)
        kind == RecentCall.Kind.Outgoing || kind == RecentCall.Kind.Incoming -> when (reason) {
            "declined", "declined_local" -> appString(R.string.history_detail_declined)
            "cancelled" -> appString(R.string.history_detail_cancelled)
            "no_answer" -> appString(R.string.history_detail_no_answer)
            "busy" -> appString(R.string.history_detail_busy)
            "answered_elsewhere" -> appString(R.string.history_detail_answered_elsewhere)
            "declined_elsewhere" -> appString(R.string.history_detail_declined_elsewhere)
            else -> null
        }
        else -> null
    }
    val name = contacts[peerDid]?.display() ?: peerName.ifBlank { appString(R.string.common_unknown) }
    return RecentCall(
        peerDid, name, kind, appString(R.string.history_label_when, label, whenShort(at, now)),
        title = if (detail != null) appString(R.string.history_label_detail, label, detail) else label, whenLong = whenLong(at, now),
        direct = if (secs > 0) direct else null, callId = callId,
    )
}

/** Sub-line under a contact's name on Home, from their newest call (`history` is newest first). */
fun contactSubLine(did: String, history: List<CallRecord>, now: Long): String {
    val r = history.firstOrNull { it.peerDid == did } ?: return appString(R.string.history_not_called)
    val at = r.startedAt.toLong()
    return when {
        r.reason == "unavailable" -> appString(R.string.history_last_call, dayWord(at, now))
        r.missed -> appString(R.string.history_missed_call_when, whenShort(at, now))
        !r.incoming && r.reason == "unreachable" -> appString(R.string.history_unreached_when, whenShort(at, now))
        else -> appString(R.string.history_last_call, dayWord(at, now))
    }
}

private fun dayWord(at: Long, now: Long): String {
    val days = daysBetween(at, now)
    return when {
        days <= 0L -> appString(R.string.history_day_today)
        days == 1L -> appString(R.string.history_day_yesterday)
        days < 7 -> dayOf(at).dayOfWeek.getDisplayName(TextStyle.FULL, Locale.getDefault())
        else -> dayOf(at).format(dm())
    }
}

/** Epoch seconds of 08:00 local tomorrow. */
fun tomorrowMorning(now: Long = System.currentTimeMillis() / 1000): Long =
    dayOf(now).plusDays(1).atTime(8, 0).atZone(zone()).toEpochSecond()

fun clockTime(secs: Long): String = hm(secs)
