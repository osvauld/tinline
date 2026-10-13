package com.osvauld.p2p

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.dp

/**
 * The playground's icon set (docs/design/playground/src/lib/ui/icons.ts): 24 grid, stroke 1.8, round caps and joins.
 * Circles and rects are written as paths. Tint with Icon(tint = ...); the stroke colour here is replaced by the tint.
 */
object PgIcons {
    private fun circle(cx: Float, cy: Float, r: Float) = "M${cx - r} ${cy}a$r $r 0 1 0 ${2 * r} 0a$r $r 0 1 0 ${-2 * r} 0z"

    private fun icon(name: String, vararg d: String): ImageVector = ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply {
        d.forEach {
            addPath(PathParser().parsePathString(it).toNodes(), stroke = SolidColor(Color.Black), strokeLineWidth = 1.8f,
                strokeLineCap = StrokeCap.Round, strokeLineJoin = StrokeJoin.Round)
        }
    }.build()

    val Chat = icon("chat", "M4 5h16v11H9l-5 4z")
    val Users = icon("users", circle(9f, 8f, 3.5f), "M2.5 20c.5-3.5 3.2-5.5 6.5-5.5s6 2 6.5 5.5", circle(17f, 9f, 2.5f), "M16.5 14.6c2.6.2 4.5 2 5 4.9")
    val Phone = icon("phone", "M5 4h3.5l1.5 4.5-2.2 1.4a11 11 0 0 0 6.3 6.3l1.4-2.2L20 15.5V19a1.5 1.5 0 0 1-1.6 1.5A16 16 0 0 1 3.5 5.6 1.5 1.5 0 0 1 5 4z")
    val Contact = icon("contact", "M6 3h12a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", circle(12f, 10f, 3f),
        "M7.5 17c.8-2 2.5-3 4.5-3s3.7 1 4.5 3", "M2 7h2M2 12h2M2 17h2")
    val Search = icon("search", circle(11f, 11f, 7f), "M20 20l-4-4")
    val Gear = icon("gear", circle(12f, 12f, 3f),
        "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z")
    val Plus = icon("plus", "M12 5v14M5 12h14")
    val Back = icon("back", "M15 5l-7 7 7 7")
    val X = icon("x", "M6 6l12 12M18 6L6 18")
    val Clock = icon("clock", circle(12f, 12f, 9f), "M12 7v5l3 2")
    val Mute = icon("mute", "M8.5 5.3A6 6 0 0 1 18 11v4M6 11v5l-2 2h12M10 21h4M3 3l18 18")
    val CallIn = icon("call-in", "M17 7L7 17M15 17H7V9")
    val CallOut = icon("call-out", "M7 17L17 7M9 7h8v8")
    val UserPlus = icon("userplus", circle(9f, 8f, 3.5f), "M2.5 20c.5-3.5 3.2-5.5 6.5-5.5s6 2 6.5 5.5M19 8v6M16 11h6")
    val Chev = icon("chev", "M7 10l5 5 5-5")
    val Chevr = icon("chevr", "M10 7l5 5-5 5")
    val Bell = icon("bell", "M6 16v-5a6 6 0 0 1 12 0v5l2 2H4zM10 21h4")
    val Devices = icon("devices", "M3 6h13v9H3zM1 18h17M18 9h4v9h-4z")
    val Vol = icon("vol", "M4 9v6h4l5 4V5L8 9zM16.5 9a4 4 0 0 1 0 6")
    val Camera = icon("camera", "M4 8h3l2-3h6l2 3h3v12H4z", circle(12f, 13.5f, 3.5f))
    private fun rect(x: Float, y: Float, w: Float, h: Float, r: Float) =
        "M${x + r} ${y}h${w - 2 * r}a$r $r 0 0 1 $r ${r}v${h - 2 * r}a$r $r 0 0 1 ${-r} ${r}h${-(w - 2 * r)}a$r $r 0 0 1 ${-r} ${-r}v${-(h - 2 * r)}a$r $r 0 0 1 $r ${-r}z"
    val Edit = icon("edit", "M4 20h4L19 9l-4-4L4 16v4zM13 7l4 4")
    val Trash = icon("trash", "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13")
    val Check = icon("check", "M5 12.5l4.5 4.5L19 7")
    val Link = icon("link", "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1")
    val Info = icon("info", circle(12f, 12f, 9f), "M12 11v6M12 7.5v.01")
    val Shield = icon("shield", "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z")
    val Off = icon("off", circle(12f, 12f, 9f), "M5.6 5.6l12.8 12.8")
    val Mic = icon("mic", rect(9f, 3f, 6f, 11f, 3f), "M5 11a7 7 0 0 0 14 0M12 18v3")
    val MicOff = icon("micoff", "M15 9.3V6a3 3 0 0 0-5.7-1.3M9 9v2a3 3 0 0 0 4.6 2.5M5 11a7 7 0 0 0 11.5 5.4M19 11a7 7 0 0 1-.6 2.8M12 18v3M3 3l18 18")
    val Battery = icon("battery", rect(2f, 7f, 17f, 10f, 2f), "M22 11v2M6 10v4M10 10v4")
    val Key = icon("key", circle(8f, 15f, 4f), "M11 12l9-9M16 7l3 3M14 9l2 2")
    val Lock = icon("lock", rect(5f, 11f, 14f, 10f, 2f), "M8 11V8a4 4 0 0 1 8 0v3")
    val Warning = icon("warning", "M12 3l10 18H2zM12 10v5M12 18v.01")
    val Image = icon("image", rect(3f, 4f, 18f, 16f, 3f), circle(9f, 10f, 1.6f), "M4 18l5-5 4 4 3-3 4 4")
}
