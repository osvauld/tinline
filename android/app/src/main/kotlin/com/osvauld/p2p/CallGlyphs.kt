package com.osvauld.p2p

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.dp

/** The call screens' glyphs: Tinline line icons, and the playground's flip arrows (VIcon.svelte). */
internal object CallGlyphs {
    private fun glyph(name: String, path: String) = ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply {
        addPath(PathParser().parsePathString(path).toNodes(), fill = null, stroke = SolidColor(Color.Black),
            strokeLineWidth = 1.8f, strokeLineCap = StrokeCap.Round, strokeLineJoin = StrokeJoin.Round)
    }.build()
    private fun t(i: TinIcon): ImageVector = i.at(24.dp).all
    // The Tinline line icons (crates/tin-icons): the mark answers, the dropped line ends. No handset.
    val Chev = t(TinIcons.Chev)
    val Info = t(TinIcons.Info)
    val Phone = t(TinIcons.Call)
    val End = t(TinIcons.Callend)
    val Mic = t(TinIcons.Mic)
    val MicOff = t(TinIcons.Micoff)
    val Speaker = t(TinIcons.Vol)
    val Video = t(TinIcons.Video)
    val VideoOff = t(TinIcons.Camoff)
    val Flip = glyph("Flip", "M19.5 10.5A8 8 0 0 0 5.2 8M5 4v4.2h4.2M4.5 13.5A8 8 0 0 0 18.8 16M19 20v-4.2h-4.2")
}
