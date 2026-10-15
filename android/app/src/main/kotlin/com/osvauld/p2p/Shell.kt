package com.osvauld.p2p

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.Crossfade
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/*
 * The Android shell from the playground (screens/shell/AndroidShell.svelte): the 56 dp app bar with the live mark,
 * the tab panel that fades through, the search dock above the tab bar, and the 4-tab bar whose indicator grows out
 * from the icon. Values are measured from the playground (getComputedStyle) and named after tokens.css / motion.ts.
 */

// ---------------------------------------------------------------- motion.ts

object Motion {
    /** --ease-out cubic-bezier(.2,.8,.2,1). */
    val EaseOut = CubicBezierEasing(0.2f, 0.8f, 0.2f, 1f)
    /** svelte cubicOut / cubicIn (the fade-through halves). */
    val CubicOut = CubicBezierEasing(0.215f, 0.61f, 0.355f, 1f)
    val CubicIn = CubicBezierEasing(0.55f, 0.055f, 0.675f, 0.19f)
    const val PRESS = 100
    const val RELEASE = 140
    const val SWITCH = 120
    const val CROSSFADE = 120
    const val TAB_OUT = 90
    const val TAB_IN = 210
    const val INDICATOR = 200
}

/** `.press`: scale .97 while pressed (100 ms in, 140 ms back), plus the hover wash as the pressed fill. */
@Composable
fun Modifier.pressable(
    onClick: () -> Unit, shape: Shape = RoundedCornerShape(14.dp), wash: Color = Tin.c.sf2, role: Role = Role.Button,
    enabled: Boolean = true, rest: Color = Color.Transparent,
): Modifier {
    val src = remember { MutableInteractionSource() }
    val down by src.collectIsPressedAsState()
    val s by animateFloatAsState(if (down && enabled) 0.97f else 1f, tween(if (down) Motion.PRESS else Motion.RELEASE, easing = Motion.EaseOut), label = "press")
    val bg by animateColorAsState(if (down && enabled) wash else rest, tween(Motion.SWITCH, easing = Motion.EaseOut), label = "wash")
    return this.graphicsLayer { scaleX = s; scaleY = s }.clip(shape).background(bg)
        .clickable(interactionSource = src, indication = null, enabled = enabled, role = role, onClick = onClick)
}

// ---------------------------------------------------------------- hooks other areas call

/**
 * A call ringing while the app is open takes over the home header's brand area (lib/calls/RingHead.svelte).
 * The CALLS area sets [ring] while an incoming call is shown in the header instead of full screen, and clears it after.
 */
object RingHead {
    data class Ring(
        val did: String, val name: String,
        /** "Incoming call", "Incoming video call", or "Calling during your call". */
        val sub: String = "Incoming call",
        /** Call waiting: the buttons are Decline and End & answer. */
        val waiting: Boolean = false,
        val onOpen: () -> Unit = {}, val onDecline: () -> Unit = {}, val onAnswer: () -> Unit = {},
    )
    val ring = MutableStateFlow<Ring?>(null)
}

/** Composer drafts per conversation, shown in the Chats list ("Draft: ..."). The CHAT area writes it from the composer. */

/** Home tab scroll positions, kept for the process: coming back from a chat lands where you were. */
object HomeScroll {
    val states: Map<HomeTab, LazyListState> = HomeTab.entries.associateWith { LazyListState() }
}

// ---------------------------------------------------------------- app bar

/** `.appbar`: min 56, padding 4 8 0 12; brand = live mark 28 + "Tinline" 19 bold, gap 8; 44 dp icon buttons. A ringing call replaces it in place. */
@Composable
fun HomeAppBar(look: MarkLook, showSearch: Boolean, onSearch: () -> Unit, onSettings: () -> Unit) {
    val c = Tin.c
    val ring by RingHead.ring.collectAsState()
    // While this bar is on screen it carries a ringing call, so CallController keeps the system heads-up away. Leaving Home
    // mid-ring posts the notification, so the call is never left without a way to answer it.
    val ctx = androidx.compose.ui.platform.LocalContext.current
    DisposableEffect(Unit) {
        AppVisible.ringHeadUp = true
        onDispose { AppVisible.ringHeadUp = false; P2pApp.get(ctx).calls.appLeftFront() }
    }
    Box(Modifier.fillMaxWidth().heightIn(min = 56.dp)) {
        val a by animateFloatAsState(if (ring == null) 1f else 0f, tween(Motion.CROSSFADE), label = "brand")
        Row(
            Modifier.matchParentSize().padding(start = 12.dp, end = 8.dp, top = 4.dp).alpha(a),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TinMark(28.dp, live = true, state = look.state, sway = look.sway, dim = look.dim, description = look.label)
                Text("Tinline", style = TinType.titleL.copy(fontSize = 19.sp, lineHeight = 21.sp), color = c.ink)
            }
            if (showSearch) PgIconButton(PgIcons.Search, "Search", onSearch, enabled = ring == null)
            PgIconButton(PgIcons.Gear, "Settings", onSettings, enabled = ring == null)
        }
        // The last ring stays drawn while it fades out.
        var last by remember { mutableStateOf(ring) }
        LaunchedEffect(ring) { if (ring != null) last = ring }
        val r = last
        if (r != null && a < 1f) RingHeadRow(r, Modifier.matchParentSize().alpha(1f - a).padding(start = 12.dp, end = 8.dp, top = 4.dp), avatar = 28.dp)
    }
}

/** The caller in the header box: avatar, name over "Incoming call", Decline and Answer in the 44 dp icon slots. */
@Composable
fun RingHeadRow(r: RingHead.Ring, modifier: Modifier, avatar: Dp) {
    val c = Tin.c
    Row(modifier.background(c.bg).semantics(mergeDescendants = false) { contentDescription = if (r.waiting) "${r.name} is calling during your call" else "${r.sub} from ${r.name}" },
        verticalAlignment = Alignment.CenterVertically) {
        Row(
            Modifier.weight(1f).fillMaxHeight().clip(RoundedCornerShape(14.dp)).clickable(role = Role.Button, onClickLabel = "Open call from ${r.name}", onClick = r.onOpen),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Avatar(r.name, r.did, avatar)
            Column(Modifier.weight(1f)) {
                Text(r.name, style = TinFs.body.copy(fontWeight = FontWeight(650), lineHeight = 19.sp), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(r.sub, style = TinFs.sub.copy(fontSize = 12.sp, lineHeight = 15.sp), color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        RingBtn(PgIcons.Phone, "Decline", c.dangerBtn, Color.White, hang = true, onClick = r.onDecline)
        RingBtn(PgIcons.Phone, if (r.waiting) "End and answer" else "Answer", c.accent, c.onAccent, hang = false, onClick = r.onAnswer)
    }
}

@Composable
private fun RingBtn(icon: ImageVector, label: String, bg: Color, fg: Color, hang: Boolean, onClick: () -> Unit) {
    Box(Modifier.size(44.dp).pressable(onClick, CircleShape, wash = Color.Transparent).semantics { contentDescription = label }, contentAlignment = Alignment.Center) {
        Box(Modifier.size(38.dp).clip(CircleShape).background(bg), contentAlignment = Alignment.Center) {
            Icon(icon, null, tint = fg, modifier = Modifier.size(22.dp).then(if (hang) Modifier.rotate(135f) else Modifier))
        }
    }
}

/** lib/ui/IconButton: 44 dp circle, 22 dp icon, hover wash. */
@Composable
fun PgIconButton(icon: ImageVector, label: String, onClick: () -> Unit, tint: Color = Tin.c.ink, enabled: Boolean = true, size: Dp = 44.dp, iconSize: Dp = 22.dp) {
    Box(Modifier.size(size).pressable(onClick, CircleShape, enabled = enabled).semantics { contentDescription = label }, contentAlignment = Alignment.Center) {
        Icon(icon, null, tint = tint, modifier = Modifier.size(iconSize).alpha(if (enabled) 1f else 0.45f))
    }
}

// ---------------------------------------------------------------- tab panel

/** TabPanel: switching tabs fades through (old out 90 ms, then new in 210 ms with a .98 -> 1 settle from the top); nothing reflows. */
@Composable
fun TabPanel(tab: HomeTab, modifier: Modifier = Modifier, content: @Composable (HomeTab) -> Unit) {
    AnimatedContent(
        tab, modifier,
        transitionSpec = {
            (fadeIn(tween(Motion.TAB_IN, Motion.TAB_OUT, Motion.CubicOut)) +
                scaleIn(tween(Motion.TAB_IN, Motion.TAB_OUT, Motion.CubicOut), initialScale = 0.98f, transformOrigin = TransformOrigin(0.5f, 0f)))
                .togetherWith(fadeOut(tween(Motion.TAB_OUT, easing = Motion.CubicIn)))
                .using(SizeTransform(clip = false))
        },
        contentAlignment = Alignment.TopCenter, label = "tab",
    ) { t -> content(t) }
}

/** `.title`: 28/34 bold, margin 4 16 8. */
@Composable
fun TabTitle(text: String) {
    Text(text, Modifier.padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 8.dp), style = TinType.h1.copy(letterSpacing = 0.sp), color = Tin.c.ink)
}

// ---------------------------------------------------------------- search dock

/** SearchDock: the search pill + the new button in one row, above the tab bar (thumb reach). Pill 48 (radius 24), button 48. */
@Composable
fun SearchDock(addLabel: String, onSearch: () -> Unit, onAdd: () -> Unit) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().background(c.bg).padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(
            Modifier.weight(1f).height(48.dp).pressable(onSearch, RoundedCornerShape(24.dp), wash = c.hairline, rest = c.sf2)
                .semantics(mergeDescendants = true) { contentDescription = "Search" }.padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Icon(PgIcons.Search, null, tint = c.ink2, modifier = Modifier.size(20.dp))
            Text("Search", style = TinFs.body, color = c.ink2)
        }
        Box(
            Modifier.size(48.dp).pressable(onAdd, CircleShape, wash = c.accentSoft, rest = c.accentSoft).semantics { contentDescription = addLabel },
            contentAlignment = Alignment.Center,
        ) { Icon(PgIcons.Plus, null, tint = c.accentInk, modifier = Modifier.size(24.dp)) }
    }
}

// ---------------------------------------------------------------- tab bar

/** `.bar`: 4 equal tabs on the surface, a hairline above. Tab: 56 x 30 pill (radius 15) over a 12 sp 600 label. The active
 * indicator grows out from the icon's centre (scaleX .3 -> 1 with opacity, 200 ms), never a hard swap. */
@Composable
fun TinTabBar(tab: HomeTab, onTab: (HomeTab) -> Unit, chatBadge: Int, showGroups: Boolean = true, groupBadge: Int = 0) {
    val c = Tin.c
    Column(Modifier.fillMaxWidth().background(c.sf)) {
        Box(Modifier.fillMaxWidth().height(1.dp).background(c.hairline))
        // The pill sits 4 dp lower inside each tab (top 2 + 5 instead of 6 + 1), so the badge at -4 stays inside the tab:
        // the tab's press clip used to cut its top off.
        Row(Modifier.fillMaxWidth().navigationBarsPadding().padding(top = 2.dp, bottom = 10.dp)) {
            listOf(HomeTab.Chats to PgIcons.Chat, HomeTab.Groups to PgIcons.Users, HomeTab.Calls to PgIcons.Phone, HomeTab.Contacts to PgIcons.Contact)
                .filter { showGroups || it.first != HomeTab.Groups }
                .forEach { (t, icon) ->
                    val on = t == tab
                    val p by animateFloatAsState(if (on) 1f else 0f, tween(Motion.INDICATOR, easing = Motion.EaseOut), label = "ind")
                    val fg by animateColorAsState(if (on) c.accentInk else c.ink2, tween(Motion.INDICATOR, easing = Motion.EaseOut), label = "tabfg")
                    val badge = when (t) { HomeTab.Chats -> chatBadge; HomeTab.Groups -> groupBadge; else -> 0 }
                    Column(
                        Modifier.weight(1f).heightIn(min = 56.dp).pressable({ onTab(t) }, RoundedCornerShape(0.dp), wash = Color.Transparent, role = Role.Tab)
                            .semantics(mergeDescendants = true) { selected = on; if (badge > 0) stateDescription = "$badge unread" },
                        horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(2.dp),
                    ) {
                        Box(Modifier.padding(top = 5.dp).size(56.dp, 30.dp), contentAlignment = Alignment.Center) {
                            Box(Modifier.matchParentSize().graphicsLayer { scaleX = 0.3f + 0.7f * p; alpha = p }.clip(RoundedCornerShape(15.dp)).background(c.accentSoft))
                            Icon(icon, null, tint = fg, modifier = Modifier.size(22.dp))
                            if (badge > 0) CountBadge(badge, Modifier.align(Alignment.TopEnd).offset(x = (-4).dp, y = (-4).dp))
                        }
                        Text(t.title, style = TinFs.sub.copy(fontSize = 12.sp, lineHeight = 21.sp, fontWeight = FontWeight.SemiBold), color = fg)
                    }
                }
        }
    }
}

val HomeTab.title: String get() = when (this) { HomeTab.Chats -> "Chats"; HomeTab.Groups -> "Groups"; HomeTab.Calls -> "Calls"; HomeTab.Contacts -> "Contacts" }

/** `.t-badge`: 18 tall, min 18 wide, padding 0 5, radius 9, --accent; muted = a hairline ring in --muted. */
@Composable
fun CountBadge(n: Int, modifier: Modifier = Modifier, muted: Boolean = false) {
    val c = Tin.c
    val m = modifier.defaultMinSize(18.dp, 18.dp).height(18.dp).clip(RoundedCornerShape(9.dp))
        .then(if (muted) Modifier.border(1.dp, c.ln, RoundedCornerShape(9.dp)) else Modifier.background(c.accent))
        .padding(horizontal = 5.dp)
    Box(m, contentAlignment = Alignment.Center) {
        // Centred on the pill's middle, without the font's extra top padding (it sat low and looked off centre).
        Text(if (n > 99) "99+" else "$n", style = TinFs.meta.copy(fontWeight = if (muted) FontWeight.SemiBold else FontWeight.Bold, lineHeight = 11.sp,
                platformStyle = androidx.compose.ui.text.PlatformTextStyle(includeFontPadding = false),
                lineHeightStyle = androidx.compose.ui.text.style.LineHeightStyle(androidx.compose.ui.text.style.LineHeightStyle.Alignment.Center, androidx.compose.ui.text.style.LineHeightStyle.Trim.Both)),
            color = if (muted) c.ink2 else c.onAccent)
    }
}

/** `.t-chip`: 32 tall, radius 16, 1 px --line, 13 600; pressed = --accent-soft with --accent-ink. */
@Composable
fun FilterChip(label: String, on: Boolean, onClick: () -> Unit) {
    val c = Tin.c
    val bg by animateColorAsState(if (on) c.accentSoft else Color.Transparent, tween(Motion.SWITCH, easing = Motion.EaseOut), label = "chip")
    val br by animateColorAsState(if (on) Color.Transparent else c.ln, tween(Motion.SWITCH, easing = Motion.EaseOut), label = "chipb")
    Box(
        Modifier.heightIn(min = 44.dp).pressable(onClick, RoundedCornerShape(16.dp), wash = bg, rest = bg).border(1.dp, br, RoundedCornerShape(16.dp))
            .semantics { selected = on }.padding(horizontal = 14.dp),
        contentAlignment = Alignment.Center,
    ) { Text(label, style = TinFs.sub.copy(fontWeight = FontWeight.SemiBold), color = if (on) c.accentInk else c.ink) }
}

/** A group's avatar in lists: --surface-2 circle with the users glyph in --accent-ink (lib/ui/Avatar group). */
@Composable
fun ListGroupAvatar(size: Dp, alpha: Float = 1f, gid: String? = null) {
    val c = Tin.c
    val art = androidx.compose.runtime.remember(gid, size) { FingerprintCache.ofGroup(gid, size.value) }
    if (art != null) { GroupMandala(art, size, Modifier.alpha(alpha)); return }
    Box(Modifier.size(size).alpha(alpha).clip(CircleShape).background(c.sf2), contentAlignment = Alignment.Center) {
        Icon(PgIcons.Users, null, tint = c.accentInk, modifier = Modifier.size(size * 0.46f))
    }
}

/** Section headers in lists (`.sec`, `h3`): 12, 650, uppercase, .06em, --muted. */
@Composable
fun ListSection(text: String, modifier: Modifier = Modifier) {
    Text(text.uppercase(), modifier, style = TinFs.section, color = Tin.c.ink2)
}

/** Crossfaded text in one cell, so a changing line never moves its neighbours. */
@Composable
fun FadeText(text: String, content: @Composable (String) -> Unit) {
    Crossfade(text, animationSpec = tween(Motion.CROSSFADE), label = "fadeText") { content(it) }
}
