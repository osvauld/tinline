package com.osvauld.p2p

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.LinkEvents

/**
 * Device linking (docs/design/device-linking.md): one link at a time, driven by the core's [LinkEvents].
 * [ui] is the state the screens render; [newDevice] says which side this phone is (N = new, shows or
 * scans with `link_new_*`; E = existing, `link_*`). Core callbacks arrive on core threads: they only
 * set flows or hop to [P2pApp.scope].
 */
class LinkController(private val app: P2pApp) : LinkEvents {
    sealed interface Ui {
        data object Idle : Ui
        /** Our QR is on screen until [expiresAt] (epoch ms). */
        data class Showing(val qr: String, val expiresAt: Long) : Ui
        /** QR read or scanned; waiting for the other device. */
        data object Connecting : Ui
        /** Both ends proved the QR: compare [code] (E approves, N waits). */
        data class Code(val code: String, val peer: String) : Ui
        /** E approved; the grant is on its way. */
        data class Approving(val code: String, val peer: String) : Ui
        data class Done(val did: String, val name: String, val peer: String) : Ui
        data class Failed(val reason: String, val detail: String? = null) : Ui
    }

    private val _ui = MutableStateFlow<Ui>(Ui.Idle)
    val ui: StateFlow<Ui> = _ui
    @Volatile var newDevice = false
        private set
    @Volatile private var label = ""
    @Volatile private var peer = ""

    private val _devices = MutableStateFlow(0)
    /** Bumped when the registry changed (linked, renamed, unlinked, seen): lists re-read `linkedDevices()`. */
    val devicesVersion: StateFlow<Int> = _devices

    /** Link operations run one at a time, in order, off the callers' threads. */
    private val ops = java.util.concurrent.Executors.newSingleThreadExecutor { r -> Thread(r, "p2p-link") }

    private fun supersede() {
        val busy = _ui.value !is Ui.Idle && _ui.value !is Ui.Done
        _ui.value = Ui.Idle
        if (busy) ops.execute { runCatching { app.node.linkCancel() } }
    }

    /** Shows our QR (E: `link_show_qr`; N: `link_new_show_qr` with this install's name). Replaces any link in progress. */
    fun showQr(new: Boolean, deviceLabel: String) {
        newDevice = new; label = deviceLabel; peer = ""
        supersede()
        ops.execute {
            try {
                val qr = if (new) app.node.linkNewShowQr(deviceLabel) else app.node.linkShowQr()
                _ui.value = Ui.Showing(qr, System.currentTimeMillis() + QR_LIFETIME_MS)
                testLog("linkqr=$qr")
            } catch (e: Exception) { _ui.value = Ui.Failed("error", friendly(e)) }
        }
    }

    /** The other device's QR text (camera, or the debug hook). */
    fun scan(text: String, new: Boolean, deviceLabel: String) {
        newDevice = new; label = deviceLabel; peer = ""
        supersede()
        _ui.value = Ui.Connecting
        ops.execute {
            try {
                if (new) app.node.linkNewScan(text.trim(), deviceLabel) else app.node.linkScan(text.trim())
            } catch (e: Exception) { _ui.value = Ui.Failed("error", friendly(e)) }
        }
    }

    /** E: the codes match. A wrong passphrase throws and keeps the link open. Blocking (Argon2). */
    fun approve(passphrase: String?) {
        val s = _ui.value as? Ui.Code ?: return
        app.node.linkApprove(passphrase)
        if (_ui.value is Ui.Code) _ui.value = Ui.Approving(s.code, s.peer)
    }

    /** Abandons whatever is in progress and goes back to Idle. */
    fun cancel() {
        val busy = _ui.value !is Ui.Idle && _ui.value !is Ui.Done
        _ui.value = Ui.Idle
        if (busy) ops.execute { runCatching { app.node.linkCancel() } }
    }

    fun reset() { _ui.value = Ui.Idle }

    // ---- LinkEvents (core threads) ----
    override fun onLinkCode(code: String, peerLabel: String) {
        peer = peerLabel
        testLog("linkcode=$code peer=$peerLabel")
        _ui.value = Ui.Code(code, peerLabel)
    }

    override fun onLinkDone(did: String, name: String) {
        testLog("linkdone did=$did name=$name")
        _ui.value = Ui.Done(did, name, peer)
        _devices.value++
    }

    override fun onLinkFailed(reason: String) {
        testLog("linkfailed=$reason")
        if (_ui.value is Ui.Done) return
        _ui.value = Ui.Failed(reason)
    }

    override fun onDevicesChanged() { _devices.value++ }
    override fun onHistoryChanged() { app.scope.launch { app.refreshHistory() } }
    override fun onUnlinked(did: String) {
        testLog("unlinked did=$did")
        app.scope.launch { app.accountGone(did, notify = true) }
    }

    companion object { const val QR_LIFETIME_MS = 5 * 60 * 1000L }
}

/** What to tell the user for a failed link ([LinkEvents.onLinkFailed] reasons). */
fun linkFailText(reason: String, detail: String?): String = when (reason) {
    "expired" -> "The code expired. Try again."
    "cancelled" -> "Cancelled on the other device."
    "rejected" -> "That QR code was already used or isn’t valid. Make a fresh one and try again."
    "bad_proof" -> "The other device didn’t accept that QR code. Make a fresh one and try again."
    "mismatch" -> "These two devices don’t belong to the same account, so nothing was shared."
    "timeout" -> "The other device didn’t answer in time. Keep both open and try again."
    "net" -> "Couldn’t reach the other device. Check that both are online."
    "locked" -> "The other device is locked. Unlock Tinline there and try again."
    "busy" -> "Tinline was busy with something else. Try again in a moment."
    "bad_grant" -> "The other device sent something this app couldn’t read. Update Tinline on both and try again."
    "account_exists" -> "This account is already on this phone."
    else -> detail?.takeIf { it.isNotBlank() } ?: "Couldn’t link. Try again."
}

/** Null if [text] is a device-link QR; otherwise why not (a contact code gets its own message). */
fun linkQrProblem(text: String): String? {
    val t = text.trim()
    return when {
        t.startsWith("OSVL1:") -> null
        t.startsWith("OSVC") -> "That’s a contact code, not a device-link code. Use Add contact for those."
        else -> "That isn’t a Tinline device-link code."
    }
}

// ------------------------------------------------------------------ the flow (both sides)

private enum class LinkStage { Intro, Scan, Show }

/**
 * Link a device, one side of it. [newDevice] = false: this phone has the account (Settings › Link a
 * device); true: this phone is the new one (onboarding). [label] is the new device's name. The screen
 * follows [LinkController.ui]; [onClose] leaves, [onDone] is called once on the new device when the
 * account has arrived (the existing device shows its own "Linked" screen).
 */
@Composable
fun LinkSession(app: P2pApp, newDevice: Boolean, label: String, onClose: () -> Unit, onDone: (LinkController.Ui.Done) -> Unit = {}) {
    val links = app.links
    val ui by links.ui.collectAsState()
    val scope = rememberCoroutineScope()
    var stage by rememberSaveable { mutableStateOf(if (newDevice) LinkStage.Show else LinkStage.Intro) }
    var scanNote by remember { mutableStateOf<String?>(null) }
    var scanNonce by remember { mutableIntStateOf(0) }
    var regen by remember { mutableIntStateOf(0) }
    DisposableEffect(Unit) { links.reset(); onDispose { links.cancel() } }

    fun onText(text: String) {
        val bad = linkQrProblem(text)
        if (bad != null) { scanNote = bad; return }
        scanNote = null
        links.scan(text, newDevice, label)
    }
    fun showQr() = links.showQr(newDevice, label)
    LaunchedEffect(stage, regen) { if (stage == LinkStage.Show && links.ui.value is LinkController.Ui.Idle) showQr() }
    LaunchedEffect(ui) { (ui as? LinkController.Ui.Done)?.let { if (newDevice) onDone(it) } }
    fun again() { links.reset(); scanNote = null; scanNonce++; regen++; if (!newDevice) stage = LinkStage.Intro }

    when (val u = ui) {
        is LinkController.Ui.Code -> if (newDevice) LinkWaitScreen(u.code, u.peer) { links.cancel(); again() }
            else LinkApproveScreen(app, u.code, u.peer, onCancel = { links.cancel() })
        is LinkController.Ui.Approving -> LinkBusyScreen("Linking ${u.peer.ifBlank { "the new device" }}…", "Sending your account over. Keep both devices open.")
        is LinkController.Ui.Connecting -> LinkBusyScreen("Connecting…", "Reaching the other device. Keep both open.") { links.cancel(); again() }
        is LinkController.Ui.Done -> if (newDevice) LinkBusyScreen("Linked", "Saving your account on this phone…")
            else LinkDoneScreen(u.peer, onClose)
        is LinkController.Ui.Failed -> LinkFailedScreen(u.reason, u.detail, onRetry = { again() }, onClose = onClose)
        is LinkController.Ui.Showing, LinkController.Ui.Idle -> when (stage) {
            LinkStage.Intro -> LinkIntroScreen(onBack = onClose, onScan = { stage = LinkStage.Scan }, onShow = { stage = LinkStage.Show })
            LinkStage.Scan -> LinkScanScreen(newDevice, scanNote, scanNonce, onBack = { if (newDevice) onClose() else { scanNote = null; stage = LinkStage.Intro } },
                onText = ::onText, onRetry = { scanNote = null; scanNonce++ },
                onShow = { scanNote = null; stage = LinkStage.Show })
            LinkStage.Show -> LinkShowScreen(newDevice, (u as? LinkController.Ui.Showing), onBack = { links.cancel(); if (newDevice) onClose() else stage = LinkStage.Intro },
                onNew = { links.cancel(); regen++ }, onScanInstead = { links.cancel(); stage = LinkStage.Scan })
        }
    }
}

// ------------------------------------------------------------------ screens

@Composable
private fun LinkIntroScreen(onBack: () -> Unit, onScan: () -> Unit, onShow: () -> Unit) {
    val c = Tin.c
    Page {
        TopBar("Link a device", onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Box(Modifier.size(64.dp).clip(androidx.compose.foundation.shape.CircleShape).background(c.prc), contentAlignment = Alignment.Center) {
                Icon(Icons.Rounded.AddLink, null, tint = c.onPrc, modifier = Modifier.size(30.dp))
            }
            H1("Use Tinline on another device")
            Lead("Your contacts, chats and call history stay in step on every device you link.")
            LinkStep(1, "On the new device, install Tinline and choose “Link to an existing account”.")
            LinkStep(2, "It shows a QR code. Scan it from this phone.")
            LinkStep(3, "Check both screens show the same 6-digit code, then approve here.")
            InfoCard("A linked device becomes you: it can call, chat and see your recovery phrase. Only link devices you own.",
                icon = Icons.Rounded.Warning, kind = BannerKind.Warn)
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp, top = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            TinButton("Scan QR code", onScan, icon = Icons.Rounded.QrCodeScanner)
            TinButton("Show a QR code here instead", onShow, style = BtnStyle.Text)
        }
    }
}

@Composable
private fun LinkStep(n: Int, text: String) {
    val c = Tin.c
    Row(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.Top) {
        Box(Modifier.size(28.dp).clip(androidx.compose.foundation.shape.CircleShape).background(c.sf3), contentAlignment = Alignment.Center) {
            Text("$n", style = TinType.label, color = c.ink)
        }
        Text(text, Modifier.weight(1f).padding(top = 3.dp), style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 22.sp), color = c.ink)
    }
}

@Composable
private fun LinkScanScreen(newDevice: Boolean, note: String?, nonce: Int, onBack: () -> Unit, onText: (String) -> Unit, onRetry: () -> Unit, onShow: () -> Unit) {
    var granted by remember { mutableStateOf(true) }
    var torch by remember { mutableStateOf(false) }
    TinlineTheme(dark = true) {
        Page(Modifier.background(Color(0xFF050807))) {
            TopBar(if (newDevice) "Scan instead" else "Scan the code", onBack) {
                if (granted) IconBtn(if (torch) Icons.Rounded.FlashlightOn else Icons.Rounded.FlashlightOff, "Torch", { torch = !torch })
            }
            Box(Modifier.weight(1f).fillMaxWidth().padding(top = 8.dp)) {
                key(nonce) { QrCameraBox(Modifier.fillMaxSize(), if (newDevice) "Point at the code on your other device" else "Point at the QR code on your other device", torch, { granted = it }, onText) }
                if (note != null) Column(
                    Modifier.align(Alignment.Center).padding(24.dp).clip(RoundedCornerShape(16.dp)).background(Color(0xE6101614)).padding(20.dp),
                    horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Icon(Icons.Rounded.ErrorOutline, null, tint = Color(0xFFFFB4AB), modifier = Modifier.size(32.dp))
                    Text(note, style = TinType.bodyL, color = Color.White, textAlign = TextAlign.Center)
                    TinButton("Scan again", onRetry, fill = false)
                }
            }
            Column(Modifier.padding(start = 24.dp, end = 24.dp, top = 16.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Hint("The code works once and expires in 5 minutes.", Modifier.fillMaxWidth(), align = TextAlign.Center)
                if (!newDevice) TinButton("Show a QR code here instead", onShow, style = BtnStyle.Text)
            }
        }
    }
}

@Composable
private fun LinkShowScreen(newDevice: Boolean, showing: LinkController.Ui.Showing?, onBack: () -> Unit, onNew: () -> Unit, onScanInstead: () -> Unit) {
    val c = Tin.c
    val qr = showing?.qr
    var left by remember(qr) { mutableLongStateOf(showing?.let { (it.expiresAt - System.currentTimeMillis()).coerceAtLeast(0) } ?: 0L) }
    LaunchedEffect(qr) {
        while (qr != null && left > 0) { delay(1000); left = (showing.expiresAt - System.currentTimeMillis()).coerceAtLeast(0) }
    }
    val expired = qr != null && left <= 0
    Page {
        TopBar(if (newDevice) "Link to your account" else "Show a QR code", onBack)
        Column(
            Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp, vertical = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                if (newDevice) "On a device that already has your account, open Settings › Link a device and scan this."
                else "On the new device choose “Link to an existing account”, then “Scan instead”, and point it at this code.",
                style = TinType.bodyL, color = c.ink2, textAlign = TextAlign.Center,
            )
            CardBox(Modifier.fillMaxWidth(), radius = 24.dp) {
                Column(Modifier.padding(20.dp).fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    Box(Modifier.size(248.dp).clip(RoundedCornerShape(14.dp)).background(Color.White), contentAlignment = Alignment.Center) {
                        if (qr != null) {
                            val bmp = remember(qr) { qrBitmap(qr, 720, fg = 0xFF17201D.toInt()) }
                            Image(bmp.asImageBitmap(), "QR code to link a device", Modifier.fillMaxSize().padding(10.dp).let { if (expired) it.background(Color.White.copy(alpha = 0.9f)) else it },
                                alpha = if (expired) 0.12f else 1f)
                            if (expired) Text("Expired", style = TinType.titleM, color = Color(0xFF17201D))
                        } else CircularDots()
                    }
                    Text(
                        when { qr == null -> "Preparing your code…"; expired -> "Expired"; else -> "Expires in ${left / 60000}:${"%02d".format(left / 1000 % 60)} · works once" },
                        style = TinType.bodyM, color = if (expired) c.er else c.ink2,
                    )
                }
            }
            TinButton(if (expired) "Make a new code" else "New code", onNew, style = if (expired) BtnStyle.Filled else BtnStyle.Text, icon = Icons.Rounded.Refresh, enabled = qr != null)
            Hint(if (newDevice) "Other device is a phone? It can show the code and this phone scans it." else "Anyone who can see this code in the next minutes could try to use it. Show it only to your own device.",
                align = TextAlign.Center)
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp)) {
            if (newDevice) TinButton("Scan instead", onScanInstead, style = BtnStyle.Outlined, icon = Icons.Rounded.QrCodeScanner)
            else TinButton("Scan a code instead", onScanInstead, style = BtnStyle.Outlined, icon = Icons.Rounded.QrCodeScanner)
        }
    }
}

@Composable
private fun CircularDots() { LinearProgressIndicator(Modifier.width(120.dp), color = Tin.c.pr, trackColor = Tin.c.sf3) }

@Composable
private fun BigCode(code: String) {
    val c = Tin.c
    Box(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(c.sf).border(1.dp, c.ln, RoundedCornerShape(16.dp)).padding(vertical = 22.dp), contentAlignment = Alignment.Center) {
        Text(code, style = TinType.mono.copy(fontSize = 44.sp, lineHeight = 52.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 4.sp), color = c.ink)
    }
}

/** E: compare the code, then approve with the passphrase (or the phone's screen lock when there is none). */
@Composable
private fun LinkApproveScreen(app: P2pApp, code: String, peer: String, onCancel: () -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val hasPass = remember { app.node.hasPassphrase() }
    var pass by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val who = peer.ifBlank { "the new device" }
    fun approve() {
        busy = true; error = null
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { app.links.approve(if (hasPass) pass else null) } }
            busy = false
            r.onFailure { error = friendly(it) }
            r.onSuccess { pass = "" }
        }
    }
    fun go() {
        val act = ctx.findActivity()
        if (hasPass || act == null) approve()
        else DeviceAuth.ask(act, "Link a new device") { ok -> if (ok) approve() else error = "Screen lock check cancelled" }
    }
    Page {
        TopBar("Link “$who”?", onCancel)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Lead("Make sure the other device shows this same code.")
            BigCode(code)
            InfoCard("Different code, or you didn’t start this? Cancel. Someone may be trying to copy your account.", icon = Icons.Rounded.Warning, kind = BannerKind.Warn)
            if (hasPass) TinField(pass, { pass = it; error = null }, "Your passphrase", mono = true, password = true, enabled = !busy,
                state = if (error != null) FieldState.Error else FieldState.Normal, hint = error ?: "Linking sends your recovery phrase, so it’s checked first.",
                keyboard = KeyboardOptions(imeAction = ImeAction.Done), actions = KeyboardActions(onDone = { if (pass.isNotEmpty() && !busy) go() }))
            else {
                Hint("No passphrase set? Your screen lock is asked for instead.")
                error?.let { Text(it, style = TinType.bodyM, color = c.er) }
            }
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth(), color = c.pr, trackColor = c.sf3)
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp, top = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            TinButton("Codes match, link it", ::go, enabled = !busy && (!hasPass || pass.isNotEmpty()))
            TinButton("Cancel", onCancel, style = BtnStyle.Text, enabled = !busy)
        }
    }
}

/** N: our code, while the existing device approves. */
@Composable
private fun LinkWaitScreen(code: String, peer: String, onCancel: () -> Unit) {
    Page {
        TopBar("Check the code", onCancel)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            H1("Same code on both?")
            Lead("Check ${peer.ifBlank { "your other device" }} shows this code. Approve there.")
            BigCode(code)
            InfoCard("Different code? Cancel here. Someone else may have scanned the QR.", icon = Icons.Rounded.Warning, kind = BannerKind.Warn)
            LinearProgressIndicator(Modifier.fillMaxWidth(), color = Tin.c.pr, trackColor = Tin.c.sf3)
            Hint("Waiting for approval…")
        }
        Box(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp)) { TinButton("Cancel", onCancel, style = BtnStyle.Text) }
    }
}

@Composable
private fun LinkBusyScreen(title: String, body: String, onCancel: (() -> Unit)? = null) {
    Page {
        Column(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically)) {
            H1(title, align = TextAlign.Center)
            Lead(body, Modifier.widthIn(max = 320.dp), align = TextAlign.Center)
            LinearProgressIndicator(Modifier.width(200.dp), color = Tin.c.pr, trackColor = Tin.c.sf3)
        }
        if (onCancel != null) Box(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp)) { TinButton("Cancel", onCancel, style = BtnStyle.Text) }
    }
}

@Composable
private fun LinkDoneScreen(peer: String, onClose: () -> Unit) {
    val c = Tin.c
    Page {
        Column(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically)) {
            Box(Modifier.size(72.dp).clip(androidx.compose.foundation.shape.CircleShape).background(c.prc), contentAlignment = Alignment.Center) {
                Icon(Icons.Rounded.Check, null, tint = c.onPrc, modifier = Modifier.size(36.dp))
            }
            H1("Linked ${peer.ifBlank { "the new device" }}", align = TextAlign.Center)
            Lead("It now has your account and is bringing your contacts and history over. Keep both devices online for a minute.", Modifier.widthIn(max = 320.dp), align = TextAlign.Center)
        }
        Box(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp)) { TinButton("Done", onClose) }
    }
}

@Composable
private fun LinkFailedScreen(reason: String, detail: String?, onRetry: () -> Unit, onClose: () -> Unit) {
    Page {
        Column(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically)) {
            Box(Modifier.size(72.dp).clip(androidx.compose.foundation.shape.CircleShape).background(Tin.c.erc), contentAlignment = Alignment.Center) {
                Icon(Icons.Rounded.LinkOff, null, tint = Tin.c.er, modifier = Modifier.size(34.dp))
            }
            H1("Couldn’t link", align = TextAlign.Center)
            Lead(linkFailText(reason, detail), Modifier.widthIn(max = 330.dp), align = TextAlign.Center)
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            TinButton("Try again", onRetry)
            TinButton("Not now", onClose, style = BtnStyle.Text)
        }
    }
}

// ------------------------------------------------------------------ new device: syncing, unlinked

/** N, after the account arrived: what is coming over. Core reports no progress, so this is indeterminate. */
@Composable
fun SyncingScreen(name: String, peer: String, contacts: Int, calls: Int, onStart: () -> Unit) {
    val c = Tin.c
    var slow by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { delay(45_000); slow = true }
    StepFrame(null, null, footer = {
        TinButton("Start using Tinline", onStart)
        Hint("Sync keeps going in the background.", Modifier.fillMaxWidth(), align = TextAlign.Center)
    }) {
        Box(Modifier.size(64.dp).clip(androidx.compose.foundation.shape.CircleShape).background(c.prc), contentAlignment = Alignment.Center) {
            Icon(Icons.Rounded.Sync, null, tint = c.onPrc, modifier = Modifier.size(30.dp))
        }
        H1(if (name.isNotBlank()) "Linked to $name" else "Linked")
        Lead(if (peer.isNotBlank()) "Bringing your things over from $peer." else "Bringing your things over.")
        CardBox(Modifier.fillMaxWidth()) {
            SyncRow(Icons.Rounded.People, "Contacts", if (contacts > 0) "$contacts" else "Waiting", contacts > 0)
            SyncRow(Icons.Rounded.History, "Call history", if (calls > 0) "$calls" else "Waiting", calls > 0)
        }
        if (contacts == 0 && calls == 0) LinearProgressIndicator(Modifier.fillMaxWidth(), color = c.pr, trackColor = c.sf3)
        Hint(if (slow) "Nothing yet. The other device may be offline: it will pick up where it left off the next time both are online."
        else "Keep both devices online. If one drops off, it picks up where it left off next time they’re both online.")
    }
}

@Composable
private fun SyncRow(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, value: String, done: Boolean) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        Icon(icon, null, tint = c.ink2)
        Text(title, Modifier.weight(1f), style = TinType.bodyL, color = c.ink)
        if (done) Icon(Icons.Rounded.Check, null, tint = c.pr, modifier = Modifier.size(18.dp))
        Text(value, style = TinType.label, color = if (done) c.pr else c.ink2)
    }
}

/** "This phone was unlinked": another device removed the account; the core already deleted it here. */
@Composable
fun UnlinkedScreen(name: String, onOk: () -> Unit, onLinkAgain: () -> Unit) {
    val c = Tin.c
    Page {
        Column(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically)) {
            Box(Modifier.size(72.dp).clip(androidx.compose.foundation.shape.CircleShape).background(c.erc), contentAlignment = Alignment.Center) {
                Icon(Icons.Rounded.LinkOff, null, tint = c.er, modifier = Modifier.size(34.dp))
            }
            H1("This phone was unlinked", align = TextAlign.Center)
            Lead("${name.ifBlank { "Your account" }} was removed from this phone by one of your other devices. Its contacts, chats and calls are deleted here. Your other devices are not affected.",
                Modifier.widthIn(max = 330.dp), align = TextAlign.Center)
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            TinButton("OK", onOk)
            TinButton("Link it again", onLinkAgain, style = BtnStyle.Text)
        }
    }
}
