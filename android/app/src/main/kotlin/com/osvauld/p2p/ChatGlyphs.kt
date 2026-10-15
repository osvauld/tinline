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

    /** The Tinline line set (crates/tin-icons), the same glyphs as PgIcons, the desktop and the playground. Only X and the
     *  bubble ticks keep the playground paths (no Tinline equivalent). */
    private fun tin(i: TinIcon): ImageVector = i.at(20.dp).all

    val Back = tin(TinIcons.Back)
    val Search = tin(TinIcons.Search)
    val Send = tin(TinIcons.Send)
    val Clip = tin(TinIcons.Clip)
    val Chev = tin(TinIcons.Chev)
    val ChevUp = tin(TinIcons.Chevup)
    val Info = tin(TinIcons.Info)
    val X = icon("x", "M6 6l12 12M18 6L6 18")
    val Reply = tin(TinIcons.Reply)
    val Edit = tin(TinIcons.Edit)
    val Trash = tin(TinIcons.Trash)
    val Copy = tin(TinIcons.Copy)
    val Check = tin(TinIcons.Check)
    /** The tick in a bubble's meta (Icon size 12, stroke 2.2 at that size). */
    val Tick = icon("tick", "M5 12.5l4.5 4.5L19 7", stroke = 2.2f * 24f / 12f)
    val TickClock = icon("tickclock", circle(12f, 12f, 9f), "M12 7v5l3 2", stroke = 2f * 24f / 11f)
    val TickWarn = icon("tickwarn", "M12 3l10 18H2zM12 10v5M12 18v.01", stroke = 2.2f * 24f / 12f)
    val Clock = tin(TinIcons.Clock)
    val File = tin(TinIcons.File)
    val Image = tin(TinIcons.Image)
    val Plus = tin(TinIcons.Plus)
    val Off = tin(TinIcons.Off)
    val Phone = tin(TinIcons.Phone)
    val Mic = tin(TinIcons.Mic)
    val Warning = tin(TinIcons.Warning)
    val Video = tin(TinIcons.Video)
    val Shield = tin(TinIcons.Shield)
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
