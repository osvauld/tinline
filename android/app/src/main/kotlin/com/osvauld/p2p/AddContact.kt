package com.osvauld.p2p

import android.Manifest
import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
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
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import com.google.zxing.BarcodeFormat
import com.journeyapps.barcodescanner.BarcodeCallback
import com.journeyapps.barcodescanner.BarcodeView
import com.journeyapps.barcodescanner.DefaultDecoderFactory
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.Contact

private enum class AddStage { Main, Paste, Adding, Added, Failed }

/** Add contact: My code / Scan tabs, Paste a card, Adding..., Added, Couldn't add. */
@Composable
fun AddContactScreen(app: P2pApp, startOnScan: Boolean, onClose: () -> Unit, onCall: (Contact) -> Unit, onVerify: (Contact) -> Unit) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val contacts by app.contacts.collectAsState()
    val meName = remember { app.node.profile()?.name ?: "" }
    var stage by rememberSaveable { mutableStateOf(AddStage.Main) }
    var tab by rememberSaveable { mutableIntStateOf(if (startOnScan) 1 else 0) }
    var pasted by rememberSaveable { mutableStateOf("") }
    var added by remember { mutableStateOf<Contact?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    val knownAtOpen = remember { contacts.map { it.did }.toSet() }
    var job by remember { mutableStateOf<kotlinx.coroutines.Job?>(null) }

    fun add(text: String) {
        stage = AddStage.Adding; failure = null
        job = scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { app.node.addContact(text.trim()) } }
            r.onSuccess { testLog("added name=${it.name} did=${it.did}"); app.refresh(); added = it; stage = AddStage.Added }
                .onFailure { failure = it.message; stage = AddStage.Failed }
        }
    }
    // The other phone dialled us while our code was on screen: they are in the contact list now.
    LaunchedEffect(contacts, stage) {
        if (stage == AddStage.Main) contacts.firstOrNull { it.did !in knownAtOpen }?.let { added = it; stage = AddStage.Added }
    }

    when (stage) {
        AddStage.Main -> AddMain(app, meName, tab, { tab = it }, onClose, onPaste = { stage = AddStage.Paste }, onScanned = { pasted = it; add(it) })
        AddStage.Paste -> PasteCardScreen(pasted, { pasted = it }, onBack = { stage = AddStage.Main }, onAdd = { add(pasted) })
        AddStage.Adding -> AddingScreen(meName) { job?.cancel(); stage = AddStage.Main }
        AddStage.Added -> added?.let { AddedScreen(meName, it, onCall = { onCall(it) }, onVerify = { onVerify(it) }, onDone = onClose) }
        AddStage.Failed -> FailedScreen(meName, failure, onRetry = { add(pasted) }, onOther = { pasted = ""; tab = 1; stage = AddStage.Main })
    }
}

// ------------------------------------------------------------------ main (tabs)

@Composable
private fun AddMain(app: P2pApp, meName: String, tab: Int, onTab: (Int) -> Unit, onClose: () -> Unit, onPaste: () -> Unit, onScanned: (String) -> Unit) {
    if (tab == 1) TinlineTheme(dark = true) {
        ScanTab(tab, onTab, onClose, onPaste, onScanned)
    } else MyCodeTab(app, meName, tab, onTab, onClose)
}

@Composable
private fun Segmented(tab: Int, onTab: (Int) -> Unit, outline: Color) {
    val c = Tin.c
    Row(Modifier.padding(horizontal = 24.dp, vertical = 4.dp).fillMaxWidth().clip(RoundedCornerShape(50)).border(1.dp, outline, RoundedCornerShape(50))) {
        listOf(Icons.Rounded.QrCode2 to "My code", Icons.Rounded.QrCodeScanner to "Scan").forEachIndexed { i, (ic, label) ->
            val on = i == tab
            Row(
                Modifier.weight(1f).heightIn(min = 44.dp).background(if (on) c.prc else Color.Transparent).clickable(role = Role.Tab) { onTab(i) },
                horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(ic, null, tint = if (on) c.onPrc else c.ink, modifier = Modifier.size(20.dp))
                Text(label, style = TinType.label.copy(fontSize = 15.sp), color = if (on) c.onPrc else c.ink)
            }
        }
    }
}

@Composable
private fun MyCodeTab(app: P2pApp, meName: String, tab: Int, onTab: (Int) -> Unit, onClose: () -> Unit) {
    val ctx = LocalContext.current
    val c = Tin.c
    val scope = rememberCoroutineScope()
    val status by app.status.collectAsState()
    var ticket by remember { mutableStateOf<String?>(null) }
    var err by remember { mutableStateOf<String?>(null) }
    var version by remember { mutableIntStateOf(0) }
    var share by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(status?.online, version) {
        withContext(Dispatchers.IO) { runCatching { app.node.myTicket() } }
            .onSuccess { ticket = it; err = null }.onFailure { err = it.message }
    }
    Page {
        TopBar("Add contact", onClose)
        Segmented(tab, onTab, c.ln2)
        Column(
            Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 24.dp, bottom = 16.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            CardBox(Modifier.fillMaxWidth(), radius = 24.dp) {
                Column(Modifier.padding(20.dp).fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    Text(meName, style = TinType.titleL.copy(fontSize = 20.sp), color = c.ink)
                    val t = ticket
                    Box(Modifier.size(248.dp).clip(RoundedCornerShape(14.dp)).background(Color.White), contentAlignment = Alignment.Center) {
                        if (t != null) {
                            val bmp = remember(t) { qrBitmap(t, 720, fg = 0xFF17201D.toInt()) }
                            Image(bmp.asImageBitmap(), "QR code of your contact card", Modifier.fillMaxSize().padding(10.dp))
                            Box(Modifier.size(40.dp).clip(RoundedCornerShape(10.dp)).background(Color.White).padding(3.dp).clip(RoundedCornerShape(8.dp)).background(Color(0xFF0B6B5B)), contentAlignment = Alignment.Center) {
                                TinMark(26.dp, can = Color.White, string = Color(0xFFE8B04A))
                            }
                        } else Text(err ?: "Preparing your code…", Modifier.padding(16.dp), style = TinType.bodyM, color = Color(0xFF4D5853), textAlign = TextAlign.Center)
                    }
                    Hint("Works once. Making a new code cancels this one.", Modifier.widthIn(max = 300.dp), align = TextAlign.Center)
                }
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                TinButton("Share", {
                    ticket?.let { t ->
                        try { ctx.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, t), "Share contact card")) }
                        catch (_: Exception) { share = "Nothing to share with" }
                    }
                }, Modifier.weight(1f), style = BtnStyle.Tonal, icon = Icons.Rounded.Share, enabled = ticket != null)
                TinButton("Copy", { ticket?.let { copyToClipboard(ctx, "ticket", it) } }, Modifier.weight(1f), style = BtnStyle.Outlined, icon = Icons.Rounded.ContentCopy, enabled = ticket != null)
            }
            share?.let { Hint(it, color = c.er) }
            TinButton("New code", {
                scope.launch { withContext(Dispatchers.IO) { runCatching { app.node.resetTicket() } }; ticket = null; version++ }
            }, style = BtnStyle.Text, icon = Icons.Rounded.Refresh)
        }
        Row(
            Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp).fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(c.sf2).padding(horizontal = 14.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Dot(c.pr, 10.dp)
            Text("Waiting for them to scan — keep this open.", style = TinType.bodyM, color = c.ink2)
        }
    }
}

@Composable
private fun ScanTab(tab: Int, onTab: (Int) -> Unit, onClose: () -> Unit, onPaste: () -> Unit, onScanned: (String) -> Unit) {
    val ctx = LocalContext.current
    val c = Tin.c
    val owner = LocalLifecycleOwner.current
    var granted by remember { mutableStateOf(Perms.granted(ctx, Manifest.permission.CAMERA)) }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted = it }
    var torch by remember { mutableStateOf(false) }
    var view by remember { mutableStateOf<BarcodeView?>(null) }
    var handled by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { if (!granted) ask.launch(Manifest.permission.CAMERA) }
    DisposableEffect(owner, view, granted) {
        val v = view
        val o = LifecycleEventObserver { _, e ->
            if (v != null && granted) when (e) {
                Lifecycle.Event.ON_RESUME -> v.resume()
                Lifecycle.Event.ON_PAUSE -> v.pause()
                else -> {}
            }
        }
        owner.lifecycle.addObserver(o)
        if (v != null && granted && owner.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) v.resume()
        onDispose { owner.lifecycle.removeObserver(o); v?.pause() }
    }
    Page(Modifier.background(Color(0xFF050807))) {
        TopBar("Add contact", onClose) {
            if (granted) IconBtn(if (torch) Icons.Rounded.FlashlightOn else Icons.Rounded.FlashlightOff, "Torch", { torch = !torch; view?.setTorch(torch) })
        }
        Segmented(tab, onTab, Color(0xFF4D5853))
        Box(Modifier.weight(1f).fillMaxWidth().padding(top = 16.dp).background(Color(0xFF2A302E)), contentAlignment = Alignment.Center) {
            if (granted) {
                AndroidView({ context ->
                    BarcodeView(context).apply {
                        decoderFactory = DefaultDecoderFactory(listOf(BarcodeFormat.QR_CODE))
                        decodeSingle(BarcodeCallback { r -> r.text?.let { if (!handled) { handled = true; onScanned(it) } } })
                        view = this
                    }
                }, Modifier.fillMaxSize())
                Canvas(Modifier.size(260.dp)) {
                    val len = 44.dp.toPx(); val w = 4.dp.toPx(); val r = 16.dp.toPx(); val s = size.width
                    fun corner(x: Float, y: Float, dx: Float, dy: Float) {
                        val p = androidx.compose.ui.graphics.Path().apply {
                            moveTo(x, y + dy * len); lineTo(x, y + dy * r); quadraticTo(x, y, x + dx * r, y); lineTo(x + dx * len, y)
                        }
                        drawPath(p, Color.White, style = Stroke(w, cap = androidx.compose.ui.graphics.StrokeCap.Round))
                    }
                    corner(0f, 0f, 1f, 1f); corner(s, 0f, -1f, 1f); corner(0f, s, 1f, -1f); corner(s, s, -1f, -1f)
                    drawRoundRect(Color(0xFFE8B04A), Offset(24.dp.toPx(), s / 2), Size(s - 48.dp.toPx(), 2.dp.toPx()), CornerRadius(1.dp.toPx()))
                }
                Text("Point at their Tinline code", Modifier.align(Alignment.BottomCenter).padding(bottom = 24.dp), style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = Color.White)
            } else Column(Modifier.padding(32.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Icon(Icons.Rounded.NoPhotography, null, tint = Color.White, modifier = Modifier.size(48.dp))
                Text("Tinline needs the camera to read the code.", style = TinType.bodyL, color = Color.White, textAlign = TextAlign.Center)
                TinButton("Allow camera", {
                    val act = ctx as? android.app.Activity
                    if (act != null && !androidx.core.app.ActivityCompat.shouldShowRequestPermissionRationale(act, Manifest.permission.CAMERA) && !granted) ask.launch(Manifest.permission.CAMERA)
                    else ask.launch(Manifest.permission.CAMERA)
                }, fill = false)
            }
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, top = 20.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            TinButton("Paste a card instead", onPaste, style = BtnStyle.Outlined, icon = Icons.Rounded.ContentPaste)
            Hint("Camera is used only to read the code. Nothing is recorded.", Modifier.fillMaxWidth(), align = TextAlign.Center)
        }
    }
}

// ------------------------------------------------------------------ paste, adding, added, failed

@Composable
fun PasteCardScreen(text: String, onChange: (String) -> Unit, onBack: () -> Unit, onAdd: () -> Unit) {
    val ctx = LocalContext.current
    val c = Tin.c
    val looks = text.trim().let { it.startsWith("OSVC2:", true) || it.startsWith("osvc1.", true) }
    Page {
        TopBar("Paste a card", onBack) {
            TinButton("Paste", { clipboardText(ctx)?.let(onChange) }, style = BtnStyle.Text, icon = Icons.Rounded.ContentPaste, fill = false, height = 40.dp, textStyle = TinType.label)
        }
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 12.dp, bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text(androidx.compose.ui.text.buildAnnotatedString {
                append("If they sent their card as a message, paste the whole text here. It starts with ")
                pushStyle(androidx.compose.ui.text.SpanStyle(fontFamily = PlexMono, color = c.ink)); append("OSVC2:"); pop()
            }, style = TinType.bodyL, color = c.ink2)
            TinField(text, onChange, "Contact card", mono = true, singleLine = false, minHeight = 150.dp)
            if (looks) Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(c.prc).padding(14.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Column(Modifier.weight(1f)) {
                    Text("Looks like a Tinline card", style = TinType.bodyL.copy(fontSize = 15.sp, fontWeight = FontWeight.Bold), color = c.onPrc)
                    Text("Single use", style = TinType.bodyM, color = c.onPrc)
                }
                Icon(Icons.Rounded.CheckCircle, null, tint = c.onPrc)
            }
            Hint("Tip: a card is safest sent over an app you already trust. Anyone who gets it first could use it instead.")
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp)) { TinButton("Add contact", onAdd, enabled = looks) }
    }
}

@Composable
private fun CenterScreen(footer: @Composable ColumnScope.() -> Unit = {}, body: @Composable ColumnScope.() -> Unit) {
    Page {
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically), content = body)
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(4.dp), content = footer)
    }
}

@Composable
fun AddingScreen(me: String, onCancel: () -> Unit) {
    CenterScreen(footer = { TinButton("Cancel", onCancel, style = BtnStyle.Text) }) {
        PairAvatars(me, "", null, unknown = true)
        H1("Connecting…", align = TextAlign.Center)
        Lead("Exchanging keys directly between your phones. Keep Tinline open on both.", Modifier.widthIn(max = 320.dp), align = TextAlign.Center)
        LinearProgressIndicator(Modifier.width(200.dp), color = Tin.c.pr, trackColor = Tin.c.sf3)
    }
}

@Composable
fun AddedScreen(me: String, contact: Contact, onCall: () -> Unit, onVerify: () -> Unit, onDone: () -> Unit) {
    val name = contact.name.ifBlank { "Contact" }
    CenterScreen(footer = {
        TinButton("Call $name", onCall, icon = Icons.Rounded.Call)
        if (Features.verify) TinButton("Verify identity", onVerify, style = BtnStyle.Outlined, icon = Icons.Rounded.VerifiedUser)
        TinButton("Done", onDone, style = BtnStyle.Text)
    }) {
        PairAvatars(me, name, contact.did)
        H1("$name is added", align = TextAlign.Center)
        Lead("You can now call each other. $name’s phone shows the same thing.", Modifier.widthIn(max = 320.dp), align = TextAlign.Center)
        // "Save as" (rename) joins here when Features.rename is wired.
    }
}

@Composable
fun FailedScreen(me: String, reason: String?, onRetry: () -> Unit, onOther: () -> Unit) {
    val c = Tin.c
    CenterScreen(footer = {
        TinButton("Try again", onRetry)
        TinButton("Scan a different code", onOther, style = BtnStyle.Text)
    }) {
        PairAvatars(me, "", null, unknown = true)
        H1("Couldn’t add them", align = TextAlign.Center)
        Lead("Both phones need Tinline open and online at the same time. Ask them to keep their code on screen, then try again.", Modifier.widthIn(max = 330.dp), align = TextAlign.Center)
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(c.sf2).padding(horizontal = 16.dp, vertical = 14.dp)) {
            Text("Other reasons", style = TinType.bodyM.copy(fontWeight = FontWeight.Bold), color = c.ink)
            Text("· The code was already used — ask for a new one.\n· One of you is offline or on a network that blocks it.", style = TinType.bodyM, color = c.ink2)
            if (!reason.isNullOrBlank()) Text("\n$reason", style = TinType.caption, color = c.ink2)
        }
    }
}
