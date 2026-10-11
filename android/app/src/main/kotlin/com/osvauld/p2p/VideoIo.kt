package com.osvauld.p2p

import android.graphics.Bitmap
import androidx.camera.core.ImageProxy
import uniffi.p2pcore.Node

/**
 * The only place call video frames cross into and out of the core. Today: camera YUV_420_888 is
 * packed to I420 here and decoded I420 is turned into ARGB here, both in Kotlin. When the core
 * takes strided planes (`pushVideoFrameYuv`) and hands out RGBA (`pullVideoFrameRgba`), [push]
 * and [pull] each become one call and the conversion code below goes.
 */
class VideoIo(private val node: () -> Node) {
    // ---- in: camera thread only
    private var i420 = ByteArray(0)
    private var row = ByteArray(0)
    /** Average time to pack a camera frame to I420 / to convert a decoded frame to the bitmap. */
    @Volatile var packNs = 0L; private set
    /** ... and of the `pushVideoFrame` call (FFI copy). */
    @Volatile var pushNs = 0L; private set
    @Volatile var pushed = 0L; private set
    @Volatile var inSize = "-"; private set

    /** One camera frame to the encoder; false = dropped by the core (camera off, no video). */
    fun push(img: ImageProxy): Boolean {
        val w = img.width
        val h = img.height
        val cw = (w + 1) / 2
        val ch = (h + 1) / 2
        val need = w * h + 2 * cw * ch
        if (i420.size != need) i420 = ByteArray(need)
        val t0 = System.nanoTime()
        val p = img.planes
        copyPlane(p[0].buffer, p[0].rowStride, p[0].pixelStride, w, h, i420, 0)
        copyPlane(p[1].buffer, p[1].rowStride, p[1].pixelStride, cw, ch, i420, w * h)
        copyPlane(p[2].buffer, p[2].rowStride, p[2].pixelStride, cw, ch, i420, w * h + cw * ch)
        val t1 = System.nanoTime()
        packNs = avg(packNs, t1 - t0)
        val ok = node().pushVideoFrame(w.toUInt(), h.toUInt(), img.imageInfo.rotationDegrees.toUInt(), i420)
        pushNs = avg(pushNs, System.nanoTime() - t1)
        inSize = "${w}x$h"
        if (ok) pushed++
        return ok
    }

    private fun copyPlane(src: java.nio.ByteBuffer, rowStride: Int, pixelStride: Int, w: Int, h: Int, dst: ByteArray, off: Int) {
        val b = src.duplicate()
        if (pixelStride == 1 && rowStride == w) {
            b.position(0); b.get(dst, off, w * h); return
        }
        // The last row may stop right after its last pixel, so read only what each row has.
        val rowLen = (w - 1) * pixelStride + 1
        if (row.size < rowLen) row = ByteArray(rowStride.coerceAtLeast(rowLen))
        var o = off
        for (y in 0 until h) {
            b.position(y * rowStride)
            if (pixelStride == 1) { b.get(dst, o, w); o += w; continue }
            b.get(row, 0, rowLen)
            var i = 0
            for (x in 0 until w) { dst[o++] = row[i]; i += pixelStride }
        }
    }

    // ---- out: render thread only
    private var argb = IntArray(0)
    private val bitmaps = arrayOfNulls<Bitmap>(2)
    private var next = 0
    @Volatile var convertNs = 0L; private set
    /** Average time of a `pullVideoFrame` that returned a frame (FFI copy of the I420 bytes). */
    @Volatile var pullNs = 0L; private set

    /**
     * The newest decoded frame as a bitmap, or null if none arrived since the last call. Two
     * bitmaps alternate, so the one returned last time stays intact while it is on screen.
     */
    fun pull(): Bitmap? {
        val tp = System.nanoTime()
        val f = node().pullVideoFrame() ?: return null
        val w = f.width.toInt()
        val h = f.height.toInt()
        val t0 = System.nanoTime()
        pullNs = avg(pullNs, t0 - tp)
        if (argb.size != w * h) argb = IntArray(w * h)
        i420ToArgb(f.data, w, h, argb)
        var bmp = bitmaps[next]
        if (bmp == null || bmp.width != w || bmp.height != h) {
            bmp = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
            bitmaps[next] = bmp
        }
        bmp.setPixels(argb, 0, w, 0, 0, w, h)
        next = 1 - next
        convertNs = avg(convertNs, System.nanoTime() - t0)
        return bmp
    }

    companion object {
        /** Moving average (1/10 weight) of per-frame times, for the stats line. */
        private fun avg(old: Long, x: Long) = if (old == 0L) x else (old * 9 + x) / 10

        /** BT.601 limited range, fixed point (x1024), two rows at a time sharing chroma. */
        fun i420ToArgb(src: ByteArray, w: Int, h: Int, out: IntArray) {
            val cw = (w + 1) / 2
            val uOff = w * h
            val vOff = uOff + cw * ((h + 1) / 2)
            for (y in 0 until h) {
                val yRow = y * w
                val cRow = (y shr 1) * cw
                var x = 0
                while (x < w) {
                    val ci = cRow + (x shr 1)
                    val u = (src[uOff + ci].toInt() and 0xff) - 128
                    val v = (src[vOff + ci].toInt() and 0xff) - 128
                    val rd = 1634 * v
                    val gd = -401 * u - 832 * v
                    val bd = 2066 * u
                    var k = 0
                    while (k < 2 && x + k < w) {
                        val yy = ((src[yRow + x + k].toInt() and 0xff) - 16) * 1192
                        val r = clamp((yy + rd) shr 10)
                        val g = clamp((yy + gd) shr 10)
                        val b = clamp((yy + bd) shr 10)
                        out[yRow + x + k] = (0xff shl 24) or (r shl 16) or (g shl 8) or b
                        k++
                    }
                    x += 2
                }
            }
        }

        private fun clamp(v: Int) = if (v < 0) 0 else if (v > 255) 255 else v
    }
}
