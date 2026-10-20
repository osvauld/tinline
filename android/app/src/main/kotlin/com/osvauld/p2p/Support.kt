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

/** Bars (1-4) from round-trip time and loss (thresholds and words: `uniffi.present.callQuality`). */
fun qualityBars(rttMs: Int, lossPct: Double): Int = uniffi.present.callQuality(rttMs.toUInt(), lossPct).bars.toInt()

fun qualityLabel(bars: Int) = signalWord(bars)
