package com.osvauld.p2p

import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.camera.view.PreviewView
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.compose.LocalLifecycleOwner
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.p2pcore.CallInfo
import uniffi.p2pcore.CallState
import uniffi.p2pcore.videoCodecLabel
import uniffi.p2pcore.videoRecvLabel
import uniffi.p2pcore.videoSendLabel

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

/** "Video" on a ringing video call: the microphone and the camera, then [then]. A refused camera still answers, with it off. */
private fun ComponentActivity.registerMicCameraThen(then: () -> Unit): () -> Unit {
    var pending: (() -> Unit)? = null
    val launcher = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestMultiplePermissions()) {
        pending?.invoke(); pending = null
    }
    return {
        val need = listOf(android.Manifest.permission.RECORD_AUDIO, android.Manifest.permission.CAMERA).filter { !Perms.granted(this, it) }
        if (need.isEmpty()) then()
        else { pending = then; launcher.launch(need.toTypedArray()) }
    }
}

/** Ended screens stay up this long (the "Call ended" board: "Closes by itself after 4 seconds"). */
const val ENDED_AUTOCLOSE_MS = 4000L

class CallActivity : ComponentActivity() {
    private lateinit var answerWithMic: () -> Unit
    private lateinit var answerWithVideo: () -> Unit
    private lateinit var cameraOn: () -> Unit
    /** Android 12+: Nearby devices lets Telecom name Bluetooth headsets and switch between them. */
    private val btAllowed = mutableStateOf(true)

    override fun onStart() {
        super.onStart()
        P2pApp.get(this).calls.callScreenShown = true
        P2pApp.get(this).video.screenShown()
        Notifications.cancelWaiting(this)  // the banner is here now
    }

    /** Message: their chat in the app (it opens once past unlock), and the call screen steps aside to the strip. */
    private fun openChat(did: String) {
        startActivity(Intent(this, MainActivity::class.java).putExtra(ChatNotifier.EXTRA_PEER, did))
        finish()
    }

    override fun onStop() {
        val app = P2pApp.get(this)
        app.calls.callScreenShown = false
        // No capture in the background: the camera pauses with the screen (and resumes with it).
        app.video.screenHidden()
        super.onStop()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        secureWindow()
        enableEdgeToEdge()
        showOverLock()
        val app = P2pApp.get(this)
        answerWithMic = registerMicThen { app.calls.answer() }
        answerWithVideo = registerMicCameraThen { app.calls.answerWithVideo() }
        val cameraLauncher = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) { ok ->
            if (ok) { app.video.setCamera(true); app.calls.speakerForVideo() } else app.video.cameraDenied()
        }
        cameraOn = {
            if (Perms.granted(this, android.Manifest.permission.CAMERA)) { app.video.setCamera(true); app.calls.speakerForVideo() }
            else cameraLauncher.launch(android.Manifest.permission.CAMERA)
        }
        btAllowed.value = Build.VERSION.SDK_INT < 31 || Perms.granted(this, android.Manifest.permission.BLUETOOTH_CONNECT)
        val btLauncher = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) { ok ->
            btAllowed.value = ok
        }
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
                // My own End: its glyph plays, and the call screen stays END_HOLD_MS after the call goes so it is seen
                // (the hang-up itself is sent at once). `last` is the call as it was.
                var last by remember { mutableStateOf<CallUi?>(null) }
                if (ui != null) last = ui
                val tap by app.calls.endTapMs.collectAsState()
                var tick by remember { mutableLongStateOf(0L) }
                // Re-read the clock when the tap, the call or the hold's end changes.
                val sinceTap = remember(tap, ui, tick) { System.currentTimeMillis() - tap }
                // End pressed during this call (a tap from an earlier call is older than this call's first sight).
                val callSeenAt = remember(last?.info?.callId) { System.currentTimeMillis() }
                val ending = tap >= callSeenAt
                val holding = ui == null && last != null && ending && sinceTap < END_HOLD_MS
                LaunchedEffect(tap, ui == null) { if (holding) { delay(END_HOLD_MS - sinceTap); tick++ } }
                // An outgoing call that fails at once is already over when this screen opens: still show why.
                val showEnded = ui == null && e != null && (seen || System.currentTimeMillis() - e.atMs < 10_000)
                LaunchedEffect(ui, seen, showEnded, holding) { if (ui == null && seen && !showEnded && !holding) finish() }
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
                        IncomingContent(u.info.peerName, u.info.peerDid, onDecline = { app.calls.decline() }, onAnswer = { answerWithMic() }, video = u.info.video,
                            onVideo = { answerWithVideo() })
                    u != null || holding -> {
                        val shown = u ?: last!!
                        val allowed by btAllowed
                        InCallScreen(shown, meName, app, onCameraOn = cameraOn, onMinimise = { finish() },
                            onAllowBtNames = if (allowed) null else ({ btLauncher.launch(android.Manifest.permission.BLUETOOTH_CONNECT) }),
                            onMessage = { openChat(shown.info.peerDid) }, ending = ending)
                    }
                    showEnded -> EndedRoute(e, meName, onClose = { finish() }, onAgain = {
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
        val answerWithVideo = registerMicCameraThen {
            app.calls.answerWithVideo()
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
                if (u != null) IncomingContent(u.info.peerName, u.info.peerDid, onDecline = { app.calls.decline(); finish() }, onAnswer = { answerWithMic() }, video = u.info.video,
                    onVideo = { answerWithVideo() })
            }
        }
    }
}

// ------------------------------------------------------------------ incoming (always dark)

@Composable
fun IncomingContent(name: String, did: String, onDecline: () -> Unit, onAnswer: () -> Unit, video: Boolean = false,
    onVideo: (() -> Unit)? = null) {
    val c = Tin.c
    val who = name.ifBlank { "Unknown" }
    CallFocus(who, did, active = false, ringing = true, incoming = true, video = video) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(36.dp, Alignment.CenterHorizontally)) {
            LabeledRound("Decline") { RoundBtn(CallGlyphs.End, "Decline", c.callButtonEnd, Color.White, onDecline, size = 64.dp) }
            LabeledRound("Answer") { RoundBtn(CallGlyphs.Phone, "Answer", c.callButtonAnswer, Color.White, onAnswer, size = 64.dp) }
            // A video call can be answered with my camera on; Answer keeps it off.
            if (video && onVideo != null) LabeledRound("Video") { RoundBtn(CallGlyphs.Video, "Answer with video", c.callButtonAnswer, Color.White, onVideo, size = 64.dp) }
        }
    }
}

// ------------------------------------------------------------------ calling / in call

private fun routeLabel(speaker: Boolean) = if (speaker) "Speaker" else "Phone"

private fun routeIcon(k: AudioRoute.Kind): ImageVector = when (k) {
    AudioRoute.Kind.Earpiece -> PgIcons.Phone
    AudioRoute.Kind.Speaker -> PgIcons.Vol
    AudioRoute.Kind.Bluetooth -> PgIcons.Bluetooth
    AudioRoute.Kind.Wired -> PgIcons.Vol
    AudioRoute.Kind.Other -> PgIcons.Vol
}

/** The speaker slot's caption and icon: "Speaker" until a headset is connected, then the route in use. */
internal fun routeSlot(routes: RouteState?): Pair<String, ImageVector> {
    val cur = routes?.current
    return if (routes?.hasHeadset == true && cur != null && cur.kind != AudioRoute.Kind.Speaker)
        Routes.label(cur) to routeIcon(cur.kind)
    else "Speaker" to CallGlyphs.Speaker
}

private const val HELD = "On hold while your other call is on"

/**
 * WaitingCard.svelte: someone else is calling during my call (core: Live::waiting). A calm card just above the controls:
 * avatar 40, "<name> is calling" over "during your call", Decline and End & answer as two equal 44 dp pills, and what
 * End & answer does. No hold: End & answer ends this call. It settles in 12 dp from above (200 ms) and leaves up 8 dp (140 ms).
 */
@Composable
fun WaitingCard(w: CallInfo?, currentName: String, onDecline: () -> Unit, onEndAnswer: () -> Unit, modifier: Modifier = Modifier) {
    // The last caller stays drawn while the card leaves.
    var last by remember { mutableStateOf(w) }
    if (w != null) last = w
    val px = with(androidx.compose.ui.platform.LocalDensity.current) { 1.dp.toPx() }
    AnimatedVisibility(w != null, modifier,
        enter = fadeIn(tween(200, easing = Motion.EaseOut)) + slideInVertically(tween(200, easing = Motion.EaseOut)) { -(12 * px).toInt() },
        exit = fadeOut(tween(140, easing = Motion.CubicIn)) + slideOutVertically(tween(140, easing = Motion.CubicIn)) { -(8 * px).toInt() }) {
        val l = last ?: return@AnimatedVisibility
        val c = Tin.c
        val who = l.peerName.ifBlank { "Unknown" }
        val first = currentName.trim().substringBefore(' ')
        Column(Modifier.widthIn(max = 360.dp).fillMaxWidth().depth(Depth.Pop, SmoothCornerShape(22.dp)).clip(SmoothCornerShape(22.dp))
            .background(c.sfRaised).border(1.dp, c.ln, SmoothCornerShape(22.dp))
            .padding(start = 14.dp, end = 14.dp, top = 14.dp, bottom = 12.dp)
            .semantics { contentDescription = "$who is calling during your call"; liveRegion = LiveRegionMode.Polite },
            verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Avatar(who, l.peerDid, 40.dp)
                Column(Modifier.weight(1f)) {
                    Text("$who is calling", style = TinFs.body.copy(fontWeight = FontWeight(650), lineHeight = 20.sp), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text("during your call", style = TinFs.sub, color = c.ink2)
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                WaitingPill("Decline", androidx.compose.ui.graphics.lerp(c.sf, c.ink, 0.09f), androidx.compose.ui.graphics.lerp(c.sf, c.ink, 0.15f), c.ink, "Decline", onDecline, Modifier.weight(1f))
                WaitingPill("End & answer", c.callButtonAnswer, c.callButtonAnswer, Color.White, "End and answer", onEndAnswer, Modifier.weight(1f))
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterHorizontally)) {
                Text("Ends your call with", style = TinFs.meta.copy(fontSize = 12.sp, lineHeight = 16.sp), color = c.ink2, maxLines = 1)
                Text(first, Modifier.weight(1f, fill = false), style = TinFs.meta.copy(fontSize = 12.sp, lineHeight = 16.sp), color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
private fun WaitingPill(label: String, rest: Color, wash: Color, fg: Color, desc: String, onClick: () -> Unit, modifier: Modifier) {
    Box(modifier.height(44.dp).pressable(onClick, SmoothCornerShape(22.dp), wash = wash, rest = rest).semantics { contentDescription = desc },
        contentAlignment = Alignment.Center) {
        Text(label, style = TinFs.sub.copy(fontWeight = FontWeight.Bold), color = fg, maxLines = 1)
    }
}

@Composable
fun InCallScreen(ui: CallUi, meName: String, app: P2pApp, onCameraOn: () -> Unit, onMinimise: () -> Unit,
    onAllowBtNames: (() -> Unit)? = null, onMessage: (() -> Unit)? = null,
    /** I pressed End: End's glyph plays (HangupGlyph.kt) while the screen holds. */
    ending: Boolean = false) {
    val calls = app.calls
    val waiting by calls.waiting.collectAsState()
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) { while (true) { delay(500); now = System.currentTimeMillis() } }
    val secs = ((now - (ui.activeSinceMs ?: now)) / 1000).coerceAtLeast(0)
    val s = ui.stats
    val loss = s?.let { ui.lossPct ?: 0.0 }
    val active = ui.state is CallState.Active
    val vi by app.video.info.collectAsState()
    val front by app.video.front.collectAsState()
    val videoProblem by app.video.problem.collectAsState()
    val v = vi?.takeIf { active && it.negotiated && it.callId == ui.info.callId }
    val hdCall by app.video.hdCall.collectAsState()
    val inVideo = v != null && (v.localCamera || v.remoteCamera)
    DisposableEffect(inVideo, ui.info.callId) {
        val stop = if (inVideo) app.video.watchBudget() else ({})
        onDispose(stop)
    }
    val sendStats by produceState<Pair<Pair<Int, Int>, String>?>(null, ui.info.callId, v?.localCamera, hdCall) {
        value = null
        if (v?.localCamera != true) return@produceState
        var initialFrames: ULong? = null
        while (true) {
            value = withContext(Dispatchers.IO) {
                runCatching { app.node.videoStats()?.let {
                    // Sharper video: the core's suggestion (once per call) and the mobile-data note.
                    app.video.hdTick(it, ui.info.peerName)
                    if (initialFrames == null) initialFrames = it.framesSent
                    // Don't label cached dimensions from before this camera start as current HD.
                    if (it.framesSent > initialFrames) (it.sendWidth.toInt() to it.sendHeight.toInt()) to videoSendLabel(it) else null
                } }.getOrNull()
            }
            delay(1000)
        }
    }
    // What I receive ("Their video · 540p · 24 fps"), whether or not my camera is on.
    val recvLabel by produceState<String?>(null, ui.info.callId, v?.negotiated) {
        value = null
        if (v?.negotiated != true) return@produceState
        while (true) {
            value = withContext(Dispatchers.IO) {
                runCatching { app.node.videoStats()?.let { videoRecvLabel(it) } }.getOrNull()
            }?.takeIf { it.isNotEmpty() }
            delay(1000)
        }
    }
    // "VP8 · hardware" / "AV1 · software" per direction, for About this call.
    val codecLabels by produceState<Pair<String, String>?>(null, ui.info.callId, v?.negotiated) {
        value = null
        if (v?.negotiated != true) return@produceState
        while (true) {
            value = withContext(Dispatchers.IO) {
                runCatching { app.node.videoStats()?.let { videoCodecLabel(it.sendCodec, it.sendHardware) to videoCodecLabel(it.recvCodec, it.recvHardware) } }.getOrNull()
            }
            delay(1000)
        }
    }
    val sendSize = sendStats?.first
    val sendLabel = sendStats?.second?.takeIf { it.isNotEmpty() }
    val hdOn = effectivePreferHd(app.preferHdCapture, hdCall)
    val hdIsPaused = hdPaused(hdOn, sendSize?.first ?: 0, sendSize?.second ?: 0)
    // The core suggests sharper video only while it is off: tell it what this call uses.
    LaunchedEffect(hdOn, v?.negotiated, ui.info.callId) { if (v?.negotiated == true) app.video.syncHd(hdOn) }
    // The ladder stepping below HD while HD is on: say so once, when it happens.
    var wasPaused by remember { mutableStateOf(false) }
    LaunchedEffect(hdIsPaused) { if (hdIsPaused && !wasPaused) app.video.say(hdLine(true, true)); wasPaused = hdIsPaused }
    val hdNote by app.video.hdNote.collectAsState()
    LaunchedEffect(hdNote?.id) { hdNote?.let { n -> delay(n.ms); app.video.noteShown(n.id) } }
    val onCamera = { if (v?.localCamera == true) app.video.setCamera(false) else onCameraOn() }
    val name = ui.info.peerName.ifBlank { "Unknown" }
    val bars = if (s != null && loss != null) qualityBars(s.rttMs.toInt(), loss) else null
    Box(Modifier.fillMaxSize()) {
    if (v != null && (v.localCamera || v.remoteCamera)) {
        TinlineTheme(dark = true) {
            VideoCallContent(
                name = name, did = ui.info.peerDid, secs = secs, direct = s?.direct, bars = bars, reconnecting = s?.reconnecting == true,
                muted = ui.muted, speaker = ui.speaker, localCamera = v.localCamera, remoteCamera = v.remoteCamera,
                problem = if (ui.held) HELD else ui.micProblem ?: videoProblem,
                videoQuality = listOfNotNull(if (v.localCamera) hdNote?.text ?: sendLabel ?: "Starting video…" else null, recvLabel).joinToString("   ").ifEmpty { null },
                sendCodec = codecLabels?.first.orEmpty(), recvCodec = codecLabels?.second.orEmpty(),
                hd = hdOn, hdPaused = hdIsPaused, onHd = { app.video.setHd(!hdOn, hdIsPaused) },
                onMute = { calls.toggleMute() }, onSpeaker = { calls.toggleSpeaker() }, onCamera = onCamera,
                onFlip = { app.video.flip() }, onEnd = { calls.hangup() }, onMinimise = onMinimise, onMessage = onMessage,
                remote = { m -> RemoteVideo(app, calls, m) },
                selfView = { m -> SelfView(app, front, m) },
                routes = ui.routes, onRoute = { calls.selectRoute(it) }, onAllowBtNames = onAllowBtNames,
                waitingCard = { m -> WaitingCard(waiting, name, onDecline = { calls.declineWaiting() }, onEndAnswer = { calls.endAndAnswer() }, modifier = m) },
                ending = ending,
            )
        }
    } else InCallContent(
        name = name, did = ui.info.peerDid, meName = meName,
        active = active, ringing = ui.state is CallState.Ringing, secs = secs,
        direct = s?.direct, bars = bars,
        muted = ui.muted, speaker = ui.speaker, micProblem = if (ui.held) HELD else ui.micProblem ?: videoProblem, reconnecting = s?.reconnecting == true,
        onMute = { calls.toggleMute() }, onSpeaker = { calls.toggleSpeaker() }, onEnd = { calls.hangup() }, onMinimise = onMinimise,
        onCamera = if (v != null) onCamera else null, rttMs = s?.rttMs?.toInt(), lossPct = loss,
        routes = ui.routes, onRoute = { calls.selectRoute(it) }, onAllowBtNames = onAllowBtNames, onMessage = onMessage,
        waiting = waiting != null,
        waitingCard = { m -> WaitingCard(waiting, name, onDecline = { calls.declineWaiting() }, onEndAnswer = { calls.endAndAnswer() }, modifier = m) },
        ending = ending,
    )
    if (BuildConfig.VIDEO_TUNING) DebugVideoStats(app, Modifier.align(Alignment.TopCenter).padding(top = 64.dp, start = 8.dp, end = 8.dp))
    }
}

@Composable
fun InCallContent(
    name: String, did: String, meName: String, active: Boolean, ringing: Boolean, secs: Long, direct: Boolean?, bars: Int?,
    muted: Boolean, speaker: Boolean, micProblem: String?,
    onMute: () -> Unit, onSpeaker: () -> Unit, onEnd: () -> Unit, onMinimise: () -> Unit, startWithSheet: Boolean = false,
    reconnecting: Boolean = false,
    /** Set when the call has video: the camera button (the camera is off on this screen). */
    onCamera: (() -> Unit)? = null,
    /** The live routes; with a headset connected the speaker slot opens the route sheet instead of toggling. */
    routes: RouteState? = null, onRoute: (AudioRoute) -> Unit = {},
    /** Set while Nearby devices is not allowed (Android 12+): the sheet offers it when a headset is unnamed. */
    onAllowBtNames: (() -> Unit)? = null,
    /** The numbers About this call shows (round trip, recent loss). */
    rttMs: Int? = null, lossPct: Double? = null, startWithInfo: Boolean = false,
    onMessage: (() -> Unit)? = null,
    /** A second caller is waiting over this call (the field shows its waiting state). */
    waiting: Boolean = false,
    /** The call-waiting card, placed just above the controls (it grows upward; nothing else moves). */
    waitingCard: (@Composable (Modifier) -> Unit)? = null,
    /** I pressed End (HangupGlyph.kt). */
    ending: Boolean = false,
) {
    var routeSheet by remember { mutableStateOf(startWithSheet) }
    var info by remember { mutableStateOf(startWithInfo) }
    val (caption, icon) = routeSlot(routes)
    Box(Modifier.fillMaxSize()) {
        CallFocus(name.ifBlank { "Unknown" }, did, active, ringing, secs, reconnecting, micProblem,
            onMinimise = onMinimise, onInfo = { info = true }, signal = bars ?: 0, direct = direct, onMessage = onMessage, waiting = waiting, muted = muted, waitingCard = waitingCard) {
            CallControls(muted, speaker, camera = false, onMute, { if (routes?.hasHeadset == true) routeSheet = true else onSpeaker() },
                onCamera, onFlip = null, onEnd, caption, icon, ending = ending)
        }
        if (info) CallInfo(name, active, onDismiss = { info = false }, reconnecting = reconnecting, signal = bars ?: 0,
            direct = direct, rttMs = rttMs, lossPct = lossPct)
    }
    if (routeSheet) AudioRouteSheet(speaker, routes, relayed = active && direct == false, onDismiss = { routeSheet = false },
        onPick = { wantSpeaker -> if (wantSpeaker != speaker) onSpeaker(); routeSheet = false },
        onRoute = { onRoute(it); routeSheet = false }, onAllowBtNames = onAllowBtNames)
}

@Composable
private fun AudioRouteSheet(speaker: Boolean, routes: RouteState?, relayed: Boolean, onDismiss: () -> Unit,
    onPick: (speaker: Boolean) -> Unit, onRoute: (AudioRoute) -> Unit, onAllowBtNames: (() -> Unit)? = null) {
    val c = Tin.c
    TinSheet(onDismiss) {
        Text("Play call through", Modifier.padding(start = 8.dp, top = 12.dp, bottom = 8.dp), style = TinType.titleL.copy(fontSize = 20.sp), color = c.ink)
        if (routes != null && routes.available.isNotEmpty()) {
            // Earpiece first, then speaker, then headsets, as a phone's own call screen orders them.
            routes.available.sortedBy { it.kind.ordinal }.forEach { r ->
                RouteOption(routeIcon(r.kind), Routes.label(r), r.key == routes.current?.key) { onRoute(r) }
            }
        } else {
            RouteOption(PgIcons.Phone, "Phone", !speaker) { onPick(false) }
            RouteOption(PgIcons.Vol, "Speaker", speaker) { onPick(true) }
        }
        if (onAllowBtNames != null && routes != null && Routes.unnamedBluetooth(routes)) {
            Hint("Allow nearby devices and Tinline can show your headset’s name and switch between headsets.",
                Modifier.padding(start = 8.dp, end = 8.dp, top = 12.dp))
            TinButton("Show my headset names", onAllowBtNames, Modifier.padding(horizontal = 8.dp, vertical = 8.dp), style = BtnStyle.Tonal)
        }
        if (relayed) Hint("Relayed: a direct line wasn’t possible on this network, so an encrypted relay is passing the call along. It can’t hear you.", Modifier.padding(start = 8.dp, end = 8.dp, top = 12.dp))
    }
}

@Composable
private fun RouteOption(icon: ImageVector, label: String, on: Boolean, onClick: () -> Unit) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().heightIn(min = 60.dp).clip(SmoothCornerShape(14.dp)).background(if (on) c.prc else Color.Transparent)
            .clickable(role = Role.RadioButton, onClick = onClick).padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(icon, null, tint = if (on) c.onPrc else c.ink, modifier = Modifier.padding(start = 8.dp))
        Text(label, Modifier.weight(1f), style = TinType.bodyL, color = if (on) c.onPrc else c.ink)
        if (on) Icon(PgIcons.Check, "Selected", tint = c.onPrc, modifier = Modifier.padding(end = 8.dp))
    }
}

// ------------------------------------------------------------------ video call

private val VideoBg = Color(0xFF0B100F)

/**
 * In-call screen with video: the peer's picture full screen (their avatar while their camera is
 * off), our camera small in the corner, controls over a scrim. Always dark.
 */
@Composable
fun VideoCallContent(
    name: String, did: String, secs: Long, direct: Boolean?, bars: Int?, reconnecting: Boolean,
    muted: Boolean, speaker: Boolean, localCamera: Boolean, remoteCamera: Boolean, problem: String?,
    onMute: () -> Unit, onSpeaker: () -> Unit, onCamera: () -> Unit, onFlip: () -> Unit, onEnd: () -> Unit, onMinimise: () -> Unit,
    remote: @Composable (Modifier) -> Unit, selfView: @Composable (Modifier) -> Unit,
    routes: RouteState? = null, onRoute: (AudioRoute) -> Unit = {}, onAllowBtNames: (() -> Unit)? = null,
    onMessage: (() -> Unit)? = null,
    waitingCard: (@Composable (Modifier) -> Unit)? = null,
    videoQuality: String? = null, sendCodec: String = "", recvCodec: String = "", hd: Boolean = false, hdPaused: Boolean = false, onHd: () -> Unit = {},
    /** I pressed End (HangupGlyph.kt). */
    ending: Boolean = false,
) {
    val c = Tin.c
    var info by remember { mutableStateOf(false) }
    var routeSheet by remember { mutableStateOf(false) }
    val (caption, icon) = routeSlot(routes)
    Box(Modifier.fillMaxSize().background(VideoBg)) {
        if (remoteCamera) remote(Modifier.fillMaxSize())
        else Column(Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically)) {
            Avatar(name, did, 120.dp)
            Text("$name’s camera is off", style = TinType.bodyL.copy(fontSize = 17.sp), color = c.ink2)
        }
        // Top: scrim, minimise, name and time, path badges.
        Column(Modifier.fillMaxWidth().background(Brush.verticalGradient(listOf(Color.Black.copy(alpha = 0.55f), Color.Transparent)))
            .statusBarsPadding().padding(bottom = 24.dp)) {
            Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 12.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                IconBtn(PgIcons.Back, "Back. The call continues", onMinimise, tint = Color.White)
                Column(Modifier.weight(1f)) {
                    Text(name, style = TinType.titleM, color = Color.White, maxLines = 1)
                    Text(if (reconnecting) "Reconnecting…" else clock(secs), style = TinType.mono.copy(fontSize = 14.sp), color = Color.White.copy(alpha = 0.8f))
                }
                if (onMessage != null) IconBtn(PgIcons.Chat, "Message. The call continues", onMessage, tint = Color.White)
                Box(Modifier.size(44.dp).clip(CircleShape).clickable(role = Role.Button) { info = true }
                    .semantics { contentDescription = if (bars != null && !reconnecting) "About this call. ${signalWord(bars)} signal, ${if (direct == true) "direct" else "relayed"}." else "About this call" },
                    contentAlignment = Alignment.Center) {
                    SignalBars(if (reconnecting) 0 else bars ?: 0, Color.White, wave = reconnecting)
                }
            }
            // One line, always: a longer line (a suggestion) is cut, never wrapped, so nothing below it moves.
            videoQuality?.let { Text(it, Modifier.padding(start = 52.dp, end = 12.dp), style = TinType.mono.copy(fontSize = 12.sp),
                color = Color.White.copy(alpha = 0.8f), maxLines = 1, overflow = TextOverflow.Ellipsis) }
        }
        if (info) CallInfo(name, active = true, onDismiss = { info = false }, reconnecting = reconnecting, signal = bars ?: 0,
            direct = direct, video = true,
            sendCodec = if (localCamera) sendCodec else "", recvCodec = if (remoteCamera) recvCodec else "")
        // Self-view: our camera (the front one mirrored), top right under the bar.
        if (localCamera) Box(Modifier.align(Alignment.TopEnd).statusBarsPadding().padding(top = 72.dp, end = 12.dp)
            .size(width = 104.dp, height = 140.dp).clip(SmoothCornerShape(16.dp)).background(Color.Black)
            .border(1.dp, Color.White.copy(alpha = 0.25f), SmoothCornerShape(16.dp))) {
            selfView(Modifier.fillMaxSize())
            HdChip(hd, hdPaused, onHd, Modifier.align(Alignment.TopStart))
        }
        // Bottom: problem line and controls.
        var barH by remember { mutableStateOf(0) }
        Column(Modifier.align(Alignment.BottomCenter).fillMaxWidth().onSizeChanged { barH = it.height }
            .background(Brush.verticalGradient(listOf(Color.Transparent, Color.Black.copy(alpha = 0.6f))))
            .navigationBarsPadding().padding(top = 32.dp, bottom = 28.dp), horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text(problem ?: " ", Modifier.padding(horizontal = 24.dp).fillMaxWidth().heightIn(min = 38.dp),
                style = TinType.bodyM, color = c.er, maxLines = 2, textAlign = TextAlign.Center)
            Box(Modifier.padding(horizontal = 20.dp)) {
                CallControls(muted, speaker, localCamera, onMute, { if (routes?.hasHeadset == true) routeSheet = true else onSpeaker() },
                    onCamera, onFlip, onEnd, caption, icon, ending = ending)
            }
        }
        // WaitingCard: 22 dp above the controls bar, growing upward.
        waitingCard?.invoke(Modifier.align(Alignment.BottomCenter).padding(start = 20.dp, end = 20.dp,
            bottom = with(androidx.compose.ui.platform.LocalDensity.current) { barH.toDp() } + 22.dp))
    }
    if (routeSheet) AudioRouteSheet(speaker, routes, relayed = direct == false, onDismiss = { routeSheet = false },
        onPick = { wantSpeaker -> if (wantSpeaker != speaker) onSpeaker(); routeSheet = false },
        onRoute = { onRoute(it); routeSheet = false }, onAllowBtNames = onAllowBtNames)
}

/** The peer's decoded frames, pulled on the display clock by a [RemoteRenderer] while shown. */
@Composable
internal fun RemoteVideo(app: P2pApp, calls: CallController, modifier: Modifier) {
    var frame by remember { mutableStateOf<android.graphics.Bitmap?>(null) }
    var n by remember { mutableLongStateOf(0L) }
    DisposableEffect(Unit) {
        val r = RemoteRenderer(app.video.io) { b -> frame = b; n++ }
        calls.renderer = r
        onDispose { r.stop(); if (calls.renderer === r) calls.renderer = null }
    }
    // The picture's drawn size for the peer's sender; hidden while the screen is stopped.
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var viewSize by remember { mutableStateOf(IntSize.Zero) }
    var started by remember { mutableStateOf(lifecycle.currentState.isAtLeast(androidx.lifecycle.Lifecycle.State.STARTED)) }
    DisposableEffect(lifecycle) {
        val o = androidx.lifecycle.LifecycleEventObserver { _, _ ->
            started = lifecycle.currentState.isAtLeast(androidx.lifecycle.Lifecycle.State.STARTED)
        }
        lifecycle.addObserver(o)
        onDispose { lifecycle.removeObserver(o) }
    }
    LaunchedEffect(viewSize, started) { app.video.setView(viewSize.width, viewSize.height, started) }
    val latestSize by rememberUpdatedState(viewSize)
    DisposableEffect(Unit) { onDispose { app.video.setView(latestSize.width, latestSize.height, false) } }
    val modifier = modifier.onSizeChanged { viewSize = it }
    val b = frame
    if (b == null) Box(modifier, contentAlignment = Alignment.Center) {
        Text("Connecting video…", style = TinType.bodyL, color = Color.White.copy(alpha = 0.7f))
    } else Canvas(modifier) {
        // Read every frame: the two bitmaps alternate, so the object alone doesn't always change.
        if (n < 0) return@Canvas
        val img = b.asImageBitmap()
        // Fill the screen, cropping the picture's overflow and keeping its aspect.
        val scale = maxOf(size.width / img.width, size.height / img.height)
        val w = (img.width * scale).toInt()
        val h = (img.height * scale).toInt()
        drawImage(img, dstOffset = IntOffset((size.width.toInt() - w) / 2, (size.height.toInt() - h) / 2), dstSize = IntSize(w, h))
    }
}

/**
 * The HD chip (playground HdChip.svelte): "HD", 24 dp tall, radius 7, 1.5 dp outline. Off = outline, on = filled (white over
 * video), paused = on but dimmed to 55% while the sender is below the HD size. The 48 dp square is the touch target.
 */
@Composable
internal fun HdChip(on: Boolean, paused: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val label = if (paused) "Sharper video on, paused while the connection is weak" else if (on) "Sharper video on, up to 540p" else "Sharper video off"
    val a = if (paused) 0.55f else 1f
    Box(modifier.size(48.dp).clickable(role = Role.Switch, onClick = onClick)
        .semantics { contentDescription = label; stateDescription = if (on) "On" else "Off" }, contentAlignment = Alignment.Center) {
        val shape = RoundedCornerShape(7.dp)
        Box(Modifier.height(24.dp).widthIn(min = 36.dp).clip(shape)
            .background(if (on) Color.White.copy(alpha = a) else Color.Transparent)
            .border(1.5.dp, Color.White.copy(alpha = a), shape).padding(horizontal = 7.dp), contentAlignment = Alignment.Center) {
            Text("HD", style = TinType.mono.copy(fontSize = 12.sp, fontWeight = FontWeight.Bold, letterSpacing = 0.5.sp),
                color = if (on) Color(0xFF111111).copy(alpha = a) else Color.White.copy(alpha = a))
        }
    }
}

/** Our camera: CameraX preview (mirrored for the front camera) plus the analysis feed to the core. */
@Composable
private fun SelfView(app: P2pApp, front: Boolean, modifier: Modifier) {
    val hdChoice by app.video.hdCall.collectAsState()
    val ctx = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val view = remember {
        PreviewView(ctx).apply {
            implementationMode = PreviewView.ImplementationMode.COMPATIBLE
            scaleType = PreviewView.ScaleType.FILL_CENTER
        }
    }
    // A new HD choice restarts the capture at once (the picture warms up for a moment).
    DisposableEffect(front, owner, hdChoice) {
        app.video.startCamera(owner, view.surfaceProvider, front)
        onDispose { app.video.stopCamera() }
    }
    AndroidView({ view }, modifier)
}

// ------------------------------------------------------------------ ended / couldn't reach / mic needed

@Composable
private fun EndedRoute(e: EndedUi, meName: String, onClose: () -> Unit, onAgain: () -> Unit) {
    if (classifyEnd(e.reason) == EndKind.Unreachable && !e.wasActive) UnreachableContent(e.peerName, e.peerDid, meName, onAgain, onClose)
    else EndedContent(e.peerName, e.peerDid, meName, endReasonText(e.reason, e.peerName, e.incoming, P2pApp.instance.blocks.value.blockedByMe(e.peerDid)), if (e.wasActive) e.secs else null, e.direct, e.bars, onAgain, onClose)
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
                if (bars != null) Badge("${qualityLabel(bars)} signal", BadgeKind.Neutral)
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
            Box(Modifier.size(96.dp).clip(CircleShape).background(c.erc), contentAlignment = Alignment.Center) { Icon(PgIcons.Micoff, null, tint = c.er, modifier = Modifier.size(44.dp)) }
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
