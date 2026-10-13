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
import androidx.compose.ui.draw.rotate
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

/** CallScreen.svelte's copy: never claim encryption while dialling or ringing. */
internal fun callStatus(active: Boolean, ringing: Boolean, reconnecting: Boolean, secs: Long): String = when {
    active && reconnecting -> "Reconnecting…"
    active -> clock(secs)
    ringing -> "Ringing…"
    else -> "Calling…"
}

internal fun callPrivacy(active: Boolean, name: String): String = if (active)
    "End-to-end encrypted. Your voice is encrypted on this device and only decrypted on $name’s. When a relay helps connect you, it passes the call along but can’t listen."
else "Connecting to $name. Once the call is connected, it is end-to-end encrypted."

/** Fixed, mirrored 44 dp action cells with the 28 dp live mark independently centered. */
@Composable
internal fun CallTop(active: Boolean, ringing: Boolean, onMinimise: (() -> Unit)?, onInfo: (() -> Unit)?,
    reconnecting: Boolean = false, incoming: Boolean = false) {
    val phase = when {
        reconnecting -> CallPhase.Reconnecting
        active -> CallPhase.Active
        incoming -> CallPhase.Incoming
        else -> CallPhase.Outgoing
    }
    val look = statusLook(call = phase, conn = ConnState.Online)
    Box(Modifier.fillMaxWidth().height(48.dp)) {
        if (onMinimise != null) Box(Modifier.align(Alignment.CenterStart)) {
            CallTopButton(CallGlyphs.Chev, "Minimize call", onMinimise)
        }
        TinMark(28.dp, live = true, state = look.state, sway = look.sway, description = look.label,
            modifier = Modifier.align(Alignment.Center))
        if (onInfo != null) Box(Modifier.align(Alignment.CenterEnd)) {
            CallTopButton(CallGlyphs.Info, "About this call", onInfo)
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
    onSpeaker: () -> Unit, onCamera: (() -> Unit)?, onFlip: (() -> Unit)?, onEnd: () -> Unit) {
    val c = Tin.c
    val fill = if (c.dark) lerp(c.sf, c.ink, .14f) else c.sf
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        CallControl("Mute", if (muted) "Muted" else "Mute", if (muted) CallGlyphs.MicOff else CallGlyphs.Mic,
            if (muted) c.ink else fill, if (muted) c.bg else c.ink, onMute, selected = muted)
        CallControl("Camera", "Camera", if (camera) CallGlyphs.Video else CallGlyphs.VideoOff,
            if (camera) c.ink else fill, if (camera) c.bg else c.ink, onCamera ?: {}, enabled = onCamera != null, selected = camera)
        CallControl("End call", "End", CallGlyphs.Phone, c.callButtonEnd, Color.White, onEnd)
        CallControl("Flip camera", "Flip", CallGlyphs.Flip, fill, c.ink, onFlip ?: {}, enabled = camera && onFlip != null)
        CallControl("Speaker", "Speaker", CallGlyphs.Speaker, if (speaker) c.ink else fill,
            if (speaker) c.bg else c.ink, onSpeaker, selected = speaker)
    }
}

@Composable
private fun RowScope.CallControl(label: String, caption: String, icon: ImageVector, bg: Color, fg: Color,
    onClick: () -> Unit, enabled: Boolean = true, selected: Boolean? = null) {
    Column(Modifier.weight(1f).alpha(if (enabled) 1f else .38f), horizontalAlignment = Alignment.CenterHorizontally) {
        BoxWithConstraints(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
            val diameter = maxWidth.coerceAtMost(56.dp).coerceAtLeast(44.dp)
            Box(Modifier.size(diameter).depth(Depth.Lift, CircleShape).clip(CircleShape).background(bg)
                .clickable(enabled = enabled, role = Role.Button, onClick = onClick).semantics {
                    contentDescription = label
                    if (selected != null) toggleableState = ToggleableState(selected)
                }, contentAlignment = Alignment.Center) {
                Icon(icon, null, tint = fg, modifier = Modifier.size(26.dp).rotate(if (label == "End call") 135f else 0f))
            }
        }
        Text(caption, Modifier.padding(top = 8.dp).heightIn(min = 18.dp), style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp),
            color = Tin.c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** Voice focus geometry; no background texture is chosen until Fog/Clouds is decided and measured. */
@Composable
internal fun CallFocus(name: String, did: String, active: Boolean, ringing: Boolean, secs: Long = 0,
    reconnecting: Boolean = false, problem: String? = null, incoming: Boolean = false,
    onMinimise: (() -> Unit)? = null, onInfo: (() -> Unit)? = null, controls: @Composable () -> Unit) {
    val c = Tin.c
    Column(Modifier.fillMaxSize().background(c.bg).systemBarsPadding().padding(start = 20.dp, end = 20.dp, top = 8.dp, bottom = 36.dp),
        horizontalAlignment = Alignment.CenterHorizontally) {
        CallTop(active, ringing, onMinimise, onInfo, reconnecting, incoming)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = 40.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
            Avatar(name, did, 112.dp)
            Text(name, Modifier.padding(top = 22.dp).fillMaxWidth().heightIn(min = 34.dp),
                style = TinType.h1.copy(fontSize = 28.sp, lineHeight = 34.sp, fontWeight = FontWeight.SemiBold),
                color = c.ink, textAlign = TextAlign.Center, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Box(Modifier.padding(top = 6.dp).heightIn(min = 30.dp), contentAlignment = Alignment.Center) {
                Text(if (incoming) "Tinline voice call" else callStatus(active, ringing, reconnecting, secs),
                    style = TinType.bodyL.copy(fontSize = if (active && !reconnecting) 20.sp else 17.sp, lineHeight = 30.sp,
                        fontWeight = FontWeight.Medium, fontFeatureSettings = "tnum"), color = c.ink, maxLines = 1)
            }
            Text(if (active && !reconnecting) "End-to-end encrypted" else " ", Modifier.padding(top = 10.dp).heightIn(min = 18.dp),
                style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = c.ink, maxLines = 1)
            // Keep a two-line error cell even when there is no microphone/video problem.
            Text(problem ?: " ", Modifier.padding(top = 8.dp).fillMaxWidth().heightIn(min = 38.dp),
                style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 19.sp), color = c.er, maxLines = 2,
                overflow = TextOverflow.Ellipsis, textAlign = TextAlign.Center)
        }
        controls()
    }
}

@Composable
internal fun CallInfo(name: String, active: Boolean, onDismiss: () -> Unit) {
    TinSheet(onDismiss) {
        Text("About this call", style = TinType.titleM, color = Tin.c.ink)
        Text(callPrivacy(active, name), Modifier.padding(top = 10.dp, bottom = 16.dp), style = TinType.bodyM, color = Tin.c.ink2)
        TinButton("Got it", onDismiss)
    }
}
