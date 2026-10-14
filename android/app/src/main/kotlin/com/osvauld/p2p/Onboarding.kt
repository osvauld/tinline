@file:OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)

package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/*
 * Sign-up, restore and link on a new install: the playground's gate (docs/design/playground/src/screens/gate/), Android form:
 * a full-screen stepped flow with the progress segments in the header and a 36 px slide between steps. The account logic
 * (create / restore / link, persist, save-failed) is unchanged from before the port.
 *
 * Kept as the app had them (SCREENS-HANDOFF section 7, not decided in the playground): no passphrase strength bars, the
 * phrase is shown without a blur, no "Remember on this phone" switch, no camera row in Permissions, and separate end
 * screens (Restored, Syncing) instead of one shared Done step.
 */

private enum class Step { Welcome, Choose, Done, Name, Pass, Phrase, Confirm, Device, Restored, Terms, Perms, Restore, SaveFailed, PassAdd, LinkName, LinkQr, LinkPass, Syncing }

/** Older onboarding frame, still used by a few non-gate screens. Same furniture as the gate now. */
@Composable
fun StepFrame(
    progress: Int?, onBack: (() -> Unit)?, modifier: Modifier = Modifier,
    footer: @Composable ColumnScope.() -> Unit = {}, body: @Composable ColumnScope.() -> Unit,
) {
    Page {
        GateHead(onBack, progress?.let { it to 6 })
        Column(
            modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 8.dp, bottom = 16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp), content = body,
        )
        Column(Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, bottom = 24.dp, top = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp), content = footer)
    }
}

@Composable
fun H1(text: String, modifier: Modifier = Modifier, align: TextAlign = TextAlign.Start) =
    Text(text, modifier, style = TinType.h1, color = Tin.c.ink, textAlign = align)

@Composable
fun Lead(text: String, modifier: Modifier = Modifier, align: TextAlign = TextAlign.Start) =
    Text(text, modifier, style = TinType.bodyL, color = Tin.c.ink2, textAlign = align)

/** Position in the flow (gate.svelte.ts progressOf): create, restore and link each have their own order. */
private fun progressOf(step: Step, restoring: Boolean, linking: Boolean, forgot: Boolean, adding: Boolean): Pair<Int, Int>? {
    if (forgot) return null
    // Welcome and Choose are step 0 of the create count: the header and its empty line are already there.
    if ((step == Step.Welcome || step == Step.Choose) && !adding) return 0 to 6
    val tail = if (adding) emptyList() else listOf(Step.Terms, Step.Perms)
    val order = when {
        linking -> listOf(Step.LinkName, Step.LinkQr, Step.LinkPass) + tail
        restoring -> listOf(Step.Restore, Step.Name, Step.Pass, Step.Device) + tail
        else -> listOf(Step.Name, Step.Pass, Step.Phrase, Step.Device) + tail
    }
    val i = order.indexOf(step)
    return if (i < 0) null else (i + 1) to order.size
}

private val STEP_LABEL = mapOf(
    Step.Name to "Your nickname", Step.Pass to "Passphrase", Step.Phrase to "Recovery phrase", Step.Device to "Device name", Step.Terms to "Terms",
    Step.Perms to "Permissions", Step.Restore to "Restore account", Step.LinkName to "Device name", Step.LinkQr to "Link to your account", Step.LinkPass to "Passphrase",
)

/**
 * First run: Welcome - Name - Passphrase (optional) - Recovery phrase (optional Check my copy) - Name this phone - Terms -
 * Permissions, or the restore path (recovery words - name - passphrase - Name this phone - Restored - Terms - Permissions).
 * [forgot] is the "forgot my passphrase" restore over a locked identity: words, new passphrase, done. When [P2pApp.adding] is set
 * the flow adds an account next to the existing ones: it starts at the name or the words, skips Terms and Permissions, and
 * backing out returns to the account that was left.
 */
@Composable
fun OnboardingFlow(
    app: P2pApp, missing: List<Need>, onFix: (Need) -> Unit, forgot: Boolean = false,
    onCancelForgot: () -> Unit = {}, onAddFirst: (() -> Unit)? = null, onDone: () -> Unit,
) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val addState = remember { app.adding.value }
    val adding = addState != null
    var step by rememberSaveable { mutableStateOf(when { forgot -> Step.Restore; adding -> if (addState!!.link) Step.LinkName else if (addState.restore) Step.Restore else Step.Name; else -> Step.Welcome }) }
    var dir by remember { mutableIntStateOf(1) }
    var name by rememberSaveable { mutableStateOf(if (forgot) app.node.profile()?.name ?: "" else "") }
    var restoring by rememberSaveable { mutableStateOf(forgot || addState?.restore == true) }
    var linking by rememberSaveable { mutableStateOf(addState?.link == true) }
    // Secrets are never saved into the instance-state bundle.
    var words by remember { mutableStateOf("") }
    var pass by remember { mutableStateOf("") }
    var phrase by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var created by remember { mutableStateOf(false) }
    var existsDialog by remember { mutableStateOf(false) }
    var chooseAccount by remember { mutableStateOf(false) }
    var saveError by remember { mutableStateOf("") }
    // Linking to an existing account: set once the account has arrived (the peer's name, for the Syncing screen).
    var linkedPeer by remember { mutableStateOf<String?>(null) }
    var linkedName by remember { mutableStateOf("") }
    var label by rememberSaveable { mutableStateOf("Personal phone") }
    // "Copy my words" was pressed: coming back from Check my copy keeps "I've saved them" (gate.phraseShown).
    var phraseCopied by rememberSaveable { mutableStateOf(false) }
    // The identity appeared some other way (debug hook, a restored process): leave onboarding.
    val has by app.hasIdentity.collectAsState()
    LaunchedEffect(has, created) { if (!forgot && !adding && has && !created && !busy && step != Step.LinkQr) onDone() }

    fun go(s: Step) { dir = 1; step = s }
    fun back(s: Step) { dir = -1; step = s }
    fun finish() { app.finishAdding(); onDone() }
    fun cancelAdd() {
        busy = true
        scope.launch { withContext(Dispatchers.IO) { runCatching { app.cancelAdding() } }; busy = false; onDone() }
    }
    fun afterTerms() { if (adding) finish() else go(Step.Terms) }
    fun afterDevice() { if (restoring && !forgot) go(Step.Restored) else afterTerms() }

    /** The identity exists in memory (or on disk): make it durable (S1), then go on. */
    fun persistThenContinue() {
        busy = true; error = null
        scope.launch {
            val err = withContext(Dispatchers.IO) { app.persistIdentity() }
            busy = false
            if (err != null) { saveError = err; go(Step.SaveFailed); return@launch }
            withContext(Dispatchers.IO) { app.startServices() }
            when {
                forgot -> onDone()
                linkedPeer != null -> go(Step.Syncing)
                phrase != null -> go(Step.Phrase)
                else -> go(Step.Device)
            }
        }
    }

    // An empty [pass] means no passphrase.
    fun finishIdentity(pass: String) {
        busy = true; error = null
        scope.launch {
            val r = withContext(Dispatchers.IO) {
                runCatching {
                    when {
                        forgot -> { app.restoreOverLocked(words, name.trim(), pass); null }
                        restoring -> { app.node.restoreIdentity(words, name.trim(), pass); null }
                        else -> app.node.createIdentity(name.trim(), pass)
                    }
                }
            }
            busy = false
            r.onFailure {
                error = friendly(it)
                if (it is uniffi.p2pcore.Exception.AccountExists) { existsDialog = true; back(Step.Restore) }
                // A bad phrase belongs on the words screen.
                else if (it is uniffi.p2pcore.Exception.BadPhrase || (it is IllegalArgumentException && forgot)) back(Step.Restore)
            }
            r.onSuccess { p ->
                created = true
                phrase = p
                persistThenContinue()
            }
        }
    }

    val prog = progressOf(step, restoring, linking, forgot, adding)
    GateStage(step, dir) { s ->
        Column(Modifier.fillMaxSize()) {
            val head: @Composable (onBack: (() -> Unit)?) -> Unit = { b -> GateHead(b, if (s == step) prog else progressOf(s, restoring, linking, forgot, adding), STEP_LABEL[s] ?: "") }
            when (s) {
                Step.Welcome -> { head(null); WelcomeStep(adding = false,
                    onCreate = { restoring = false; linking = false; go(Step.Name) },
                    onHave = { go(Step.Choose) }) }
                Step.Choose -> { head { back(Step.Welcome) }; ChooseStep(
                    onLink = { restoring = false; linking = true; go(Step.LinkName) },
                    onRestore = { restoring = true; linking = false; go(Step.Restore) }) }
                Step.Done -> { head(null); DoneStep(name, onAdd = { (onAddFirst ?: onDone)() }, onNotNow = onDone) }
                Step.LinkName -> { head { if (adding) cancelAdd() else back(Step.Choose) }
                    DeviceStep(label, { label = it; error = null }, false, null, onNext = { label = label.trim(); go(Step.LinkQr) }) }
                Step.LinkQr -> LinkSession(app, newDevice = true, label = label.trim(), onClose = { back(Step.LinkName) }, onDone = { d ->
                    created = true; linkedPeer = d.peer.ifBlank { "your other device" }; linkedName = d.name; go(Step.LinkPass)
                }, progress = prog)
                Step.LinkPass -> { head(null); PassStep(PassKind.Link, pass, { pass = it; error = null }, busy, error,
                    onNext = {
                        val p = pass; pass = ""; busy = true; error = null
                        scope.launch {
                            val r = withContext(Dispatchers.IO) { runCatching { app.node.setPassphrase(null, p) } }
                            busy = false
                            r.onFailure { error = friendly(it) }
                            r.onSuccess { persistThenContinue() }
                        }
                    },
                    onSkip = { pass = ""; persistThenContinue() }) }
                Step.Syncing -> {
                    val contacts by app.contacts.collectAsState()
                    val calls by app.history.collectAsState()
                    head(null)
                    SyncingStep(app.node.profile()?.name ?: linkedName, linkedPeer.orEmpty(), contacts.size, calls.size, onStart = { afterTerms() })
                }
                Step.Restore -> { head { if (forgot) onCancelForgot() else if (adding) cancelAdd() else back(Step.Choose) }
                    RestoreStep(
                        error = error,
                        warning = if (forgot) "Restoring keeps the same identity: use the recovery phrase of this account. You’ll set a new passphrase."
                        else "Next, you’ll choose a passphrase for this account.",
                        onError = { error = it },
                        onSubmit = { w -> words = w; error = null; go(if (forgot) Step.Pass else Step.Name) },
                    ) }
                Step.Name -> { head { if (restoring) back(Step.Restore) else if (adding) cancelAdd() else back(Step.Welcome) }
                    NameStep(name, { name = it }, onNext = { go(Step.Pass) }) }
                Step.Pass -> { head { back(if (forgot) Step.Restore else Step.Name) }
                    PassStep(if (forgot) PassKind.Forgot else if (restoring) PassKind.Restore else PassKind.Create, pass, { pass = it; error = null }, busy, error,
                        onNext = { val p = pass; pass = ""; finishIdentity(p) },
                        onSkip = { pass = ""; finishIdentity("") }) }
                Step.Phrase -> { head(null); PhraseStep(phrase.orEmpty(), onNext = { go(Step.Device) }, onCheck = { go(Step.Confirm) },
                    copiedAlready = phraseCopied, onCopied = { phraseCopied = true }) }
                Step.Confirm -> { head { back(Step.Phrase) }; ConfirmStep(phrase.orEmpty(), onDone = { back(Step.Phrase) }) }
                Step.Device -> { head(null); DeviceStep(label, { label = it; error = null }, busy, error, onNext = {
                    busy = true; error = null
                    scope.launch {
                        val r = withContext(Dispatchers.IO) { runCatching { app.node.setDeviceLabel(label.trim()) } }
                        busy = false
                        r.onFailure { error = friendly(it) }
                        r.onSuccess { afterDevice() }
                    }
                }) }
                Step.Restored -> { head(null); RestoredStep(app.node.profile()?.name ?: name, adding, onStart = { afterTerms() }) }
                Step.Terms -> { head(null); TermsStep { LegalStore.accept(ctx); go(Step.Perms) } }
                // A new account ends on Done ("Your line is open"); restore and link already had their own end screen.
                Step.Perms -> { head(null); PermsStep(missing, onFix, onDone = { if (!restoring && !linking) go(Step.Done) else onDone() }) }
                Step.SaveFailed -> { head(null); SaveFailedStep(saveError, busy,
                    onRetry = { persistThenContinue() }, onPassphrase = { pass = ""; error = null; go(Step.PassAdd) }) }
                Step.PassAdd -> { head { back(Step.SaveFailed) }
                    PassStep(PassKind.Save, pass, { pass = it; error = null }, busy, error,
                        onNext = {
                            val p = pass; pass = ""; busy = true; error = null
                            scope.launch {
                                val r = withContext(Dispatchers.IO) { runCatching { app.node.setPassphrase(null, p) } }
                                busy = false
                                r.onFailure { error = friendly(it) }
                                r.onSuccess { persistThenContinue() }
                            }
                        }, onSkip = {}) }
            }
        }
    }
    if (existsDialog) TinDialog("Already on this phone", { existsDialog = false }, "Choose account", { existsDialog = false; chooseAccount = true },
        dismiss = "Not now") {
        DialogText("That recovery phrase belongs to an account that is already here. Switch to it instead of adding it again.")
    }
    if (chooseAccount) AccountSheet(app, onDismiss = { chooseAccount = false }, onPick = { did ->
        chooseAccount = false; busy = true
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { app.switchTo(did, null) } }
            busy = false
            r.onFailure { error = friendly(it) }
            r.onSuccess { finish() }
        }
    })
}

// ------------------------------------------------------------------ welcome (Welcome.svelte)

@Composable
private fun WelcomeStep(adding: Boolean, onCreate: () -> Unit, onHave: () -> Unit, onCancel: () -> Unit = {}) {
    val c = Tin.c
    // Laid out like every step: title and lead top-start, the mark (slack: nobody on the line yet) filling the free space,
    // the emergency note at the bottom, the two choices stacked full width with "Create my account" on top (thumb reach).
    GateStep(
        fill = true,
        title = if (adding) "Add an account" else "A direct line to your people",
        lead = if (adding) "Create a new account or bring one you already have.\nYour current account stays on this phone."
        else "Tinline connects your phone straight to theirs.\nYou just need a nickname. Encrypted end to end.",
        foot = {
            GBtn("Create my account", onCreate)
            GBtn("I have an account", onHave, kind = GKind.Secondary)
            if (adding) GBtn("Cancel", onCancel, kind = GKind.Text)
        },
    ) {
        Box(Modifier.fillMaxWidth().weight(1f).heightIn(min = 120.dp), contentAlignment = Alignment.Center) {
            TinMark(120.dp, live = true, lineColor = true, description = "Tinline")
        }
        Text(buildAnnotatedString {
            pushStyle(SpanStyle(fontWeight = FontWeight.Bold)); append("Not for emergency calls."); pop()
            append("\nUse your phone’s dialer to reach emergency services.")
        }, style = PgType.sub, color = c.ink2)
    }
}

// ------------------------------------------------------------------ choose (Choose.svelte)

@Composable
private fun ChooseStep(onLink: () -> Unit, onRestore: () -> Unit) {
    GateStep("Use your account here", fill = true, lead = "Your account is its recovery phrase, so you can bring it to this phone two ways.") {
        // Touch: the choices sit at the bottom, in thumb reach, where Welcome's buttons were.
        Spacer(Modifier.weight(1f))
        Column(Modifier.padding(bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            ChooseOpt(PgIcons.Qr, "Link my other device", "Scan a code on a device that already has your account. Your contacts and chats come along.", onLink)
            ChooseOpt(PgIcons.Key, "Restore my account", "Type your 24 recovery words. This phone starts with an empty contact list.", onRestore)
        }
    }
}

@Composable
private fun ChooseOpt(icon: ImageVector, title: String, sub: String, onClick: () -> Unit) {
    val c = Tin.c
    val shape = SmoothCornerShape(14.dp)
    Row(
        Modifier.fillMaxWidth().heightIn(min = 76.dp).clip(shape).background(c.sf).border(1.dp, c.ln, shape)
            .clickable(role = Role.Button, onClick = onClick).padding(16.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        IconDisc(icon, 44.dp, 24.dp)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, style = PgType.body.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
            Text(sub, style = PgType.sub, color = c.ink2)
        }
        Icon(PgIcons.Chevr, null, Modifier.size(20.dp), tint = c.ink2)
    }
}

// ------------------------------------------------------------------ done (Done.svelte)

@Composable
private fun DoneStep(name: String, onAdd: () -> Unit, onNotNow: () -> Unit) {
    val first = name.trim().split(Regex("\\s+")).firstOrNull().orEmpty()
    // The line arrives slack and pulls tight 450 ms after the step arrives, in two tugs on the `line` cue's two notes; the cue
    // plays on the first tug, and the line warms from faded gold to gold. Reduced motion: it settles taut, colour and cue still come.
    var taut by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { kotlinx.coroutines.delay(450); taut = true }
    GateStep(
        title = if (first.isNotBlank()) "Your line is open, $first" else "Your line is open",
        lead = "Scan their code or share yours, and you can call each other.",
        center = true,
        hero = { Box(Modifier.padding(top = 28.dp, bottom = 4.dp)) {
            TinMark(96.dp, state = if (taut) MarkState.Taut else MarkState.Slack, live = true, lineColor = true, spring = MarkSpring.Line,
                onTug = { n -> if (n == 1) Cues.line() }, description = "Tinline")
        } },
        foot = {
            GBtn("Add my first contact", onAdd)
            GBtn("Not now", onNotNow, kind = GKind.Text)
        },
    ) {
        // Two gentle "good to know" notes, said as tips at the happy moment, not as warnings.
        Column(Modifier.widthIn(max = 420.dp).padding(top = 8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            DoneTip(PgIcons.Devices, "Two devices are better than one", "Link another device when you can, so losing one never locks you out.")
            DoneTip(PgIcons.Laptop, "Groups are hosted on a computer", "To start a group, add Tinline on a computer that can stay on.")
        }
    }
}

@Composable
private fun DoneTip(icon: ImageVector, title: String, body: String) {
    val c = Tin.c
    Row(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.Top) {
        IconDisc(icon, 36.dp, 20.dp)
        Column {
            Text(title, style = PgType.body.copy(fontWeight = FontWeight.Bold), color = c.ink)
            Text(body, style = PgType.sub19, color = c.ink2)
        }
    }
}

/** Welcome on its own (kept for callers outside the flow). */
@Composable
fun WelcomeScreen(onStart: () -> Unit, onRestore: () -> Unit, onLink: () -> Unit = {}) = Page { GateHead(null, 0 to 6); WelcomeStep(false, onStart, onRestore) }

// ------------------------------------------------------------------ name (Name.svelte)

@Composable
internal fun NameStep(name: String, onChange: (String) -> Unit, onNext: () -> Unit) {
    val next = { if (name.isNotBlank()) onNext() }
    GateStep("What should people call you?", lead = "Only the people you add will see it.",
        foot = { GNextRow({ GNext("Continue", next, enabled = name.isNotBlank()) }) }) {
        GField(name, onChange, "Your nickname", hint = "You can change it any time in Settings.", focus = rememberAutofocus(),
            keyboard = KeyboardOptions(capitalization = KeyboardCapitalization.Words, imeAction = ImeAction.Done), onDone = next)
    }
}

/** StepShell focuses the first field on arrival ([data-autofocus]). */
@Composable
fun rememberAutofocus(): androidx.compose.ui.focus.FocusRequester {
    val f = remember { androidx.compose.ui.focus.FocusRequester() }
    LaunchedEffect(Unit) { kotlinx.coroutines.delay(PgMotion.ENTER.toLong()); runCatching { f.requestFocus() } }
    return f
}

// ------------------------------------------------------------------ passphrase (Pass.svelte)

private enum class PassKind { Create, Restore, Forgot, Link, Save }

@Composable
private fun PassStep(kind: PassKind, pass: String, onPass: (String) -> Unit, busy: Boolean, error: String?, onNext: () -> Unit, onSkip: () -> Unit) {
    val c = Tin.c
    val title = when (kind) { PassKind.Forgot -> "Choose a new passphrase"; PassKind.Link -> "Protect this phone"; else -> "Add a passphrase" }
    val cta = when (kind) { PassKind.Forgot, PassKind.Restore -> "Restore account"; PassKind.Link -> "Add passphrase"; PassKind.Save -> "Save account"; PassKind.Create -> "Continue" }
    val submit = { if (pass.isNotEmpty() && !busy) onNext() }
    // Says what the lock gives you (tone pass 2026-10-13); the way back comes before anything that sounds final.
    GateStep(title, lead = "Optional. A lock, so only you can open Tinline on this phone.", foot = {
        val skip: (@Composable () -> Unit)? = if (kind != PassKind.Save) ({ GBtn("Skip for now", onSkip, kind = GKind.Text, enabled = !busy, fill = false) }) else null
        if (kind == PassKind.Create) GNextRow({ GNext(if (busy) "Encrypting…" else cta, submit, enabled = pass.isNotEmpty(), busy = busy) }, skip)
        else { GBtn(if (busy) "Encrypting…" else cta, submit, enabled = pass.isNotEmpty(), busy = busy); skip?.invoke() }
    }) {
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            GField(pass, onPass, "Passphrase", secret = true, enabled = !busy, focus = rememberAutofocus(), keyboard = KeyboardOptions(imeAction = ImeAction.Done), onDone = submit)
            StrengthMeter(pass)
        }
        GNotice("Encrypts your account on this phone. Your phone remembers it, so calls still ring after a restart. If you forget it, your recovery words let you set a new one.", icon = PgIcons.Lock)
        if (error != null) GNotice(error, NKind.Error)
    }
}

/** Pass.svelte: three 28 x 6 bars and one line; every line shares one cell (the tallest sets the height), so nothing below moves. */
@Composable
private fun StrengthMeter(pass: String) {
    val c = Tin.c
    val words = pass.trim().split(Regex("\\s+")).size
    val s = when { pass.isEmpty() -> 0; pass.length >= 16 || words >= 4 -> 3; pass.length >= 8 -> 2; else -> 1 }
    val says = listOf("Any length works. A few random words are easy to remember.", "Short. A few more words make it stronger.", "Fair", "Strong")
    val col = when (s) { 1 -> c.danger; 2 -> c.thread; else -> c.accent }
    Row(Modifier.fillMaxWidth().semantics { liveRegion = androidx.compose.ui.semantics.LiveRegionMode.Polite }, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(Modifier.padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            for (i in 1..3) Box(Modifier.size(28.dp, 6.dp).clip(RoundedCornerShape(3.dp)).background(if (s >= i) col else c.ln))
        }
        Box(Modifier.weight(1f)) {
            says.forEachIndexed { i, t -> Text(t, Modifier.graphicsLayer { alpha = if (i == s) 1f else 0f }, style = PgType.sub, color = c.ink2) }
        }
    }
}

/** One passphrase field (Settings' add / change and the legacy seal screen). */
@Composable
fun NewPassphraseField(pass: String, onPass: (String) -> Unit, enabled: Boolean = true, onDone: () -> Unit = {}, label: String = "Passphrase", why: Boolean = true) {
    GField(pass, onPass, label, secret = true, enabled = enabled, keyboard = KeyboardOptions(imeAction = ImeAction.Done), onDone = { if (pass.isNotEmpty() && enabled) onDone() })
    if (why) GNotice("Encrypts your account on this phone. Your phone remembers it, so calls still ring after a restart. If you forget it, your recovery words let you set a new one.", icon = PgIcons.Lock)
}

// ------------------------------------------------------------------ recovery phrase (Phrase.svelte, Confirm.svelte)

/** Two columns, column-major (1-12 left, 13-24 right): cells 32 tall, gap 4, r8, line border, number 20 wide, mono 15/20. */
@Composable
fun WordGrid(phrase: String, hidden: Boolean = false) {
    val c = Tin.c
    val words = phrase.trim().split(Regex("\\s+")).filter { it.isNotEmpty() }
    val half = (words.size + 1) / 2
    Column(Modifier.padding(4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        for (r in 0 until half) Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            for (col in 0..1) {
                val i = r + col * half
                if (i < words.size) Row(
                    Modifier.weight(1f).heightIn(min = 32.dp).clip(SmoothCornerShape(8.dp)).background(c.sf).border(1.dp, c.ln, SmoothCornerShape(8.dp)).padding(horizontal = 12.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Text("${i + 1}", Modifier.width(20.dp), style = PgType.meta.copy(fontFeatureSettings = "tnum"), color = c.ink2, textAlign = TextAlign.End)
                    Text(if (hidden) "••••••" else words[i], style = PgType.mono, color = if (hidden) c.ink2 else c.ink, maxLines = 1)
                } else Spacer(Modifier.weight(1f))
            }
        }
    }
}

@Composable
internal fun PhraseStep(phrase: String, onNext: () -> Unit, onCheck: () -> Unit, copiedAlready: Boolean = false, onCopied: () -> Unit = {}) {
    val ctx = LocalContext.current
    // One primary slot, two moments: "Copy my words" first; once copied the same button says "I've saved them" (same size, so
    // nothing moves). Calm, never alarming: no blur, no warning box; paper is a suggestion, not a rule.
    var copied by remember { mutableStateOf(copiedAlready) }
    // Words or key: the same secret two ways (core recovery_key.rs). Words suit paper, the key a password manager. Either
    // copy counts as saved; the key gets no quiz (its checksum catches a bad copy when it is pasted).
    var asKey by remember { mutableStateOf(false) }
    val key = remember(phrase) { runCatching { uniffi.p2pcore.recoveryKeyFromPhrase(phrase) }.getOrDefault("") }
    GateStep(if (asKey) "Here is your recovery key" else "Here are your recovery words",
        lead = if (asKey) "Your recovery key is the same as your 24 words, as one line.\nPaste it somewhere safe. A password manager works great."
            else "If you ever switch devices, these 24 words bring your account along.\nSave a copy somewhere safe. Paper works great.",
        foot = {
            if (copied) {
                GBtn(if (asKey) "I’ve saved it" else "I’ve saved them", onNext)
                if (!asKey) GBtn("Check my copy", onCheck, kind = GKind.Text)
            } else {
                // flagged sensitive (EXTRA_IS_SENSITIVE), so Android 13+ keeps it out of the clipboard preview
                GBtn(if (asKey) "Copy my key" else "Copy my words", {
                    if (asKey) copyToClipboard(ctx, "Recovery key", key) else copyToClipboard(ctx, "Recovery words", phrase)
                    copied = true; onCopied()
                })
                GBtn(if (asKey) "I saved it elsewhere" else "I wrote them down", onNext, kind = GKind.Text)
            }
        }) {
        WordsKeySeg(asKey) { asKey = it }
        if (asKey) KeyCard(key) else WordGrid(phrase)
    }
}

/** Positions asked on Check my copy (gate.svelte.ts CONFIRM_AT). */
private val CONFIRM_AT = listOf(4, 11, 19)

@Composable
internal fun ConfirmStep(phrase: String, onDone: () -> Unit) {
    val words = remember(phrase) { phrase.trim().split(Regex("\\s+")) }
    val answers = remember { mutableStateListOf("", "", "") }
    var tried by remember { mutableStateOf(false) }
    fun wrong(i: Int) = tried && answers[i].trim().lowercase() != words.getOrNull(CONFIRM_AT[i] - 1)
    val submit = { tried = true; if (CONFIRM_AT.indices.none { answers[it].trim().lowercase() != words.getOrNull(CONFIRM_AT[it] - 1) }) onDone() }
    val first = rememberAutofocus()
    GateStep("Check my copy", lead = "Optional. Type a few words from your saved copy, to be sure it’s right.", foot = {
        GBtn("Done", submit, enabled = answers.none { it.isBlank() })
        GBtn("Back to the words", onDone, kind = GKind.Text)
    }) {
        Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            CONFIRM_AT.forEachIndexed { i, p ->
                GField(answers[i], { answers[i] = it; tried = false }, "Word $p", mono = true, reserveLines = 1f, focus = if (i == 0) first else null,
                    error = if (wrong(i)) "That isn’t word $p. Check your saved copy." else null,
                    keyboard = KeyboardOptions(imeAction = if (i == 2) ImeAction.Done else ImeAction.Next, autoCorrectEnabled = false))
            }
        }
    }
}

// ------------------------------------------------------------------ device name (Device.svelte)

@Composable
private fun DeviceStep(label: String, onChange: (String) -> Unit, busy: Boolean, error: String?, onNext: () -> Unit) {
    val next = { if (label.isNotBlank() && !busy) onNext() }
    GateStep("Name this phone", lead = "So you can tell your devices apart in Linked devices. Only your own devices see this name; people you call never do.",
        foot = { GNextRow({ GNext("Continue", next, enabled = label.isNotBlank(), busy = busy) }) }) {
        GField(label, onChange, "Device name", hint = "You can rename it later.", enabled = !busy, focus = rememberAutofocus(),
            keyboard = KeyboardOptions(capitalization = KeyboardCapitalization.Words, imeAction = ImeAction.Done), onDone = next)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf("Personal phone", "Work phone", "Tablet").forEach { s -> PgChip(s, label.trim() == s, { onChange(s) }, minHeight = 44.dp, padH = 16.dp) }
        }
        if (error != null) GNotice(error, NKind.Error)
    }
}

/** Kept for callers outside the flow. */
@Composable
fun DeviceNameScreen(label: String, onChange: (String) -> Unit, busy: Boolean, error: String?, onNext: () -> Unit, progress: Int? = 4, cta: String = "Continue", onBack: (() -> Unit)? = null) =
    Page { GateHead(onBack, progress?.let { it to 6 }); DeviceStep(label, onChange, busy, error, onNext) }

// ------------------------------------------------------------------ restored / linked (the app's own end screens, Done.svelte look)

@Composable
private fun RestoredStep(name: String, adding: Boolean, onStart: () -> Unit) {
    val first = name.trim().split(Regex("\\s+")).firstOrNull().orEmpty()
    GateStep(
        title = if (adding) "Added ${name.trim().ifBlank { "your account" }} to this phone" else if (first.isNotBlank()) "Welcome back, $first" else "Welcome back",
        lead = if (adding) "Only the account you select is online at a time, so ${name.trim().ifBlank { "this account" }} is online now and your other account is paused. You can switch in Settings."
        else "Your account is back on this phone. Your contacts, chats and call history are kept on your other devices, not with us. This phone starts with an empty contact list.",
        center = true, bigTitle = true,
        hero = { Box(Modifier.padding(top = 28.dp, bottom = 4.dp)) { TinMark(96.dp, state = MarkState.Taut, live = true, description = "Tinline") } },
        foot = { GBtn(if (adding) "Open Tinline" else "Continue", onStart) },
    )
}

@Composable
private fun SyncingStep(name: String, peer: String, contacts: Int, calls: Int, onStart: () -> Unit) {
    val c = Tin.c
    var slow by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { kotlinx.coroutines.delay(45_000); slow = true }
    val from = peer.ifBlank { "your other device" }
    GateStep(
        title = "Linked to $from", center = true, bigTitle = true,
        lead = "Your contacts and chats are arriving from $from and will keep in step on both.",
        hero = { Box(Modifier.padding(top = 28.dp, bottom = 4.dp)) { TinMark(96.dp, state = MarkState.Taut, live = true, description = "Tinline") } },
        foot = { GBtn("Open Tinline", onStart) },
    ) {
        PfCard {
            row { SyncRow(PgIcons.Users, "Contacts", if (contacts > 0) "$contacts" else "Waiting", contacts > 0) }
            row { SyncRow(Icons.Rounded.History, "Call history", if (calls > 0) "$calls" else "Waiting", calls > 0) }
        }
        ReservedLine(if (slow && contacts == 0 && calls == 0) "Nothing yet. The other device may be offline: it picks up where it left off the next time both are online."
            else "Keep both devices online. Sync keeps going in the background.", style = PgType.sub19, color = c.ink2, align = TextAlign.Center, lines = 2)
    }
}

@Composable
private fun SyncRow(icon: ImageVector, title: String, value: String, done: Boolean) {
    val c = Tin.c
    PfAct(title, icon = icon, minHeight = 56.dp, trailing = {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            if (done) Icon(PgIcons.Check, null, Modifier.size(18.dp), tint = c.accentInk)
            Text(value, Modifier.widthIn(min = 56.dp), style = PgType.sub.copy(fontWeight = FontWeight.SemiBold, fontFeatureSettings = "tnum"),
                color = if (done) c.accentInk else c.ink2, textAlign = TextAlign.End)
        }
    })
}

/** Kept for callers outside the flow (Link.kt used it before the port). */
@Composable
fun SyncingScreen(name: String, peer: String, contacts: Int, calls: Int, onStart: () -> Unit) = Page { GateHead(null, null); SyncingStep(name, peer, contacts, calls, onStart) }

// ------------------------------------------------------------------ save failed (Failed.svelte, savefailed)

@Composable
private fun SaveFailedStep(message: String, busy: Boolean, onRetry: () -> Unit, onPassphrase: () -> Unit) {
    GateStep("Couldn’t save your account", lead = "Tinline couldn’t store the key that opens your account on this phone, so your account has not been saved yet.",
        center = true, anchor = true,
        hero = { Box(Modifier.padding(top = 12.dp)) { IconDisc(PgIcons.Warning, 72.dp, 32.dp, danger = true) } },
        foot = {
            GBtn(if (busy) "Trying…" else "Try again", onRetry, busy = busy)
            GBtn("Add a passphrase instead", onPassphrase, kind = GKind.Text, enabled = !busy)
        }) {
        GNotice("Nothing was kept on this phone yet, so there is nothing to lose. Try again, or protect the account with a passphrase instead of the phone’s own key.")
        if (message.isNotBlank()) GNotice(message, NKind.Error)
    }
}

// ------------------------------------------------------------------ terms (Terms.svelte)

@Composable
private fun TermsStep(onAgree: () -> Unit) {
    val c = Tin.c
    // No checkbox: the line right above the button is the consent, and the button ("I agree", the arrow) is the agreement.
    GateStep("Before you start", fill = true, foot = { GNextRow({ GNext("I agree", onAgree) }) }) {
        Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            TermPoint(PgIcons.Shield, "Free of ads and tracking.", "Tinline doesn’t collect your contacts, calls or location.")
            TermPoint(PgIcons.Lock, "Calls are end-to-end encrypted.", "When a direct line isn’t possible, an encrypted relay passes the call along. It can’t hear it.")
            TermPoint(PgIcons.Key, "Your account is yours.", "It lives on your devices, not on a server. Your recovery words are the way back in, so keep them safe.")
            TermPoint(PgIcons.Warning, "Not for emergency calls.", "")
        }
        Spacer(Modifier.weight(1f))
        val link = SpanStyle(color = c.accentInk, textDecoration = TextDecoration.Underline)
        Text(buildAnnotatedString {
            append("By continuing, you agree to the ")
            withLink(LinkAnnotation.Url(URL_TERMS, TextLinkStyles(link))) { append("Terms of Use") }
            append(" and have read the ")
            withLink(LinkAnnotation.Url(URL_PRIVACY, TextLinkStyles(link))) { append("Privacy Policy") }
            append(".")
        }, style = PgType.sub19, color = c.ink2)
    }
}

@Composable
private fun TermPoint(icon: ImageVector, bold: String, rest: String) {
    val c = Tin.c
    Row(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.Top) {
        IconDisc(icon, 36.dp, 20.dp)
        Text(buildAnnotatedString {
            pushStyle(SpanStyle(color = c.ink, fontWeight = FontWeight.Bold)); append(bold); pop()
            if (rest.isNotEmpty()) { append("\n"); append(rest) }
        }, style = PgType.body, color = c.ink2)
    }
}

/** Terms on their own (an existing account that hasn't accepted the current terms). */
@Composable
fun TermsScreen(progress: Int?, onBack: (() -> Unit)?, onAgree: () -> Unit) = Page { GateHead(onBack, progress?.let { it to 6 }); TermsStep(onAgree) }

// ------------------------------------------------------------------ permissions (Perms.svelte)

@Composable
private fun PermsStep(missing: List<Need>, onFix: (Need) -> Unit, onDone: () -> Unit) {
    val rows = listOf(
        Triple(Need.Mic, PgIcons.Mic, "Microphone" to "So they can hear you. Used only during a call."),
        Triple(Need.Notifications, PgIcons.Bell, "Notifications" to "So you see incoming calls and messages."),
        Triple(Need.FullScreen, PgIcons.Phone, "Ring over lock screen" to "Shows incoming calls full screen, like the phone app."),
        Triple(Need.Battery, PgIcons.Battery, "Run in background" to "Android pauses idle apps to save battery. This keeps your line open, using a little battery."),
    )
    val touched = rows.any { it.first !in missing }
    GateStep("So calls can reach you", lead = "Tinline works like a phone line: it stays open quietly in the background. Each of these is optional, and you can change them later.",
        foot = { GBtn(if (touched) "Continue" else "Not now", onDone, kind = if (touched) GKind.Primary else GKind.Secondary) }) {
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            rows.forEach { (need, icon, t) -> PermRow(icon, t.first, t.second, need !in missing) { onFix(need) } }
        }
    }
}

@Composable
private fun PermRow(icon: ImageVector, title: String, why: String, granted: Boolean, onAllow: () -> Unit) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.CenterVertically) {
        IconDisc(icon, 40.dp, 20.dp)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(title, style = PgType.body.copy(fontWeight = FontWeight.Bold), color = c.ink)
            Text(why, style = PgType.sub, color = c.ink2)
        }
        Box(Modifier.widthIn(min = 72.dp), contentAlignment = Alignment.Center) {
            if (granted) Icon(PgIcons.Check, "$title allowed", Modifier.size(22.dp), tint = c.accentInk)
            else PBtn("Allow", onAllow, variant = PVariant.Quiet, colorOverride = c.accentInk, minWidth = 72.dp, padH = 14.dp,
                modifier = Modifier.border(1.dp, c.ln, RoundedCornerShape(50)).semantics { contentDescription = "Allow ${title.lowercase()}" })
        }
    }
}

/** Kept for callers outside the flow. */
@Composable
fun PermissionsScreen(missing: List<Need>, onFix: (Need) -> Unit, onDone: () -> Unit) = Page { GateHead(null, null); PermsStep(missing, onFix, onDone) }

// ------------------------------------------------------------------ restore: 24 words (Restore.svelte)

/**
 * 24 real fields in two columns. Typing a space or Enter moves on (completing a word when the prefix has one answer); pasting a
 * run of words spreads them from that field; Backspace in an empty field goes back. A word turns red once it is left (or after
 * Continue) if it isn't on the list, or at once when no word starts that way. The message and suggestions sit in reserved space.
 */
@Composable
private fun RestoreStep(error: String?, warning: String, onError: (String?) -> Unit, onSubmit: (String) -> Unit) {
    val ctx = LocalContext.current
    val c = Tin.c
    val list = remember { Wordlist.get(ctx) }
    val set = remember(list) { list.toHashSet() }
    val words = remember { mutableStateListOf<String>().apply { repeat(24) { add("") } } }
    val seen = remember { mutableStateListOf<Boolean>().apply { repeat(24) { add(false) } } }
    var active by remember { mutableIntStateOf(-1) }
    var tried by remember { mutableStateOf(false) }
    // A pasted recovery key (one line, `tlkey-…`) replaces the boxes ("" = typing words).
    var recKey by remember { mutableStateOf("") }
    val focus = remember { List(24) { androidx.compose.ui.focus.FocusRequester() } }
    LaunchedEffect(Unit) { kotlinx.coroutines.delay(PgMotion.ENTER.toLong()); runCatching { focus[0].requestFocus() } }

    fun suggest(p: String, n: Int) = if (p.isEmpty()) emptyList() else list.asSequence().filter { it.startsWith(p) }.take(n).toList()
    fun bad(i: Int): Boolean { val v = words[i]; if (v.isEmpty() || v in set) return false; return seen[i] || tried || suggest(v, 1).isEmpty() }
    val badList = (0 until 24).filter { bad(it) }
    val filled = words.count { it.isNotEmpty() }
    val sugg = if (active >= 0 && words[active] !in set) suggest(words[active], 4) else emptyList()
    fun setWord(i: Int, raw: String) {
        if (uniffi.p2pcore.isRecoveryKey(raw)) { recKey = raw.trim(); onError(null); return }
        val toks = raw.lowercase().split(Regex("[^a-z]+")).filter { it.isNotEmpty() }
        if (toks.size > 1) {
            toks.forEachIndexed { k, t -> if (i + k < 24) words[i + k] = t }
            runCatching { focus[minOf(23, i + toks.size - 1)].requestFocus() }
        } else words[i] = (toks.firstOrNull() ?: "").take(12)
        onError(null)
    }
    fun submit() {
        if (recKey.isNotEmpty()) {
            // The core checks the key's checksum: a typo is caught before anything is restored.
            runCatching { uniffi.p2pcore.recoveryPhraseFromInput(recKey) }
                .onSuccess { onSubmit(it) }
                .onFailure { onError("That key has a typo or is cut short. Paste it again from where you saved it.") }
            return
        }
        tried = true
        if (badList.isNotEmpty() || filled < 24) {
            val first = badList.firstOrNull() ?: words.indexOfFirst { it.isEmpty() }
            if (first >= 0) runCatching { focus[first].requestFocus() }
            return
        }
        onSubmit(words.joinToString(" "))
    }
    fun advance(i: Int) {
        val w = words[i]
        if (w.isNotEmpty() && w !in set) suggest(w, 2).let { if (it.size == 1) words[i] = it[0] }
        if (i < 23) runCatching { focus[i + 1].requestFocus() } else submit()
    }
    val msg = when {
        badList.isNotEmpty() -> "Word ${badList[0] + 1}${if (badList.size > 1) " and ${badList.size - 1} more" else ""} ${if (badList.size > 1) "aren’t" else "isn’t"} in the recovery word list. Check the spelling."
        recKey.isEmpty() && tried && filled < 24 -> "${24 - filled} words are still missing."
        else -> error ?: ""
    }

    Column(Modifier.fillMaxSize()) {
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 8.dp, bottom = 8.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("Restore account", style = PgType.h1, color = c.ink)
            Text(if (recKey.isNotEmpty()) "Your recovery key is in. It stays on this phone."
                else "Type your 24 words in order, or paste them, or your recovery key, into the first box. They stay on this phone.", style = PgType.lead, color = c.ink2)
            GNotice(warning, NKind.Warn, PgIcons.Info)
            if (recKey.isNotEmpty()) {
                Column(Modifier.fillMaxWidth().clip(SmoothCornerShape(10.dp)).background(c.sf).border(1.dp, c.accentSoft, SmoothCornerShape(10.dp))
                    .padding(horizontal = 16.dp, vertical = 14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text("Recovery key", style = TinType.caption, color = c.ink2)
                    Text(recKey, style = TinType.mono.copy(fontSize = 15.sp, lineHeight = 24.sp), color = c.ink)
                    GBtn("Use my words instead", { recKey = ""; onError(null) }, kind = GKind.Text, fill = false, modifier = Modifier.offset(x = (-20).dp))
                }
            } else Row(Modifier.fillMaxWidth().padding(top = 0.dp).offset(y = (-4).dp), verticalAlignment = Alignment.CenterVertically) {
                GBtn("Paste from clipboard", {
                    val t = clipboardText(ctx)
                    if (t == null) onError("Couldn’t read the clipboard. Paste into the first box instead.") else setWord(0, t)
                }, kind = GKind.Text, fill = false, modifier = Modifier.offset(x = (-20).dp))
                Spacer(Modifier.weight(1f))
                Text("$filled of 24", style = PgType.sub.copy(fontFeatureSettings = "tnum"), color = c.ink2)
            }
            if (recKey.isEmpty()) Column(Modifier.semantics { contentDescription = "Recovery phrase words" }, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                for (r in 0 until 12) Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (col in 0..1) {
                        val i = r * 2 + col
                        WordCell(i, words[i], bad(i), words[i].isNotEmpty() && words[i] in set, focus[i],
                            onValue = { setWord(i, it) },
                            onFocus = { f -> if (f) active = i else { seen[i] = true; if (active == i) active = -1 } },
                            onNext = { advance(i) },
                            onBackEmpty = { if (i > 0) runCatching { focus[i - 1].requestFocus() } },
                            modifier = Modifier.weight(1f))
                    }
                }
            }
        }
        // The dock: suggestions (44 tall, always there) and the message line, above the footer.
        Column(Modifier.fillMaxWidth().background(c.bg).padding(start = 24.dp, end = 24.dp, top = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            FlowRow(Modifier.fillMaxWidth().heightIn(min = 44.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.Center) {
                sugg.forEach { s ->
                    Box(Modifier.heightIn(min = 44.dp).clip(RoundedCornerShape(22.dp)).background(c.accentSoft)
                        .clickable(role = Role.Button) { if (active >= 0) { val n = active; words[n] = s; if (n < 23) runCatching { focus[n + 1].requestFocus() } } }
                        .padding(horizontal = 16.dp), contentAlignment = Alignment.Center) {
                        Text(s, style = PgType.mono.copy(fontSize = 14.sp, fontWeight = FontWeight.SemiBold), color = c.accentInk)
                    }
                }
            }
            Text(msg, Modifier.fillMaxWidth().heightIn(min = 18.dp), style = PgType.sub.copy(fontWeight = FontWeight.SemiBold), color = c.er)
        }
        Column(Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, top = 12.dp, bottom = 24.dp)) { GBtn("Continue", ::submit, enabled = filled > 0 || recKey.isNotEmpty()) }
    }
}

@Composable
private fun WordCell(
    i: Int, value: String, bad: Boolean, ok: Boolean, focus: androidx.compose.ui.focus.FocusRequester,
    onValue: (String) -> Unit, onFocus: (Boolean) -> Unit, onNext: () -> Unit, onBackEmpty: () -> Unit, modifier: Modifier,
) {
    val c = Tin.c
    val src = remember { MutableInteractionSource() }
    val focused by src.collectIsFocusedAsState()
    val shape = SmoothCornerShape(10.dp)
    Row(
        modifier.heightIn(min = 44.dp).focusRing(focused, c.accentInk, 10.dp).clip(shape).background(if (bad) c.erc else c.sf)
            .border(1.dp, when { focused -> androidx.compose.ui.graphics.Color.Transparent; bad -> c.er; ok -> c.accentSoft; else -> c.ln }, shape)
            .padding(start = 10.dp, end = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text("${i + 1}", Modifier.width(20.dp), style = PgType.meta.copy(fontFeatureSettings = "tnum"), color = c.ink2, textAlign = TextAlign.End)
        androidx.compose.foundation.text.BasicTextField(
            value, { v -> if (v.endsWith(" ") && v.trim().split(Regex("\\s+")).size <= 1) { onValue(v.trim()); onNext() } else onValue(v) },
            Modifier.weight(1f).focusRequester(focus).onFocusChanged { onFocus(it.isFocused) }
                .onPreviewKeyEvent { e ->
                    if (e.type == androidx.compose.ui.input.key.KeyEventType.KeyDown && e.key == androidx.compose.ui.input.key.Key.Backspace && value.isEmpty()) { onBackEmpty(); true } else false
                }.semantics { contentDescription = "Word ${i + 1}" },
            singleLine = true, textStyle = PgType.mono.copy(color = c.ink), cursorBrush = androidx.compose.ui.graphics.SolidColor(c.accentInk), interactionSource = src,
            keyboardOptions = KeyboardOptions(keyboardType = androidx.compose.ui.text.input.KeyboardType.Password, autoCorrectEnabled = false, imeAction = if (i == 23) ImeAction.Done else ImeAction.Next),
            keyboardActions = androidx.compose.foundation.text.KeyboardActions(onNext = { onNext() }, onDone = { onNext() }),
            visualTransformation = androidx.compose.ui.text.input.VisualTransformation.None,
        )
    }
}

/** The 24-word screen on its own (the forgot-passphrase path from Unlock uses the flow; kept for other callers). */
@Composable
fun RestoreScreen(onBack: () -> Unit, error: String?, busy: Boolean, warning: String, onSubmit: (String) -> Unit) {
    var err by remember(error) { mutableStateOf(error) }
    Page { GateHead(onBack, null); RestoreStep(err, warning, { err = it }, onSubmit) }
}

// ------------------------------------------------------------------ previews (debug gallery: `g-<step>`)

/** One sign-up step with its header, synthetic data only (the debug gallery and screenshots). */
@Composable
internal fun GateStepPreview(which: String) {
    val phrase = "abandon ability able about above absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress actual"
    var name by remember { mutableStateOf("Maya Kurian") }
    var pass by remember { mutableStateOf(if (which == "pass-typed") "blue kettle" else "") }
    var label by remember { mutableStateOf("Personal phone") }
    val none = {}
    Page {
        when (which) {
            "welcome" -> { GateHead(null, 0 to 6); WelcomeStep(false, none, none) }
            "choose" -> { GateHead(none, 0 to 6); ChooseStep(none, none) }
            "name" -> { GateHead(none, 1 to 6); NameStep(name, { name = it }, none) }
            "pass", "pass-typed" -> { GateHead(none, 2 to 6); PassStep(PassKind.Create, pass, { pass = it }, false, null, none, none) }
            "phrase" -> { GateHead(null, 3 to 6); PhraseStep(phrase, none, onCheck = none) }
            "phrase-copied" -> { GateHead(null, 3 to 6); PhraseStep(phrase, none, onCheck = none, copiedAlready = true) }
            "confirm" -> { GateHead(none, null); ConfirmStep(phrase, none) }
            "device" -> { GateHead(none, 4 to 6); DeviceStep(label, { label = it }, false, null, none) }
            "terms" -> { GateHead(null, 5 to 6); TermsStep(none) }
            "perms" -> { GateHead(null, 6 to 6); PermsStep(listOf(Need.Mic, Need.FullScreen, Need.Battery), {}, none) }
            "done" -> { GateHead(null, null); DoneStep(name, none, none) }
        }
    }
}

/** Words | Key: the same secret two ways (recovery step, Settings). Hugs its two buttons. */
@Composable
internal fun WordsKeySeg(asKey: Boolean, onChange: (Boolean) -> Unit) {
    val c = Tin.c
    Row(Modifier.clip(SmoothCornerShape(12.dp)).background(c.sf3).padding(3.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        listOf(false to "Words", true to "Key").forEach { (k, label) ->
            val on = k == asKey
            Box(
                Modifier.heightIn(min = 40.dp).clip(SmoothCornerShape(9.dp)).background(if (on) c.sf else androidx.compose.ui.graphics.Color.Transparent)
                    .clickable(role = Role.Tab) { onChange(k) }.padding(horizontal = 14.dp),
                contentAlignment = Alignment.Center,
            ) { Text(label, style = TinType.bodyM.copy(fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal), color = c.ink) }
        }
    }
}

/** The recovery key: monospace, broken anywhere (it has no spaces), the same card look as the words. */
@Composable
internal fun KeyCard(key: String) {
    val c = Tin.c
    Box(Modifier.fillMaxWidth().clip(SmoothCornerShape(10.dp)).background(c.sf).border(1.dp, c.ln, SmoothCornerShape(10.dp)).padding(horizontal = 16.dp, vertical = 14.dp)) {
        Text(key, style = TinType.mono.copy(fontSize = 15.sp, lineHeight = 24.sp), color = c.ink)
    }
}
