package com.osvauld.p2p

import android.view.WindowManager
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.MutableTransitionState
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.ripple
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import kotlinx.coroutines.launch

/*
 * The groups area's building blocks, ported from the playground (docs/design/playground/src/screens/groups: 
 * Btn, Pill, Collapse, fold; lib/ui Badge, Icon; chat/Layer as the Android sheet). Values come from tokens.css and
 * lib/fx/motion.ts; the few colours TinColors doesn't name yet are the extension values below.
 */

// ------------------------------------------------------------------ tokens beyond TinColors (tokens.css)

// Shared colour extensions live in Theme.kt.

// ------------------------------------------------------------------ motion (lib/fx/motion.ts)

object GMotion {
    const val PRESS = 100
    const val RELEASE = 140
    const val SWITCH = 120
    const val CROSSFADE = 120
    const val ENTER = 200
    const val EXIT = 140
    val EaseOut = CubicBezierEasing(.2f, .8f, .2f, 1f)
    val EaseIn = CubicBezierEasing(.4f, 0f, 1f, 1f)
    /** svelte cubicOut / cubicIn, used by fold() for list inserts and removals. */
    val CubicOut = CubicBezierEasing(.33f, 1f, .68f, 1f)
    val CubicIn = CubicBezierEasing(.32f, 0f, .67f, 0f)
}

/** Reduced motion: the system's animator scale at 0 ("Remove animations"); every motion here then jumps to its end. */
@Composable
fun reducedMotion(): Boolean {
    val ctx = LocalView.current.context
    return remember { android.provider.Settings.Global.getFloat(ctx.contentResolver, android.provider.Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f }
}

// ------------------------------------------------------------------ icons (lib/ui/icons.ts, 24 grid, stroke 1.8, round)

object GIcons {
    private fun icon(name: String, d: String): ImageVector = ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply {
        addPath(PathParser().parsePathString(d).toNodes(), fill = null, stroke = SolidColor(Color.Black), strokeLineWidth = 1.8f,
            strokeLineCap = StrokeCap.Round, strokeLineJoin = StrokeJoin.Round)
    }.build()

    val Users = icon("users", "M5.5,8a3.5,3.5 0 1 0 7,0a3.5,3.5 0 1 0 -7,0M2.5 20c.5-3.5 3.2-5.5 6.5-5.5s6 2 6.5 5.5M14.5,9a2.5,2.5 0 1 0 5,0a2.5,2.5 0 1 0 -5,0M16.5 14.6c2.6.2 4.5 2 5 4.9")
    val Plus = icon("plus", "M12 5v14M5 12h14")
    val Chev = icon("chev", "M7 10l5 5 5-5")
    val ChevR = icon("chevr", "M10 7l5 5-5 5")
    val Back = icon("back", "M15 5l-7 7 7 7")
    val UserPlus = icon("userplus", "M5.5,8a3.5,3.5 0 1 0 7,0a3.5,3.5 0 1 0 -7,0M2.5 20c.5-3.5 3.2-5.5 6.5-5.5s6 2 6.5 5.5M19 8v6M16 11h6")
    val Link = icon("link", "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1")
    val Server = icon("server", "M5,4h14a2,2 0 0 1 2,2v3a2,2 0 0 1 -2,2h-14a2,2 0 0 1 -2,-2v-3a2,2 0 0 1 2,-2zM5,13h14a2,2 0 0 1 2,2v3a2,2 0 0 1 -2,2h-14a2,2 0 0 1 -2,-2v-3a2,2 0 0 1 2,-2zM7 7.5v.01M7 16.5v.01")
    val Phone = icon("phone", "M5 4h3.5l1.5 4.5-2.2 1.4a11 11 0 0 0 6.3 6.3l1.4-2.2L20 15.5V19a1.5 1.5 0 0 1-1.6 1.5A16 16 0 0 1 3.5 5.6 1.5 1.5 0 0 1 5 4z")
    val Leave = icon("leave", "M10 4H5v16h5M15 8l4 4-4 4M19 12H9")
    val Flag = icon("flag", "M5 21V4M5 4h11l-2 4 2 4H5")
    val Chat = icon("chat", "M4 5h16v11H9l-5 4z")
    val Clock = icon("clock", "M3,12a9,9 0 1 0 18,0a9,9 0 1 0 -18,0M12 7v5l3 2")
    val Off = icon("off", "M3,12a9,9 0 1 0 18,0a9,9 0 1 0 -18,0M5.6 5.6l12.8 12.8")
    val Shield = icon("shield", "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z")
    val Check = icon("check", "M5 12.5l4.5 4.5L19 7")
    val X = icon("x", "M6 6l12 12M18 6L6 18")
    val Info = icon("info", "M3,12a9,9 0 1 0 18,0a9,9 0 1 0 -18,0M12 11v6M12 7.5v.01")
}

// ------------------------------------------------------------------ type (tokens.css --fs-*)

object GType {
    private val base = TextStyle(fontFamily = Figtree)
    val title = base.copy(fontSize = 17.sp, lineHeight = 24.sp, fontWeight = FontWeight.SemiBold)
    val body = base.copy(fontSize = 15.sp, lineHeight = 20.sp)
    val bodyStrong = body.copy(fontWeight = FontWeight.SemiBold)
    val sub = base.copy(fontSize = 13.sp, lineHeight = 18.sp)
    val note = base.copy(fontSize = 13.sp, lineHeight = 19.sp)
    val meta = base.copy(fontSize = 11.sp, lineHeight = 14.sp, fontFeatureSettings = "tnum")
    /** Section heads: 12/16, 650, +0.06em, uppercase. */
    val sec = base.copy(fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.06.em)
    val hero = base.copy(fontSize = 22.sp, lineHeight = 28.sp, fontWeight = FontWeight.SemiBold)
}

// ------------------------------------------------------------------ press, buttons, pills

/** `.press`: scale to .97 in 100 ms on press, back in 140 ms (ease-out); none under reduced motion. */
@Composable
private fun Modifier.groupPress(source: MutableInteractionSource): Modifier {
    val pressed by source.collectIsPressedAsState()
    if (reducedMotion()) return this
    val s by animateFloatAsState(if (pressed) .97f else 1f, tween(if (pressed) GMotion.PRESS else GMotion.RELEASE, easing = GMotion.EaseOut), label = "press")
    return graphicsLayer { scaleX = s; scaleY = s }
}

/** A pressable area with the playground's hover/press wash (surface-2) and the .97 press scale. */
@Composable
fun Modifier.gTap(enabled: Boolean = true, role: Role = Role.Button, label: String? = null, onClick: () -> Unit): Modifier {
    val src = remember { MutableInteractionSource() }
    return this.groupPress(src).clickable(src, ripple(color = Tin.c.ink), enabled = enabled, role = role, onClickLabel = label, onClick = onClick)
}

enum class GBtnKind { Primary, Outline, Quiet, Danger, DangerQuiet }

/** Btn.svelte: 44 dp, 18 dp side padding, radius 22, 15 sp 600. Disabled at 45 %. */
@Composable
fun GBtn(text: String, onClick: () -> Unit, kind: GBtnKind, modifier: Modifier = Modifier, enabled: Boolean = true,
         desc: String? = null, fill: Boolean = false, content: (@Composable RowScope.() -> Unit)? = null) {
    val c = Tin.c
    val (bg, fg) = when (kind) {
        GBtnKind.Primary -> c.accentFill to c.onAccent
        GBtnKind.Outline -> c.sf to c.ink
        GBtnKind.Quiet -> Color.Transparent to c.ink
        GBtnKind.Danger -> c.dangerBtn to c.onDangerBtn
        GBtnKind.DangerQuiet -> Color.Transparent to c.er
    }
    val shape = RoundedCornerShape(22.dp)
    val src = remember { MutableInteractionSource() }
    Row(
        modifier.let { if (fill) it.fillMaxWidth() else it }.heightIn(min = 44.dp).alpha(if (enabled) 1f else .45f).groupPress(src).clip(shape).background(bg)
            .let { if (kind == GBtnKind.Outline) it.border(1.dp, c.ln, shape) else it }
            .clickable(src, ripple(color = fg), enabled = enabled, role = Role.Button, onClick = onClick)
            .let { if (desc != null) it.semantics { contentDescription = desc } else it }
            .padding(horizontal = 18.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically,
    ) {
        if (content != null) content() else Text(text, style = GType.bodyStrong, color = fg, maxLines = 1)
    }
}

enum class GTone { Owner, Admin, Host, Call, Warn, Muted }

/** Pill.svelte: 20 dp, radius 10, 8 dp sides, 11 sp 650. */
@Composable
fun GPill(text: String, tone: GTone) {
    val c = Tin.c
    val (bg, fg) = when (tone) {
        GTone.Owner -> c.accentSoft to c.accentInk
        GTone.Admin -> c.lav to c.lavInk
        GTone.Host -> c.warn to c.onWarn
        GTone.Call -> c.sf2 to c.ink
        GTone.Warn -> c.erc to c.er
        GTone.Muted -> c.sf2 to c.ink2
    }
    val shape = RoundedCornerShape(10.dp)
    Box(Modifier.height(20.dp).clip(shape).background(bg).let { if (tone == GTone.Call) it.border(1.dp, c.ln, shape) else it }.padding(horizontal = 8.dp),
        contentAlignment = Alignment.Center) {
        Text(text, style = TextStyle(fontFamily = Figtree, fontSize = 11.sp, lineHeight = 11.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.01.em), color = fg, maxLines = 1)
    }
}

/** A member's host / call-host mark (`.mk`): a 22 dp disc with a 14 dp glyph. */
@Composable
fun GMark(icon: ImageVector, desc: String, call: Boolean = false) {
    val c = Tin.c
    Box(Modifier.size(22.dp).clip(CircleShape).background(if (call) c.sf2 else c.warn).let { if (call) it.border(1.dp, c.ln, CircleShape) else it }
        .semantics { contentDescription = desc }, contentAlignment = Alignment.Center) {
        Icon(icon, null, tint = if (call) c.ink else c.onWarn, modifier = Modifier.size(14.dp))
    }
}

/** t-badge: 18 dp, radius 9, 11 sp 700 tabular; muted is an outline. Space is not reserved: callers keep it at the row's end. */
@Composable
fun GBadge(count: Int, muted: Boolean = false) {
    if (count <= 0) return
    val c = Tin.c
    val shape = RoundedCornerShape(9.dp)
    Box(
        Modifier.heightIn(min = 18.dp).widthIn(min = 18.dp).clip(shape).let { if (muted) it.border(1.dp, c.ln, shape) else it.background(c.accentFill) }
            .padding(horizontal = 5.dp).semantics { contentDescription = "$count unread" },
        contentAlignment = Alignment.Center,
    ) {
        Text(if (count > 99) "99+" else "$count", style = TextStyle(fontFamily = Figtree, fontSize = 11.sp, lineHeight = 11.sp,
            fontWeight = if (muted) FontWeight.SemiBold else FontWeight.Bold, fontFeatureSettings = "tnum"), color = if (muted) c.ink2 else c.onAccent)
    }
}

// ------------------------------------------------------------------ sections, cards

/** Section head (`.sec`): 12/16 uppercase muted, padding 18 16 6; an optional "· n" in tabular digits at 80 %. */
@Composable
fun GSec(text: String, count: Int? = null, modifier: Modifier = Modifier, countColor: Color? = null, countText: String? = null,
         padding: PaddingValues = PaddingValues(start = 16.dp, end = 16.dp, top = 18.dp, bottom = 6.dp), trailing: (@Composable () -> Unit)? = null) {
    val c = Tin.c
    Row(modifier.fillMaxWidth().padding(padding).semantics(mergeDescendants = true) { heading() }, verticalAlignment = Alignment.CenterVertically) {
        Text(text.uppercase(), style = GType.sec, color = c.ink2, maxLines = 1)
        val n = countText ?: count?.toString()
        if (n != null) {
            Text("·", Modifier.padding(start = 6.dp).alpha(.8f), style = GType.sec, color = c.ink2)
            Text(n, Modifier.padding(start = 6.dp).alpha(if (countColor == null) .8f else 1f), style = GType.sec.copy(fontFeatureSettings = "tnum"), color = countColor ?: c.ink2)
        }
        Spacer(Modifier.weight(1f))
        trailing?.invoke()
    }
}

/** `.sub2`: a quiet sub-head inside a section (no caps). */
@Composable
fun GSub2(text: String, modifier: Modifier = Modifier) {
    Text(text, modifier.padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 2.dp), style = GType.sec.copy(letterSpacing = 0.sp), color = Tin.c.ink2)
}

/** Flat card: surface, 1 px hairline, smooth 14 corners. */
@Composable
fun GCard(modifier: Modifier = Modifier, bg: Color = Tin.c.sf, content: @Composable ColumnScope.() -> Unit) {
    val shape = SmoothCornerShape(14.dp)
    Column(modifier.clip(shape).background(bg).border(1.dp, Tin.c.hairline, shape), content = content)
}

/** A thin separator between rows of a card. */
@Composable
fun GRule() = Box(Modifier.fillMaxWidth().height(1.dp).background(Tin.c.hairline))

/** Muted note under a section (13/19). */
@Composable
fun GNote(text: String, modifier: Modifier = Modifier) = Text(text, modifier, style = GType.note, color = Tin.c.ink2)

/**
 * Several texts sharing one cell: the tallest sets the height and only [current] is visible, so a status changing never
 * moves what is below (the playground's `.stk`). The swap cross-fades in 120 ms.
 */
@Composable
fun GStack(lines: List<String>, current: Int, style: TextStyle, color: Color, modifier: Modifier = Modifier) {
    Box(modifier.semantics(mergeDescendants = true) { contentDescription = lines.getOrElse(current) { "" } }) {
        lines.forEachIndexed { i, line ->
            val a by animateFloatAsState(if (i == current) 1f else 0f, tween(GMotion.CROSSFADE, easing = GMotion.EaseOut), label = "stk")
            Text(line, Modifier.alpha(a).clearAndSetSemantics {}, style = style, color = color)
        }
    }
}

/** A status dot (8 dp): accent when on, muted when off. */
@Composable
fun GDot(on: Boolean, size: Dp = 8.dp) {
    val c = Tin.c
    val col by androidx.compose.animation.animateColorAsState(if (on) c.accentFill else c.ink2, tween(GMotion.ENTER, easing = GMotion.EaseOut), label = "dot")
    Box(Modifier.size(size).clip(CircleShape).background(col))
}

// ------------------------------------------------------------------ fold / collapse

/** Collapse.svelte: height and opacity, open in 200 ms ease-out, closed in 140 ms ease-in; instant under reduced motion. */
@Composable
fun GCollapse(open: Boolean, content: @Composable ColumnScope.() -> Unit) {
    val reduced = reducedMotion()
    AnimatedVisibility(
        open,
        enter = if (reduced) fadeIn(tween(0)) else expandVertically(tween(GMotion.ENTER, easing = GMotion.EaseOut), expandFrom = Alignment.Top) + fadeIn(tween(GMotion.ENTER, easing = GMotion.EaseOut)),
        exit = if (reduced) fadeOut(tween(0)) else shrinkVertically(tween(GMotion.EXIT, easing = GMotion.EaseIn), shrinkTowards = Alignment.Top) + fadeOut(tween(GMotion.EXIT, easing = GMotion.EaseIn)),
    ) { Column(content = content) }
}

/** fold(): a list item or section arriving or leaving folds its height with opacity (cubic out in, cubic in out). */
@Composable
fun GFold(visible: Boolean, content: @Composable ColumnScope.() -> Unit) {
    val reduced = reducedMotion()
    val state = remember { MutableTransitionState(visible) }
    state.targetState = visible
    AnimatedVisibility(
        state,
        enter = if (reduced) fadeIn(tween(0)) else expandVertically(tween(GMotion.ENTER, easing = GMotion.CubicOut), expandFrom = Alignment.Top) + fadeIn(tween(GMotion.ENTER, easing = GMotion.CubicOut)),
        exit = if (reduced) fadeOut(tween(0)) else shrinkVertically(tween(GMotion.EXIT, easing = GMotion.CubicIn), shrinkTowards = Alignment.Top) + fadeOut(tween(GMotion.EXIT, easing = GMotion.CubicIn)),
    ) { Column(content = content) }
}

// ------------------------------------------------------------------ the Android sheet (chat/Layer 'dialog' on Android)

/**
 * Bottom sheet as the playground draws it: scrim rgb(10 16 14 / .5), surface-raised, radius 24 smooth on top, a 28 x 4 notch,
 * the sheet shadow. In: rises 24 dp while fading .4 → 1 (200 ms ease-out); out: sinks 24 dp and fades (140 ms ease-in).
 * Back and a tap on the scrim close it. [content] gets a `close` that plays the exit before [onDismiss].
 */
@Composable
fun GSheet(onDismiss: () -> Unit, label: String, content: @Composable ColumnScope.(close: () -> Unit) -> Unit) {
    val c = Tin.c
    val reduced = reducedMotion()
    val t = remember { Animatable(0f) }
    val scope = rememberCoroutineScope()
    var closing by remember { mutableStateOf(false) }
    val close: () -> Unit = {
        if (!closing) {
            closing = true
            scope.launch {
                if (!reduced) t.animateTo(0f, tween(GMotion.EXIT, easing = GMotion.EaseIn)) else t.snapTo(0f)
                onDismiss()
            }
        }
    }
    LaunchedEffect(Unit) { if (reduced) t.snapTo(1f) else t.animateTo(1f, tween(GMotion.ENTER, easing = GMotion.EaseOut)) }
    Dialog(onDismissRequest = close, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        // Our own scrim: no platform dim, no window animation.
        val view = LocalView.current
        SideEffect {
            (view.parent as? DialogWindowProvider)?.window?.let { w ->
                w.setDimAmount(0f); w.setWindowAnimations(0)
                w.setLayout(WindowManager.LayoutParams.MATCH_PARENT, WindowManager.LayoutParams.MATCH_PARENT)
            }
        }
        BackHandler(onBack = close)
        val shape = SmoothCornerShape(topStart = 24.dp, topEnd = 24.dp)
        val rise = with(LocalDensity.current) { 24.dp.toPx() }
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.BottomCenter) {
            Box(Modifier.fillMaxSize().graphicsLayer { alpha = t.value }.background(Color(0x800A100E))
                .clickable(remember { MutableInteractionSource() }, indication = null, onClick = close))
            Column(
                Modifier.widthIn(max = 520.dp).fillMaxWidth().statusBarsPadding().padding(top = 32.dp)
                    .graphicsLayer {
                        translationY = (1f - t.value) * rise
                        alpha = if (closing) t.value else .4f + .6f * t.value
                    }
                    .depth(Depth.Sheet, shape).clip(shape).background(c.sfRaised)
                    .clickable(remember { MutableInteractionSource() }, indication = null, onClick = {})
                    .semantics { contentDescription = label }
                    .navigationBarsPadding().imePadding().verticalScroll(rememberScrollState()),
            ) {
                Box(Modifier.padding(top = 8.dp, bottom = 2.dp).size(28.dp, 4.dp).clip(RoundedCornerShape(2.dp)).background(c.ln).align(Alignment.CenterHorizontally))
                content(close)
            }
        }
    }
}

/** The sheet's dialog body (`.dlg`): padding 20 20 16, a 17/24 title. */
@Composable
fun GDialogBody(title: String, modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)) {
        Text(title, Modifier.semantics { heading() }, style = GType.title, color = Tin.c.ink)
        content()
    }
}

/** A muted paragraph in a sheet (13/19, 8 dp above). */
@Composable
fun GPara(text: String, top: Dp = 8.dp, color: Color = Tin.c.ink2, style: TextStyle = GType.note) =
    Text(text, Modifier.padding(top = top), style = style, color = color)

/** End-aligned actions row (`.acts`), 8 dp apart, 18 dp above. */
@Composable
fun GActs(modifier: Modifier = Modifier, top: Dp = 18.dp, content: @Composable RowScope.() -> Unit) =
    Row(modifier.fillMaxWidth().padding(top = top), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End), verticalAlignment = Alignment.CenterVertically, content = content)

// ------------------------------------------------------------------ ongoing group call (groups/OngoingCall.svelte)

/** The list row's call line: three calm bars, then the text in accent-ink 600 (`.callpv` + `.eq`). Static under reduced motion. */
@Composable
fun GCallLine(text: String) {
    val c = Tin.c
    val reduced = reducedMotion()
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
        Row(Modifier.height(12.dp), verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
            val inf = rememberInfiniteTransition(label = "eq")
            listOf(0, 500, 1000).forEach { delay ->
                val h = if (reduced) 8f else inf.animateFloat(4f, 12f, infiniteRepeatable(tween(800, delayMillis = 0, easing = CubicBezierEasing(.42f, 0f, .58f, 1f)), RepeatMode.Reverse,
                    initialStartOffset = androidx.compose.animation.core.StartOffset(delay, androidx.compose.animation.core.StartOffsetType.FastForward)), label = "b").value
                Box(Modifier.width(2.5.dp).height(h.dp).clip(RoundedCornerShape(1.dp)).background(c.accentInk))
            }
        }
        Text(text, style = GType.sub.copy(fontWeight = FontWeight.SemiBold, fontFeatureSettings = "tnum"), color = c.accentInk, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** The list row's "Join" chip (`.join`): decorative, the row itself opens the group. */
@Composable
fun GJoinChip() {
    val c = Tin.c
    Box(Modifier.clip(RoundedCornerShape(12.dp)).background(c.accentFill).padding(horizontal = 11.dp, vertical = 2.dp)) {
        Text("Join", style = TextStyle(fontFamily = Figtree, fontSize = 11.sp, lineHeight = 16.sp, fontWeight = FontWeight.Bold), color = c.onAccent)
    }
}

/**
 * "Call in progress · n joined · Join" under a group's header. The wording and the button's width are fixed (72 dp minimum), so
 * someone joining only changes a number. [joined] is null while the core doesn't report a count: the line then reads only
 * "Call in progress".
 */
@Composable
fun GOngoingCall(inCall: Boolean, joined: Int?, sub: String? = null, onJoin: () -> Unit, onOpen: () -> Unit) {
    val c = Tin.c
    val bg = if (c.dark) lerp(c.sf, c.accentFill, .24f) else c.accentSoft
    val fg = if (c.dark) c.ink else c.accentInk
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).background(bg).padding(start = 14.dp, end = 12.dp, top = 8.dp, bottom = 8.dp)
            .semantics(mergeDescendants = true) { contentDescription = "A call is in progress" + (joined?.let { ", $it joined" } ?: "") },
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(if (inCall) "You’re in the call" else "Call in progress", style = GType.body.copy(fontWeight = FontWeight.SemiBold), color = fg, maxLines = 1, overflow = TextOverflow.Ellipsis)
            val line = sub ?: joined?.let { "$it joined" }
            if (line != null) Text(line, Modifier.alpha(.85f), style = GType.sub.copy(lineHeight = 17.sp, fontFeatureSettings = "tnum"), color = fg)
        }
        val src = remember { MutableInteractionSource() }
        val shape = RoundedCornerShape(18.dp)
        Box(
            Modifier.widthIn(min = 72.dp).heightIn(min = 44.dp).groupPress(src).clip(shape)
                .let { if (inCall) it.border(1.5.dp, fg, shape) else it.background(c.accentFill) }
                .clickable(src, ripple(), role = Role.Button, onClick = if (inCall) onOpen else onJoin).padding(horizontal = 18.dp),
            contentAlignment = Alignment.Center,
        ) { Text(if (inCall) "Open" else "Join", style = GType.sub.copy(fontWeight = FontWeight.Bold), color = if (inCall) fg else c.onAccent) }
    }
    Box(Modifier.fillMaxWidth().height(1.dp).background(c.hairline))
}
