package com.osvauld.p2p

import android.util.Log
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
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
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.GroupCall
import uniffi.p2pcore.GroupCallEvent
import uniffi.p2pcore.GroupCallEventKind

/*
 * Group calls on the phone (GroupCallScreen.svelte, gcall.svelte.ts): start or join a group's call, the roster, mute,
 * leave, speaker, minimise to the strip, and the calls running in my groups. The core calls and [GroupCallAudio] runs
 * the mic and speaker; this is the presentation, driven by the same core events.
 *
 * Not shown yet because the core doesn't expose them (SCREENS-PORT-STATUS.md): who is speaking, other people's mute,
 * each person's line to the host (§ 6h), how many joined a call I'm not in, and what kind of device the host is.
 */
enum class GcPhase { Idle, Starting, Connecting, Active, Reconnecting, Ended, Failed }

/** GroupCallAnnounce.svelte: "Sam started a call in Weekend hike". A quiet banner that settles after [BANNER_HOLD_MS]. */
data class GcAnnounce(val gid: String, val callId: String, val by: String, val bannerOn: Boolean = true, val dismissed: Boolean = false,
    /** Rings like an incoming call (up to `groupCallRingMs`): Join / Not now. Then the quiet banner stays. */
    val ringing: Boolean = false)

/** gcall.svelte.ts `GCALL_MS.bannerHold`. */
const val BANNER_HOLD_MS = 9000L

data class GcState(
    val gid: String? = null,
    val callId: String? = null,
    val phase: GcPhase = GcPhase.Idle,
    /** Everyone in the call (DIDs, me included). */
    val parts: List<String> = emptyList(),
    val sinceMs: Long? = null,
    val muted: Boolean = false,
    val minimized: Boolean = false,
    val about: Boolean = false,
    /** The holder of the call host device (from `Joined`), and whether it is this phone. */
    val host: String? = null,
    val hosting: Boolean = false,
    val reason: String = "",
    val note: String? = null,
    /** Calls running in my groups. */
    val live: Map<String, GroupCall> = emptyMap(),
    val endedAt: Long = 0,
    val announce: GcAnnounce? = null,
) {
    val active get() = phase != GcPhase.Idle
    val connected get() = phase == GcPhase.Active || phase == GcPhase.Reconnecting
    /** gcall.svelte.ts `statusText`. */
    val status: String get() = when (phase) {
        GcPhase.Starting -> "Finding a call host…"
        GcPhase.Connecting -> "Connecting…"
        GcPhase.Reconnecting -> "Reconnecting…"
        GcPhase.Ended, GcPhase.Failed -> reason
        else -> ""
    }
}

class GroupCallUi(private val app: P2pApp) {
    private val _s = MutableStateFlow(GcState())
    val state: StateFlow<GcState> = _s

    init {
        app.scope.launch { app.groupCalls.events.collect { onEvent(it) } }
    }

    fun refreshLive(gid: String) {
        val c = runCatching { app.node.groupCalls(gid) }.getOrNull()?.maxByOrNull { it.startedAt }
        _s.update { s -> s.copy(live = if (c != null) s.live + (gid to c) else s.live - gid) }
    }

    private val _needMic = MutableStateFlow<String?>(null)
    /** A call or join waiting for the microphone permission: MainActivity asks, then calls [call] again. */
    val needMic: StateFlow<String?> = _needMic
    fun micAsked() { _needMic.value = null }

    /** The group header's call button: start a call, join the running one, or return to mine. */
    fun call(gid: String, onError: (String) -> Unit) {
        val cur = _s.value
        // Microphone first, as for a 1:1 call: without it the call would join silent with no way to fix it.
        if (!(cur.gid == gid && cur.active) && !Perms.granted(app, android.Manifest.permission.RECORD_AUDIO)) { _needMic.value = gid; return }
        if (cur.announce?.gid == gid) stopRing()
        _s.update { s -> if (s.announce?.gid == gid) s.copy(announce = null) else s }
        if (cur.gid == gid && cur.active) { _s.update { it.copy(minimized = false) }; return }
        if (cur.active || app.calls.ui.value != null) { onError("You’re already in a call."); return }
        app.scope.launch(Dispatchers.IO) {
            refreshLive(gid)
            val live = _s.value.live[gid]
            _s.value = GcState(gid = gid, live = _s.value.live, phase = if (live != null) GcPhase.Connecting else GcPhase.Starting,
                callId = live?.callId, sinceMs = live?.startedAt?.toLong())
            app.groupCalls.prepare()
            val r = runCatching { if (live != null) { app.node.groupCallJoin(gid, live.callId); live.callId } else app.node.groupCallStart(gid) }
            r.onSuccess { id -> _s.update { if (it.phase == GcPhase.Starting) it.copy(callId = id, phase = GcPhase.Connecting) else it } }
             .onFailure { app.groupCalls.unprepare(); end(failed = true, reason = startFailure(it.message ?: ""), holdMs = 2600) }
        }
    }

    fun leave() {
        app.groupCalls.leave()
        end(failed = false, reason = "Call ended", holdMs = 0)
    }

    fun toggleMute() {
        val m = !_s.value.muted
        _s.update { it.copy(muted = m) }
        app.scope.launch(Dispatchers.IO) { runCatching { app.node.groupCallSetMuted(m) } }
        CoreService.refreshNotification()
    }

    /** The debug gallery's screenshots only: show a given state without a call. */
    fun showForGallery(s: GcState) { _s.value = s }

    fun minimize() = _s.update { if (it.phase in listOf(GcPhase.Starting, GcPhase.Connecting, GcPhase.Active, GcPhase.Reconnecting)) it.copy(minimized = true, about = false) else it }
    fun restore() = _s.update { it.copy(minimized = false) }
    fun dismissAnnounce() { stopRing(); _s.update { s -> s.copy(announce = s.announce?.copy(dismissed = true)) } }
    /** "Not now": the ring and its notification stop; the quiet "ongoing" pill in the group stays. */
    fun notNow() = dismissAnnounce()

    // ---- the ring of a started group call
    private val ringMs: Long by lazy { runCatching { app.node.groupCallRingMs().toLong() }.getOrDefault(30_000L) }
    private var ringtone: CallSounds.Ring? = null
    private var ringJob: kotlinx.coroutines.Job? = null

    private fun groupLabel(gid: String) = app.groups.list.value.firstOrNull { it.id == gid }?.name ?: "Group"

    private fun startRing(gid: String, callId: String) {
        stopRingSound()
        if (!AlertPrefs.quiet(app)) ringtone = CallSounds.startRing(app)
        if (!AppVisible.resumed) postRingNotification()
        ringJob?.cancel()
        ringJob = app.scope.launch { delay(ringMs); if (_s.value.announce?.callId == callId) stopRing() }
    }

    private fun postRingNotification() {
        val a = _s.value.announce?.takeIf { it.ringing } ?: return
        Notifications.groupRing(app, a.gid, groupLabel(a.gid), gcName(app, a.by).substringBefore(' '), ringMs)
    }

    /** The app left the front while a group call is still ringing: the notification takes over. */
    fun appLeftFront() = postRingNotification()

    private fun stopRingSound() { ringtone?.stop(); ringtone = null }

    /** The ring ends (joined, Not now, ended, or the ring time): the quiet banner / pill stays. */
    fun stopRing() {
        ringJob?.cancel(); ringJob = null
        stopRingSound()
        Notifications.cancelGroupRing(app)
        _s.update { s -> if (s.announce?.ringing == true) s.copy(announce = s.announce.copy(ringing = false)) else s }
    }
    fun about(on: Boolean) = _s.update { it.copy(about = on) }
    fun noteOk() = _s.update { it.copy(note = null) }

    private fun end(failed: Boolean, reason: String, holdMs: Long) {
        val at = System.currentTimeMillis()
        _s.update { it.copy(phase = if (failed) GcPhase.Failed else GcPhase.Ended, reason = reason, about = false, endedAt = at) }
        app.scope.launch {
            delay(holdMs)
            _s.update { if (it.endedAt == at && (it.phase == GcPhase.Ended || it.phase == GcPhase.Failed)) GcState(live = it.live) else it }
        }
    }

    private fun onEvent(e: GroupCallEvent) {
        val s = _s.value
        val mine = s.gid == e.groupId && s.active
        if (e.kind == GroupCallEventKind.ENDED || e.kind == GroupCallEventKind.LEFT) app.scope.launch(Dispatchers.IO) { app.refreshGroupLog() }
        try {
            when (e.kind) {
                GroupCallEventKind.ANNOUNCED -> app.scope.launch(Dispatchers.IO) {
                    val fresh = _s.value.live[e.groupId]?.callId != e.callId
                    refreshLive(e.groupId)
                    val c = _s.value.live[e.groupId]
                    // Someone else's call, in a group I'm not on the line in: it rings (the core says whether: not blocked, no
                    // group call here; a 1:1 call or "Calls" off in Settings keeps it quiet), then the banner stays.
                    if (fresh && c != null && c.callId == e.callId && !c.joined && c.starterDid != app.currentDid() && !_s.value.active) {
                        val rings = app.calls.ui.value == null && AlertPrefs.calls(app) && runCatching { app.node.groupCallRings(e.groupId, e.callId) }.getOrDefault(false)
                        _s.update { it.copy(announce = GcAnnounce(e.groupId, e.callId, e.detail.ifBlank { c.starterDid }, ringing = rings)) }
                        if (rings) startRing(e.groupId, e.callId)
                        delay(BANNER_HOLD_MS + if (rings) ringMs else 0)
                        _s.update { st -> if (st.announce?.callId == e.callId) st.copy(announce = st.announce.copy(bannerOn = false)) else st }
                    }
                }
                GroupCallEventKind.ENDED -> {
                    if (_s.value.announce?.gid == e.groupId) stopRing()
                    _s.update { it.copy(live = it.live - e.groupId, announce = if (it.announce?.gid == e.groupId) null else it.announce) }
                    if (mine && s.callId == e.callId) end(false, "Call ended", 900)
                }
                GroupCallEventKind.START_FAILED -> if (mine) end(true, startFailure(e.detail), 2600)
                GroupCallEventKind.JOINED -> if (mine) {
                    val me = app.currentDid()
                    _s.update { it.copy(callId = e.callId, phase = GcPhase.Active, sinceMs = it.sinceMs ?: System.currentTimeMillis(),
                        host = e.detail.ifBlank { null }, hosting = e.detail.isBlank() || e.detail == me) }
                    app.scope.launch(Dispatchers.IO) { refreshLive(e.groupId) }
                    CoreService.refreshNotification()
                }
                GroupCallEventKind.ROSTER_CHANGED -> if (mine) _s.update { it.copy(parts = e.participants) }
                GroupCallEventKind.RECONNECTING -> if (mine) _s.update { it.copy(phase = GcPhase.Reconnecting) }
                GroupCallEventKind.LEFT -> {
                    // The call may be over for everyone (or still going without me): re-read, so Join never lingers or lies.
                    if (e.groupId.isNotEmpty()) app.scope.launch(Dispatchers.IO) { refreshLive(e.groupId) }
                    if (mine && s.phase != GcPhase.Ended && s.phase != GcPhase.Failed) {
                        end(false, if (e.detail.contains("Lost")) "Call ended — host lost" else "Call ended", 900)
                    }
                }
                GroupCallEventKind.HOSTING_STARTED -> _s.update { it.copy(hosting = true) }
                GroupCallEventKind.HOSTING_ENDED -> _s.update { it.copy(hosting = false) }
                GroupCallEventKind.HOSTING_LIMITED -> if (mine && s.hosting) _s.update {
                    it.copy(note = "Battery under 15 %. Hosting is limited: only one person is heard at a time, and no one new can join.")
                }
                GroupCallEventKind.HOSTING_UNLIMITED -> _s.update { it.copy(note = null) }
                else -> {}
            }
        } catch (x: Exception) { Log.w("GroupCallUi", "event: $x") }
    }

    private fun startFailure(d: String) = if (d.lowercase().contains("host")) "No call host available" else "Couldn’t connect"
}

internal fun clockOf(secs: Long) = "%02d:%02d".format(secs / 60, secs % 60)

/** A person in a group call: "You", their name in the open group, a contact's name, else "Someone". */
private fun gcName(app: P2pApp, did: String): String {
    if (did == app.currentDid()) return "You"
    app.groups.open.value?.detail?.members?.firstOrNull { it.did == did }?.let { return it.name }
    return app.contacts.value.firstOrNull { it.did == did }?.display() ?: "Someone"
}

private fun gcHost(app: P2pApp, s: GcState): String? = when {
    s.hosting -> "this phone"
    s.host != null -> "${gcName(app, s.host).substringBefore(' ')}’s device"
    else -> null
}

@Composable
internal fun rememberCallSecs(s: GcState): Long {
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(s.phase) { while (s.phase == GcPhase.Active) { delay(500); now = System.currentTimeMillis() } }
    return ((now - (s.sinceMs ?: now)) / 1000).coerceAtLeast(0)
}

/** The full group call screen while a call is on and not minimised (GroupCallScreen.svelte, phone form). */
@Composable
fun GroupCallLayer(app: P2pApp) {
    val s by app.groupCallUi.state.collectAsState()
    if (!s.active || s.minimized) return
    val ui = app.groupCallUi
    val c = Tin.c
    val secs = rememberCallSecs(s)
    BackHandler { if (s.about) ui.about(false) else ui.minimize() }
    val groupName = app.groups.list.collectAsState().value.firstOrNull { it.id == s.gid }?.name ?: "Group call"
    val live = s.phase in listOf(GcPhase.Starting, GcPhase.Connecting, GcPhase.Active, GcPhase.Reconnecting)
    Box(Modifier.fillMaxSize().background(c.bg).clickable(interactionSource = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }, indication = null) {}) {
        Column(Modifier.fillMaxSize().systemBarsPadding().padding(start = 20.dp, end = 20.dp, top = 8.dp, bottom = 36.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            // .top: minimise · the mark · the "i" (no line data from the core yet, so no bars).
            Box(Modifier.fillMaxWidth().height(48.dp)) {
                if (live) GcTopBtn(Pg.Back, "Minimize call", Modifier.align(Alignment.CenterStart).rotate(-90f)) { ui.minimize() }
                TinMark(28.dp, live = true, state = if (s.connected) MarkState.Taut else MarkState.Ringing, modifier = Modifier.align(Alignment.Center))
                if (live && (s.host != null || s.hosting)) GcTopBtn(Pg.Info, "About this call", Modifier.align(Alignment.CenterEnd)) { ui.about(true) }
            }
            // .head: name 26/32, the status or the timer, the count, the (reserved) speaking line.
            Text(groupName, Modifier.padding(top = 6.dp).fillMaxWidth(), style = PgBodyText.copy(fontSize = 26.sp, lineHeight = 32.sp, fontWeight = FontWeight.SemiBold),
                color = c.ink, textAlign = TextAlign.Center, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Box(Modifier.padding(top = 4.dp).heightIn(min = 30.dp), contentAlignment = Alignment.Center) {
                if (s.phase == GcPhase.Active) Text(clockOf(secs), style = PgBodyText.copy(fontSize = 20.sp, lineHeight = 30.sp, fontWeight = FontWeight.Medium, fontFeatureSettings = "tnum"), color = c.ink)
                else Text(s.status, style = PgBodyText.copy(fontSize = 17.sp, lineHeight = 24.sp, fontWeight = FontWeight.Medium), color = c.ink)
            }
            val n = s.parts.size
            Text(if (s.connected && n > 0) (if (n == 1) "Only you so far. Waiting for others." else "$n people in the call") else " ",
                Modifier.padding(top = 4.dp).heightIn(min = 20.dp), style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 20.sp, fontFeatureSettings = "tnum"), color = c.ink)
            Spacer(Modifier.height(20.dp))
            // .stage: the roster (phone: up to 2 → 104, up to 4 → 2 cols 92, up to 6 → 3 cols 76, else 3 cols 68).
            Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                val tiles = minOf(n, 9)
                val (cols, av) = when { tiles <= 2 -> maxOf(tiles, 1) to 104; tiles <= 4 -> 2 to 92; tiles <= 6 -> 3 to 76; else -> 3 to 68 }
                Column(verticalArrangement = Arrangement.spacedBy(20.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    s.parts.take(if (n > 9) 8 else 9).chunked(cols).forEach { row ->
                        Row {
                            row.forEach { did ->
                                val name = if (did == app.currentDid()) app.node.profile()?.name ?: "You" else gcName(app, did)
                                Column(Modifier.width((av + 32).dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                    Avatar(name, did, av.dp)
                                    Text(if (did == app.currentDid()) "You" else name.substringBefore(' '), style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Medium),
                                        color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                }
                            }
                        }
                    }
                    if (n > 9) Text("+${n - 8} more", style = PgBodyText.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold), color = c.ink)
                }
                s.note?.let { note ->
                    Row(Modifier.align(Alignment.BottomCenter).padding(bottom = 4.dp).fillMaxWidth().depth(Depth.Pop, RoundedCornerShape(18.dp)).clip(RoundedCornerShape(18.dp)).background(c.warn)
                        .padding(start = 14.dp, end = 8.dp, top = 8.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        Text(note, Modifier.weight(1f), style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp), color = c.onWarn)
                        Box(Modifier.heightIn(min = 36.dp).clip(RoundedCornerShape(18.dp)).background(c.onWarn.copy(alpha = .14f)).clickable { ui.noteOk() }.padding(horizontal = 14.dp), contentAlignment = Alignment.Center) {
                            Text("OK", style = PgBodyText.copy(fontSize = 13.sp, fontWeight = FontWeight.Bold), color = c.onWarn)
                        }
                    }
                }
            }
            // .host: "Call host: Arjun's device" with the info icon; it opens About this call.
            val host = gcHost(app, s)
            Box(Modifier.padding(top = 4.dp, bottom = 12.dp).heightIn(min = 44.dp), contentAlignment = Alignment.Center) {
                if (host != null && (s.connected || s.phase == GcPhase.Connecting)) Row(Modifier.heightIn(min = 44.dp).clip(RoundedCornerShape(22.dp)).clickable { ui.about(true) }.padding(horizontal = 14.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(if (s.hosting) "You’re the call host" else "Call host: $host", style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 20.sp), color = c.ink)
                    Icon(Pg.Info, null, Modifier.size(16.dp).alpha(.75f), tint = c.ink)
                }
            }
            // .controls (phone): Mute · Leave · Speaker.
            val fill = if (c.dark) lerp(c.sf, c.ink, .14f) else c.sf
            // The route the call plays through (Telecom routes): the speaker slot shows it and toggles the loudspeaker.
            val routes by app.groupCalls.routes.collectAsState()
            val speaker = routes.speaker
            val (routeCaption, routeIcon) = routeSlot(routes)
            Row(horizontalArrangement = Arrangement.spacedBy(36.dp)) {
                GcControl(if (s.muted) "Muted" else "Mute", if (s.muted) CallGlyphs.MicOff else CallGlyphs.Mic, if (s.muted) c.ink else fill, if (s.muted) c.bg else c.ink, s.connected) { ui.toggleMute() }
                GcControl("Leave", CallGlyphs.End, c.callButtonEnd, Color.White, live) { ui.leave() }
                GcControl(routeCaption, routeIcon, if (speaker) c.ink else fill, if (speaker) c.bg else c.ink, true) { app.groupCalls.setSpeaker(!speaker) }
            }
        }
        if (s.about) GcAbout(app, s)
    }
}

@Composable
private fun GcTopBtn(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, modifier: Modifier, onClick: () -> Unit) {
    Box(modifier.size(44.dp).clip(CircleShape).clickable(role = Role.Button, onClick = onClick).semantics { contentDescription = label }, contentAlignment = Alignment.Center) {
        Icon(icon, null, Modifier.size(24.dp), tint = Tin.c.ink)
    }
}

@Composable
private fun GcControl(caption: String, icon: androidx.compose.ui.graphics.vector.ImageVector, bg: Color, fg: Color, enabled: Boolean, onClick: () -> Unit) {
    Column(Modifier.width(76.dp).alpha(if (enabled) 1f else .6f), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(Modifier.size(64.dp).depth(Depth.Lift, CircleShape).clip(CircleShape).background(bg).clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .semantics { contentDescription = caption }, contentAlignment = Alignment.Center) {
            Icon(icon, null, Modifier.size(26.dp), tint = fg)
        }
        Text(caption, style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Medium), color = Tin.c.ink, maxLines = 1)
    }
}

/** About this call (group): who carries it and what that means; honest that the host can hear the call. */
@Composable
private fun GcAbout(app: P2pApp, s: GcState) {
    val c = Tin.c
    val ui = app.groupCallUi
    val host = (gcHost(app, s) ?: "the call host").replaceFirstChar { it.uppercase() }
    BackHandler { ui.about(false) }
    Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = .24f)).clickable(interactionSource = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }, indication = null) { ui.about(false) }) {
        Column(Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(start = 16.dp, end = 16.dp, bottom = 24.dp).widthIn(max = 420.dp).fillMaxWidth()
            .depth(Depth.Sheet, SmoothCornerShape(24.dp)).clip(SmoothCornerShape(24.dp)).background(c.sfRaised)
            .clickable(interactionSource = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }, indication = null) {}
            .padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            val p = PgBodyText.copy(fontSize = 13.sp, lineHeight = 19.sp)
            Text("About this call", style = PgBodyText.copy(fontSize = 17.sp, lineHeight = 24.sp, fontWeight = FontWeight.SemiBold), color = c.ink)
            Text(androidx.compose.ui.text.buildAnnotatedString {
                pushStyle(androidx.compose.ui.text.SpanStyle(color = c.ink, fontWeight = FontWeight.SemiBold)); append(host); pop()
                append(" ${if (s.hosting) "is carrying" else "carries"} this call. Everyone in the call connects to it, and it passes the audio on and chooses who is heard when several people speak.")
            }, style = p, color = c.ink2)
            Text("Your connection to the call host is encrypted, but the call host itself receives the audio, so it can hear this call. You trust it for the call the way you trust a chat host with posts.", style = p, color = c.ink2)
            Text("A call host is a device a group admin chose and that agreed to it. It is separate from chat hosting. Admins can change call hosts in Group info.", style = p, color = c.ink2)
            TinButton("Got it", { ui.about(false) }, Modifier.padding(top = 8.dp), height = 44.dp)
        }
    }
}

/**
 * OngoingCall.svelte: "Call in progress" under a group's header while its call runs and I'm not in it (Join), or
 * "You're in the call" with the timer (Open). The core doesn't say how many joined, so the line names who started it.
 */
@Composable
fun OngoingCallBanner(app: P2pApp, gid: String, onError: (String) -> Unit) {
    val s by app.groupCallUi.state.collectAsState()
    val mine = s.gid == gid && s.active
    val call = s.live[gid]
    if (!mine && call == null) return
    val c = Tin.c
    val secs = rememberCallSecs(s)
    val fg = if (c.dark) c.ink else c.accentInk
    val bg = if (c.dark) lerp(c.sf, c.accent, .24f) else c.accentSoft
    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).background(bg).padding(start = 14.dp, end = 12.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Icon(PgIcons.Users, null, Modifier.size(20.dp), tint = fg)
        Column(Modifier.weight(1f)) {
            Text(if (mine) "You’re in the call" else "Call in progress", style = PgBodyText.copy(fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold), color = fg, maxLines = 1)
            Text(if (mine) (if (s.phase == GcPhase.Active) clockOf(secs) else s.status) else "Started by ${gcName(app, call!!.starterDid).substringBefore(' ')}",
                style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 17.sp, fontFeatureSettings = "tnum"), color = fg.copy(alpha = .85f))
        }
        val shape = RoundedCornerShape(18.dp)
        Box(Modifier.widthIn(min = 72.dp).heightIn(min = 36.dp).clip(shape).then(if (mine) Modifier.border(1.5.dp, c.accentInk, shape) else Modifier.background(c.accent))
            .clickable { if (mine) app.groupCallUi.restore() else app.groupCallUi.call(gid, onError) }.padding(horizontal = 18.dp), contentAlignment = Alignment.Center) {
            Text(if (mine) "Open" else "Join", style = PgBodyText.copy(fontSize = 13.sp, fontWeight = FontWeight.Bold), color = if (mine) c.accentInk else c.onAccent)
        }
    }
}

/**
 * GroupCallAnnounce.svelte (phone): a quiet card under the status bar, 8 dp from the sides: the starter's avatar, the
 * group's name over "<name> started a call", a dismiss X and Join. Slides 20 dp down in (200 ms), 12 dp up out (140 ms).
 * Shown while the call still runs, the banner has not settled or been dismissed, I hold no line, and that group is
 * not the one open. The core has no group mute yet, so a muted group cannot be left quiet (gap).
 */
@Composable
fun GroupCallAnnounce(app: P2pApp, openGid: String?, onOpen: (String) -> Unit) {
    val s by app.groupCallUi.state.collectAsState()
    val calls by app.calls.ui.collectAsState()
    val groups by app.groups.list.collectAsState()
    val a = s.announce
    val show = a != null && a.bannerOn && !a.dismissed && s.live.containsKey(a.gid) && !s.active && calls == null && (openGid != a.gid || a.ringing)
    var last by remember { mutableStateOf(a) }
    if (a != null) last = a
    val px = with(androidx.compose.ui.platform.LocalDensity.current) { 1.dp.toPx() }
    Box(Modifier.fillMaxSize().statusBarsPadding().padding(start = 8.dp, end = 8.dp, top = 8.dp), contentAlignment = Alignment.TopCenter) {
        AnimatedVisibility(show,
            enter = fadeIn(tween(200, easing = Motion.EaseOut)) + slideInVertically(tween(200, easing = Motion.EaseOut)) { -(20 * px).toInt() },
            exit = fadeOut(tween(140, easing = Motion.CubicIn)) + slideOutVertically(tween(140, easing = Motion.CubicIn)) { -(12 * px).toInt() }) {
            val l = last ?: return@AnimatedVisibility
            val c = Tin.c
            val group = groups.firstOrNull { it.id == l.gid }?.name ?: "Group"
            val by = gcName(app, l.by)
            Row(Modifier.fillMaxWidth().depth(Depth.Pop, SmoothCornerShape(20.dp)).clip(SmoothCornerShape(20.dp)).background(c.sfRaised)
                .border(1.dp, c.ln, SmoothCornerShape(20.dp)).padding(start = 8.dp, end = 10.dp, top = 8.dp, bottom = 8.dp)
                .semantics { contentDescription = if (l.ringing) "$by is calling the group $group" else "$by started a call in $group"; liveRegion = LiveRegionMode.Polite },
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(Modifier.weight(1f).heightIn(min = 48.dp).pressable({ app.groupCallUi.dismissAnnounce(); onOpen(l.gid) }, RoundedCornerShape(14.dp))
                    .semantics { contentDescription = "Open $group" }.padding(horizontal = 4.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Avatar(by, l.by, 44.dp)
                    Column(Modifier.weight(1f)) {
                        Text(group, style = TinFs.body.copy(fontWeight = FontWeight(650), lineHeight = 20.sp), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(if (l.ringing) "${by.substringBefore(' ')} is calling the group" else "$by started a call", style = TinFs.sub, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
                if (l.ringing) {
                    Box(Modifier.height(44.dp).pressable({ app.groupCallUi.notNow() }, SmoothCornerShape(22.dp), wash = c.sfRaised, rest = c.sf2)
                        .padding(horizontal = 14.dp), contentAlignment = Alignment.Center) {
                        Text("Not now", style = TinFs.sub.copy(fontWeight = FontWeight.Bold), color = c.ink)
                    }
                } else PgIconButton(PgIcons.X, "Dismiss", { app.groupCallUi.dismissAnnounce() }, tint = c.ink2, size = 36.dp, iconSize = 18.dp)
                Box(Modifier.height(44.dp).pressable({ app.groupCallUi.call(l.gid) {} }, SmoothCornerShape(22.dp), wash = c.callButtonAnswer, rest = c.callButtonAnswer)
                    .padding(horizontal = 20.dp), contentAlignment = Alignment.Center) {
                    Text("Join", style = TinFs.sub.copy(fontWeight = FontWeight.Bold), color = Color.White)
                }
            }
        }
    }
}
