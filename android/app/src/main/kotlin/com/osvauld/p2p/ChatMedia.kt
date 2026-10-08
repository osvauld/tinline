package com.osvauld.p2p

import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Build
import android.provider.MediaStore
import android.provider.OpenableColumns
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.core.content.FileProvider
import uniffi.p2pcore.Message
import java.io.File
import java.util.UUID

/**
 * Files around attachments. The core keeps attachments encrypted; anything shown or shared is decrypted
 * into `cache/chat_media` (wiped at app start) with `Node.save_attachment`.
 */
object ChatMedia {
    private fun dir(ctx: Context) = File(ctx.cacheDir, "chat_media")

    fun clear(ctx: Context) { dir(ctx).deleteRecursively(); File(ctx.cacheDir, "chat_out").deleteRecursively(); File(ctx.cacheDir, "chat_camera").deleteRecursively() }

    private fun safe(name: String) = name.replace(Regex("[^A-Za-z0-9._ -]"), "_").take(80).ifBlank { "file" }

    /** The decrypted copy of [m]'s attachment, made on first use. Blocking. */
    fun decrypt(ctx: Context, source: ChatSource, m: Message): File {
        val a = m.attachment ?: throw IllegalArgumentException("no attachment")
        val f = File(dir(ctx), "${a.hash.take(16)}_${safe(a.name)}")
        if (!f.exists() || f.length() == 0L) {
            f.parentFile?.mkdirs()
            val tmp = File(f.path + ".part")
            source.save(m.peerDid, m.id, tmp.path)
            tmp.renameTo(f)
        }
        return f
    }

    /** Decodes [file] down to about [maxPx] on the long side. Blocking. */
    fun bitmap(file: File, maxPx: Int = 1280): ImageBitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(file.path, bounds)
        var sample = 1
        while (bounds.outWidth / sample > maxPx * 2 || bounds.outHeight / sample > maxPx * 2) sample *= 2
        return BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inSampleSize = sample })?.asImageBitmap()
    }

    /** Copies a picked content URI into app cache under its own name; returns the file and its MIME type. Blocking. */
    fun copyIn(ctx: Context, uri: Uri): Pair<File, String> {
        var name = "file"
        ctx.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
            if (it.moveToFirst() && !it.isNull(0)) name = it.getString(0)
        }
        val out = File(File(ctx.cacheDir, "chat_out/${UUID.randomUUID()}"), safe(name)).also { it.parentFile?.mkdirs() }
        ctx.contentResolver.openInputStream(uri)?.use { i -> out.outputStream().use { i.copyTo(it) } } ?: throw java.io.IOException("cannot read $uri")
        val mime = ctx.contentResolver.getType(uri)
            ?: android.webkit.MimeTypeMap.getSingleton().getMimeTypeFromExtension(out.extension.lowercase()) ?: "application/octet-stream"
        return out to mime
    }

    /** "Save to phone": copies into the shared gallery / Downloads with MediaStore (Android 10+). Blocking. */
    fun saveToPhone(ctx: Context, file: File, name: String, mime: String): Boolean {
        if (Build.VERSION.SDK_INT < 29) return false
        val (collection, folder) = when {
            mime.startsWith("image/") -> MediaStore.Images.Media.EXTERNAL_CONTENT_URI to "Pictures/Tinline"
            mime.startsWith("video/") -> MediaStore.Video.Media.EXTERNAL_CONTENT_URI to "Movies/Tinline"
            mime.startsWith("audio/") -> MediaStore.Audio.Media.EXTERNAL_CONTENT_URI to "Music/Tinline"
            else -> MediaStore.Downloads.EXTERNAL_CONTENT_URI to "Download/Tinline"
        }
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, name)
            put(MediaStore.MediaColumns.MIME_TYPE, mime)
            put(MediaStore.MediaColumns.RELATIVE_PATH, folder)
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val r = ctx.contentResolver
        val uri = r.insert(collection, values) ?: return false
        return try {
            r.openOutputStream(uri)?.use { o -> file.inputStream().use { it.copyTo(o) } } ?: return false
            r.update(uri, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null)
            true
        } catch (e: Exception) { r.delete(uri, null, null); false }
    }

    fun share(ctx: Context, file: File, mime: String) {
        val uri = FileProvider.getUriForFile(ctx, "${ctx.packageName}.files", file)
        val send = Intent(Intent.ACTION_SEND).setType(mime).putExtra(Intent.EXTRA_STREAM, uri).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        ctx.startActivity(Intent.createChooser(send, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    fun cameraTarget(ctx: Context): Pair<File, Uri> {
        val f = File(File(ctx.cacheDir, "chat_camera"), "IMG_${System.currentTimeMillis()}.jpg").also { it.parentFile?.mkdirs() }
        return f to FileProvider.getUriForFile(ctx, "${ctx.packageName}.files", f)
    }
}
