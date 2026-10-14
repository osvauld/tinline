package com.osvauld.p2p

import org.json.JSONArray
import org.json.JSONObject
import java.net.URI
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.TextStyle
import java.util.Locale

/*
 * Profile rules and copy, ported from playground src/lib/model/actions/profile.ts, screens/profile/info.ts and copy.ts.
 * Messages are the playground's, verbatim. Fields are data: nothing here fetches, embeds or runs anything a field contains.
 */

/** A check's outcome: null is fine, otherwise the message to show. */
object ProfileRules {
    const val NAME_LIMIT = 256
    const val TEXT_LIMIT = 4096
    const val FIELDS = 64
    const val VALUE_LIMIT = 16 * 1024
    const val ALL_LIMIT = 64 * 1024
    const val KEY_LIMIT = 256
    val reserved = setOf("name", "status", "about", "availability", "picture")

    fun utf8(s: String) = s.toByteArray(Charsets.UTF_8).size
    private val CONTROL = Regex("[\\u0000-\\u001f\\u007f-\\u009f]")
    private val BIDI = Regex("[\\u202a-\\u202e\\u2066-\\u2069]")
    private val TEXT_CONTROL = Regex("[\\u0000-\\u0008\\u000b\\u000c\\u000e-\\u001f\\u007f]")

    fun checkName(raw: String, what: String = "name"): String? {
        val s = raw.trim()
        return when {
            s.isEmpty() -> "The $what can’t be empty."
            CONTROL.containsMatchIn(s) || BIDI.containsMatchIn(s) -> "The $what can’t contain control or text-direction characters."
            utf8(s) > NAME_LIMIT -> "That $what is too long. Up to $NAME_LIMIT bytes; some characters take more than one."
            else -> null
        }
    }
    fun checkText(s: String, what: String): String? = when {
        utf8(s) > TEXT_LIMIT -> "That is too long for $what: up to $TEXT_LIMIT bytes, and some characters take more than one."
        TEXT_CONTROL.containsMatchIn(s) || BIDI.containsMatchIn(s) -> what.replaceFirstChar { it.uppercase() } + " can’t contain control or text-direction characters."
        else -> null
    }
    fun checkKey(key: String): String? = when {
        key.isBlank() -> "Give the information a name."
        CONTROL.containsMatchIn(key) || utf8(key) > KEY_LIMIT -> "That name can’t be used. Keep it short, without control characters."
        else -> null
    }
    /** profile.ts checkValue: per value 16 KiB, 64 fields, 64 KiB in all. [json] is the value's JSON text. */
    fun checkValue(key: String, json: String, fields: JSONObject): String? {
        val size = utf8(json)
        if (size > VALUE_LIMIT) return "That is too long. Each piece of information can be up to 16 KiB."
        val others = fields.keys().asSequence().filter { it != key }.toList()
        if (others.size >= FIELDS && !fields.has(key)) return "You’ve reached 64 pieces of information. Remove one to add another."
        val total = others.sumOf { utf8(it) + utf8(jsonOf(fields.opt(it))) } + utf8(key) + size
        if (total > ALL_LIMIT) return "Your profile is full. Remove some information to add more."
        return null
    }
    /** The full pre-check for `setField(key, value)`, in profile.ts's order. Null value = remove. */
    fun checkField(key: String, value: String?, fields: JSONObject): String? {
        checkKey(key)?.let { return it }
        if (key == "name") return value?.let { checkName(it) }
        if (key == "picture") return "Use the picture controls for that."
        if (value == null) return if (key == "availability") "Availability can be changed but not removed." else null
        if (key == "status" || key == "about") checkText(value, if (key == "status") "a status" else "about")?.let { return it }
        return checkValue(key, JSONObject.quote(value), fields)
    }

    private fun jsonOf(v: Any?): String = when (v) {
        null, JSONObject.NULL -> "null"
        is String -> JSONObject.quote(v)
        is JSONObject, is JSONArray -> v.toString()
        else -> v.toString()
    }
}

/** info.ts KNOWN: keys with their own control. */
data class KnownField(val label: String, val link: Boolean, val hint: String)

object ProfileInfo {
    val KNOWN = linkedMapOf(
        "pronouns" to KnownField("Pronouns", false, "For example, she/her"),
        "location" to KnownField("Location", false, "City or region"),
        "website" to KnownField("Website", true, "https://"),
    )
    /** "work-title" / "work_title" -> "Work title". The stored key is never changed. */
    fun humanize(key: String): String {
        val s = key.replace(Regex("[-_.]+"), " ").replace(Regex("([a-z])([A-Z])"), "$1 $2").trim()
        return if (s.isEmpty()) key else s.substring(0, 1).uppercase() + s.substring(1)
    }
    fun labelOf(key: String) = KNOWN[key]?.label ?: humanize(key)
    /** Additional-information keys in display order: known ones first, then the rest in the order they were added. */
    fun extraKeys(fields: JSONObject?): List<String> {
        val keys = fields?.keys()?.asSequence()?.filter { it !in ProfileRules.reserved }?.toList() ?: return emptyList()
        return KNOWN.keys.filter { it in keys } + keys.filter { it !in KNOWN }
    }
    /** http(s) only, no embedded credentials. Anything else is shown as plain text and never offered as a link. */
    fun safeUrl(raw: Any?): String? {
        if (raw !is String) return null
        return runCatching {
            val u = URI(raw.trim())
            val scheme = u.scheme?.lowercase(Locale.ROOT)
            if ((scheme == "https" || scheme == "http") && u.rawUserInfo == null && !u.host.isNullOrEmpty()) {
                // As URL.href: lower-case scheme and host, an empty path becomes "/".
                val port = if (u.port >= 0) ":${u.port}" else ""
                "$scheme://${u.host.lowercase(Locale.ROOT)}$port${u.rawPath.ifEmpty { "/" }}" +
                    (u.rawQuery?.let { "?$it" } ?: "") + (u.rawFragment?.let { "#$it" } ?: "")
            } else null
        }.getOrNull()
    }
    /** What the user typed -> a stored address. A bare "arjun.example" becomes https://arjun.example/. */
    fun normaliseLink(raw: String): String? {
        val t = raw.trim()
        if (t.isEmpty()) return null
        return safeUrl(if (Regex("^[a-z][a-z0-9+.-]*:", RegexOption.IGNORE_CASE).containsMatchIn(t)) t else "https://$t")
    }
    fun shortUrl(u: String) = u.replace(Regex("^https?://"), "").removeSuffix("/")
    fun isScalar(v: Any?) = v is String || v is Number || v is Boolean

    /** "18:00", "tomorrow 08:00", "Friday 08:00": a deadline as people say it. [atMs], [nowMs] in milliseconds. */
    fun untilLabel(atMs: Long, nowMs: Long = System.currentTimeMillis(), zone: ZoneId = ZoneId.systemDefault()): String {
        val a = Instant.ofEpochMilli(atMs).atZone(zone)
        val today = Instant.ofEpochMilli(nowMs).atZone(zone).toLocalDate()
        val hm = "%02d:%02d".format(a.hour, a.minute)
        return when (a.toLocalDate()) {
            today -> hm
            today.plusDays(1) -> "tomorrow $hm"
            else -> "${a.dayOfWeek.getDisplayName(TextStyle.FULL, Locale.getDefault())} $hm"
        }
    }
}

/** profile.ts BUSY_CHOICES; `until` in milliseconds (null = no end time). */
data class BusyChoice(val id: String, val label: String, val until: (Long) -> Long?)

val BUSY_CHOICES = listOf(
    BusyChoice("30m", "30 minutes") { it + 30 * 60_000L },
    BusyChoice("1h", "1 hour") { it + 3_600_000L },
    BusyChoice("4h", "4 hours") { it + 4 * 3_600_000L },
    BusyChoice("tom", "Tomorrow morning") { now ->
        val z = ZoneId.systemDefault()
        LocalDate.ofInstant(Instant.ofEpochMilli(now), z).plusDays(1).atTime(8, 0).atZone(z).toInstant().toEpochMilli()
    },
    BusyChoice("open", "No end time") { null },
)

/** The availability field ({available, until?} with until in unix seconds) at this moment: a passed deadline is Available again. */
data class Avail(val available: Boolean, val untilMs: Long? = null)

fun effectiveAvailability(fields: JSONObject?, nowMs: Long = System.currentTimeMillis()): Avail {
    val a = fields?.optJSONObject("availability") ?: return Avail(true)
    if (a.optBoolean("available", true)) return Avail(true)
    val until = if (a.has("until") && !a.isNull("until")) a.optLong("until") * 1000 else null
    if (until != null && until <= nowMs) return Avail(true)
    return Avail(false, until)
}

/** copy.ts, verbatim. */
const val PICTURE_HELP = "Replacing or removing your picture requests deletion of the previous cached copy on your linked devices and contacts when they sync. Screenshots, backups, and copies retained by modified clients cannot be erased by the app."
const val FIELD_HELP = "Your contacts can see your profile. Removing information hides it from your profile; earlier versions can remain in synced history."
