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
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
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
    /** Received bubble fill and its hairline (tokens.css --bub-in, --bub-in-line). */
    val bubIn: Color, val bubInLine: Color,
    /** Fill of raised things (--surface-raised): the surface in light, the lighter surface in dark. */
    val sfRaised: Color,
    val dark: Boolean,
)

val LightTin = TinColors(
    bg = Color(0xFFF5F6F3), sf = Color(0xFFFFFFFF), sf2 = Color(0xFFECEFEB), sf3 = Color(0xFFE2E6E1),
    ink = Color(0xFF17201D), ink2 = Color(0xFF4D5853), ln = Color(0xFFD3D9D4), ln2 = Color(0xFF8E9A94),
    pr = Color(0xFF0B6B5B), onPr = Color(0xFFFFFFFF), prc = Color(0xFFCDEBE3), onPrc = Color(0xFF06352D),
    thread = Color(0xFFC98A1E), threadText = Color(0xFF845A10),
    er = Color(0xFFA12B23), onEr = Color(0xFFFFFFFF), erc = Color(0xFFF8E3E1), onErc = Color(0xFF410E0B),
    relay = Color(0xFF4B5AA8), relayC = Color(0xFFE1E4F7), onRelayC = Color(0xFF374582),
    warn = Color(0xFFFCEFD2), onWarn = Color(0xFF5C3B00),
    pk = Color(0xFFF3DDE6), onPk = Color(0xFF5E1F3A),
    callAccept = Color(0xFF0E7C69), callEnd = Color(0xFFC8372D),
    bubIn = Color(0xFFFFFFFF), bubInLine = Color(0xFFE2E7E1), sfRaised = Color(0xFFFFFFFF), dark = false,
)

val DarkTin = TinColors(
    bg = Color(0xFF0F1513), sf = Color(0xFF161D1B), sf2 = Color(0xFF242D2A), sf3 = Color(0xFF26302D),
    ink = Color(0xFFE6ECE9), ink2 = Color(0xFFA6B2AD), ln = Color(0xFF33403B), ln2 = Color(0xFF7C8984),
    pr = Color(0xFF6FD3BD), onPr = Color(0xFF00382E), prc = Color(0xFF0E4E43), onPrc = Color(0xFFBDEFE2),
    thread = Color(0xFFE8B04A), threadText = Color(0xFFE8B04A),
    // The boards give the dark error only as text #F2B8B5 on container #8C1D18.
    er = Color(0xFFF2B8B5), onEr = Color(0xFF601410), erc = Color(0xFF4D201C), onErc = Color(0xFFFFDAD6),
    relay = Color(0xFFB8C1F5), relayC = Color(0xFF2F3A75), onRelayC = Color(0xFFE1E4F7),
    warn = Color(0xFF4A3500), onWarn = Color(0xFFFFDFA0),
    pk = Color(0xFF5A2A3E), onPk = Color(0xFFF3DDE6),
    callAccept = Color(0xFF0E7C69), callEnd = Color(0xFFC8372D),
    bubIn = Color(0xFF1C2422), bubInLine = Color(0xFF2A3531), sfRaised = Color(0xFF242D2A), dark = true,
)

/** Playground --danger-btn / --accent: call action fills, distinct from danger/accent text. */
val TinColors.callButtonEnd: Color get() = if (dark) Color(0xFFC73A30) else Color(0xFFA12B23)
val TinColors.callButtonAnswer: Color get() = if (dark) Color(0xFF117E6B) else Color(0xFF0B6B5B)

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

/** Material3 takes menu (and snackbar) corners from the theme's extraSmall, and 1.3.1's DropdownMenu does not honour its `shape`
 * parameter, so the 14 dp control radius goes here. Theme shapes must be CornerBasedShape, so these are circular, not smoothed. */
private val TinShapes = androidx.compose.material3.Shapes(extraSmall = androidx.compose.foundation.shape.RoundedCornerShape(14.dp))

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
        MaterialTheme(colorScheme = scheme(c), typography = M3Type, shapes = TinShapes, content = content)
    }
}

/** Shorthand: `Tin.c.pr`. */
object Tin {
    val c: TinColors @Composable @ReadOnlyComposable get() = LocalTin.current
}

/**
 * The depth ladder (PHILOSOPHY section 9; tokens.css --shadow-lift/pop/sheet). Flat is no shadow, only a hairline.
 * Each step is two layers, a tight contact shadow and a wide ambient one; light tints them with the palette green-black
 * (9, 23, 17), dark uses black and leans on [TinColors.sfRaised]. Android draws shadows from an elevation, so the CSS blur radii
 * map to elevations (contact, ambient) and the CSS alphas to the colour alpha.
 */
enum class Depth(val contactDp: Float, val ambientDp: Float, val lightAlpha: Float, val darkAlpha: Float) {
    Flat(0f, 0f, 0f, 0f),
    Lift(1.5f, 5f, 0.30f, 0.60f),
    Pop(3f, 12f, 0.32f, 0.70f),
    Sheet(4f, 22f, 0.36f, 0.75f),
}

private val ShadowTint = Color(9, 23, 17)

/** Raise this element to [level]. Apply before clip/background; [shape] is the outline the shadow follows. */
@Composable
fun Modifier.depth(level: Depth, shape: androidx.compose.ui.graphics.Shape): Modifier {
    if (level == Depth.Flat) return this
    val dark = Tin.c.dark
    val tint = (if (dark) Color.Black else ShadowTint).copy(alpha = if (dark) level.darkAlpha else level.lightAlpha)
    return this
        .shadow(level.ambientDp.dp, shape, clip = false, ambientColor = tint, spotColor = tint)
        .shadow(level.contactDp.dp, shape, clip = false, ambientColor = tint, spotColor = tint)
}

// ---- tokens.css values the screens port needs that TinColors has no slot for (shell area, 2026-10-13; additive) ----
/** --accent: filled primary surfaces (badges, Answer). Dark is #117e6b, not [TinColors.pr] (which is --accent-ink there). */
val TinColors.accent: Color get() = if (dark) Color(0xFF117E6B) else Color(0xFF0B6B5B)
val TinColors.onAccent: Color get() = if (dark) Color(0xFFF2FFFB) else Color.White
/** --accent-ink: teal as text and icons. */
val TinColors.accentInk: Color get() = if (dark) Color(0xFF6FD3BD) else Color(0xFF0A5D50)
/** --accent-soft: selected chips, active nav fills, the new-chat button. */
val TinColors.accentSoft: Color get() = if (dark) Color(0xFF14433A) else Color(0xFFD7ECE6)
/** --hairline. */
val TinColors.hairline: Color get() = if (dark) Color(0xFF2A3531) else Color(0xFFE2E7E1)
/** --danger-btn / --on-danger-btn: filled Decline. */
val TinColors.dangerBtn: Color get() = if (dark) Color(0xFFC73A30) else Color(0xFFA12B23)
/** --danger as text. */
val TinColors.danger: Color get() = if (dark) Color(0xFFF2B8B5) else Color(0xFFA12B23)

/** tokens.css type scale: --fs-title 17, --fs-body 15 (line 21), --fs-sub 13, --fs-meta 11. */
object TinFs {
    val body = TextStyle(fontFamily = Figtree, fontSize = 15.sp, lineHeight = 21.sp, fontWeight = FontWeight.Normal)
    val sub = TextStyle(fontFamily = Figtree, fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Normal)
    val meta = TextStyle(fontFamily = Figtree, fontSize = 11.sp, lineHeight = 14.sp, fontWeight = FontWeight.Normal, fontFeatureSettings = "tnum")
    /** Section headers: 12, 650, uppercase, .06em. */
    val section = TextStyle(fontFamily = Figtree, fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight(650), letterSpacing = 0.06.em)
}

// Conversation tokens from tokens.css.
/** --canvas: the conversation's scroll area behind the bubbles. */
val TinColors.canvas: Color get() = if (dark) Color(0xFF0B100E) else Color(0xFFEDEFE9)
/** --bub-meta-in / --bub-meta-out: the inline time and status in a received / sent bubble. */
val TinColors.bubMetaIn: Color get() = if (dark) Color(0xFF95A19B) else Color(0xFF68746E)
val TinColors.bubMetaOut: Color get() = if (dark) Color(0xFF8FC9B9) else Color(0xFF33655A)
/** --accent / --on-accent: primary-action fills (dark `pr` is --accent-ink, a text colour). */
val TinColors.accentFill: Color get() = accent
val TinColors.onAccentFill: Color get() = onAccent

/** Shared people/groups palette from tokens.css. */
val TinColors.onDangerBtn: Color get() = Color.White
val TinColors.lav: Color get() = if (dark) Color(0xFF2F3A75) else Color(0xFFE1E4F7)
val TinColors.lavInk: Color get() = if (dark) Color(0xFFDFE3FF) else Color(0xFF374582)
val TinColors.gold: Color get() = if (dark) Color(0xFFE8B04A) else Color(0xFFC98A1E)
val TinColors.goldInk: Color get() = if (dark) Color(0xFFE8B04A) else Color(0xFF845A10)
