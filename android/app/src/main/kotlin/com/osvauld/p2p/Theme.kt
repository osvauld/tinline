package com.osvauld.p2p

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp

/** Design tokens (docs/design/Main.dc.html), light and dark. Names follow the boards' CSS variables. */
@Immutable
class TinColors(
    val bg: Color, val sf: Color, val sf2: Color, val sf3: Color,
    val ink: Color, val ink2: Color, val ln: Color, val ln2: Color,
    val pr: Color, val onPr: Color, val prc: Color, val onPrc: Color,
    /** Thread amber: the line motif and the "in call" state. Never body text (use [threadText]). */
    val thread: Color, val threadText: Color,
    val er: Color, val onEr: Color, val erc: Color, val onErc: Color,
    val relay: Color, val relayC: Color, val onRelayC: Color,
    val warn: Color, val onWarn: Color,
    val pk: Color, val onPk: Color,
    val callAccept: Color, val callEnd: Color,
    val dark: Boolean,
)

val LightTin = TinColors(
    bg = Color(0xFFF5F6F3), sf = Color(0xFFFFFFFF), sf2 = Color(0xFFECEFEB), sf3 = Color(0xFFE2E6E1),
    ink = Color(0xFF17201D), ink2 = Color(0xFF4D5853), ln = Color(0xFFD3D9D4), ln2 = Color(0xFF8E9A94),
    pr = Color(0xFF0B6B5B), onPr = Color(0xFFFFFFFF), prc = Color(0xFFCDEBE3), onPrc = Color(0xFF06352D),
    thread = Color(0xFFC98A1E), threadText = Color(0xFF8A5A00),
    er = Color(0xFFB3261E), onEr = Color(0xFFFFFFFF), erc = Color(0xFFF9DEDC), onErc = Color(0xFF410E0B),
    relay = Color(0xFF4B5AA8), relayC = Color(0xFFE1E4F7), onRelayC = Color(0xFF2F3A75),
    warn = Color(0xFFFCEFD2), onWarn = Color(0xFF5C3B00),
    pk = Color(0xFFF3DDE6), onPk = Color(0xFF5E1F3A),
    callAccept = Color(0xFF0E7C69), callEnd = Color(0xFFC8372D), dark = false,
)

val DarkTin = TinColors(
    bg = Color(0xFF0F1513), sf = Color(0xFF161D1B), sf2 = Color(0xFF1D2623), sf3 = Color(0xFF26302D),
    ink = Color(0xFFE6ECE9), ink2 = Color(0xFFA6B2AD), ln = Color(0xFF33403B), ln2 = Color(0xFF7C8984),
    pr = Color(0xFF6FD3BD), onPr = Color(0xFF00382E), prc = Color(0xFF0E4E43), onPrc = Color(0xFFBDEFE2),
    thread = Color(0xFFE8B04A), threadText = Color(0xFFE8B04A),
    // The boards give the dark error only as text #F2B8B5 on container #8C1D18.
    er = Color(0xFFF2B8B5), onEr = Color(0xFF601410), erc = Color(0xFF8C1D18), onErc = Color(0xFFFFDAD6),
    relay = Color(0xFFB8C1F5), relayC = Color(0xFF2F3A75), onRelayC = Color(0xFFE1E4F7),
    warn = Color(0xFF4A3500), onWarn = Color(0xFFFFDFA0),
    pk = Color(0xFF5A2A3E), onPk = Color(0xFFF3DDE6),
    callAccept = Color(0xFF0E7C69), callEnd = Color(0xFFC8372D), dark = true,
)

val LocalTin = staticCompositionLocalOf { LightTin }

@OptIn(ExperimentalTextApi::class)
private fun figtree(vararg weights: Int) = FontFamily(weights.map { w ->
    Font(R.font.figtree_variable, FontWeight(w), variationSettings = FontVariation.Settings(FontVariation.weight(w)))
})

val Figtree = figtree(400, 500, 600, 700)
val PlexMono = FontFamily(
    Font(R.font.plexmono_regular, FontWeight.Normal),
    Font(R.font.plexmono_medium, FontWeight.Medium),
)

/** Type scale from the tokens board: size/line-height weight. All sp, so the system font scale applies. */
object TinType {
    val display = TextStyle(fontFamily = Figtree, fontSize = 34.sp, lineHeight = 40.sp, fontWeight = FontWeight.Bold, letterSpacing = (-0.02).em)
    /** Screen headings in the Android boards (28/34). */
    val h1 = TextStyle(fontFamily = Figtree, fontSize = 28.sp, lineHeight = 34.sp, fontWeight = FontWeight.Bold, letterSpacing = (-0.01).em)
    val titleL = TextStyle(fontFamily = Figtree, fontSize = 24.sp, lineHeight = 30.sp, fontWeight = FontWeight.Bold)
    val titleM = TextStyle(fontFamily = Figtree, fontSize = 18.sp, lineHeight = 24.sp, fontWeight = FontWeight.SemiBold)
    val bodyL = TextStyle(fontFamily = Figtree, fontSize = 16.sp, lineHeight = 24.sp, fontWeight = FontWeight.Normal)
    val bodyM = TextStyle(fontFamily = Figtree, fontSize = 14.sp, lineHeight = 20.sp, fontWeight = FontWeight.Normal)
    val label = TextStyle(fontFamily = Figtree, fontSize = 14.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold)
    val caption = TextStyle(fontFamily = Figtree, fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.Medium)
    val mono = TextStyle(fontFamily = PlexMono, fontSize = 15.sp, lineHeight = 24.sp, fontWeight = FontWeight.Medium)
    val button = TextStyle(fontFamily = Figtree, fontSize = 16.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold)
}

private val M3Type = Typography(
    displayMedium = TinType.display, headlineLarge = TinType.h1, headlineMedium = TinType.titleL,
    titleLarge = TinType.titleL, titleMedium = TinType.titleM, titleSmall = TinType.label,
    bodyLarge = TinType.bodyL, bodyMedium = TinType.bodyM, bodySmall = TinType.caption,
    labelLarge = TinType.label, labelMedium = TinType.caption, labelSmall = TinType.caption,
)

private fun scheme(c: TinColors) = if (c.dark) darkColorScheme(
    primary = c.pr, onPrimary = c.onPr, primaryContainer = c.prc, onPrimaryContainer = c.onPrc,
    secondary = c.relay, onSecondary = c.onRelayC, secondaryContainer = c.relayC, onSecondaryContainer = c.onRelayC,
    tertiary = c.thread, onTertiary = c.ink, tertiaryContainer = c.warn, onTertiaryContainer = c.onWarn,
    background = c.bg, onBackground = c.ink, surface = c.sf, onSurface = c.ink, surfaceVariant = c.sf2, onSurfaceVariant = c.ink2,
    surfaceContainerLowest = c.bg, surfaceContainerLow = c.sf, surfaceContainer = c.sf2,
    surfaceContainerHigh = c.sf3, surfaceContainerHighest = c.sf3,
    outline = c.ln2, outlineVariant = c.ln, error = c.er, onError = c.onEr, errorContainer = c.erc, onErrorContainer = c.onErc,
) else lightColorScheme(
    primary = c.pr, onPrimary = c.onPr, primaryContainer = c.prc, onPrimaryContainer = c.onPrc,
    secondary = c.relay, onSecondary = Color.White, secondaryContainer = c.relayC, onSecondaryContainer = c.onRelayC,
    tertiary = c.thread, onTertiary = c.ink, tertiaryContainer = c.warn, onTertiaryContainer = c.onWarn,
    background = c.bg, onBackground = c.ink, surface = c.sf, onSurface = c.ink, surfaceVariant = c.sf2, onSurfaceVariant = c.ink2,
    surfaceContainerLowest = c.bg, surfaceContainerLow = c.sf, surfaceContainer = c.sf2,
    surfaceContainerHigh = c.sf3, surfaceContainerHighest = c.sf3,
    outline = c.ln2, outlineVariant = c.ln, error = c.er, onError = c.onEr, errorContainer = c.erc, onErrorContainer = c.onErc,
)

/** Dynamic colour is deliberately off: the teal is the brand. [dark] follows the system. */
@Composable
fun TinlineTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    val c = if (dark) DarkTin else LightTin
    CompositionLocalProvider(LocalTin provides c) {
        MaterialTheme(colorScheme = scheme(c), typography = M3Type, content = content)
    }
}

/** Shorthand: `Tin.c.pr`. */
object Tin {
    val c: TinColors @Composable @ReadOnlyComposable get() = LocalTin.current
}
