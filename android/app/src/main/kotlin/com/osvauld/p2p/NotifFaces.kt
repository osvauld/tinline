package com.osvauld.p2p

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.BitmapShader
import android.graphics.Canvas
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.Shader
import android.util.Log
import androidx.core.graphics.drawable.IconCompat
import java.util.concurrent.ConcurrentHashMap

/**
 * A person's profile picture for notifications (the [androidx.core.app.Person] icon), so a message or call shows their photo
 * instead of the system's letter tile. Read on a worker through [P2pApp.profileWork], never on a core callback; kept per DID
 * with the picture hash, so a new picture replaces the old one on the next notification. Nothing leaves the phone.
 */
object NotifFaces {
    private const val PX = 192
    private class Face(val hash: String, val bitmap: Bitmap)
    private val faces = ConcurrentHashMap<String, Face>()

    /** The picture already in memory, or null. For callers that must not wait (a ringing call). */
    fun cached(did: String): Bitmap? = faces[did]?.bitmap

    /** Re-reads [did]'s current picture (a worker only). Null when it has none, it isn't downloaded yet, or the account changed. */
    fun load(app: P2pApp, did: String): Bitmap? {
        val ticket = app.profiles.session.ticket() ?: return null
        return try {
            val meta = app.profileWork(ticket) { it.accountProfile(did) }
            val hash = meta?.pictureHash?.takeIf { meta.pictureReady }
            if (hash == null) { faces.remove(did); return null }
            faces[did]?.takeIf { it.hash == hash }?.let { return it.bitmap }
            val bytes = app.profileWork(ticket) { it.profilePicture(did) } ?: return null
            val round = BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.let(::circle) ?: return null
            faces[did] = Face(hash, round)
            round
        } catch (e: Exception) {
            Log.d(P2pApp.TAG, "notification picture: $e")
            null
        }
    }

    fun icon(b: Bitmap?): IconCompat? = b?.let { IconCompat.createWithBitmap(it) }

    /** Centre-cropped to a [PX] circle (the Avatar's object-fit: cover). */
    private fun circle(src: Bitmap): Bitmap {
        val out = Bitmap.createBitmap(PX, PX, Bitmap.Config.ARGB_8888)
        val s = PX.toFloat() / minOf(src.width, src.height)
        val m = Matrix().apply { setScale(s, s); postTranslate((PX - src.width * s) / 2f, (PX - src.height * s) / 2f) }
        val paint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG).apply {
            shader = BitmapShader(src, Shader.TileMode.CLAMP, Shader.TileMode.CLAMP).apply { setLocalMatrix(m) }
        }
        Canvas(out).drawCircle(PX / 2f, PX / 2f, PX / 2f, paint)
        return out
    }
}
