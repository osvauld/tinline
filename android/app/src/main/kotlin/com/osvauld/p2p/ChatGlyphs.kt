package com.osvauld.p2p

import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.dp

/**
 * The playground's 24 x 24 stroke icons (docs/design/playground/src/lib/ui/icons.ts and video/VIcon.svelte), for the
 * conversation, its composer, find bar and sheets. Stroke 1.8, round caps and joins, as Icon.svelte draws them.
 * `<circle>` and `<rect>` are written out as paths (PathParser takes path data only).
 */
object Pg {
    private fun circle(cx: Float, cy: Float, r: Float) = "M${cx - r},${cy}a$r,$r 0 1 0 ${2 * r},0a$r,$r 0 1 0 ${-2 * r},0z"
    private fun rect(x: Float, y: Float, w: Float, h: Float, r: Float) =
        "M${x + r},${y}h${w - 2 * r}a$r,$r 0 0 1 $r,$r" + "v${h - 2 * r}a$r,$r 0 0 1 ${-r},$r" + "h${-(w - 2 * r)}a$r,$r 0 0 1 ${-r},${-r}" + "v${-(h - 2 * r)}a$r,$r 0 0 1 $r,${-r}z"

    private fun icon(name: String, vararg d: String, stroke: Float = 1.8f): ImageVector =
        ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply {
            d.forEach {
                addPath(PathParser().parsePathString(it).toNodes(), fill = null, stroke = SolidColor(Color.Black),
                    strokeLineWidth = stroke, strokeLineCap = StrokeCap.Round, strokeLineJoin = StrokeJoin.Round)
            }
        }.build()

    val Back = icon("back", "M15 5l-7 7 7 7")
    val Search = icon("search", circle(11f, 11f, 7f), "M20 20l-4-4")
    val Send = icon("send", "M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z")
    val Clip = icon("clip", "M21 11l-9 9a5 5 0 0 1-7-7l9-9a3.5 3.5 0 0 1 5 5l-9 9a2 2 0 0 1-3-3l8-8")
    val Chev = icon("chev", "M7 10l5 5 5-5")
    val ChevUp = icon("chevup", "M7 14l5-5 5 5")
    val Info = icon("info", circle(12f, 12f, 9f), "M12 11v6M12 7.5v.01")
    val X = icon("x", "M6 6l12 12M18 6L6 18")
    val Reply = icon("reply", "M9 14L4 9l5-5M4 9h10a6 6 0 0 1 6 6v3")
    val Edit = icon("edit", "M4 20h4L19 9l-4-4L4 16v4zM13 7l4 4")
    val Trash = icon("trash", "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13")
    val Copy = icon("copy", rect(9f, 9f, 11f, 11f, 2f), "M5 15V6a2 2 0 0 1 2-2h8")
    val Check = icon("check", "M5 12.5l4.5 4.5L19 7")
    /** The tick in a bubble's meta (Icon size 12, stroke 2.2 at that size). */
    val Tick = icon("tick", "M5 12.5l4.5 4.5L19 7", stroke = 2.2f * 24f / 12f)
    val TickClock = icon("tickclock", circle(12f, 12f, 9f), "M12 7v5l3 2", stroke = 2f * 24f / 11f)
    val TickWarn = icon("tickwarn", "M12 3l10 18H2zM12 10v5M12 18v.01", stroke = 2.2f * 24f / 12f)
    val Clock = icon("clock", circle(12f, 12f, 9f), "M12 7v5l3 2")
    val File = icon("file", "M7 3h7l5 5v13H7zM14 3v5h5")
    val Image = icon("image", rect(3f, 4f, 18f, 16f, 3f), circle(9f, 10f, 1.6f), "M4 18l5-5 4 4 3-3 4 4")
    val Plus = icon("plus", "M12 5v14M5 12h14")
    val Off = icon("off", circle(12f, 12f, 9f), "M5.6 5.6l12.8 12.8")
    val Phone = icon("phone", "M5 4h3.5l1.5 4.5-2.2 1.4a11 11 0 0 0 6.3 6.3l1.4-2.2L20 15.5V19a1.5 1.5 0 0 1-1.6 1.5A16 16 0 0 1 3.5 5.6 1.5 1.5 0 0 1 5 4z")
    val Mic = icon("mic", rect(9f, 3f, 6f, 11f, 3f), "M5 11a7 7 0 0 0 14 0M12 18v3")
    val Warning = icon("warning", "M12 3l10 18H2zM12 10v5M12 18v.01")
    val Video = icon("video", rect(3f, 6.5f, 12.5f, 11f, 3f), "M15.5 10.6l4.4-2.5a.6.6 0 0 1 .9.5v6.8a.6.6 0 0 1-.9.5l-4.4-2.5")
    val Shield = icon("shield", "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z")
}

/** Motion tokens (playground src/lib/fx/motion.ts): durations in ms and the easings. */
object PgMotion {
    const val PRESS = 100; const val RELEASE = 140; const val SWITCH = 120; const val CROSSFADE = 120
    const val ENTER = 200; const val EXIT = 140; const val BUBBLE_IN = 160
    const val OVERLAY_OPEN = 240; const val OVERLAY_CLOSE = 220; const val INDICATOR = 200; const val TAB_OUT = 90; const val TAB_IN = 210
    val Out = CubicBezierEasing(.2f, .8f, .2f, 1f)
    val In = CubicBezierEasing(.4f, 0f, 1f, 1f)
    val InOut = CubicBezierEasing(.4f, 0f, .2f, 1f)
    val easeOut = Out
    val easeInOut = InOut
    /** slide.ts: 1 - (1 - t)^3. */
    val cubicOut = androidx.compose.animation.core.Easing { t -> 1f - (1f - t) * (1f - t) * (1f - t) }
}
