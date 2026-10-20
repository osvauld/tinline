package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import kotlin.math.min
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.withStyle

/** CallScreen.svelte's copy: never claim encryption while dialling or ringing. */
internal fun callStatus(active: Boolean, ringing: Boolean, reconnecting: Boolean, secs: Long): String = when {
    active && reconnecting -> "Reconnecting…"
    active -> uniffi.present.clock(secs)
    ringing -> "Ringing…"
    else -> "Calling…"
}

internal fun callPrivacy(active: Boolean, name: String): String = if (active)
    "End-to-end encrypted. Your voice is encrypted on this device and only decrypted on $name’s."
else "Connecting to $name. Once the call is connected, it is end-to-end encrypted."

/** Fixed, mirrored 44 dp action cells with the 28 dp live mark independently centered. The end cell is the
 *  signal (SignalBars, where the "i" was): tapping it opens About this call. */
@Composable
internal fun CallTop(active: Boolean, ringing: Boolean, onMinimise: (() -> Unit)?, onInfo: (() -> Unit)?,
    reconnecting: Boolean = false, incoming: Boolean = false, signal: Int = 0, direct: Boolean? = null,
    /** Message (decided 2026-10-14, the in-call lab): leave the call screen for their chat; the call continues. */
    onMessage: (() -> Unit)? = null) {
    val phase = when {
        reconnecting -> CallPhase.Reconnecting
        active -> CallPhase.Active
        incoming -> CallPhase.Incoming
        else -> CallPhase.Outgoing
    }
    val look = statusLook(call = phase, conn = ConnState.Online)
    Box(Modifier.fillMaxWidth().height(48.dp)) {
        if (onMinimise != null) Box(Modifier.align(Alignment.CenterStart)) {
            // ← back, like every other screen: to where I was; the call goes on in the strip.
            CallTopButton(PgIcons.Back, "Back. The call continues", onMinimise)
        }
        if (onMessage != null) Box(Modifier.align(Alignment.CenterEnd).padding(end = 44.dp)) {
            CallTopButton(PgIcons.Chat, "Message. The call continues", onMessage)
        }
        TinMark(28.dp, live = true, state = look.state, sway = look.sway, description = look.label,
            modifier = Modifier.align(Alignment.Center))
        if (onInfo != null) Box(Modifier.align(Alignment.CenterEnd)) {
            val up = active && !reconnecting && signal > 0
            val label = when {
                active && reconnecting -> "About this call. Reconnecting."
                up -> "About this call. ${signalWord(signal)} signal, ${if (direct == true) "direct" else "relayed"}."
                else -> "About this call"
            }
            Box(Modifier.size(44.dp).clip(CircleShape).clickable(role = Role.Button, onClick = onInfo)
                .semantics { contentDescription = label }, contentAlignment = Alignment.Center) {
                SignalBars(if (up) signal else 0, Tin.c.ink, wave = active && reconnecting)
            }
        }
    }
}

@Composable
private fun CallTopButton(icon: ImageVector, label: String, onClick: () -> Unit) {
    Box(Modifier.size(44.dp).clip(CircleShape).clickable(role = Role.Button, onClick = onClick)
        .semantics { contentDescription = label }, contentAlignment = Alignment.Center) {
        Icon(icon, null, tint = Tin.c.ink, modifier = Modifier.size(24.dp))
    }
}

/** Five stable slots. End stays on the axis; Flip never vanishes when the camera turns off. */
@Composable
internal fun CallControls(muted: Boolean, speaker: Boolean, camera: Boolean, onMute: () -> Unit,
    onSpeaker: () -> Unit, onCamera: (() -> Unit)?, onFlip: (() -> Unit)?, onEnd: () -> Unit,
    /** With a headset connected the speaker slot becomes the route picker: its name and icon. */
    routeCaption: String = "Speaker", routeIcon: ImageVector = CallGlyphs.Speaker,
    /** I pressed End: its glyph plays the cut string letting go (HangupGlyph.kt). */
    ending: Boolean = false) {
    val c = Tin.c
    val fill = if (c.dark) lerp(c.sf, c.ink, .14f) else c.sf
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        CallControl("Mute", if (muted) "Muted" else "Mute", if (muted) CallGlyphs.MicOff else CallGlyphs.Mic,
            if (muted) c.ink else fill, if (muted) c.bg else c.ink, onMute, selected = muted)
        CallControl("Camera", "Camera", if (camera) CallGlyphs.Video else CallGlyphs.VideoOff,
            if (camera) c.ink else fill, if (camera) c.bg else c.ink, onCamera ?: {}, enabled = onCamera != null, selected = camera)
        CallControl("End call", "End", CallGlyphs.End, c.callButtonEnd, Color.White, onEnd,
            glyph = { HangupGlyph(ending, Color.White, Modifier.size(28.dp)) })
        CallControl("Flip camera", "Flip", CallGlyphs.Flip, fill, c.ink, onFlip ?: {}, enabled = camera && onFlip != null)
        CallControl(if (routeCaption == "Speaker") "Speaker" else "Audio: $routeCaption", routeCaption, routeIcon, if (speaker) c.ink else fill,
            if (speaker) c.bg else c.ink, onSpeaker, selected = speaker)
    }
}

@Composable
private fun RowScope.CallControl(label: String, caption: String, icon: ImageVector, bg: Color, fg: Color,
    onClick: () -> Unit, enabled: Boolean = true, selected: Boolean? = null, glyph: (@Composable () -> Unit)? = null) {
    Column(Modifier.weight(1f).alpha(if (enabled) 1f else .38f), horizontalAlignment = Alignment.CenterHorizontally) {
        BoxWithConstraints(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
            val diameter = maxWidth.coerceAtMost(56.dp).coerceAtLeast(44.dp)
            Box(Modifier.size(diameter).depth(Depth.Lift, CircleShape).clip(CircleShape).background(bg)
                .clickable(enabled = enabled, role = Role.Button, onClick = onClick).semantics {
                    contentDescription = label
                    if (selected != null) toggleableState = ToggleableState(selected)
                }, contentAlignment = Alignment.Center) {
                if (glyph != null) glyph() else Icon(icon, null, tint = fg, modifier = Modifier.size(26.dp))
            }
        }
        Text(caption, Modifier.padding(top = 8.dp).heightIn(min = 18.dp), style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp),
            color = Tin.c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/**
 * Voice focus geometry on the call's colour field (Fog, as on the playground and desktop): the field state is
 * mapped as call_screen.rs does (incoming, outgoing while dialling, connecting while ringing, active, waiting,
 * reconnecting), anchored at the avatar, with calm patches behind the name/status block and the controls in dark mode.
 */
@Composable
internal fun CallFocus(name: String, did: String, active: Boolean, ringing: Boolean, secs: Long = 0,
    reconnecting: Boolean = false, problem: String? = null, incoming: Boolean = false, video: Boolean = false,
    onMinimise: (() -> Unit)? = null, onInfo: (() -> Unit)? = null, signal: Int = 0, direct: Boolean? = null,
    onMessage: (() -> Unit)? = null, waiting: Boolean = false, muted: Boolean = false,
    waitingCard: (@Composable (Modifier) -> Unit)? = null, controls: @Composable () -> Unit) {
    val c = Tin.c
    val density = androidx.compose.ui.platform.LocalDensity.current.density
    var root by remember { mutableStateOf(androidx.compose.ui.geometry.Rect.Zero) }
    var avatar by remember { mutableStateOf(androidx.compose.ui.geometry.Rect.Zero) }
    var text by remember { mutableStateOf(androidx.compose.ui.geometry.Rect.Zero) }
    var ctrl by remember { mutableStateOf(androidx.compose.ui.geometry.Rect.Zero) }
    val fieldState = when {
        incoming -> CallFieldState.Incoming
        active && waiting -> CallFieldState.Waiting
        active && reconnecting -> CallFieldState.Reconnecting
        active -> CallFieldState.Active
        ringing -> CallFieldState.Connecting
        else -> CallFieldState.Outgoing
    }
    fun centre(r: androidx.compose.ui.geometry.Rect) = androidx.compose.ui.geometry.Offset((r.center.x - root.left) / density, (r.center.y - root.top) / density)
    val calm = if (text.isEmpty || ctrl.isEmpty) emptyList() else listOf(
        centre(text).let { FieldCalm(it.x, it.y, min(text.width / density, 420f) / 2f, text.height / density / 2f) },
        centre(ctrl).let { FieldCalm(it.x, it.y, min(ctrl.width / density, 340f) / 2f, ctrl.height / density / 2f) },
    )
    Box(Modifier.fillMaxSize().background(c.bg).onGloballyPositioned { root = it.boundsInRoot() }) {
        if (!root.isEmpty) CallFieldBackground(fieldState, muted, if (avatar.isEmpty) androidx.compose.ui.geometry.Offset(root.width / density / 2f, root.height / density * .3f) else centre(avatar), calm)
        Column(Modifier.fillMaxSize().systemBarsPadding().padding(start = 20.dp, end = 20.dp, top = 8.dp, bottom = 36.dp),
            horizontalAlignment = Alignment.CenterHorizontally) {
            CallTop(active, ringing, onMinimise, onInfo, reconnecting, incoming, signal, direct, onMessage)
            Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = 40.dp),
                horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
                Box(Modifier.onGloballyPositioned { avatar = it.boundsInRoot() }) { Avatar(name, did, 112.dp) }
                Column(Modifier.fillMaxWidth().onGloballyPositioned { text = it.boundsInRoot() }, horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(name, Modifier.padding(top = 22.dp).fillMaxWidth().heightIn(min = 34.dp),
                        style = TinType.h1.copy(fontSize = 28.sp, lineHeight = 34.sp, fontWeight = FontWeight.SemiBold),
                        color = c.ink, textAlign = TextAlign.Center, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Box(Modifier.padding(top = 6.dp).heightIn(min = 30.dp), contentAlignment = Alignment.Center) {
                        Text(if (incoming) (if (video) "Tinline video call" else "Tinline voice call") else callStatus(active, ringing, reconnecting, secs),
                            style = TinType.bodyL.copy(fontSize = if (active && !reconnecting) 20.sp else 17.sp, lineHeight = 30.sp,
                                fontWeight = FontWeight.Medium, fontFeatureSettings = "tnum"), color = c.ink, maxLines = 1)
                    }
                    // The path, on the main screen: honest, and About this call explains it.
                    Text(if (active && !reconnecting) "End-to-end encrypted" + (direct?.let { if (it) " · Direct" else " · Relayed" } ?: "") else " ", Modifier.padding(top = 10.dp).heightIn(min = 18.dp),
                        style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = c.ink, maxLines = 1)
                    // Keep a two-line error cell even when there is no microphone/video problem.
                    Text(problem ?: " ", Modifier.padding(top = 8.dp).fillMaxWidth().heightIn(min = 38.dp),
                        style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 19.sp), color = c.er, maxLines = 2,
                        overflow = TextOverflow.Ellipsis, textAlign = TextAlign.Center)
                }
            }
            Box(Modifier.onGloballyPositioned { ctrl = it.boundsInRoot() }) { controls() }
        }
        // WaitingCard: its bottom edge 22 dp above the controls, growing upward, so the avatar and name never move.
        if (waitingCard != null) {
            val above = if (ctrl.isEmpty || root.isEmpty) 150f else (root.bottom - ctrl.top) / density + 22f
            waitingCard(Modifier.align(Alignment.BottomCenter).padding(start = 20.dp, end = 20.dp, bottom = above.dp))
        }
    }
}

/**
 * CallScreen.svelte `.info`: a calm card over a 24 % scrim, 16 from the sides and 24 above the bottom, at most 420
 * wide. Encryption first, then the connection row (bars + "Strong signal · Direct" and one line, in a block that
 * reserves the title + four lines so the card never grows when the wording changes), then the numbers.
 */
@Composable
internal fun CallInfo(name: String, active: Boolean, onDismiss: () -> Unit, reconnecting: Boolean = false,
    signal: Int = 0, direct: Boolean? = null, rttMs: Int? = null, lossPct: Double? = null, video: Boolean = false,
    sendCodec: String = "", recvCodec: String = "",
    /** Developer builds: "Stats for nerds" (CallNerds.kt), shown in place of the codec rows. */
    nerds: (@Composable () -> Unit)? = null) {
    val c = Tin.c
    val first = name.trim().substringBefore(' ')
    // An overlay in the call screen's own window (not a dialog window), so the system bar insets apply.
    androidx.activity.compose.BackHandler(onBack = onDismiss)
    run {
        Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = .24f))
            .clickable(interactionSource = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }, indication = null, onClick = onDismiss)) {
            Column(Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(start = 16.dp, end = 16.dp, bottom = 24.dp)
                .widthIn(max = 420.dp).fillMaxWidth()
                .heightIn(max = (androidx.compose.ui.platform.LocalConfiguration.current.screenHeightDp * 0.85f).dp)
                .depth(Depth.Sheet, SmoothCornerShape(24.dp)).clip(SmoothCornerShape(24.dp))
                .background(c.sfRaised).verticalScroll(rememberScrollState()).clickable(interactionSource = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }, indication = null, onClick = {})
                .padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)
                .semantics { contentDescription = "About this call" }) {
                val p = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 19.sp)
                Text("About this call", style = TinType.titleM.copy(fontSize = 17.sp, lineHeight = 24.sp), color = c.ink)
                if (active) {
                    Text(androidx.compose.ui.text.buildAnnotatedString {
                        withStyle(androidx.compose.ui.text.SpanStyle(color = c.ink, fontWeight = FontWeight.SemiBold)) { append("End-to-end encrypted.") }
                        append(" ${if (video) "Your voice and picture are" else "Your voice is"} encrypted on this device and only decrypted on $name’s.")
                    }, Modifier.padding(top = 10.dp), style = p, color = c.ink2)
                    val up = !reconnecting && signal > 0
                    Row(Modifier.padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        SignalBars(if (up) signal else 0, c.ink, Modifier.padding(top = 1.dp), wave = reconnecting, size = 22.dp)
                        Text(androidx.compose.ui.text.buildAnnotatedString {
                            withStyle(androidx.compose.ui.text.SpanStyle(color = c.ink, fontWeight = FontWeight.SemiBold)) {
                                append(if (reconnecting) "Reconnecting" else "${signalWord(signal)} signal · ${if (direct == true) "Direct" else "Relayed"}")
                            }
                            append("\n")
                            append(when {
                                reconnecting -> "The connection dropped for a moment. Tinline is finding a way back to $first."
                                direct == true -> "Your call goes straight between your device and $first’s."
                                else -> "Your devices can’t reach each other directly right now, so a relay passes the call along. It can’t ${if (video) "listen or watch" else "listen"}."
                            })
                            if (up && signal == 1) append(" The line is weak, so $first’s voice may break up.")
                        }, Modifier.weight(1f).heightIn(min = 95.dp), style = p, color = c.ink2)
                    }
                    if (up && rttMs != null) Text("$rttMs ms round trip · ${lossText(lossPct ?: 0.0)}% lost",
                        Modifier.padding(top = 6.dp, start = 34.dp), style = TinFs.meta, color = c.ink2)
                    // Codec per direction while that camera is on. Both rows are always there on a video call
                    // (blank when off), so the card never changes height.
                    if (nerds != null) nerds()
                    else if (video) for ((who, what) in listOf("Your video" to sendCodec, "Their video" to recvCodec)) Row(
                        Modifier.padding(top = 6.dp, start = 34.dp).heightIn(min = 19.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Text(who, Modifier.width(88.dp), style = TinFs.meta, color = c.ink2)
                        Text(what, style = TinFs.meta, color = c.ink)
                    }
                } else Text(callPrivacy(false, name), Modifier.padding(top = 10.dp), style = p, color = c.ink2)
                TinButton("Got it", onDismiss, Modifier.padding(top = 16.dp), height = 44.dp)
            }
        }
    }
}
