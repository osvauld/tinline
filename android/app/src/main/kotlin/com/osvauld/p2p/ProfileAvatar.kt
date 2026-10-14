package com.osvauld.p2p

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.keyframes
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.PlatformTextStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.LineHeightStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import kotlin.math.max
import kotlin.math.roundToInt

/** tokens.css --avN-bg / --avN-ink: the six stable hue pairs. */
fun avatarColors(c: TinColors, hue: Int): Pair<Color, Color> = if (c.dark) when (Math.floorMod(hue, 6)) {
    0 -> Color(0xFF164F45) to Color(0xFFB7E6D8)
    1 -> Color(0xFF2F3A75) to Color(0xFFDFE3FF)
    2 -> Color(0xFF5C2A35) to Color(0xFFFFD9E0)
    3 -> Color(0xFF58431A) to Color(0xFFFFE2AE)
    4 -> Color(0xFF3A4F26) to Color(0xFFD8F0C0)
    else -> Color(0xFF4A3458) to Color(0xFFECD9FA)
} else when (Math.floorMod(hue, 6)) {
    0 -> Color(0xFFD7ECE6) to Color(0xFF0B5447)
    1 -> Color(0xFFE1E4F7) to Color(0xFF374582)
    2 -> Color(0xFFF6E3E6) to Color(0xFF8C3A4A)
    3 -> Color(0xFFF3E8D2) to Color(0xFF6D4E12)
    4 -> Color(0xFFE3EDD9) to Color(0xFF44622A)
    else -> Color(0xFFECE2F2) to Color(0xFF5D3F73)
}

/**
 * The fingerprint mandala of a DID (crates/core avatar.rs via UniFFI), built once per DID and size class into Compose paths
 * (100 x 100 box units, theme independent). The colours are picked per theme at draw time. See docs/design/FINGERPRINT-AVATARS.md.
 */
class FingerprintDrawing(val strokes: List<Stroke>, val strokeWidth: Float, val lightBg: Color, val lightFg: Color, val darkBg: Color, val darkFg: Color) {
    class Stroke(val path: Path, val gold: Boolean, val alpha: Float)
}

object FingerprintCache {
    /** Below this many dp the small variant is drawn (the core's SMALL_BELOW_PX). */
    const val SMALL_BELOW_DP = 40f
    private val none = Any()
    private val cache = java.util.concurrent.ConcurrentHashMap<String, Any>()

    /** The drawing for [did], or null when it is not a DID (or the native library is not loaded, as in JVM unit tests). */
    fun of(did: String?, sizeDp: Float): FingerprintDrawing? {
        if (did == null || !did.startsWith("did:")) return null
        val small = sizeDp < SMALL_BELOW_DP
        val v = cache.getOrPut((if (small) "s" else "l") + did) {
            runCatching { uniffi.p2pcore.avatarArt(did, small)?.let(::build) }.getOrNull() ?: none
        }
        return v as? FingerprintDrawing
    }

    private fun build(a: uniffi.p2pcore.AvatarArt): FingerprintDrawing {
        val strokes = a.strokes.map { s ->
            val p = Path()
            for (i in 0 until s.points.size / 2) {
                val x = s.points[2 * i]; val y = s.points[2 * i + 1]
                if (i == 0) p.moveTo(x, y) else p.lineTo(x, y)
            }
            if (s.closed) p.close()
            FingerprintDrawing.Stroke(p, s.gold, s.alpha)
        }
        fun col(v: UInt) = Color(v.toInt())
        return FingerprintDrawing(strokes, a.strokeWidth, col(a.light.bg), col(a.light.fg), col(a.dark.bg), col(a.dark.fg))
    }
}

/** Draws [d] filling the draw area: the strokes in [fg] (the gold layer in [gold]). */
fun DrawScope.drawFingerprint(d: FingerprintDrawing, fg: Color, gold: Color) {
    val s = size.minDimension / 100f
    val style = androidx.compose.ui.graphics.drawscope.Stroke(width = d.strokeWidth, cap = StrokeCap.Round, join = StrokeJoin.Round)
    withTransform({ scale(s, s, Offset.Zero) }) {
        for (st in d.strokes) drawPath(st.path, if (st.gold) gold else fg, alpha = st.alpha, style = style)
    }
}

/** The profile of [did] from the app's profile adapter (watching it), or null. Every avatar and profile surface reads through here. */
@Composable
fun rememberProfileView(did: String?): ProfileView? {
    if (did == null || !did.startsWith("did:")) return null
    val app = LocalContext.current.applicationContext as? P2pApp ?: return null
    val store = remember(app) { ProfileBackend.of(app) }
    val views by store.views.collectAsState()
    LaunchedEffect(store, did) { store.watch(did) }
    return views[did]
}

/**
 * lib/ui/Avatar.svelte, the one person avatar. Initials (round(size × .34) px, 700, +.02em) on the hue pair; with a [person] DID it
 * also shows that person's picture: ready -> the image fading in over the initials (crossfade 120 ms ease-out) with a 1 px inner
 * hairline; selected but not cached yet -> initials with a faint ring breathing .5 -> .12 -> .5 every 2.88 s (static under reduced
 * motion); removed, broken or undecodable -> initials. Never a spinner or percentage.
 */
@Composable
fun PgAvatar(name: String, hue: Int, size: Dp, person: String? = null, modifier: Modifier = Modifier, alpha: Float = 1f) {
    val c = Tin.c
    val art = remember(person, size) { FingerprintCache.of(person, size.value) }
    val (bg, fg) = if (art != null) (if (c.dark) art.darkBg to art.darkFg else art.lightBg to art.lightFg) else avatarColors(c, hue)
    val view = rememberProfileView(person)
    val image = view?.image
    val awaiting = view?.awaiting == true
    val reduced = rememberReducedMotion()
    val fontPx = (size.value * 0.34f).roundToInt()
    Box(
        modifier.size(size).graphicsLayer { this.alpha = alpha }.clip(CircleShape).background(bg).clearAndSetSemantics {},
        contentAlignment = Alignment.Center,
    ) {
        if (art != null) {
            // A person with a DID: the fingerprint mandala instead of initials (a picture, when ready, covers it below).
            val gold = c.gold
            Box(Modifier.fillMaxSize().drawBehind { drawFingerprint(art, fg, gold) })
        } else Text(
            initialsOf(name), color = fg, maxLines = 1, softWrap = false,
            style = TextStyle(
                fontFamily = Figtree, fontSize = fontPx.sp, lineHeight = fontPx.sp, fontWeight = FontWeight.Bold, letterSpacing = 0.02.em,
                platformStyle = PlatformTextStyle(includeFontPadding = false),
                lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.None),
            ),
        )
        if (awaiting) {
            val ringAlpha = if (reduced) .5f else rememberInfiniteTransition(label = "breathe").animateFloat(
                .5f, .5f, infiniteRepeatable(keyframes {
                    durationMillis = PgMotion.OVERLAY_OPEN * 12
                    .12f at durationMillis / 2 using CubicBezierEasing(.42f, 0f, .58f, 1f)
                }), label = "ring",
            ).value
            val ring = max(1.5f, fontPx * 0.03f)
            Box(Modifier.fillMaxSize().drawWithContent {
                val w = ring.dp.toPx()
                drawCircle(fg.copy(alpha = .45f * ringAlpha), radius = size.toPx() / 2 - w / 2, style = Stroke(w))
            })
        }
        if (image != null) {
            val a = remember(image) { Animatable(if (reduced) 1f else 0f) }
            LaunchedEffect(image) { a.animateTo(1f, tween(PgMotion.CROSSFADE, easing = PgMotion.easeOut)) }
            Image(image, null, Modifier.fillMaxSize().graphicsLayer { this.alpha = a.value }, contentScale = ContentScale.Crop)
            Box(Modifier.fillMaxSize().border(1.dp, if (c.dark) Color.White.copy(alpha = .12f) else Color.Black.copy(alpha = .1f), CircleShape))
        }
    }
}

/** A person's avatar that opens their photo full screen when tapped (only when a photo is ready; the mandala or initials
 *  don't open anything). Used on the contact page and my own profile. */
@Composable
fun ViewablePgAvatar(name: String, hue: Int, size: Dp, person: String?, modifier: Modifier = Modifier) {
    val view = rememberProfileView(person)
    var open by remember { mutableStateOf(false) }
    val image = view?.image
    val tap = if (image != null) Modifier.clip(CircleShape).clickable(onClickLabel = "View photo", role = androidx.compose.ui.semantics.Role.Image) { open = true } else Modifier
    PgAvatar(name, hue, size, person = person, modifier = modifier.then(tap))
    if (open && image != null) ProfilePhotoViewer(name, image) { open = false }
}

/** The photo, full screen on black, fit to the screen; the name at the top; tap anywhere, the close button or Back to go. */
@Composable
fun ProfilePhotoViewer(name: String, image: androidx.compose.ui.graphics.ImageBitmap, onClose: () -> Unit) {
    androidx.compose.ui.window.Dialog(onDismissRequest = onClose, properties = androidx.compose.ui.window.DialogProperties(usePlatformDefaultWidth = false)) {
        Box(Modifier.fillMaxSize().background(Color.Black).clickable(onClickLabel = "Close", onClick = onClose)) {
            Image(image, "$name’s photo", Modifier.fillMaxSize(), contentScale = ContentScale.Fit)
            androidx.compose.foundation.layout.Row(
                Modifier.fillMaxSize().padding(horizontal = 8.dp, vertical = 12.dp), verticalAlignment = Alignment.Top,
            ) {
                Text(name, Modifier.weight(1f).padding(start = 12.dp, top = 12.dp), color = Color.White, maxLines = 1,
                    style = TextStyle(fontFamily = Figtree, fontSize = 17.sp, fontWeight = FontWeight.SemiBold))
                Box(Modifier.size(48.dp).clip(CircleShape).clickable(onClickLabel = "Close", onClick = onClose), contentAlignment = Alignment.Center) {
                    androidx.compose.material3.Icon(Pg.X, "Close", tint = Color.White, modifier = Modifier.size(24.dp))
                }
            }
        }
    }
}
