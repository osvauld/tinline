package com.osvauld.p2p

import android.graphics.Bitmap
import androidx.camera.core.ImageProxy
import com.sun.jna.Native
import com.sun.jna.Pointer
import java.nio.ByteBuffer
import uniffi.p2pcore.Node

/**
 * The only place call video frames cross into and out of the core. By default through
 * [VideoJni]: the camera's planes go in as the direct buffers they are (Rust copies them once)
 * and decoded frames come out as RGBA into a direct buffer for `Bitmap.copyPixelsFromBuffer`.
 * [jni] = false selects the UniFFI path (plane copies into ByteArrays, I420 pull + Kotlin
 * conversion), kept to compare the two (DebugReceiver `video_path`).
 */
class VideoIo(private val node: () -> Node) {
    @Volatile var jni = true
    private var handleOf: Node? = null
    private var handle = 0L

    /** The node's handle for [VideoJni] (a new node after an account switch gets its own). */
    @Synchronized private fun handle(): Long {
        val n = node()
        if (n !== handleOf) { handle = n.videoFrameHandle().toLong(); handleOf = n }
        return handle
    }

    // ---- in: camera thread only
    private var yBuf = ByteArray(0)
    private var uBuf = ByteArray(0)
    private var vBuf = ByteArray(0)
    private val empty = ByteArray(0)
    /** Average time to copy a camera frame's planes (UniFFI path) / to fill the bitmap. */
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
        val p = img.planes
        val ok = pushPlanes(
            img.width, img.height, img.imageInfo.rotationDegrees, p[0].buffer, p[0].rowStride,
            p[1].buffer, p[2].buffer, p[1].rowStride, p[1].pixelStride,
        )
        inSize = "${img.width}x${img.height}"
        if (ok) pushed++
        return ok
    }

    /** [push] of loose planes (also the DebugReceiver `video_io_bench`). */
    fun pushPlanes(w: Int, h: Int, rotation: Int, y: ByteBuffer, yRowStride: Int, u: ByteBuffer, v: ByteBuffer, uvRowStride: Int, ps: Int): Boolean {
        val t0 = System.nanoTime()
        if (jni) {
            val ok = VideoJni.pushYuv(handle(), w, h, rotation, y, yRowStride, u, v, uvRowStride, ps, true)
            packNs = 0
            pushNs = avg(pushNs, System.nanoTime() - t0)
            return ok
        }
        yBuf = copyAll(y, yBuf)
        var uArr = empty
        var vArr = empty
        if (ps == 2 && interleaved(v, u)) {
            vBuf = copyAll(v, vBuf, u); vArr = vBuf  // NV21: V U V U ..
        } else if (ps == 2 && interleaved(u, v)) {
            uBuf = copyAll(u, uBuf, v); uArr = uBuf  // NV12
        } else {
            uBuf = copyAll(u, uBuf); vBuf = copyAll(v, vBuf); uArr = uBuf; vArr = vBuf
        }
        val t1 = System.nanoTime()
        packNs = avg(packNs, t1 - t0)
        val ok = node().pushVideoFrameYuv(
            w.toUInt(), h.toUInt(), rotation.toUInt(), yBuf, yRowStride.toUInt(),
            uArr, vArr, uvRowStride.toUInt(), ps.toUInt(), true,
        )
        pushNs = avg(pushNs, System.nanoTime() - t1)
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
    /** Average time of a pull that returned a frame (JNI: incl. the RGBA conversion in Rust). */
    @Volatile var pullNs = 0L; private set

    /**
     * The newest decoded frame as a bitmap, or null if none arrived since the last call. Two
     * bitmaps alternate, so the one returned last time stays intact while it is on screen.
     */
    fun pull(): Bitmap? = if (jni) pullJni() else pullUniffi()

    private var rgba: ByteBuffer = ByteBuffer.allocateDirect(0)
    private val info = IntArray(3)

    private fun pullJni(): Bitmap? {
        val tp = System.nanoTime()
        val h = handle()
        var r = VideoJni.pullRgba(h, rgba, info)
        if (r == -1) {
            rgba = ByteBuffer.allocateDirect(info[0] * info[1] * 4)
            r = VideoJni.pullRgba(h, rgba, info)
        }
        if (r != 1) return null
        val t0 = System.nanoTime()
        pullNs = avg(pullNs, t0 - tp)
        val bmp = bitmap(info[0], info[1])
        rgba.rewind()
        bmp.copyPixelsFromBuffer(rgba)
        next = 1 - next
        convertNs = avg(convertNs, System.nanoTime() - t0)
        return bmp
    }

    /** The next of the two alternating bitmaps, at [w] x [h]. */
    private fun bitmap(w: Int, h: Int): Bitmap {
        var bmp = bitmaps[next]
        if (bmp == null || bmp.width != w || bmp.height != h) {
            bmp = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
            bitmaps[next] = bmp
        }
        return bmp
    }

    private fun pullUniffi(): Bitmap? {
        val tp = System.nanoTime()
        val f = node().pullVideoFrame() ?: return null
        val w = f.width.toInt()
        val h = f.height.toInt()
        val t0 = System.nanoTime()
        pullNs = avg(pullNs, t0 - tp)
        if (argb.size != w * h) argb = IntArray(w * h)
        i420ToArgb(f.data, w, h, argb)
        val bmp = bitmap(w, h)
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

/** Frame calls straight into the core (`crates/core/src/video_jni.rs`), no UniFFI copies. */
object VideoJni {
    init { System.loadLibrary("p2pcore") }

    /** `Node.pushVideoFrameYuv` reading the planes' direct buffers in place (whole buffers). */
    @JvmStatic external fun pushYuv(
        handle: Long, width: Int, height: Int, rotation: Int, y: ByteBuffer, yRowStride: Int,
        u: ByteBuffer, v: ByteBuffer, uvRowStride: Int, uvPixelStride: Int, fullRange: Boolean,
    ): Boolean

    /**
     * The newest decoded frame as RGBA at the start of [dst] (direct). 1 = frame, [info] =
     * width, height, seq; 0 = none; -1 = [dst] too small, [info] = width, height, frame kept;
     * -2 = bad arguments.
     */
    @JvmStatic external fun pullRgba(handle: Long, dst: ByteBuffer, info: IntArray): Int
}
