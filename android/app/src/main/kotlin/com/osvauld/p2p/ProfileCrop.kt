package com.osvauld.p2p

import android.content.ContentResolver
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.net.Uri
import java.io.ByteArrayOutputStream
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/*
 * Picture preparation, ported from playground src/lib/model/images.ts: input checks (the packet's limits), the crop, and the
 * square PNG the core receives. Same numbers, same messages.
 */

object ImageLimits {
    const val MAX_BYTES = 2 * 1024 * 1024
    const val MAX_SIDE = 4096
    const val OUT = 512
    const val OUT_MAX_BYTES = 1024 * 1024
}

/** The crop viewport is `view` px square; the picture sits at (ox, oy) scaled so it always covers it. */
data class Crop(val ox: Float, val oy: Float, val zoom: Float)

fun coverScale(w: Int, h: Int, view: Float, zoom: Float) = max(view / w, view / h) * zoom

fun clampCrop(c: Crop, w: Int, h: Int, view: Float): Crop {
    val s = coverScale(w, h, view, c.zoom)
    val minX = view - w * s; val minY = view - h * s
    return Crop(min(0f, max(minX, c.ox)), min(0f, max(minY, c.oy)), c.zoom)
}

/** Centre the picture in the viewport at a zoom. */
fun centred(w: Int, h: Int, view: Float, zoom: Float = 1f): Crop {
    val s = coverScale(w, h, view, zoom)
    return Crop((view - w * s) / 2, (view - h * s) / 2, zoom)
}

/** Zoom about the viewport centre, keeping the point under the centre where it is. */
fun zoomAt(c: Crop, zoom: Float, w: Int, h: Int, view: Float): Crop {
    val s0 = coverScale(w, h, view, c.zoom); val s1 = coverScale(w, h, view, zoom)
    val cx = (view / 2 - c.ox) / s0; val cy = (view / 2 - c.oy) / s0
    return clampCrop(Crop(view / 2 - cx * s1, view / 2 - cy * s1, zoom), w, h, view)
}

/** A decoded, checked picture ready for the crop step. */
class Picked(val bitmap: Bitmap) { val w get() = bitmap.width; val h get() = bitmap.height }

sealed interface PickResult {
    class Ok(val picked: Picked) : PickResult
    data class Refused(val error: String) : PickResult
}

private fun mb(n: Long): String = "%.1f".format(n / (1024.0 * 1024.0)).removeSuffix(".0")

/** Look at the first bytes, not the file name or the claimed type. */
fun sniff(h: ByteArray): String? {
    fun at(i: Int, n: Int) = h.getOrNull(i)?.toInt()?.and(255) == n
    return when {
        at(0, 0x89) && at(1, 0x50) && at(2, 0x4e) && at(3, 0x47) -> "png"
        at(0, 0xff) && at(1, 0xd8) && at(2, 0xff) -> "jpeg"
        at(0, 0x52) && at(1, 0x49) && at(2, 0x46) && at(3, 0x46) && at(8, 0x57) && at(9, 0x45) && at(10, 0x42) && at(11, 0x50) -> "webp"
        else -> null
    }
}

/** images.ts inspectImage, for bytes already read. Blocking (decode): call on a worker. */
fun inspectImage(bytes: ByteArray, size: Long = bytes.size.toLong()): PickResult {
    if (size > ImageLimits.MAX_BYTES) return PickResult.Refused(appString(R.string.profile_pick_too_big, mb(size), mb(ImageLimits.MAX_BYTES.toLong())))
    if (sniff(bytes) == null) return PickResult.Refused(appString(R.string.profile_pick_wrong_type))
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return PickResult.Refused(appString(R.string.profile_pick_unreadable))
    if (bounds.outWidth > ImageLimits.MAX_SIDE || bounds.outHeight > ImageLimits.MAX_SIDE)
        return PickResult.Refused(appString(R.string.profile_pick_too_wide, bounds.outWidth.toString(), bounds.outHeight.toString(), ImageLimits.MAX_SIDE.toString()))
    val bmp = runCatching { BitmapFactory.decodeByteArray(bytes, 0, bytes.size) }.getOrNull()
        ?: return PickResult.Refused(appString(R.string.profile_pick_unreadable))
    return PickResult.Ok(Picked(bmp))
}

/** Reads a picked document (at most 2 MiB + 1 byte, so an oversized file is refused without loading it) and inspects it. Blocking. */
fun inspectUri(resolver: ContentResolver, uri: Uri): PickResult {
    val size = runCatching {
        resolver.query(uri, arrayOf(android.provider.OpenableColumns.SIZE), null, null, null)?.use { c -> if (c.moveToFirst() && !c.isNull(0)) c.getLong(0) else null }
    }.getOrNull()
    if (size != null && size > ImageLimits.MAX_BYTES) return inspectImage(ByteArray(0), size)
    val bytes = runCatching {
        resolver.openInputStream(uri)?.use { s ->
            val out = ByteArrayOutputStream(); val buf = ByteArray(64 * 1024); var total = 0
            while (true) { val n = s.read(buf); if (n < 0) break; total += n; out.write(buf, 0, n); if (total > ImageLimits.MAX_BYTES) break }
            out.toByteArray()
        }
    }.getOrNull() ?: return PickResult.Refused(appString(R.string.profile_pick_unreadable))
    return inspectImage(bytes, max(bytes.size.toLong(), size ?: 0L))
}

/** App-side preparation: crop the chosen square and scale it to a PNG of at most 512 px. Blocking. */
fun renderSquare(p: Picked, crop: Crop, view: Float): ByteArray {
    val s = coverScale(p.w, p.h, view, crop.zoom)
    val side = view / s
    val out = min(ImageLimits.OUT, max(1, side.roundToInt()))
    val dst = Bitmap.createBitmap(out, out, Bitmap.Config.ARGB_8888)
    val src = RectF(-crop.ox / s, -crop.oy / s, -crop.ox / s + side, -crop.oy / s + side)
    val m = android.graphics.Matrix().apply { setRectToRect(src, RectF(0f, 0f, out.toFloat(), out.toFloat()), android.graphics.Matrix.ScaleToFit.FILL) }
    Canvas(dst).drawBitmap(p.bitmap, m, Paint(Paint.FILTER_BITMAP_FLAG or Paint.ANTI_ALIAS_FLAG))
    val bytes = ByteArrayOutputStream().also { dst.compress(Bitmap.CompressFormat.PNG, 100, it) }.toByteArray()
    dst.recycle()
    return bytes
}
