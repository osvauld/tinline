package com.osvauld.p2p

import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.content.FileProvider
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * One text a tester can paste or send instead of running adb: the app and phone, the call trace
 * (which survives restarts) and this app's own logcat (Kotlin plus the core's "p2pcore" lines).
 * An app without READ_LOGS only sees its own uid's lines, so nothing from other apps gets in.
 */
object DebugLog {
    /** Logcat lines for the shared file; the clipboard copy keeps only the newest [CLIP_CHARS]. */
    private const val LOGCAT_LINES = 5000
    private const val CLIP_CHARS = 200_000

    fun collect(c: Context): String = buildString {
        val now = SimpleDateFormat("yyyy-MM-dd HH:mm:ss Z", Locale.US).format(Date())
        append("Tinline ${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE}) debug log, $now\n")
        append("${Build.MANUFACTURER} ${Build.MODEL}, Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})\n")
        append("\n--- call trace\n").append(CallTrace.dump()).append('\n')
        append("\n--- logcat\n").append(logcat())
    }

    /** Newest part only: a clipboard clip goes through one binder transaction (~1 MB). */
    fun forClipboard(full: String): String =
        if (full.length <= CLIP_CHARS) full
        else full.substring(0, full.indexOf("\n--- logcat\n").coerceIn(0, CLIP_CHARS / 4)) +
            "\n--- logcat (newest part; share the file for all of it)\n" + full.takeLast(CLIP_CHARS)

    fun share(c: Context, full: String) {
        val dir = File(c.cacheDir, "debug_log").apply { mkdirs() }
        val f = File(dir, "tinline-${BuildConfig.VERSION_NAME}-log.txt").apply { writeText(full) }
        val uri = FileProvider.getUriForFile(c, "${c.packageName}.files", f)
        val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_STREAM, uri)
            .putExtra(Intent.EXTRA_SUBJECT, "Tinline debug log").addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        c.startActivity(Intent.createChooser(send, "Send debug log").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    private fun logcat(): String = try {
        val p = ProcessBuilder("logcat", "-d", "-v", "time", "-t", LOGCAT_LINES.toString()).redirectErrorStream(true).start()
        val out = p.inputStream.bufferedReader().readText()
        p.waitFor()
        out
    } catch (e: Exception) { "logcat unavailable: $e\n" }
}
