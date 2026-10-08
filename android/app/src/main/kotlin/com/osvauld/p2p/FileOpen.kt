package com.osvauld.p2p

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Context
import android.content.Intent
import androidx.core.content.FileProvider
import java.io.File

/** How a received file may be opened. Decided from BOTH the declared MIME and the extension, because both come from the other person. */
enum class OpenKind { PDF, TEXT, EXTERNAL, NONE }

/** [mime] is what we pass to other apps: derived from the extension, never the sender's claim. */
data class OpenPlan(val kind: OpenKind, val mime: String)

object FileOpen {
    const val TEXT_LIMIT = 1_000_000L

    private val textExt = setOf("txt", "md", "csv", "log")
    private val externalMime = mapOf(
        "doc" to "application/msword", "docx" to "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" to "application/vnd.ms-excel", "xlsx" to "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" to "application/vnd.ms-powerpoint", "pptx" to "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" to "application/vnd.oasis.opendocument.text", "ods" to "application/vnd.oasis.opendocument.spreadsheet",
        "odp" to "application/vnd.oasis.opendocument.presentation", "rtf" to "application/rtf", "epub" to "application/epub+zip",
        "mp3" to "audio/mpeg", "m4a" to "audio/mp4", "aac" to "audio/aac", "ogg" to "audio/ogg", "opus" to "audio/ogg", "wav" to "audio/x-wav", "flac" to "audio/flac",
        "mp4" to "video/mp4", "m4v" to "video/mp4", "mkv" to "video/x-matroska", "webm" to "video/webm", "mov" to "video/quicktime", "3gp" to "video/3gpp",
    )
    private val generic = setOf("", "application/octet-stream", "binary/octet-stream", "application/x-binary", "application/unknown")
    private val blockedMime = setOf("application/vnd.android.package-archive", "application/x-apk", "application/xapk-package-archive")
    private val textMime = setOf("text/plain", "text/markdown", "text/x-markdown", "text/csv", "text/comma-separated-values", "text/x-log")
    private val officeMime = setOf("application/msword", "application/vnd.ms-excel", "application/vnd.ms-powerpoint", "application/rtf", "text/rtf", "application/epub+zip")

    fun extOf(name: String): String = name.substringAfterLast('.', "").lowercase()

    private fun extKind(ext: String) = when (ext) {
        "pdf" -> OpenKind.PDF
        in textExt -> OpenKind.TEXT
        in externalMime -> OpenKind.EXTERNAL
        else -> OpenKind.NONE
    }

    /** null = the MIME says nothing (generic); NONE = it names something that is not on the list. */
    private fun mimeKind(mime: String): OpenKind? {
        val m = mime.substringBefore(';').trim().lowercase()
        return when {
            m in generic -> null
            m in blockedMime -> OpenKind.NONE
            m == "application/pdf" -> OpenKind.PDF
            m in textMime -> OpenKind.TEXT
            m.startsWith("audio/") || m.startsWith("video/") -> OpenKind.EXTERNAL
            m.startsWith("application/vnd.openxmlformats-officedocument.") || m.startsWith("application/vnd.oasis.opendocument.") || m in officeMime -> OpenKind.EXTERNAL
            else -> OpenKind.NONE
        }
    }

    fun plan(name: String, mime: String): OpenPlan {
        val none = OpenPlan(OpenKind.NONE, "application/octet-stream")
        val ext = extOf(name)
        if (ext in setOf("apk", "apks", "xapk", "apkm")) return none
        val e = extKind(ext)
        if (e == OpenKind.NONE) return none
        val m = mimeKind(mime)
        if (m != null && m != e) return none
        val outMime = when (e) { OpenKind.PDF -> "application/pdf"; OpenKind.TEXT -> "text/plain"; else -> externalMime.getValue(ext) }
        return OpenPlan(e, outMime)
    }

    /** Hands the decrypted cache copy to another app. Returns false when nothing on the phone can open it. */
    fun openExternally(ctx: Context, file: File, mime: String): Boolean {
        val uri = FileProvider.getUriForFile(ctx, "${ctx.packageName}.files", file)
        val view = Intent(Intent.ACTION_VIEW).setDataAndType(uri, mime).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        view.clipData = ClipData.newRawUri(file.name, uri)
        if (ctx.packageManager.queryIntentActivities(view, 0).isEmpty()) return false
        return try {
            ctx.startActivity(Intent.createChooser(view, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION)); true
        } catch (e: ActivityNotFoundException) { false }
    }
}
