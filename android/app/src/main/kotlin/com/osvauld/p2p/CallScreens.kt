package com.osvauld.p2p

import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.p2pcore.CallState

private fun ComponentActivity.showOverLock() {
    if (Build.VERSION.SDK_INT >= 27) { setShowWhenLocked(true); setTurnScreenOn(true) }
    else @Suppress("DEPRECATION") window.addFlags(
        WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON)
    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
}

/** Asks for the microphone (if missing) and then runs [then] whatever the answer: a call can still be heard without it. */
private fun ComponentActivity.registerMicThen(then: () -> Unit): () -> Unit {
    var pending: (() -> Unit)? = null
    val launcher = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) {
        pending?.invoke(); pending = null
    }
    return {
        if (Perms.granted(this, android.Manifest.permission.RECORD_AUDIO)) then()
        else { pending = then; launcher.launch(android.Manifest.permission.RECORD_AUDIO) }
    }
}

/** Ended screens stay up this long (the "Call ended" board: "Closes by itself after 4 seconds"). */
const val ENDED_AUTOCLOSE_MS = 4000L

class CallActivity : ComponentActivity() {
    private lateinit var answerWithMic: () -> Unit

    override fun onStart() {
        super.onStart()
        P2pApp.get(this).calls.callScreenShown = true
        Notifications.cancelWaiting(this)  // the banner is here now
    }

    override fun onStop() { P2pApp.get(this).calls.callScreenShown = false; super.onStop() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        secureWindow()
        enableEdgeToEdge()
        showOverLock()
        val app = P2pApp.get(this)
        answerWithMic = registerMicThen { app.calls.answer() }
        // Only a fresh launch answers: a recreation (rotation) must not answer a second time.
        if (savedInstanceState == null) handle(intent)
        val meName = app.node.profile()?.name ?: ""
        setContent {
            TinlineTheme {
                val ui by app.calls.ui.collectAsState()
                val ended by app.calls.ended.collectAsState()
                var seen by remember { mutableStateOf(false) }
                if (ui != null) seen = true
                val e = ended
                // An outgoing call that fails at once is already over when this screen opens: still show why.
                val showEnded = ui == null && e != null && (seen || System.currentTimeMillis() - e.atMs < 10_000)
                LaunchedEffect(ui, seen, showEnded) { if (ui == null && seen && !showEnded) finish() }
                // Opened with nothing to show (no call, and no recent ended call): do not sit there blank.
                LaunchedEffect(Unit) {
                    delay(1500)
                    val en = app.calls.ended.value
                    if (app.calls.ui.value == null && (en == null || System.currentTimeMillis() - en.atMs > 10_000)) finish()
                }
                val u = ui
                when {
                    // Their call arrived while ours was open (ours yielded): ring it here.
                    u != null && u.info.incoming && u.state is CallState.Ringing ->
                        IncomingContent(u.info.peerName, u.info.peerDid, onDecline = { app.calls.decline() }, onAnswer = { answerWithMic() })
                    u != null -> {
                        InCallScreen(u, meName, app.calls, onMinimise = { finish() })
                        val w by app.calls.waiting.collectAsState()
                        w?.let { WaitingBanner(it.peerName.ifBlank { "Unknown" }, it.peerDid, onDecline = { app.calls.declineWaiting() }, onEndAnswer = { app.calls.endAndAnswer() }) }
                    }
                    showEnded && e != null -> EndedRoute(e, meName, onClose = { finish() }, onAgain = {
                        app.scope.launch { app.calls.place(e.peerDid) }
                    })
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) { super.onNewIntent(intent); setIntent(intent); handle(intent) }

    private fun handle(i: Intent?) {
        if (i?.getBooleanExtra(EXTRA_ANSWER, false) == true) {
            i.removeExtra(EXTRA_ANSWER)
            answerWithMic()
        }
    }

    companion object { const val EXTRA_ANSWER = "answer" }
}

class IncomingCallActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        secureWindow()
        enableEdgeToEdge()
        showOverLock()
        val app = P2pApp.get(this)
        val answerWithMic = registerMicThen {
            app.calls.answer()
            startActivity(Intent(this@IncomingCallActivity, CallActivity::class.java)); finish()
        }
        setContent {
            TinlineTheme(dark = true) {
                val ui by app.calls.ui.collectAsState()
                val u = ui
                LaunchedEffect(u?.state) {
                    if (u == null) finish()
                    else if (u.state is CallState.Active) {
                        startActivity(Intent(this@IncomingCallActivity, CallActivity::class.java)); finish()
                    }
                }
                if (u != null) IncomingContent(u.info.peerName, u.info.peerDid, onDecline = { app.calls.decline(); finish() }, onAnswer = { answerWithMic() })
            }
        }
    }
}

// ------------------------------------------------------------------ incoming (always dark)

@Composable
fun IncomingContent(name: String, did: String, onDecline: () -> Unit, onAnswer: () -> Unit) {
    val c = Tin.c
    val who = name.ifBlank { "Unknown" }
    Column(Modifier.fillMaxSize().background(Color(0xFF0B100F)).systemBarsPadding(), horizontalAlignment = Alignment.CenterHorizontally) {
        Row(Modifier.padding(top = 24.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Glyphs.Cans, null, tint = c.ink2, modifier = Modifier.size(20.dp))
            Text("Tinline call", style = TinType.bodyM, color = c.ink2)
        }
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = 40.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp, Alignment.CenterVertically)) {
            // Two thin amber rings around the avatar: the line is ringing.
            Box(Modifier.size(168.dp).border(2.dp, c.thread.copy(alpha = 0.35f), CircleShape), contentAlignment = Alignment.Center) {
                Box(Modifier.size(144.dp).border(2.dp, c.thread.copy(alpha = 0.6f), CircleShape), contentAlignment = Alignment.Center) {
                    Avatar(who, did, 120.dp)
                }
            }
            Text(who, Modifier.padding(top = 10.dp, start = 24.dp, end = 24.dp), style = TinType.display, color = c.ink, textAlign = TextAlign.Center)
            Text("is calling you", style = TinType.bodyL.copy(fontSize = 17.sp), color = c.ink2)
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Rounded.Lock, null, tint = c.ink2, modifier = Modifier.size(16.dp))
                Text("End-to-end encrypted", style = TinType.bodyM.copy(fontSize = 13.sp), color = c.ink2)
            }
        }
        Row(Modifier.fillMaxWidth().padding(start = 32.dp, end = 32.dp, bottom = 56.dp), horizontalArrangement = Arrangement.SpaceAround) {
            LabeledRound("Decline") { RoundBtn(Icons.Rounded.CallEnd, "Decline", c.callEnd, Color.White, onDecline) }
            LabeledRound("Answer") { RoundBtn(Icons.Rounded.Call, "Answer", c.callAccept, Color.White, onAnswer) }
        }
    }
}

// ------------------------------------------------------------------ calling / in call

private fun routeLabel(speaker: Boolean) = if (speaker) "Speaker" else "Phone"

/** "Arjun is calling" over the in-call screen: no hold, no call waiting, so the two choices end one call or the other. */
@Composable
fun WaitingBanner(name: String, did: String, onDecline: () -> Unit, onEndAnswer: () -> Unit) {
    val c = Tin.c
    Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.28f)).statusBarsPadding().padding(start = 12.dp, end = 12.dp, top = 12.dp)) {
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(20.dp)).background(c.sf).padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Column(Modifier.weight(1f)) {
                    Text("$name is calling", style = TinType.h1.copy(fontSize = 16.sp, lineHeight = 22.sp), color = c.ink)
                    Text("Your call continues until you choose.", style = TinType.bodyM, color = c.ink2)
                }
                Avatar(name, did, 44.dp)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(Modifier.clip(RoundedCornerShape(22.dp)).background(c.callEnd).clickable(onClickLabel = "Decline", role = Role.Button, onClick = onDecline)
                    .height(44.dp).padding(horizontal = 14.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Icon(Icons.Rounded.CallEnd, null, tint = Color.White, modifier = Modifier.size(18.dp))
                    Text("Decline", color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                }
                Row(Modifier.clip(RoundedCornerShape(22.dp)).background(c.callAccept).clickable(onClickLabel = "End and answer", role = Role.Button, onClick = onEndAnswer)
                    .height(44.dp).padding(horizontal = 14.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Icon(Icons.Rounded.Call, null, tint = Color.White, modifier = Modifier.size(18.dp))
                    Text("End & answer", color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                }
            }
            Text("Decline tells $name you\u2019re busy. No hold, no call waiting.", style = TinType.bodyM.copy(fontSize = 12.sp, lineHeight = 16.sp), color = c.ink2)
        }
    }
}

@Composable
fun InCallScreen(ui: CallUi, meName: String, calls: CallController, onMinimise: () -> Unit) {
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) { while (true) { delay(500); now = System.currentTimeMillis() } }
    val secs = ((now - (ui.activeSinceMs ?: now)) / 1000).coerceAtLeast(0)
    val s = ui.stats
    val loss = s?.let { val t = (it.received + it.lost).toDouble(); if (t > 0) 100.0 * it.lost.toDouble() / t else 0.0 }
    InCallContent(
        name = ui.info.peerName.ifBlank { "Unknown" }, did = ui.info.peerDid, meName = meName,
        active = ui.state is CallState.Active, ringing = ui.state is CallState.Ringing, secs = secs,
        direct = s?.direct, bars = if (s != null && loss != null) qualityBars(s.rttMs.toInt(), loss) else null,
        muted = ui.muted, speaker = ui.speaker, micProblem = ui.micProblem, reconnecting = s?.reconnecting == true,
        onMute = { calls.toggleMute() }, onSpeaker = { calls.toggleSpeaker() }, onEnd = { calls.hangup() }, onMinimise = onMinimise,
    )
}

@Composable
fun InCallContent(
    name: String, did: String, meName: String, active: Boolean, ringing: Boolean, secs: Long, direct: Boolean?, bars: Int?,
    muted: Boolean, speaker: Boolean, micProblem: String?,
    onMute: () -> Unit, onSpeaker: () -> Unit, onEnd: () -> Unit, onMinimise: () -> Unit, startWithSheet: Boolean = false,
    reconnecting: Boolean = false,
) {
    val c = Tin.c
    var sheet by remember { mutableStateOf(startWithSheet) }
    Page {
        Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 8.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconBtn(Icons.Rounded.KeyboardArrowDown, "Minimise", onMinimise)
            Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically) {
                if (!active) Badge(if (ringing) "Ringing…" else "Finding a path…", BadgeKind.Neutral) { Dot(c.thread, 8.dp) }
                else if (reconnecting) Badge("Reconnecting\u2026", BadgeKind.Warn, Icons.Rounded.SyncProblem)
                else {
                    if (direct == true) Badge("Direct", BadgeKind.Direct, Glyphs.Direct)
                    if (direct == false) Badge("Relayed · encrypted", BadgeKind.Relayed, Glyphs.Relayed)
                    if (bars != null) Badge(qualityLabel(bars), BadgeKind.Neutral) { QualityBars(bars, warn = bars <= 1) }
                }
            }
            Spacer(Modifier.width(48.dp))
        }
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(10.dp, Alignment.CenterVertically)) {
            PairAvatars(meName.ifBlank { "?" }, name, did, otherAlpha = if (reconnecting) 0.6f else 1f)
            Text(name, Modifier.padding(top = 16.dp), style = TinType.h1.copy(fontSize = 30.sp, lineHeight = 36.sp), color = c.ink, textAlign = TextAlign.Center)
            if (active) Text(clock(secs), style = TinType.mono.copy(fontSize = 20.sp, lineHeight = 28.sp), color = c.ink2)
            else Text(if (ringing) "Ringing…" else "Calling…", style = TinType.bodyL.copy(fontSize = 17.sp), color = c.ink2)
            if (active && reconnecting) Text(
                "Your network changed. Hold on \u2014 Tinline is finding the line again.", Modifier.widthIn(max = 300.dp).padding(top = 6.dp),
                style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 22.sp), color = c.ink2, textAlign = TextAlign.Center,
            )
            else if (active && direct != null) Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Rounded.Lock, null, tint = c.ink2, modifier = Modifier.size(16.dp))
                Text("End-to-end encrypted · " + if (direct) "straight to their phone" else "through an encrypted relay", style = TinType.bodyM.copy(fontSize = 13.sp), color = c.ink2)
            }
            if (micProblem != null) Text(micProblem, Modifier.padding(top = 4.dp), style = TinType.bodyM, color = c.er, textAlign = TextAlign.Center)
        }
        Row(Modifier.fillMaxWidth().padding(bottom = 28.dp), horizontalArrangement = Arrangement.spacedBy(24.dp, Alignment.CenterHorizontally)) {
            LabeledRound(if (muted) "Muted" else "Mute") {
                RoundBtn(if (muted) Icons.Rounded.MicOff else Icons.Rounded.Mic, if (muted) "Unmute" else "Mute",
                    if (muted) c.ink else c.sf3, if (muted) c.bg else c.ink, onMute, size = 64.dp, iconSize = 26.dp)
            }
            LabeledRound(routeLabel(speaker)) {
                RoundBtn(if (speaker) Icons.Rounded.VolumeUp else Icons.Rounded.PhoneInTalk, "Audio: ${routeLabel(speaker)}", c.sf3, c.ink, { sheet = true }, size = 64.dp, iconSize = 26.dp)
            }
        }
        Box(Modifier.fillMaxWidth().padding(bottom = 64.dp), contentAlignment = Alignment.Center) {
            RoundBtn(Icons.Rounded.CallEnd, if (active) "End call" else "Cancel call", c.callEnd, Color.White, onEnd)
        }
    }
    if (sheet) AudioRouteSheet(speaker, relayed = direct == false, onDismiss = { sheet = false }, onPick = { wantSpeaker -> if (wantSpeaker != speaker) onSpeaker(); sheet = false })
}

@Composable
private fun AudioRouteSheet(speaker: Boolean, relayed: Boolean, onDismiss: () -> Unit, onPick: (speaker: Boolean) -> Unit) {
    val c = Tin.c
    TinSheet(onDismiss) {
        Text("Play call through", Modifier.padding(start = 8.dp, top = 12.dp, bottom = 8.dp), style = TinType.titleL.copy(fontSize = 20.sp), color = c.ink)
        RouteOption(Icons.Rounded.PhoneInTalk, "Phone", !speaker) { onPick(false) }
        RouteOption(Icons.Rounded.VolumeUp, "Speaker", speaker) { onPick(true) }
        // Bluetooth and wired headsets join this list once AudioEngine can route to them.
        if (relayed) Hint("Relayed: a direct line wasn’t possible on this network, so an encrypted relay is passing the call along. It can’t hear you.", Modifier.padding(start = 8.dp, end = 8.dp, top = 12.dp))
    }
}

@Composable
private fun RouteOption(icon: ImageVector, label: String, on: Boolean, onClick: () -> Unit) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().heightIn(min = 60.dp).clip(RoundedCornerShape(14.dp)).background(if (on) c.prc else Color.Transparent)
            .clickable(role = Role.RadioButton, onClick = onClick).padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(icon, null, tint = if (on) c.onPrc else c.ink, modifier = Modifier.padding(start = 8.dp))
        Text(label, Modifier.weight(1f), style = TinType.bodyL, color = if (on) c.onPrc else c.ink)
        if (on) Icon(Icons.Rounded.Check, "Selected", tint = c.onPrc, modifier = Modifier.padding(end = 8.dp))
    }
}

// ------------------------------------------------------------------ ended / couldn't reach / mic needed

@Composable
private fun EndedRoute(e: EndedUi, meName: String, onClose: () -> Unit, onAgain: () -> Unit) {
    if (classifyEnd(e.reason) == EndKind.Unreachable && !e.wasActive) UnreachableContent(e.peerName, e.peerDid, meName, onAgain, onClose)
    else EndedContent(e.peerName, e.peerDid, meName, endReasonText(e.reason, e.peerName, e.incoming), if (e.wasActive) e.secs else null, e.direct, e.bars, onAgain, onClose)
}

@Composable
fun EndedContent(
    name: String, did: String, meName: String, reasonText: String, secs: Long?, direct: Boolean?, bars: Int?,
    onAgain: () -> Unit, onClose: () -> Unit, autoClose: Boolean = true,
) {
    val c = Tin.c
    if (autoClose) LaunchedEffect(Unit) { delay(ENDED_AUTOCLOSE_MS); onClose() }
    Page {
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(10.dp, Alignment.CenterVertically)) {
            PairAvatars(meName.ifBlank { "?" }, name.ifBlank { "Unknown" }, did)
            Text("Call ended", Modifier.padding(top = 16.dp), style = TinType.h1.copy(fontSize = 30.sp, lineHeight = 36.sp), color = c.ink)
            Text(reasonText, style = TinType.bodyL.copy(fontSize = 17.sp), color = c.ink2, textAlign = TextAlign.Center)
            if (secs != null) Row(Modifier.padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Badge(clock(secs), BadgeKind.Mono)
                if (direct == true) Badge("Direct", BadgeKind.Direct)
                if (direct == false) Badge("Relayed", BadgeKind.Relayed)
                if (bars != null) Badge("${qualityLabel(bars)} quality", BadgeKind.Neutral)
            }
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 48.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            TinButton("Call again", onAgain, style = BtnStyle.Outlined)
            TinButton("Close", onClose, style = BtnStyle.Text)
            if (autoClose) Hint("Closes by itself after 4 seconds.", Modifier.fillMaxWidth(), align = TextAlign.Center)
        }
    }
}

@Composable
fun UnreachableContent(name: String, did: String, meName: String, onAgain: () -> Unit, onClose: () -> Unit) {
    val c = Tin.c
    val who = name.ifBlank { "them" }
    Page {
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 28.dp), horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                SelfAvatar(meName.ifBlank { "?" }, 64.dp)
                Box(Modifier.width(36.dp).height(3.dp).background(c.thread.copy(alpha = 0.5f), RoundedCornerShape(2.dp)))
                Box(Modifier.size(96.dp).clip(CircleShape).background(c.sf3), contentAlignment = Alignment.Center) {
                    Text(initialsOf(name), style = TinType.titleL.copy(fontSize = 34.sp), color = c.ink2)
                }
            }
            H1("Couldn’t reach $who", Modifier.padding(top = 16.dp), align = TextAlign.Center)
            Lead("${if (name.isBlank()) "They" else name} may be offline, out of signal, or not taking calls right now. Tinline can’t leave messages.", align = TextAlign.Center)
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 48.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            TinButton("Try again", onAgain)
            TinButton("Close", onClose, style = BtnStyle.Text)
        }
    }
}

/** Before the first call without the microphone: say why and send the user to the permission page. */
@Composable
fun MicNeededScreen(onOpenSettings: () -> Unit, onNotNow: () -> Unit) {
    val c = Tin.c
    Page {
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 28.dp), horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically)) {
            Box(Modifier.size(96.dp).clip(CircleShape).background(c.erc), contentAlignment = Alignment.Center) { Icon(Icons.Rounded.MicOff, null, tint = c.er, modifier = Modifier.size(44.dp)) }
            H1("Tinline can’t use the microphone", Modifier.padding(top = 12.dp), align = TextAlign.Center)
            Lead("Without it, they won’t hear you. Tinline only listens during calls — never in the background.", align = TextAlign.Center)
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 48.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            TinButton("Open settings", onOpenSettings)
            Hint("Then go to Permissions › Microphone and allow it.", Modifier.fillMaxWidth(), align = TextAlign.Center)
            TinButton("Not now", onNotNow, style = BtnStyle.Text)
        }
    }
}
