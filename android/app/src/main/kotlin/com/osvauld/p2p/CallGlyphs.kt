package com.osvauld.p2p

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.dp

/** Playground ui/icons.ts and calls/video/VIcon.svelte: 24 grid, 1.8 stroke, round ends. */
internal object CallGlyphs {
    private fun glyph(name: String, path: String) = ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply {
        addPath(PathParser().parsePathString(path).toNodes(), fill = null, stroke = SolidColor(Color.Black),
            strokeLineWidth = 1.8f, strokeLineCap = StrokeCap.Round, strokeLineJoin = StrokeJoin.Round)
    }.build()
    val Chev = glyph("Minimize", "M7 10l5 5 5-5")
    val Info = glyph("Info", "M3,12a9,9 0 1 0 18,0a9,9 0 1 0 -18,0M12 11v6M12 7.5v.01")
    val Phone = glyph("Phone", "M5 4h3.5l1.5 4.5-2.2 1.4a11 11 0 0 0 6.3 6.3l1.4-2.2L20 15.5V19a1.5 1.5 0 0 1-1.6 1.5A16 16 0 0 1 3.5 5.6 1.5 1.5 0 0 1 5 4z")
    val Mic = glyph("Mic", "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM5 11a7 7 0 0 0 14 0M12 18v3")
    val MicOff = glyph("Mic off", "M15 9.3V6a3 3 0 0 0-5.7-1.3M9 9v2a3 3 0 0 0 4.6 2.5M5 11a7 7 0 0 0 11.5 5.4M19 11a7 7 0 0 1-.6 2.8M12 18v3M3 3l18 18")
    val Speaker = glyph("Speaker", "M4 9v6h4l5 4V5L8 9zM16.5 9a4 4 0 0 1 0 6")
    val Video = glyph("Camera", "M6 6.5h6.5a3 3 0 0 1 3 3v5a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3v-5a3 3 0 0 1 3-3zM15.5 10.6l4.4-2.5a.6.6 0 0 1 .9.5v6.8a.6.6 0 0 1-.9.5l-4.4-2.5")
    val VideoOff = glyph("Camera off", "M10 6.5H6.5A3.5 3.5 0 0 0 3 10v4a3.5 3.5 0 0 0 3.5 3.5h6A3 3 0 0 0 15.5 14.5v-4M15.5 10.6l4.4-2.5a.6.6 0 0 1 .9.5v6.8a.6.6 0 0 1-.9.5l-.4-.2M4 4l16 16")
    val Flip = glyph("Flip", "M19.5 10.5A8 8 0 0 0 5.2 8M5 4v4.2h4.2M4.5 13.5A8 8 0 0 0 18.8 16M19 20v-4.2h-4.2")
}
