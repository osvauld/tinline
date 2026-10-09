package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@Composable
fun TinSwitch(checked: Boolean, onChange: (Boolean) -> Unit, desc: String) {
    val c = Tin.c
    Switch(
        checked, onChange,
        colors = SwitchDefaults.colors(
            checkedThumbColor = c.onPr, checkedTrackColor = c.pr, checkedBorderColor = c.pr,
            uncheckedThumbColor = c.ln2, uncheckedTrackColor = c.sf2, uncheckedBorderColor = c.ln2,
        ),
        modifier = Modifier.semantics { contentDescription = desc },
    )
}

@Composable
fun SettingsScreen(
    app: P2pApp, missing: List<Need>, onBack: () -> Unit, onBattery: () -> Unit, onPassphrase: () -> Unit, onPhrase: () -> Unit,
    onAbout: () -> Unit, onDiagnostics: () -> Unit, onSwitchPick: (String) -> Unit = {}, onCreateAccount: () -> Unit = {},
    onRestoreAccount: () -> Unit = {}, onDevices: () -> Unit = {}, onLinkDevice: () -> Unit = {}, onLinkAccount: () -> Unit = {},
) {
    val avail by app.availability.collectAsState()
    val available = avail.available
    val c = Tin.c
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf(app.node.profile()?.name ?: "") }
    var editing by remember { mutableStateOf(false) }
    var sheet by remember { mutableStateOf(false) }
    val deviceLabel = remember { app.node.deviceLabel() }
    val hasPass = remember { app.node.hasPassphrase() }
    val bgOk = missing.none { it == Need.Battery || it == Need.Notifications || it == Need.FullScreen || it == Need.Mic }
    Page {
        TopBar("Settings", onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
            CardBox(Modifier.padding(start = 16.dp, end = 16.dp, top = 4.dp).fillMaxWidth()) {
                Row(Modifier.padding(horizontal = 16.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                    SelfAvatar(name.ifBlank { "?" }, 52.dp)
                    Column(Modifier.weight(1f)) {
                        Text(name, style = TinType.titleM, color = c.ink, maxLines = 1)
                        Hint(if (deviceLabel != null) "On this phone as “$deviceLabel”" else "Shown to people you add")
                    }
                    IconBtn(Icons.Rounded.Edit, "Edit name", { editing = true }, tint = c.pr)
                    TinButton("Switch", { sheet = true }, style = BtnStyle.Tonal, fill = false, height = 40.dp, textStyle = TinType.label)
                }
            }
            SectionLabel("DEVICES")
            val devVer by app.links.devicesVersion.collectAsState()
            val devCount by produceState(1, devVer) { value = withContext(Dispatchers.IO) { runCatching { app.node.linkedDevices().count { !it.removed } }.getOrDefault(1) } }
            ListItem("Linked devices", onClick = onDevices, icon = Icons.Rounded.Devices, sub = if (devCount <= 1) "This phone only" else "$devCount devices")
            ListItem("Link a device", onClick = onLinkDevice, icon = Icons.Rounded.AddLink, sub = "Use your account on another phone or computer")
            SectionLabel("CALLS")
            ListItem("Available for calls", sub = if (available) "Your line is open" else "Calls won’t ring", icon = Icons.Rounded.Call,
                trailing = { TinSwitch(available, { on -> scope.launch(Dispatchers.IO) { app.setAvailable(on) } }, "Available for calls") })
            ListItem("Background & battery", onClick = onBattery, icon = Icons.Rounded.BatteryChargingFull,
                sub = if (bgOk) "All set — calls will ring" else "Needs attention")
            SectionLabel("SECURITY")
            if (hasPass) ListItem("Change passphrase", onClick = onPassphrase, icon = Icons.Rounded.Key)
            else ListItem("Add passphrase", onClick = onPassphrase, icon = Icons.Rounded.Key, sub = "Optional — encrypts your account")
            ListItem("Recovery phrase", onClick = onPhrase, icon = Icons.Rounded.Lock, sub = if (hasPass) "Needs your passphrase" else "Needs your screen lock")
            SectionLabel("ABOUT")
            ListItem("About Tinline", onClick = onAbout, icon = Icons.Rounded.Info, sub = "Version, licences, terms, privacy")
            if (BuildConfig.DEBUG) ListItem("Diagnostics", onClick = onDiagnostics, icon = Icons.Rounded.BugReport, sub = "Test tone and core version (debug builds)")
        }
    }
    if (sheet) AccountSheet(app, onDismiss = { sheet = false }, onPick = { sheet = false; onSwitchPick(it) },
        actions = AccountActions(onCreate = { sheet = false; onCreateAccount() }, onRestore = { sheet = false; onRestoreAccount() }, onLink = { sheet = false; onLinkAccount() }))
    if (editing) {
        var text by remember { mutableStateOf(name) }
        TinDialog("Your name", { editing = false }, "Save", {
            val n = text.trim()
            scope.launch(Dispatchers.IO) { runCatching { app.node.setName(n) } }
            name = n; editing = false
        }, confirmEnabled = text.isNotBlank()) {
            TinField(text, { text = it }, "Shown to people you add")
        }
    }
}

// ------------------------------------------------------------------ background & battery

@Composable
fun BatteryScreen(missing: List<Need>, onBack: () -> Unit, onFix: (Need) -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    Page {
        TopBar("Background & battery", onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
            Column(Modifier.padding(horizontal = 20.dp, vertical = 8.dp)) {
                Text("Keep my line open", style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
                Hint("Tinline stays connected in the background so calls ring like a normal phone. It uses a little battery. The checklist below is what Android needs to allow that.")
            }
            SectionLabel("CHECKLIST")
            CheckRow(Icons.Rounded.BatteryChargingFull, "Battery", if (Need.Battery in missing) "Optimised — Android may pause Tinline" else null, Need.Battery !in missing, "On") { onFix(Need.Battery) }
            CheckRow(Icons.Rounded.Notifications, "Notifications", null, Need.Notifications !in missing, "On") { onFix(Need.Notifications) }
            CheckRow(Icons.Rounded.PhoneLocked, "Ring over lock screen", null, Need.FullScreen !in missing, "On") { onFix(Need.FullScreen) }
            CheckRow(Icons.Rounded.Mic, "Microphone", null, Need.Mic !in missing, "Allowed") { onFix(Need.Mic) }
            Column(
                Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp).fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(c.sf2).padding(14.dp),
            ) {
                Text(buildAnnotatedStringBold("Still missing calls?", "Some phones need an extra step to keep apps running."), style = TinType.bodyM, color = c.ink2)
                Text("Steps for your phone", Modifier.clickable(role = androidx.compose.ui.semantics.Role.Button) { openUrl(ctx, "https://dontkillmyapp.com") }.padding(vertical = 8.dp),
                    style = TinType.label, color = c.pr)
            }
        }
    }
}

@Composable
private fun CheckRow(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, sub: String?, ok: Boolean, okText: String, onFix: () -> Unit) {
    val c = Tin.c
    ListItem(title, icon = icon, sub = sub, trailing = {
        if (ok) Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Icon(Icons.Rounded.Check, null, tint = c.pr, modifier = Modifier.size(18.dp)); Text(okText, style = TinType.label, color = c.pr)
        } else TinButton("Fix", onFix, fill = false, height = 40.dp, textStyle = TinType.label)
    })
}

// ------------------------------------------------------------------ recovery phrase

/** Asks for the passphrase (or, with none set, the screen lock), then reveals the phrase through [onPhrase]. */
@Composable
fun PhraseGateScreen(app: P2pApp, onBack: () -> Unit, onPhrase: (String) -> Unit) {
    val scope = rememberCoroutineScope()
    val ctx = LocalContext.current
    val hasPass = remember { app.node.hasPassphrase() }
    var pass by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    fun reveal() {
        busy = true; error = null
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { app.node.recoveryPhrase(pass) } }
            busy = false
            r.onFailure { error = friendly(it) }
            r.onSuccess { pass = ""; onPhrase(it) }
        }
    }
    fun go() {
        val act = ctx.findActivity()
        if (hasPass || act == null) reveal()
        else DeviceAuth.ask(act, "Show recovery phrase") { ok -> if (ok) reveal() else error = "Screen lock check cancelled" }
    }
    Page {
        TopBar("Recovery phrase", onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 24.dp, bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Box(Modifier.size(64.dp).clip(CircleShape).background(Tin.c.prc), contentAlignment = Alignment.Center) { Icon(Icons.Rounded.Lock, null, tint = Tin.c.onPrc, modifier = Modifier.size(28.dp)) }
            Text(if (hasPass) "Enter your passphrase" else "Confirm it’s you", style = TinType.h1.copy(fontSize = 26.sp, lineHeight = 32.sp), color = Tin.c.ink)
            Lead(if (hasPass) "Your recovery phrase is the key to your account, so we check it’s really you first."
            else "Your recovery phrase is the key to your account, so we ask for your phone’s screen lock first.")
            if (hasPass) TinField(pass, { pass = it; error = null }, "Passphrase", mono = true, password = true, enabled = !busy,
                state = if (error != null) FieldState.Error else FieldState.Normal, hint = error,
                keyboard = KeyboardOptions(imeAction = ImeAction.Done), actions = KeyboardActions(onDone = { if (pass.isNotEmpty() && !busy) go() }))
            else error?.let { Text(it, style = TinType.bodyM, color = Tin.c.er) }
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth(), color = Tin.c.pr, trackColor = Tin.c.sf3)
        }
        Box(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp)) { TinButton("Show recovery phrase", ::go, enabled = (!hasPass || pass.isNotEmpty()) && !busy) }
    }
}

@Composable
fun PhraseShownScreen(phrase: String, onHide: () -> Unit) {
    Page {
        TopBar("Recovery phrase", onHide)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 4.dp, bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            InfoCard("Anyone who sees these words can become you. Check nobody is looking.", icon = Icons.Rounded.Warning, kind = BannerKind.Warn)
            WordGrid(phrase)
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Rounded.Lock, null, tint = Tin.c.ink2, modifier = Modifier.size(16.dp))
                Hint("Screenshots and screen recording are blocked here.")
            }
        }
        Box(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp)) { TinButton("Hide", onHide) }
    }
}

// ------------------------------------------------------------------ add / change passphrase

/** Adds a passphrase when none is set (the data key is rewrapped, nothing re-encrypted), else changes it. */
@Composable
fun ChangePassphraseScreen(app: P2pApp, onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    val has = remember { app.node.hasPassphrase() }
    var old by remember { mutableStateOf("") }
    var pass by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val title = if (has) "Change passphrase" else "Add passphrase"
    Page {
        TopBar(title, onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 8.dp, bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            if (has) TinField(old, { old = it; error = null }, "Current passphrase", mono = true, password = true, enabled = !busy,
                state = if (error != null) FieldState.Error else FieldState.Normal, hint = error)
            else Lead("Your account lives only on this phone. A passphrase encrypts it, so nobody holding your phone can copy it.")
            NewPassphraseField(pass, { pass = it; error = null }, !busy, label = if (has) "New passphrase" else "Passphrase", why = !has)
            if (!has) error?.let { Text(it, style = TinType.bodyM, color = Tin.c.er) }
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth(), color = Tin.c.pr, trackColor = Tin.c.sf3)
        }
        Box(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp)) {
            TinButton(if (busy) "Encrypting…" else title, {
                busy = true; error = null
                scope.launch {
                    val r = withContext(Dispatchers.IO) { runCatching { app.node.setPassphrase(if (has) old else null, pass) } }
                    busy = false
                    r.onFailure { error = friendly(it) }
                    r.onSuccess { withContext(Dispatchers.IO) { app.persistIdentity() }; old = ""; pass = ""; onBack() }
                }
            }, enabled = (!has || old.isNotEmpty()) && pass.isNotEmpty() && !busy)
        }
    }
}

// ------------------------------------------------------------------ about

@Composable
fun AboutScreen(onBack: () -> Unit, onLicences: () -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    val ext: @Composable () -> Unit = { Icon(Icons.Rounded.OpenInNew, null, tint = c.ink2) }
    Page {
        TopBar("About", onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
            Column(Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, top = 16.dp, bottom = 8.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                AppIconBadge(88.dp)
                Text("Tinline", Modifier.padding(top = 6.dp), style = TinType.titleL, color = c.ink)
                Hint("Version ${BuildConfig.VERSION_NAME} · by Osvauld")
                Text("Free and open source under the GPL. No ads, no tracking, no analytics.", Modifier.padding(top = 8.dp), style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 22.sp),
                    color = c.ink2, textAlign = TextAlign.Center)
            }
            ListItem("Source code", { openUrl(ctx, URL_SOURCE) }, sub = "GPL-3.0", icon = Icons.Rounded.Code, trailing = ext)
            ListItem("Terms of Use", { openUrl(ctx, URL_TERMS) }, icon = Icons.Rounded.Description, trailing = ext)
            ListItem("Privacy Policy", { openUrl(ctx, URL_PRIVACY) }, icon = Icons.Rounded.PrivacyTip, trailing = ext)
            ListItem("Open-source licences", onLicences, icon = Icons.Rounded.Gavel)
            ListItem("tinline.osvauld.com", { openUrl(ctx, URL_SITE) }, icon = Icons.Rounded.Language, trailing = ext)
            Hint("Not for emergency calls.", Modifier.padding(horizontal = 20.dp, vertical = 16.dp))
        }
    }
}

@Composable
fun LicencesScreen(onBack: () -> Unit) {
    val ctx = LocalContext.current
    val c = Tin.c
    val fonts = remember {
        listOf("Figtree-OFL.txt" to "Figtree (SIL Open Font License 1.1)", "IBMPlexMono-OFL.txt" to "IBM Plex Mono (SIL Open Font License 1.1)")
            .map { (f, t) -> t to runCatching { ctx.assets.open("licenses/$f").bufferedReader().readText() }.getOrDefault("") }
    }
    val libs = listOf(
        "Tinline — GPL-3.0-or-later", "iroh and the Rust crates in the core — Apache-2.0 / MIT", "Opus — BSD-3-Clause",
        "ZXing and zxing-android-embedded — Apache-2.0", "JNA — Apache-2.0 / LGPL-2.1", "AndroidX, Jetpack Compose, Material Symbols — Apache-2.0",
        "BIP-39 English word list — MIT",
    )
    Page {
        TopBar("Open-source licences", onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            libs.forEach { Text(it, style = TinType.bodyL.copy(fontSize = 15.sp), color = c.ink) }
            fonts.forEach { (t, body) ->
                Text(t, Modifier.padding(top = 12.dp), style = TinType.titleM, color = c.ink)
                Text(body, style = TinType.mono.copy(fontSize = 11.sp, lineHeight = 15.sp, fontWeight = FontWeight.Normal), color = c.ink2)
            }
        }
    }
}

// ------------------------------------------------------------------ diagnostics (debug builds)

@Composable
fun DiagnosticsScreen(app: P2pApp, onBack: () -> Unit) {
    var tone by remember { mutableStateOf(app.testToneHz != null) }
    Page {
        TopBar("Diagnostics", onBack)
        ListItem("Test tone", sub = "Send a 440 Hz tone instead of the microphone", trailing = {
            TinSwitch(tone, { tone = it; app.testToneHz = if (it) 440f else null; app.node.setTestTone(app.testToneHz) }, "Test tone")
        })
        Hint("p2pcore ${uniffi.p2pcore.coreVersion()}", Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
    }
}
