package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
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
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import android.os.SystemClock
import android.view.ViewTreeObserver
import kotlinx.coroutines.delay
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
            CheckRow(PgIcons.Battery, "Battery", if (Need.Battery in missing) "Optimised — Android may pause Tinline" else null, Need.Battery !in missing, "On") { onFix(Need.Battery) }
            CheckRow(PgIcons.Bell, "Notifications", null, Need.Notifications !in missing, "On") { onFix(Need.Notifications) }
            CheckRow(PgIcons.Lock, "Ring over lock screen", null, Need.FullScreen !in missing, "On") { onFix(Need.FullScreen) }
            Column(
                Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp).fillMaxWidth().clip(SmoothCornerShape(14.dp)).background(c.sf2).padding(14.dp),
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
            Icon(PgIcons.Check, null, tint = c.pr, modifier = Modifier.size(18.dp)); Text(okText, style = TinType.label, color = c.pr)
        } else TinButton("Fix", onFix, fill = false, height = 40.dp, textStyle = TinType.label)
    })
}

// ------------------------------------------------------------------ recovery phrase

/** Both Profile and Privacy use this gate; no credential or phrase enters synchronized profile data. */
@Composable
fun PhraseGateScreen(app: P2pApp, onBack: () -> Unit, onPhrase: (String) -> Unit) {
    val scope = rememberCoroutineScope()
    val ctx = LocalContext.current
    val owner = LocalLifecycleOwner.current
    var hasPass by remember { mutableStateOf<Boolean?>(null) }
    var pass by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var prompting by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var generation by remember { mutableIntStateOf(0) }
    var alive by remember { mutableStateOf(true) }
    LaunchedEffect(Unit) {
        val ticket = app.profiles.session.ticket() ?: return@LaunchedEffect
        val result = withContext(Dispatchers.IO) { runCatching { app.profileWork(ticket) { it.hasPassphrase() } }.getOrNull() }
        if (app.profiles.session.accepts(ticket)) hasPass = result
        if (result == null) error = "Couldn’t open recovery. Go back and try again."
    }
    DisposableEffect(owner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_PAUSE) {
                pass = ""; generation++
                if (!prompting) busy = false
            }
        }
        owner.lifecycle.addObserver(observer)
        onDispose { alive = false; generation++; pass = ""; owner.lifecycle.removeObserver(observer) }
    }
    fun reveal() {
        val ticket = app.profiles.session.ticket() ?: return
        val attempt = ++generation
        val password = pass
        busy = true; error = null
        scope.launch {
            val result = withContext(Dispatchers.IO) { runCatching { app.profileWork(ticket) { it.recoveryPhrase(password) } } }
            if (!alive || attempt != generation || !app.profiles.session.accepts(ticket)) return@launch
            busy = false; pass = ""
            if (!owner.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) return@launch
            result.onSuccess(onPhrase).onFailure {
                error = if (it is uniffi.p2pcore.Exception.WrongPassphrase) "That didn’t match. Try again." else "Couldn’t open your backup. Try again."
            }
        }
    }
    fun go() {
        if (busy || hasPass == null) return
        error = null
        if (hasPass == true) { if (pass.isNotEmpty()) reveal(); return }
        val act = ctx.findActivity()
        // DeviceAuth's legacy fallback is permissive. Recovery must not treat no prompt as authentication.
        if (act == null || !DeviceAuth.available(act)) {
            error = "Add a screen lock or an app passphrase to open your backup."
            return
        }
        val ticket = app.profiles.session.ticket() ?: return
        busy = true; prompting = true
        DeviceAuth.ask(act, "Account recovery") { ok ->
            if (alive) {
                prompting = false; busy = false
                if (ok && app.profiles.session.accepts(ticket) && owner.lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) reveal()
            }
        }
    }
    Page {
        TopBar("Account recovery", onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp, vertical = 24.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
            Hint("Only you")
            Lead("Check it’s you before opening your backup.")
            if (hasPass == true) TinField(pass, { pass = it; error = null }, "Your passphrase", password = true, enabled = !busy,
                keyboard = KeyboardOptions(imeAction = ImeAction.Done), actions = KeyboardActions(onDone = { go() }))
            else if (hasPass == false) Hint("Use your phone’s screen lock to continue.")
            Text(error.orEmpty(), Modifier.semantics { liveRegion = LiveRegionMode.Polite }, style = TinType.bodyM, color = Tin.c.er)
        }
        Box(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp)) {
            TinButton(if (hasPass == false) "Use screen lock" else "Continue", ::go, enabled = hasPass != null && (hasPass == false || pass.isNotEmpty()) && !busy)
        }
    }
}

@Composable
fun PhraseShownScreen(phrase: String, onHide: () -> Unit, onRelock: () -> Unit = onHide) {
    val ctx = LocalContext.current
    val view = LocalView.current
    val relock by rememberUpdatedState(onRelock)
    var state by remember(phrase) { mutableStateOf(RecoveryBackupState()) }
    var copied by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }
    val key = remember(phrase) { runCatching { uniffi.p2pcore.recoveryKeyFromPhrase(phrase) }.getOrDefault("") }
    val shown = state.visible(SystemClock.elapsedRealtime())
    LaunchedEffect(state.revealUntil) {
        state.revealUntil?.let { deadline ->
            delay((deadline - SystemClock.elapsedRealtime()).coerceAtLeast(0))
            state = state.hide(); copied = false
        }
    }
    DisposableEffect(view) {
        val observer = view.viewTreeObserver
        val listener = ViewTreeObserver.OnWindowFocusChangeListener { focused -> if (!focused) relock() }
        observer.addOnWindowFocusChangeListener(listener)
        onDispose { if (observer.isAlive) observer.removeOnWindowFocusChangeListener(listener) }
    }
    Page {
        TopBar("Your recovery backup", onHide)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp, vertical = 16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Hint("Only you")
            Lead("Keep this private. Anyone with it can act as you.")
            Row(Modifier.fillMaxWidth().selectableGroup(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf(true to "Key", false to "24 words").forEach { (asKey, label) ->
                    Box(Modifier.weight(1f).heightIn(min = 44.dp).clip(RoundedCornerShape(12.dp)).background(if (state.asKey == asKey) Tin.c.prc else Tin.c.sf)
                        .selectable(state.asKey == asKey, role = Role.Tab) { state = state.select(asKey); copied = false; error = "" }.padding(12.dp), contentAlignment = Alignment.Center) {
                        Text(label, style = TinType.label, color = if (state.asKey == asKey) Tin.c.onPrc else Tin.c.ink)
                    }
                }
            }
            if (state.asKey) KeyCard(if (shown) key else "•••• •••• •••• ••••") else WordGrid(phrase, hidden = !shown)
            if (state.asKey && key.isEmpty()) Hint("Couldn’t prepare the key. Use your 24 words instead.")
            TinButton(if (shown) "Hide" else "Reveal", {
                state = if (shown) state.hide() else state.reveal(SystemClock.elapsedRealtime()); copied = false; error = ""
            }, style = BtnStyle.Outlined, enabled = !state.asKey || key.isNotEmpty())
            Hint("Hides after 30 seconds. Locks when you leave the app.")
            Hint("Key or words restore the same account. Keep either in a private, safe place.")
        }
        Column(Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(error.ifEmpty { if (copied && shown) "Copied. Keep it somewhere private." else "" }, Modifier.semantics { liveRegion = LiveRegionMode.Polite }, style = TinType.bodyM, color = Tin.c.ink2)
            if (state.asKey) TinButton(if (copied) "Copied" else "Copy key", {
                // Check the monotonic deadline at the gesture, not just at the last composition.
                if (state.canCopyKey(SystemClock.elapsedRealtime()) && key.isNotEmpty() && view.hasWindowFocus()) {
                    copied = runCatching { copyToClipboard(ctx, "Recovery key", key) }.isSuccess
                    error = if (copied) "" else "Couldn’t copy. Try again."
                }
            }, enabled = shown && key.isNotEmpty())
            else TinButton("Done", onHide)
        }
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

/**
 * About.svelte: the mark, "Tinline", the version, who makes it, the expandable open-source list and the source. Terms, Privacy and the
 * website follow (Play requires the policy to be reachable in the app; not in the playground), then the emergency-calls note.
 * [libs] is the core's list plus the Android-only libraries; the full licence texts stay one row away.
 */
@Composable
fun AboutScreen(onBack: () -> Unit, onLicences: () -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    val libs = remember { runCatching { uniffi.p2pcore.openSourceLicences().map { it.name to it.licence } }.getOrDefault(emptyList()) + ANDROID_LIBS }
    var open by rememberSaveable { mutableStateOf(false) }
    var logNote by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    // Reading logcat takes a moment; off the main thread, then copy or hand the file to the share sheet.
    fun logs(send: Boolean) = scope.launch {
        logNote = "Collecting the log…"
        val full = withContext(Dispatchers.IO) { DebugLog.collect(ctx) }
        logNote = runCatching {
            if (send) { withContext(Dispatchers.IO) { DebugLog.share(ctx, full) }; null }
            else { copyToClipboard(ctx, "Tinline debug log", DebugLog.forClipboard(full)); "Debug log copied. Paste it into a message." }
        }.getOrElse { "Couldn't get the log: ${friendly(it)}" }
    }
    PfPage("About Tinline", onBack) {
        Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 24.dp, bottom = 8.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(4.dp)) {
            TinMark(56.dp)
            Text("Tinline", Modifier.padding(top = 8.dp), style = PgType.title, color = c.ink)
            Text("Version ${BuildConfig.VERSION_NAME}", style = PgType.sub.copy(fontFeatureSettings = "tnum"), color = c.ink2)
            Text("Made by Osvauld", style = PgType.sub, color = c.ink2)
        }
        PfSec {
            PfCard {
                act {
                    PfAct("Open-source licences", onClick = { open = !open }, icon = PgIcons.Licence, sub = "The libraries Tinline is built on", role = androidx.compose.ui.semantics.Role.Button,
                        trailing = { Icon(if (open) PgIcons.Chevup else PgIcons.Chev, null, Modifier.size(18.dp), tint = c.ink2) })
                }
                if (open) act {
                    Column(Modifier.fillMaxWidth().padding(start = 50.dp, end = 16.dp, top = 4.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        libs.forEach { (n, l) ->
                            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                Text(n, Modifier.weight(1f), style = PgType.sub.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
                                Text(l, style = PgType.sub, color = c.ink2, textAlign = TextAlign.End)
                            }
                        }
                    }
                }
                if (open) act { PfAct("Full licence texts", onClick = onLicences, icon = PgIcons.File, chevron = true) }
                act { PfAct("Source code", onClick = { openUrl(ctx, URL_SOURCE) }, icon = PgIcons.Code, sub = "Tinline is open source", chevron = true) }
            }
            PfNote("Tinline has no ads and no tracking. Your messages and calls go between your devices and the people you talk to.")
        }
        PfSec {
            PfCard {
                act { PfAct("Terms of Use", onClick = { openUrl(ctx, URL_TERMS) }, icon = PgIcons.File, chevron = true) }
                act { PfAct("Privacy Policy", onClick = { openUrl(ctx, URL_PRIVACY) }, icon = PgIcons.Shield, chevron = true) }
                act { PfAct("tinline.osvauld.com", onClick = { openUrl(ctx, URL_SITE) }, icon = PgIcons.Globe, chevron = true) }
            }
            PfNote("Not for emergency calls.")
        }
        PfSec {
            PfCard {
                act { PfAct("Copy debug log", onClick = { logs(false) }, icon = PgIcons.Copy, sub = "Paste it to whoever is helping you fix a problem") }
                act { PfAct("Send debug log", onClick = { logs(true) }, icon = PgIcons.Share, sub = "The full log as a file") }
            }
            PfNote(logNote ?: "The log has what Tinline did on this phone, with ids of contacts and calls. It never has your messages or your root secret.")
        }
    }
}

/** The libraries only the Android app ships (the core's own are `openSourceLicences()`). */
private val ANDROID_LIBS = listOf(
    "ZXing and ZXing Android Embedded" to "Apache-2.0", "JNA" to "Apache-2.0 or LGPL-2.1+", "AndroidX and Jetpack Compose" to "Apache-2.0",
    "libyuv (in CameraX)" to "BSD", "Figtree and IBM Plex Mono" to "SIL OFL 1.1",
)

@Composable
fun LicencesScreen(onBack: () -> Unit) {
    val ctx = LocalContext.current
    val c = Tin.c
    val texts = remember {
        listOf(
            "libaom-LICENSE.txt" to "libaom (BSD-2-Clause)", "libaom-PATENTS.txt" to "libaom (Alliance for Open Media Patent License 1.0)",
            "rav1d-COPYING.txt" to "rav1d (BSD-2-Clause)",
            "Figtree-OFL.txt" to "Figtree (SIL Open Font License 1.1)", "IBMPlexMono-OFL.txt" to "IBM Plex Mono (SIL Open Font License 1.1)",
        )
            .map { (f, t) -> t to runCatching { ctx.assets.open("licenses/$f").bufferedReader().readText() }.getOrDefault("") }
    }
    val libs = listOf(
        "Tinline — GPL-3.0-or-later", "iroh and the Rust crates in the core — Apache-2.0 / MIT", "Opus — BSD-3-Clause",
        "libaom AV1 encoder — BSD-2-Clause + AOMedia Patent License 1.0", "rav1d AV1 decoder — BSD-2-Clause",
        "ZXing and zxing-android-embedded — Apache-2.0", "JNA — Apache-2.0 / LGPL-2.1", "AndroidX, Jetpack Compose, Material Symbols — Apache-2.0",
        "BIP-39 English word list — MIT",
    )
    Page {
        TopBar("Open-source licences", onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            libs.forEach { Text(it, style = TinType.bodyL.copy(fontSize = 15.sp), color = c.ink) }
            texts.forEach { (t, body) ->
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
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            ListItem("Test tone", sub = "Send a 440 Hz tone instead of the microphone", trailing = {
                TinSwitch(tone, { tone = it; app.testToneHz = if (it) 440f else null; app.node.setTestTone(app.testToneHz) }, "Test tone")
            })
            // The call/notification transitions ([CallTrace]): short call ids and states only.
            ListItem("Copy call log", sub = "Recent call and notification changes, no names", onClick = {
                copyToClipboard(app, "Tinline call log", CallTrace.dump().ifEmpty { "(empty)" })
            })
            Hint("p2pcore ${uniffi.p2pcore.coreVersion()}", Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            if (BuildConfig.VIDEO_TUNING) VideoTuningPanel(app)
        }
    }
}
