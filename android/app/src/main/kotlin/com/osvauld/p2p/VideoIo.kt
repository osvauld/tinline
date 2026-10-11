package com.osvauld.p2p

import android.graphics.Bitmap
import androidx.camera.core.ImageProxy
import com.sun.jna.Native
import com.sun.jna.Pointer
import java.nio.ByteBuffer
import uniffi.p2pcore.Node

/**
 * The only place call video frames cross into and out of the core. Today: camera YUV_420_888 is
 * packed to I420 here and decoded I420 is turned into ARGB here, both in Kotlin. When the core
 * takes strided planes (`pushVideoFrameYuv`) and hands out RGBA (`pullVideoFrameRgba`), [push]
 * and [pull] each become one call and the conversion code below goes.
 */
class VideoIo(private val node: () -> Node) {
    // ---- in: camera thread only
    private var yBuf = ByteArray(0)
    private var uBuf = ByteArray(0)
    private var vBuf = ByteArray(0)
    private val empty = ByteArray(0)
    /** Average time to copy a camera frame's planes / to convert a decoded frame to the bitmap. */
    @Volatile var packNs = 0L; private set
    /** ... and of the push call (FFI). */
    @Volatile var pushNs = 0L; private set
    @Volatile var pushed = 0L; private set
    @Volatile var inSize = "-"; private set

    /**
     * One camera frame to the encoder as its YUV_420_888 planes (full range); the core converts
     * it. With pixel stride 2 the interleaved chroma plane goes across once. False = dropped by
     * the core (camera off, no video).
     */
    fun push(img: ImageProxy): Boolean {
        val w = img.width
        val h = img.height
        val p = img.planes
        val t0 = System.nanoTime()
        yBuf = copyAll(p[0].buffer, yBuf)
        val ps = p[1].pixelStride
        var u = empty
        var v = empty
        if (ps == 2 && interleaved(p[2].buffer, p[1].buffer)) {
            vBuf = copyAll(p[2].buffer, vBuf, p[1].buffer); v = vBuf  // NV21: V U V U ..
        } else if (ps == 2 && interleaved(p[1].buffer, p[2].buffer)) {
            uBuf = copyAll(p[1].buffer, uBuf, p[2].buffer); u = uBuf  // NV12
        } else {
            uBuf = copyAll(p[1].buffer, uBuf); vBuf = copyAll(p[2].buffer, vBuf); u = uBuf; v = vBuf
        }
        val t1 = System.nanoTime()
        packNs = avg(packNs, t1 - t0)
        val ok = node().pushVideoFrameYuv(
            w.toUInt(), h.toUInt(), img.imageInfo.rotationDegrees.toUInt(), yBuf, p[0].rowStride.toUInt(),
            u, v, p[1].rowStride.toUInt(), ps.toUInt(), true,
        )
        pushNs = avg(pushNs, System.nanoTime() - t1)
        inSize = "${w}x$h"
        if (ok) pushed++
        return ok
    }

    /** [b] starts one byte before [next]: the two views of one interleaved chroma plane. */
    private fun interleaved(b: ByteBuffer, next: ByteBuffer) =
        b.isDirect && next.isDirect && next.limit() >= b.limit() &&
            addr(b) + 1 == addr(next)

    private fun addr(b: ByteBuffer) = Pointer.nativeValue(Native.getDirectBufferPointer(b))

    /**
     * The whole of [src] into [dst] (reused when the size matches); with [tail], also the last
     * byte of that overlapping view, which [src] stops just short of.
     */
    private fun copyAll(src: ByteBuffer, dst: ByteArray, tail: ByteBuffer? = null): ByteArray {
        val n = src.remaining()
        val need = if (tail != null) n + 1 else n
        val out = if (dst.size == need) dst else ByteArray(need)
        src.duplicate().get(out, 0, n)
        if (tail != null) out[n] = tail.get(n - 1)
        return out
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
