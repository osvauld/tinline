package com.osvauld.p2p

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PersistableBundle
import uniffi.p2pcore.Exception as CoreError

const val URL_TERMS = "https://tinline.osvauld.com/terms"
const val URL_PRIVACY = "https://tinline.osvauld.com/privacy"
const val URL_SITE = "https://tinline.osvauld.com"
const val URL_SOURCE = "https://github.com/osvauld/tinline"

/** Friendly text for a core error; never includes secrets. */
fun friendly(e: Throwable): String = when (e) {
    is CoreError.WrongPassphrase -> "That didn’t match. Check the spelling and spaces."
    is CoreError.WeakPassphrase -> "Type a passphrase, or skip it"
    is CoreError.Locked -> "Locked — enter your passphrase to unlock"
    is CoreError.BadPhrase -> "That recovery phrase is not valid"
    is CoreError.AccountExists -> "That account is already on this phone"
    is CoreError.InCall -> "Finish your call first"
    else -> e.message ?: "Something went wrong"
}

/** The BIP-39 English list, bundled as an asset (the core does not expose it). */
object Wordlist {
    @Volatile private var cached: List<String>? = null
    fun get(ctx: Context): List<String> = cached ?: synchronized(this) {
        cached ?: ctx.applicationContext.assets.open("bip39_english.txt").bufferedReader().readLines().map { it.trim() }.filter { it.isNotEmpty() }
            .also { cached = it }
    }
    fun suggestions(ctx: Context, prefix: String, max: Int = 3): List<String> {
        if (prefix.isEmpty()) return emptyList()
        return get(ctx).filter { it.startsWith(prefix.lowercase()) }.take(max)
    }
}

/** Terms & privacy acceptance, with the version that was accepted. Bump [CURRENT] to ask again. */
object LegalStore {
    const val CURRENT = 1
    private fun prefs(c: Context) = c.getSharedPreferences("legal", Context.MODE_PRIVATE)
    fun accepted(c: Context) = prefs(c).getInt("terms_version", 0) >= CURRENT
    fun accept(c: Context) { prefs(c).edit().putInt("terms_version", CURRENT).putLong("terms_accepted_at", System.currentTimeMillis()).apply() }
}

/** Copies [text]; flagged sensitive so Android 13+ hides it from the clipboard preview. */
fun copyToClipboard(c: Context, label: String, text: String) {
    val clip = ClipData.newPlainText(label, text)
    clip.description.extras = PersistableBundle().apply {
        if (Build.VERSION.SDK_INT >= 33) putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
        else putBoolean("android.content.extra.IS_SENSITIVE", true)
    }
    (c.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(clip)
}

fun clipboardText(c: Context): String? =
    (c.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).primaryClip?.getItemAt(0)?.coerceToText(c)?.toString()

fun openUrl(c: Context, url: String) {
    try { c.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) } catch (_: Exception) {}
}

/** Call duration as mm:ss (or h:mm:ss). */
fun clock(secs: Long): String {
    val s = secs.coerceAtLeast(0)
    return if (s >= 3600) "%d:%02d:%02d".format(s / 3600, s % 3600 / 60, s % 60) else "%02d:%02d".format(s / 60, s % 60)
}

/** Bars (1-4) from round-trip time and loss, as the tokens board describes: 4 Excellent, 3 Good, 2 OK, 1 Poor. */
fun qualityBars(rttMs: Int, lossPct: Double): Int = when {
    rttMs < 120 && lossPct < 1.0 -> 4
    rttMs < 250 && lossPct < 3.0 -> 3
    rttMs < 450 && lossPct < 8.0 -> 2
    else -> 1
}

fun qualityLabel(bars: Int) = when (bars) { 4 -> "Excellent"; 3 -> "Good"; 2 -> "OK"; else -> "Poor" }

// ---------------------------------------------------------------- call end reasons

enum class EndKind { Normal, Unreachable, Declined, Superseded }

/**
 * The stable end-reason tokens of `CallState.Ended` / `CallRecord.reason` (docs/protocol.md,
 * "End reasons"): hangup_local, hangup_remote, declined, declined_local, cancelled, no_answer,
 * unreachable, connection_lost, busy, superseded, and the history-only unavailable.
 */
fun classifyEnd(reason: String): EndKind = when (reason) {
    "unreachable" -> EndKind.Unreachable
    "declined", "declined_local", "busy" -> EndKind.Declined
    "superseded" -> EndKind.Superseded
    else -> EndKind.Normal
}

/** An incoming call that rang out or was given up on without us answering it. */
fun isMissedReason(reason: String, incoming: Boolean) =
    incoming && (reason == "cancelled" || reason == "no_answer" || reason == "busy")

/**
 * THE one place that turns an end reason into words for the Call ended screen. [incoming] is whether
 * the call came in (the same token reads differently from each side). Unknown reasons give "Call ended".
 */
fun endReasonText(reason: String, peerName: String, incoming: Boolean = false): String {
    val who = peerName.trim().ifEmpty { "They" }
    return when (reason) {
        "hangup_local" -> "You hung up"
        "hangup_remote" -> "$who hung up"
        "declined" -> "$who declined"
        "declined_local" -> "You declined"
        "cancelled" -> if (incoming) "$who stopped calling" else "You cancelled the call"
        "no_answer" -> if (incoming) "Missed call" else "No answer"
        "busy" -> if (incoming) "Missed call" else "$who is on another call"
        "unreachable" -> "Couldn\u2019t reach $who"
        "connection_lost" -> "Connection lost"
        "unavailable" -> "Turned away while you were not available"
        "answered_elsewhere" -> "Answered on another device"
        "declined_elsewhere" -> "Declined on another device"
        else -> "Call ended"
    }
}
