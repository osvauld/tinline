package com.osvauld.p2p

import androidx.annotation.PluralsRes
import androidx.annotation.StringRes

/**
 * Localized text for code that has no Context at hand (model and controller code, top-level helpers).
 * Composables use stringResource; services and notification builders use their own Context.
 * Reads the running app. Before it exists (JVM unit tests) there are no resources, so the id is returned
 * as text; real code always runs after Application.onCreate.
 */
fun appString(@StringRes id: Int, vararg args: Any): String {
    val app = P2pApp.instanceOrNull() ?: return "string#$id"
    return if (args.isEmpty()) app.getString(id) else app.getString(id, *args)
}

fun appPlural(@PluralsRes id: Int, count: Int, vararg args: Any): String {
    val app = P2pApp.instanceOrNull() ?: return "plural#$id"
    return app.resources.getQuantityString(id, count, *args)
}

/**
 * [items] as a sentence list in the app's language ("Ana, Sam and Leo"; Arabic and others have their own separators and
 * words). Android 10+ formats it with ICU; older phones join with the translated separator.
 */
fun listText(items: List<String>): String {
    if (items.size <= 1) return items.firstOrNull().orEmpty()
    if (android.os.Build.VERSION.SDK_INT >= 29) return android.icu.text.ListFormatter.getInstance(java.util.Locale.getDefault()).format(items)
    return items.joinToString(appString(R.string.common_list_separator))
}

/**
 * A date/time formatter for an ICU [skeleton] ("MMMd", "EEEEHm") in [locale]: the order of the parts, the punctuation
 * and the month and weekday names come from the language, not from this code. [fallback] is used where Android's
 * pattern lookup is not available (JVM unit tests).
 */
fun localFormatter(skeleton: String, fallback: String, locale: java.util.Locale = java.util.Locale.getDefault()): java.time.format.DateTimeFormatter {
    val pattern = runCatching { android.text.format.DateFormat.getBestDateTimePattern(locale, skeleton) }.getOrNull()?.takeIf { it.isNotBlank() } ?: fallback
    return java.time.format.DateTimeFormatter.ofPattern(pattern, locale)
}

/** How many words make a recovery phrase (shown in strings as a number, never typed into them). */
const val RECOVERY_WORDS = 24

/** Hosting a group call is limited below this battery level, in percent (the core decides; the notice names it). */
const val HOSTING_BATTERY_LIMIT_PERCENT = 15
