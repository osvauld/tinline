package com.osvauld.p2p

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Shader
import androidx.compose.ui.graphics.asImageBitmap
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import org.json.JSONObject
import java.io.File

/**
 * Debug gallery only: an in-memory profile adapter seeded like the playground (seed.ts: Maya Kurian and her contacts), so the
 * profile / settings screens can be captured next to the playground with the same data. Never used by production code.
 */
class FakeProfileStore(private val pictures: File? = null) : ProfileStore {
    companion object {
        const val ME = "did:key:z6MkMayaKurianDemo"
        fun did(name: String) = "did:key:z6Mk${name.replace(" ", "")}Demo"
    }
    private val _views = MutableStateFlow<Map<String, ProfileView>>(emptyMap())
    override val views: StateFlow<Map<String, ProfileView>> = _views
    override val self = MutableStateFlow<String?>(ME)
    override val online = MutableStateFlow(true)
    /** Make the next write fail (the playground drawer's "fail next save"). */
    var failNext = false

    init {
        put(ME, JSONObject().put("name", "Maya Kurian").put("status", "Slow mornings, fast evenings")
            .put("about", "Designing calm software. Ask me about trains.").put("pronouns", "she/her")
            .put("availability", JSONObject().put("available", true)))
    }

    fun put(did: String, fields: JSONObject, picture: String? = null, ready: Boolean = true) {
        val img = picture?.let { name -> pictures?.let { File(it, "$name.png") }?.takeIf { it.exists() }?.let { BitmapFactory.decodeFile(it.path) } ?: sample(name) }
        _views.value = _views.value + (did to ProfileView(fields, picture?.let { "sample:$it" }, ready, if (ready) img?.asImageBitmap() else null))
    }
    fun fields(did: String = ME): JSONObject = JSONObject(_views.value[did]?.fields?.toString() ?: "{}")

    override fun watch(did: String) {}
    override fun refresh(did: String) {}

    override suspend fun apply(op: ProfileOp): String? {
        delay(60)
        if (failNext) {
            failNext = false
            return when (op) {
                is ProfileOp.ReplacePicture -> "Couldn’t save your picture. Your current picture is unchanged."
                ProfileOp.RemovePicture -> "Couldn’t remove your picture. It is unchanged."
                else -> ProfilesController.SAVE_FAILED
            }
        }
        val f = fields()
        val v = _views.value[ME] ?: ProfileView()
        when (op) {
            is ProfileOp.SetName -> { f.put("name", op.name); _views.value = _views.value + (ME to v.copy(fields = f)) }
            is ProfileOp.SetField -> { if (op.json == null) f.remove(op.key) else f.put(op.key, JSONObject("{\"v\":${op.json}}").get("v")); _views.value = _views.value + (ME to v.copy(fields = f)) }
            is ProfileOp.SetAvailable -> {
                f.put("availability", JSONObject().put("available", op.available).also { if (op.untilSecs != null) it.put("until", op.untilSecs) })
                _views.value = _views.value + (ME to v.copy(fields = f))
            }
            is ProfileOp.ReplacePicture -> {
                val bmp = BitmapFactory.decodeByteArray(op.png, 0, op.png.size)
                _views.value = _views.value + (ME to v.copy(hash = "local:${op.png.size}", ready = true, image = bmp?.asImageBitmap()))
            }
            ProfileOp.RemovePicture -> _views.value = _views.value + (ME to v.copy(hash = null, ready = false, image = null))
        }
        return null
    }
}

/** A soft stand-in portrait when the playground's exported sample PNG isn't on the device. */
fun sample(seed: String, size: Int = 512, w: Int = size, h: Int = size): Bitmap {
    val b = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
    val c = Canvas(b)
    val hue = (seed.hashCode() and 0xffff) % 360f
    val p = Paint(Paint.ANTI_ALIAS_FLAG)
    p.shader = LinearGradient(0f, 0f, w.toFloat(), h.toFloat(), android.graphics.Color.HSVToColor(floatArrayOf(hue, .45f, .85f)),
        android.graphics.Color.HSVToColor(floatArrayOf((hue + 60) % 360, .5f, .55f)), Shader.TileMode.CLAMP)
    c.drawRect(0f, 0f, w.toFloat(), h.toFloat(), p)
    p.shader = null
    val m = minOf(w, h).toFloat(); val cx = w * .5f
    p.color = android.graphics.Color.HSVToColor(floatArrayOf((hue + 200) % 360, .38f, .4f)); c.drawOval(cx - m * .46f, h * 1.02f - m * .34f, cx + m * .46f, h * 1.02f + m * .34f, p)
    p.color = android.graphics.Color.HSVToColor(floatArrayOf(24f, .35f, .62f)); c.drawOval(cx - m * .17f, h * .46f - m * .205f, cx + m * .17f, h * .46f + m * .205f, p)
    return b
}
