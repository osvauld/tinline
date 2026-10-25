package com.osvauld.p2p

import androidx.annotation.StringRes
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

    fun checkName(raw: String): String? {
        val s = raw.trim()
        return when {
            s.isEmpty() -> appString(R.string.profile_err_name_empty)
            CONTROL.containsMatchIn(s) || BIDI.containsMatchIn(s) -> appString(R.string.profile_err_name_chars)
            utf8(s) > NAME_LIMIT -> appString(R.string.profile_err_name_long, NAME_LIMIT.toString())
            else -> null
        }
    }
    /** [isStatus]: the field is "status", otherwise "about". */
    fun checkText(s: String, isStatus: Boolean): String? = when {
        utf8(s) > TEXT_LIMIT -> appString(if (isStatus) R.string.profile_err_status_long else R.string.profile_err_about_long, TEXT_LIMIT.toString())
        TEXT_CONTROL.containsMatchIn(s) || BIDI.containsMatchIn(s) -> appString(if (isStatus) R.string.profile_err_status_chars else R.string.profile_err_about_chars)
        else -> null
    }
    fun checkKey(key: String): String? = when {
        key.isBlank() -> appString(R.string.profile_err_key_blank)
        CONTROL.containsMatchIn(key) || utf8(key) > KEY_LIMIT -> appString(R.string.profile_err_key_bad)
        else -> null
    }
    /** profile.ts checkValue: per value 16 KiB, 64 fields, 64 KiB in all. [json] is the value's JSON text. */
    fun checkValue(key: String, json: String, fields: JSONObject): String? {
        val size = utf8(json)
        if (size > VALUE_LIMIT) return appString(R.string.profile_err_value_long)
        val others = fields.keys().asSequence().filter { it != key }.toList()
        if (others.size >= FIELDS && !fields.has(key)) return appString(R.string.profile_err_fields_full)
        val total = others.sumOf { utf8(it) + utf8(jsonOf(fields.opt(it))) } + utf8(key) + size
        if (total > ALL_LIMIT) return appString(R.string.profile_err_profile_full)
        return null
    }
    /** The full pre-check for `setField(key, value)`, in profile.ts's order. Null value = remove. */
    fun checkField(key: String, value: String?, fields: JSONObject): String? {
        checkKey(key)?.let { return it }
        if (key == "name") return value?.let { checkName(it) }
        if (key == "picture") return appString(R.string.profile_err_use_picture_controls)
        if (value == null) return if (key == "availability") appString(R.string.profile_err_availability_remove) else null
        if (key == "status" || key == "about") checkText(value, key == "status")?.let { return it }
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
data class KnownField(@StringRes val labelRes: Int, val link: Boolean, @StringRes val hintRes: Int) {
    val label: String get() = appString(labelRes)
    val hint: String get() = appString(hintRes)
}

object ProfileInfo {
    val KNOWN = linkedMapOf(
        "pronouns" to KnownField(R.string.profile_known_pronouns, false, R.string.profile_known_pronouns_hint),
        "location" to KnownField(R.string.profile_known_location, false, R.string.profile_known_location_hint),
        "website" to KnownField(R.string.profile_known_website, true, R.string.profile_known_website_hint),
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
            today.plusDays(1) -> appString(R.string.profile_until_tomorrow, hm)
            else -> appString(R.string.profile_until_weekday, a.dayOfWeek.getDisplayName(TextStyle.FULL, Locale.getDefault()), hm)
        }
    }
}

/** profile.ts BUSY_CHOICES; `until` in milliseconds (null = no end time). */
data class BusyChoice(val id: String, @StringRes val labelRes: Int, val until: (Long) -> Long?) {
    val label: String get() = appString(labelRes)
}

val BUSY_CHOICES = listOf(
    BusyChoice("30m", R.string.profile_busy_30m) { it + 30 * 60_000L },
    BusyChoice("1h", R.string.profile_busy_1h) { it + 3_600_000L },
    BusyChoice("4h", R.string.profile_busy_4h) { it + 4 * 3_600_000L },
    BusyChoice("tom", R.string.profile_busy_tomorrow) { now ->
        val z = ZoneId.systemDefault()
        LocalDate.ofInstant(Instant.ofEpochMilli(now), z).plusDays(1).atTime(8, 0).atZone(z).toInstant().toEpochMilli()
    },
    BusyChoice("open", R.string.profile_busy_open) { null },
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

