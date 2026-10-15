package com.osvauld.p2p

import android.content.Intent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.foundation.border
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import uniffi.p2pcore.CallState

/*
 * The minimised call (CallStrip.svelte / GroupCallStrip.svelte), decided 2026-10-14 in the in-call lab: at the BOTTOM
 * of the app, under the tab bar or the composer and above the system navigation, so nothing to tap sits by the ear and
 * the proximity sensor when I come back to the app mid-call. In flow: the app makes room for it. No End (in a chat it
 * would sit right under Send): ending is on the call screen and in the notification. While the keyboard is up it stays
 * (I may be writing to someone else) but slims to a 26 dp line above the keyboard.
 */

/** A 1:1 call I'm in, outside its call screen (an incoming call still ringing has its own screen). */
@Composable
fun callStripOn(app: P2pApp): Boolean {
    val u by app.calls.ui.collectAsState()
    val g by app.groupCallUi.state.collectAsState()
    val one = u?.let { !(it.info.incoming && it.state is CallState.Ringing) && it.state !is CallState.Ended } == true
    return one || (g.active && g.minimized)
}

/** The strip(s) for the bottom of the app. The caller consumes the bottom insets for the content above while shown. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun CallStrips(app: P2pApp) {
    val ctx = LocalContext.current
    val u by app.calls.ui.collectAsState()
    val g by app.groupCallUi.state.collectAsState()
    val slim = WindowInsets.isImeVisible
    val one = u?.takeIf { !(it.info.incoming && it.state is CallState.Ringing) && it.state !is CallState.Ended }
    val group = g.active && g.minimized
    // The bottom insets are the strip's only while one shows (the app above gives them up then); with no call the
    // screens keep their own navigation/keyboard padding, so it must not be counted here as well.
    Column(Modifier.fillMaxWidth().then(if (one != null || group) Modifier.windowInsetsPadding(WindowInsets.navigationBars.union(WindowInsets.ime)) else Modifier)) {
        AnimatedVisibility(one != null, enter = expandVertically(tween(160, delayMillis = 80)) + fadeIn(tween(160, delayMillis = 80)),
            exit = shrinkVertically(tween(160)) + fadeOut(tween(160))) {
            val c = one ?: return@AnimatedVisibility
            var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
            LaunchedEffect(Unit) { while (true) { delay(500); now = System.currentTimeMillis() } }
            val secs = ((now - (c.activeSinceMs ?: now)) / 1000).coerceAtLeast(0)
            val vi by app.video.info.collectAsState()
            val video = vi?.let { it.callId == c.info.callId && (it.localCamera || it.remoteCamera) } == true
            val name = c.info.peerName.ifBlank { "Unknown" }
            val status = callStatus(c.state is CallState.Active, c.state is CallState.Ringing, c.stats?.reconnecting == true, secs)
            StripRow(if (video) PgIcons.Video else PgIcons.Phone, name, status, slim, "Return to call with $name") {
                ctx.startActivity(Intent(ctx, CallActivity::class.java))
            }
        }
        AnimatedVisibility(group, enter = expandVertically(tween(160, delayMillis = 80)) + fadeIn(tween(160, delayMillis = 80)),
            exit = shrinkVertically(tween(160)) + fadeOut(tween(160))) {
            val name = app.groups.list.collectAsState().value.firstOrNull { it.id == g.gid }?.name ?: "Group call"
            val secs = rememberCallSecs(g)
            StripRow(PgIcons.Users, name, if (g.phase == GcPhase.Active) clockOf(secs) else g.status, slim, "Return to group call in $name") {
                app.groupCallUi.restore()
            }
        }
    }
}

/** 44 dp accent line: icon 18, the name 15/600, the time (or status) 13 tabular at the end. Slim: 26 dp, 13 and 11. */
@Composable
private fun StripRow(icon: ImageVector, name: String, status: String, slim: Boolean, label: String, onOpen: () -> Unit) {
    val c = Tin.c
    val h by animateDpAsState(if (slim) 26.dp else 44.dp, tween(160), label = "strip")
    Row(Modifier.fillMaxWidth().height(h).background(c.accent).clickable(role = Role.Button, onClick = onOpen)
        .semantics(mergeDescendants = true) { contentDescription = "$label. $status" }.padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Icon(icon, null, Modifier.size(if (slim) 14.dp else 18.dp), tint = c.onAccent)
        Text(name, Modifier.weight(1f), style = PgBodyText.copy(fontSize = if (slim) 13.sp else 15.sp, fontWeight = FontWeight.SemiBold),
            color = c.onAccent, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(status, style = PgBodyText.copy(fontSize = if (slim) 11.sp else 13.sp, fontFeatureSettings = "tnum"), color = c.onAccent, maxLines = 1)
    }
}

/**
 * MiniVideo.svelte: the call is minimised during video, so a small floating tile of their picture sits over the app, in
 * addition to the strip: 92 dp wide, 3:4, radius 16, a 1.5 dp light ring, 12 dp from the end and 92 dp above the bottom
 * of the content (the strip is below it). Tap it to return to the call. It pops in (.9 to 1, with a fade).
 *
 * My own picture is not offered when theirs is off (the playground does): the camera stops with the call screen on the
 * phone ("no capture in the background"), so there is no picture of mine to show here.
 */
@Composable
fun MiniVideo(app: P2pApp, modifier: Modifier = Modifier) {
    val ctx = LocalContext.current
    val u by app.calls.ui.collectAsState()
    val vi by app.video.info.collectAsState()
    val live = u?.takeIf { it.state is CallState.Active && it.info.callId == vi?.callId }
    val theirs = vi?.let { it.negotiated && it.remoteCamera } == true
    val show = live != null && theirs
    val name = live?.info?.peerName?.ifBlank { "Unknown" } ?: ""
    val frozen = live?.stats?.reconnecting == true
    AnimatedVisibility(show, modifier, enter = fadeIn(tween(200, easing = Motion.EaseOut)) + scaleIn(tween(200, easing = Motion.EaseOut), initialScale = 0.9f),
        exit = fadeOut(tween(140, easing = Motion.CubicIn)) + scaleOut(tween(140, easing = Motion.CubicIn), targetScale = 0.9f)) {
        val shape = SmoothCornerShape(16.dp)
        Box(Modifier.width(92.dp).aspectRatio(3f / 4f).depth(Depth.Pop, shape).clip(shape).background(Color(0xFF15171C))
            .border(1.5.dp, Color.White.copy(alpha = 0.38f), shape)
            .clickable(onClickLabel = "Return to video call with $name", role = Role.Button) { ctx.startActivity(Intent(ctx, CallActivity::class.java)) }) {
            RemoteVideo(app, app.calls, Modifier.fillMaxSize())
            if (frozen) Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.2f)))
        }
    }
}
