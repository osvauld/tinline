package com.osvauld.p2p

import androidx.compose.ui.res.stringResource
import androidx.compose.foundation.Image
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.unit.em
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
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
fun linkFailText(reason: String, detail: String?): String = uniffi.present.linkFailText(reason, detail, "phone")

// ------------------------------------------------------------------ the flow (both sides)

private enum class LinkStage { Intro, Scan, Show }

/**
 * Link a device, one side of it. [newDevice] = false: this phone has the account (Settings › Link a device; the playground's
 * devices/LinkDevice.svelte page); true: this phone is the new one (onboarding; gate/Link.svelte). [label] is the new device's
 * name. The screen follows [LinkController.ui]; [onClose] leaves, [onDone] is called once on the new device when the account has
 * arrived (the existing device shows its own "linked" screen). The real QR and the core's code are used throughout.
 */
@Composable
fun LinkSession(app: P2pApp, newDevice: Boolean, label: String, onClose: () -> Unit, onDone: (LinkController.Ui.Done) -> Unit = {}, progress: Pair<Int, Int>? = null) {
    val links = app.links
    val ui by links.ui.collectAsState()
    var stage by rememberSaveable { mutableStateOf(if (newDevice) LinkStage.Show else LinkStage.Intro) }
    var scanNote by remember { mutableStateOf<String?>(null) }
    var scanNonce by remember { mutableIntStateOf(0) }
    var regen by remember { mutableIntStateOf(0) }
    DisposableEffect(Unit) { links.reset(); onDispose { links.cancel() } }

    fun onText(text: String) {
        val bad = uniffi.present.linkQrProblem(text)
        if (bad != null) { scanNote = bad; return }
        scanNote = null
        links.scan(text, newDevice, label)
    }
    fun showQr() = links.showQr(newDevice, label)
    LaunchedEffect(stage, regen) { if (stage == LinkStage.Show && links.ui.value is LinkController.Ui.Idle) showQr() }
    LaunchedEffect(ui) { (ui as? LinkController.Ui.Done)?.let { if (newDevice) onDone(it) } }
    fun again() { links.reset(); scanNote = null; scanNonce++; regen++; if (!newDevice) stage = LinkStage.Intro }

    if (stage == LinkStage.Scan && (ui is LinkController.Ui.Idle || ui is LinkController.Ui.Showing)) {
        LinkScanScreen(newDevice, scanNote, scanNonce, onBack = { scanNote = null; stage = if (newDevice) LinkStage.Show else LinkStage.Intro },
            onText = ::onText, onRetry = { scanNote = null; scanNonce++ }, onShow = { scanNote = null; stage = LinkStage.Show })
        return
    }
    if (newDevice) NewDeviceLink(ui, progress, onBack = { links.cancel(); onClose() }, onNew = { links.cancel(); regen++ },
        onScanInstead = { links.cancel(); stage = LinkStage.Scan }, onCancel = { links.cancel(); again() }, onRetry = { again() })
    else ExistingDeviceLink(app, ui, stage, scanNote, onClose = onClose, onScan = { stage = LinkStage.Scan }, onShow = { stage = LinkStage.Show },
        onTyped = ::onText, onClearNote = { scanNote = null }, onNew = { links.cancel(); regen++ }, onCancel = { links.cancel(); again() }, onRetry = { again() })
}

// ------------------------------------------------------------------ new device (gate/Link.svelte)

@Composable
private fun NewDeviceLink(ui: LinkController.Ui, progress: Pair<Int, Int>?, onBack: () -> Unit, onNew: () -> Unit, onScanInstead: () -> Unit, onCancel: () -> Unit, onRetry: () -> Unit) {
    val c = Tin.c
    Column(Modifier.fillMaxSize()) {
        GateHead(if (ui is LinkController.Ui.Idle || ui is LinkController.Ui.Showing || ui is LinkController.Ui.Failed) onBack else null, progress, stringResource(R.string.link_title))
        when (ui) {
            is LinkController.Ui.Code -> GateStep(stringResource(R.string.link_same_code), lead = stringResource(R.string.link_same_code_lead, ui.peer.ifBlank { stringResource(R.string.link_fallback_other_device) }),
                center = true, anchor = true, foot = { GBtn(stringResource(R.string.common_cancel), onCancel, kind = GKind.Secondary) }) {
                BigCode(ui.code)
                GNotice(stringResource(R.string.link_code_differs), NKind.Warn)
                WaitingLine(stringResource(R.string.link_waiting))
            }
            is LinkController.Ui.Approving, is LinkController.Ui.Connecting, is LinkController.Ui.Done ->
                GateStep(stringResource(R.string.link_linking), lead = stringResource(R.string.link_receiving), center = true, anchor = true) {
                    Box(Modifier.padding(vertical = 40.dp)) { PgSpinner(40.dp, c.accent, 4.dp) }
                }
            is LinkController.Ui.Failed -> GateStep(stringResource(R.string.link_failed_title), center = true, anchor = true, foot = {
                GBtn(stringResource(R.string.common_try_again), onRetry)
                GBtn(stringResource(R.string.common_not_now), onBack, kind = GKind.Text)
            }) { GNotice(linkFailText(ui.reason, ui.detail), NKind.Error) }
            is LinkController.Ui.Showing, LinkController.Ui.Idle -> {
                val showing = ui as? LinkController.Ui.Showing
                GateStep(stringResource(R.string.link_title), lead = stringResource(R.string.link_show_lead),
                    center = true, anchor = true, foot = {
                        GBtn(stringResource(R.string.link_new_code), onNew, kind = GKind.Text, enabled = showing != null)
                        GBtn(stringResource(R.string.link_scan_instead), onScanInstead, kind = GKind.Text)
                    }) {
                    QrPanel(showing)
                    Text(stringResource(R.string.link_qr_warning),
                        Modifier.widthIn(max = 300.dp), style = PgType.sub, color = c.ink2, textAlign = TextAlign.Center)
                }
            }
        }
    }
}

/** The real QR in a white tile (padding 12, r20, line border), and the expiry line under it (reserved, tabular). */
@Composable
private fun QrPanel(showing: LinkController.Ui.Showing?) {
    val c = Tin.c
    val qr = showing?.qr
    var left by remember(qr) { mutableLongStateOf(showing?.let { (it.expiresAt - System.currentTimeMillis()).coerceAtLeast(0) } ?: 0L) }
    LaunchedEffect(qr) { while (qr != null && left > 0) { delay(1000); left = (showing.expiresAt - System.currentTimeMillis()).coerceAtLeast(0) } }
    val expired = qr != null && left <= 0
    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Box(Modifier.clip(SmoothCornerShape(20.dp)).background(Color.White).border(1.dp, c.ln, SmoothCornerShape(20.dp)).padding(12.dp)) {
            Box(Modifier.size(208.dp), contentAlignment = Alignment.Center) {
                if (qr != null) {
                    val bmp = remember(qr) { qrBitmap(qr, 624, fg = 0xFF17201D.toInt()) }
                    Image(bmp.asImageBitmap(), stringResource(R.string.link_qr_desc), Modifier.fillMaxSize().clip(SmoothCornerShape(12.dp)), alpha = if (expired) 0.18f else 1f)
                } else PgSpinner(28.dp, Color(0xFF0B6B5B), 3.dp)
            }
        }
        Text(when { qr == null -> stringResource(R.string.link_preparing); expired -> stringResource(R.string.link_expired); else -> stringResource(R.string.link_expires, "${left / 60000}:${"%02d".format(left / 1000 % 60)}") },
            style = PgType.sub.copy(fontFeatureSettings = "tnum"), color = if (expired) c.er else c.ink2)
    }
}

private const val LINK_BOLD_MARKER = "\u0001"

/** [text] has [marker] where the bold phrase goes (a placeholder in the string resource, so translations place it themselves). */
private fun withBold(text: String, marker: String, bold: String, style: SpanStyle): androidx.compose.ui.text.AnnotatedString = buildAnnotatedString {
    val i = text.indexOf(marker)
    if (i < 0) { append(text); return@buildAnnotatedString }
    append(text.substring(0, i)); pushStyle(style); append(bold); pop(); append(text.substring(i + marker.length))
}

/** Six digits, 44/52 mono, letter-spaced .12em. */
@Composable
private fun BigCode(code: String, weight: FontWeight = FontWeight.SemiBold) {
    val desc = stringResource(R.string.link_code_desc, code.toList().joinToString(" "))
    Text(code, Modifier.fillMaxWidth().padding(vertical = 8.dp).semantics { contentDescription = desc },
        style = PgType.mono.copy(fontSize = 44.sp, lineHeight = 52.sp, fontWeight = weight, letterSpacing = 0.12.em, fontFeatureSettings = "tnum"),
        color = Tin.c.ink, textAlign = TextAlign.Center)
}

/** "Waiting for approval…" with an 8 px accent dot pulsing to 25 % every 1.2 s (still when motion is reduced). */
@Composable
private fun WaitingLine(text: String) {
    val c = Tin.c
    val reduced = rememberReducedMotion()
    val a = if (reduced) 1f else rememberInfiniteTransition(label = "pulse").animateFloat(1f, 0.25f,
        infiniteRepeatable(tween(600, easing = androidx.compose.animation.core.FastOutSlowInEasing), RepeatMode.Reverse), label = "pulse").value
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(Modifier.size(8.dp).graphicsLayer { alpha = a }.clip(androidx.compose.foundation.shape.CircleShape).background(c.accent))
        Text(text, style = PgType.sub, color = c.ink2)
    }
}

// ------------------------------------------------------------------ existing device (devices/LinkDevice.svelte)

@Composable
private fun ExistingDeviceLink(
    app: P2pApp, ui: LinkController.Ui, stage: LinkStage, note: String?, onClose: () -> Unit, onScan: () -> Unit, onShow: () -> Unit,
    onTyped: (String) -> Unit, onClearNote: () -> Unit, onNew: () -> Unit, onCancel: () -> Unit, onRetry: () -> Unit,
) {
    // The codes are compared first, then the passphrase approves: two steps over the core's one Code state.
    var matched by remember(ui) { mutableStateOf(false) }
    val key = when (ui) { is LinkController.Ui.Code -> if (matched) "auth" else "verify"; is LinkController.Ui.Approving, LinkController.Ui.Connecting -> "transfer"
        is LinkController.Ui.Done -> "done"; is LinkController.Ui.Failed -> "failed"; else -> if (stage == LinkStage.Show) "show" else "scan" }
    val reduced = rememberReducedMotion()
    PfPage(stringResource(R.string.link_existing_title), onBack = { onCancel(); onClose() }) {
        AnimatedContent(key, transitionSpec = { fadeIn(tween(if (reduced) 0 else PgMotion.CROSSFADE)) togetherWith fadeOut(tween(0)) }, label = "link") { k ->
            Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                when (k) {
                    "scan" -> ScanStep(note, onScan, onShow, onTyped, onClearNote)
                    "show" -> {
                        PageH1(stringResource(R.string.link_show_h1))
                        Text(stringResource(R.string.link_show_body), style = PgType.sub19, color = Tin.c.ink2)
                        Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) { QrPanel(ui as? LinkController.Ui.Showing) }
                        PfActs(top = 6.dp) { PBtn(stringResource(R.string.common_back), onCancel, variant = PVariant.Quiet); PBtn(stringResource(R.string.link_new_code), onNew) }
                    }
                    "verify" -> (ui as? LinkController.Ui.Code)?.let { u ->
                        val who = u.peer.ifBlank { stringResource(R.string.link_fallback_new_device_cap) }
                        PageH1(stringResource(R.string.link_match_title))
                        Text(boldLead(who, stringResource(R.string.link_match_body)), style = PgType.body, color = Tin.c.ink)
                        BigCode(u.code, FontWeight.Bold)
                        Text(stringResource(R.string.link_match_note), style = PgType.sub19, color = Tin.c.ink2)
                        PfActs(top = 6.dp) { PBtn(stringResource(R.string.link_no_match), onCancel); PBtn(stringResource(R.string.link_match), { matched = true }, variant = PVariant.Primary) }
                    }
                    "auth" -> (ui as? LinkController.Ui.Code)?.let { u -> ApproveStep(app, u.peer.ifBlank { stringResource(R.string.link_fallback_new_device) }, onCancel) }
                    "transfer" -> {
                        val peer = (ui as? LinkController.Ui.Approving)?.peer?.ifBlank { null } ?: stringResource(R.string.link_fallback_new_device)
                        PageH1(if (ui is LinkController.Ui.Connecting) stringResource(R.string.link_connecting) else stringResource(R.string.link_linking_peer, peer))
                        Row(Modifier.heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) { PgSpinner(14.dp, Tin.c.accent) }
                            Text(if (ui is LinkController.Ui.Connecting) stringResource(R.string.link_reaching) else stringResource(R.string.link_sending), Modifier.weight(1f), style = PgType.body, color = Tin.c.ink)
                            Text(stringResource(R.string.link_working), style = PgType.sub, color = Tin.c.ink2)
                        }
                        Text(stringResource(R.string.link_keep_open), style = PgType.sub19, color = Tin.c.ink2)
                        if (ui is LinkController.Ui.Connecting) PfActs(top = 6.dp) { PBtn(stringResource(R.string.common_cancel), onCancel, variant = PVariant.Quiet) }
                    }
                    "done" -> {
                        val peer = (ui as? LinkController.Ui.Done)?.peer?.ifBlank { null } ?: stringResource(R.string.link_fallback_new_device_cap)
                        ResultBlock(PgIcons.Check, ok = true, title = stringResource(R.string.link_done_title, peer),
                            body = stringResource(R.string.link_done_body))
                        PfActs(top = 6.dp) { PBtn(stringResource(R.string.common_done), onClose, variant = PVariant.Primary) }
                    }
                    else -> (ui as? LinkController.Ui.Failed)?.let { u ->
                        ResultBlock(PgIcons.Info, ok = false, title = stringResource(R.string.link_failed_title), body = linkFailText(u.reason, u.detail))
                        PfActs(top = 6.dp) { PBtn(stringResource(R.string.common_close), onClose, variant = PVariant.Quiet); PBtn(stringResource(R.string.common_try_again), onRetry, variant = PVariant.Primary) }
                    }
                }
            }
        }
    }
}

@Composable
private fun PageH1(text: String) = Text(text, Modifier.fillMaxWidth().semantics { heading() }, style = PgType.h1Page, color = Tin.c.ink)

@Composable
private fun ScanStep(note: String?, onScan: () -> Unit, onShow: () -> Unit, onTyped: (String) -> Unit, onClearNote: () -> Unit) {
    val c = Tin.c
    var text by remember { mutableStateOf("") }
    PageH1(stringResource(R.string.link_scan_h1))
    val step1 = withBold(stringResource(R.string.link_scan_step1, LINK_BOLD_MARKER), LINK_BOLD_MARKER, stringResource(R.string.settings_add_link), SpanStyle(color = c.ink, fontWeight = FontWeight.SemiBold))
    Column(Modifier.padding(start = 4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(step1, style = PgType.sub19, color = c.ink2)
        Text(stringResource(R.string.link_scan_step2), style = PgType.sub19, color = c.ink2)
    }
    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
        Column(Modifier.size(220.dp).clip(SmoothCornerShape(20.dp)).background(Color(0xFF0E1513)), horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(14.dp, Alignment.CenterVertically)) {
            Icon(PgIcons.Camera, null, Modifier.size(30.dp), tint = Color(0xFF9DB0AA))
            PBtn(stringResource(R.string.link_scan_qr), onScan, variant = PVariant.Primary)
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(stringResource(R.string.link_type_code), style = PgType.sub.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.weight(1f)) {
                PassField(text, { text = it; onClearNote() }, label = "", error = "", secret = false, mono = true, placeholder = "OSVL1:…",
                    keyboard = KeyboardOptions(imeAction = ImeAction.Go, autoCorrectEnabled = false), onDone = { if (text.isNotBlank()) onTyped(text) }, msgLines = 0)
            }
            PBtn(stringResource(R.string.common_continue), { onTyped(text) }, enabled = text.isNotBlank())
        }
        Text(note.orEmpty(), Modifier.fillMaxWidth().heightIn(min = 38.dp), style = PgType.sub19, color = c.er)
    }
    PBtn(stringResource(R.string.link_show_here), onShow, variant = PVariant.Quiet, colorOverride = c.accentInk)
}

@Composable
private fun ApproveStep(app: P2pApp, who: String, onCancel: () -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val hasPass = remember { app.node.hasPassphrase() }
    var pass by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }
    fun approve() {
        busy = true; error = ""
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { app.links.approve(if (hasPass) pass else null) } }
            busy = false
            r.onFailure { error = if (it is uniffi.p2pcore.Exception.WrongPassphrase) ctx.getString(R.string.link_wrong_pass) else friendly(it) }
            r.onSuccess { pass = "" }
        }
    }
    fun go() {
        val act = ctx.findActivity()
        if (hasPass || act == null) approve()
        else DeviceAuth.ask(act, ctx.getString(R.string.link_existing_title)) { ok -> if (ok) approve() else error = ctx.getString(R.string.devices_lock_cancelled) }
    }
    PageH1(stringResource(R.string.link_approve_title, who))
    val warning = withBold(stringResource(R.string.link_approve_warning, LINK_BOLD_MARKER, who), LINK_BOLD_MARKER, stringResource(R.string.link_approve_bold), SpanStyle(fontWeight = FontWeight.Bold))
    Row(Modifier.fillMaxWidth().clip(SmoothCornerShape(12.dp)).background(c.warn).padding(12.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Icon(PgIcons.Warning, null, Modifier.padding(top = 1.dp).size(18.dp), tint = c.onWarn)
        Text(warning, style = PgType.body.copy(lineHeight = 20.sp), color = c.onWarn)
    }
    if (hasPass) PassField(pass, { pass = it; error = "" }, label = stringResource(R.string.settings_your_passphrase), error = error, enabled = !busy, focus = rememberAutofocus(), onDone = { if (pass.isNotEmpty() && !busy) go() })
    else if (error.isNotEmpty()) Text(error, style = PgType.sub19, color = c.er)
    PfActs(top = 6.dp) {
        PBtn(stringResource(R.string.link_dont_link), onCancel, variant = PVariant.Quiet, enabled = !busy)
        PBtn(if (busy) stringResource(R.string.link_approving) else stringResource(R.string.link_approve), ::go, variant = PVariant.Primary, enabled = !busy && (!hasPass || pass.isNotEmpty()))
    }
}

@Composable
private fun ResultBlock(icon: androidx.compose.ui.graphics.vector.ImageVector, ok: Boolean, title: String, body: String) {
    val c = Tin.c
    Column(Modifier.fillMaxWidth().padding(top = 24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(Modifier.size(56.dp).clip(androidx.compose.foundation.shape.CircleShape).background(if (ok) c.accentSoft else c.warn), contentAlignment = Alignment.Center) {
            Icon(icon, null, Modifier.size(if (ok) 28.dp else 26.dp), tint = if (ok) c.accentInk else c.onWarn)
        }
        PageH1Center(title)
        Text(body, Modifier.widthIn(max = 340.dp), style = PgType.body, color = c.ink2, textAlign = TextAlign.Center)
    }
}

@Composable
private fun PageH1Center(text: String) = Text(text, Modifier.fillMaxWidth().semantics { heading() }, style = PgType.h1Page, color = Tin.c.ink, textAlign = TextAlign.Center)

// ------------------------------------------------------------------ camera

@Composable
private fun LinkScanScreen(newDevice: Boolean, note: String?, nonce: Int, onBack: () -> Unit, onText: (String) -> Unit, onRetry: () -> Unit, onShow: () -> Unit) {
    var granted by remember { mutableStateOf(true) }
    var torch by remember { mutableStateOf(false) }
    TinlineTheme(dark = true) {
        Page(Modifier.background(Color(0xFF050807))) {
            TopBar(if (newDevice) stringResource(R.string.link_scan_instead) else stringResource(R.string.link_scan_title), onBack) {
                if (granted) IconBtn(if (torch) PgIcons.Torch else PgIcons.Torchoff, stringResource(R.string.link_torch), { torch = !torch })
            }
            Box(Modifier.weight(1f).fillMaxWidth().padding(top = 8.dp)) {
                key(nonce) { QrCameraBox(Modifier.fillMaxSize(), if (newDevice) stringResource(R.string.link_point_new) else stringResource(R.string.link_point_existing), torch, { granted = it }, onText) }
                if (note != null) Column(
                    Modifier.align(Alignment.Center).padding(24.dp).clip(SmoothCornerShape(20.dp)).background(Color(0xE6101614)).padding(20.dp),
                    horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Icon(PgIcons.Warning, null, tint = Color(0xFFF2B8B5), modifier = Modifier.size(32.dp))
                    Text(note, style = PgType.body, color = Color.White, textAlign = TextAlign.Center)
                    PBtn(stringResource(R.string.link_scan_again), onRetry, variant = PVariant.Primary)
                }
            }
            Column(Modifier.padding(start = 24.dp, end = 24.dp, top = 16.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.link_scan_foot), Modifier.fillMaxWidth(), style = PgType.sub, color = Tin.c.ink2, textAlign = TextAlign.Center)
                if (!newDevice) GBtn(stringResource(R.string.link_show_here), onShow, kind = GKind.Text)
            }
        }
    }
}

// ------------------------------------------------------------------ new device: unlinked

/** "This phone was unlinked": another device removed the account; the core already deleted it here. */
@Composable
fun UnlinkedScreen(name: String, onOk: () -> Unit, onLinkAgain: () -> Unit) {
    Page {
        GateHead(null, null)
        GateStep(stringResource(R.string.link_unlinked_title), center = true, anchor = true,
            lead = stringResource(R.string.link_unlinked_lead, name.ifBlank { stringResource(R.string.link_your_account) }),
            hero = { Box(Modifier.padding(top = 12.dp)) { IconDisc(PgIcons.Off, 72.dp, 32.dp, danger = true) } },
            foot = {
                GBtn(stringResource(R.string.link_ok), onOk)
                GBtn(stringResource(R.string.link_again), onLinkAgain, kind = GKind.Text)
            })
    }
}
