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

const val MIN_PASSPHRASE = 8

const val URL_TERMS = "https://tinline.osvauld.com/terms"
const val URL_PRIVACY = "https://tinline.osvauld.com/privacy"
const val URL_SITE = "https://tinline.osvauld.com"
const val URL_SOURCE = "https://github.com/osvauld/tinline"

/** Friendly text for a core error; never includes secrets. */
fun friendly(e: Throwable): String = when (e) {
    is CoreError.WrongPassphrase -> "That didn’t match. Check the spelling and spaces."
    is CoreError.WeakPassphrase -> "At least $MIN_PASSPHRASE characters"
    is CoreError.Locked -> "Locked — enter your passphrase to unlock"
    is CoreError.BadPhrase -> "That recovery phrase is not valid"
    else -> e.message ?: "Something went wrong"
}

/** Null when the pair is acceptable, else what to tell the user. */
fun passphraseProblem(pass: String, confirm: String): String? = when {
    pass.length < MIN_PASSPHRASE -> "At least $MIN_PASSPHRASE characters"
    pass != confirm -> "Passphrases don’t match"
    else -> null
}

/** 0 = nothing typed, 1 weak, 2 okay, 3 good, 4 strong. A cheap local length / variety heuristic, no dictionary. */
fun passphraseStrength(p: String): Int {
    if (p.isEmpty()) return 0
    val classes = listOf(p.any { it.isLowerCase() }, p.any { it.isUpperCase() }, p.any { it.isDigit() }, p.any { !it.isLetterOrDigit() }).count { it }
    val words = p.split(Regex("[\\s\\-_.,]+")).count { it.length >= 3 }
    val distinct = p.toSet().size
    return when {
        p.length < MIN_PASSPHRASE || distinct < 4 -> 1
        p.length >= 16 && (classes >= 2 || words >= 3) -> 4
        p.length >= 12 && (classes >= 3 || words >= 3) -> 3
        p.length >= 12 || classes >= 3 -> 2
        else -> 1
    }
}

fun strengthLabel(s: Int) = when (s) { 1 -> "Weak"; 2 -> "Okay"; 3 -> "Good"; 4 -> "Strong"; else -> "" }

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

/** Local-only marker the controller substitutes for the core's ambiguous "hung up" when WE ended the call. */
const val REASON_LOCAL_HANGUP = "you hung up"

enum class EndKind { Normal, Unreachable, Declined, Mic }

/**
 * THE one place that turns a `CallState.Ended.reason` from the core into words. Unknown reasons
 * fall back to "Call ended". The reasons the core emits today: "ended", "hung up", "cancelled",
 * "declined", "declined: <text>", "busy", "rejected: <text>", "missed", "no answer", "no audio",
 * "connection lost: ...", "could not reach <name>: ...", "they called at the same time".
 */
fun classifyEnd(reason: String): EndKind = when {
    reason.startsWith("could not reach") || reason == "no answer" -> EndKind.Unreachable
    reason.startsWith("declined") || reason == "busy" || reason.startsWith("rejected") -> EndKind.Declined
    else -> EndKind.Normal
}

fun endReasonText(reason: String, peerName: String): String {
    val who = peerName.trim().ifEmpty { "They" }
    return when {
        reason == REASON_LOCAL_HANGUP -> "You hung up"
        reason == "hung up" -> "$who hung up"
        reason == "cancelled" -> "You cancelled the call"
        reason.startsWith("declined") -> "$who declined"
        reason == "busy" -> "$who is on another call"
        reason.startsWith("rejected") -> "$who can’t take this call"
        reason == "missed" -> "Missed call"
        reason == "no answer" -> "No answer"
        reason == "no audio" || reason.startsWith("connection lost") -> "Connection lost"
        reason.startsWith("could not reach") -> "Couldn’t reach $who"
        else -> "Call ended"
    }
}
