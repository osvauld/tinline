package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private enum class Step { Welcome, Name, Pass, Phrase, Terms, Perms, Restore }

/** Common frame of the onboarding steps: back arrow, progress dots, scrolling body, pinned footer. */
@Composable
fun StepFrame(
    progress: Int?, onBack: (() -> Unit)?, modifier: Modifier = Modifier,
    footer: @Composable ColumnScope.() -> Unit = {}, body: @Composable ColumnScope.() -> Unit,
) {
    Page {
        if (onBack != null || progress != null) TopBar(onBack = onBack)
        if (progress != null) ProgressDots(progress)
        Column(
            modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 28.dp, bottom = 8.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp), content = body,
        )
        Column(Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, bottom = 24.dp, top = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp), content = footer)
    }
}

@Composable
fun H1(text: String, modifier: Modifier = Modifier, align: TextAlign = TextAlign.Start) =
    Text(text, modifier, style = TinType.h1, color = Tin.c.ink, textAlign = align)

@Composable
fun Lead(text: String, modifier: Modifier = Modifier, align: TextAlign = TextAlign.Start) =
    Text(text, modifier, style = TinType.bodyL, color = Tin.c.ink2, textAlign = align)

/**
 * First run: Welcome - Name - Passphrase (optional) - Recovery phrase - Terms - Permissions, or the
 * restore path (recovery words - name - passphrase - Terms - Permissions). [forgot] is the
 * "forgot my passphrase" restore over a locked identity: words, new passphrase, done.
 */
@Composable
fun OnboardingFlow(
    app: P2pApp, missing: List<Need>, onFix: (Need) -> Unit, forgot: Boolean = false,
    onCancelForgot: () -> Unit = {}, onDone: () -> Unit,
) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var step by rememberSaveable { mutableStateOf(if (forgot) Step.Restore else Step.Welcome) }
    var name by rememberSaveable { mutableStateOf(if (forgot) app.node.profile()?.name ?: "" else "") }
    var restoring by rememberSaveable { mutableStateOf(forgot) }
    // Secrets are never saved into the instance-state bundle.
    var words by remember { mutableStateOf("") }
    var pass by remember { mutableStateOf("") }
    var phrase by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var created by remember { mutableStateOf(false) }
    // The identity appeared some other way (debug hook, a restored process): leave onboarding.
    val has by app.hasIdentity.collectAsState()
    LaunchedEffect(has, created) { if (!forgot && has && !created && !busy) onDone() }

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
                // A bad phrase belongs on the words screen.
                if (it is uniffi.p2pcore.Exception.BadPhrase || (it is IllegalArgumentException && forgot)) step = Step.Restore
            }
            r.onSuccess { p ->
                created = true
                app.identityReady()
                when {
                    forgot -> onDone()
                    p == null -> step = Step.Terms
                    else -> { phrase = p; step = Step.Phrase }
                }
            }
        }
    }

    when (step) {
        Step.Welcome -> WelcomeScreen(onStart = { restoring = false; step = Step.Name }, onRestore = { restoring = true; step = Step.Restore })
        Step.Restore -> RestoreScreen(
            onBack = { if (forgot) onCancelForgot() else step = Step.Welcome },
            error = error, busy = false,
            warning = if (forgot) "Restoring keeps the same identity: use the recovery phrase of this account. Your contacts stay."
            else "Restoring replaces whatever account is on this phone. Next, you’ll choose a new passphrase.",
            onSubmit = { w -> words = w; error = null; step = if (forgot) Step.Pass else Step.Name },
        )
        Step.Name -> NameScreen(name, { name = it }, onBack = { step = if (restoring) Step.Restore else Step.Welcome },
            progress = 1, onNext = { step = Step.Pass })
        Step.Pass -> PassphraseStep(
            pass, { pass = it; error = null }, busy, error,
            progress = if (forgot) null else 2, title = if (forgot) "Choose a new passphrase" else "Add a passphrase",
            cta = if (restoring) "Restore account" else "Continue",
            onBack = { step = if (forgot) Step.Restore else Step.Name },
            onNext = { val p = pass; pass = ""; finishIdentity(p) },
            onSkip = { pass = ""; finishIdentity("") },
        )
        Step.Phrase -> PhraseScreen(phrase.orEmpty(), onNext = { step = Step.Terms })
        Step.Terms -> TermsScreen(progress = 4, onBack = null) { LegalStore.accept(ctx); step = Step.Perms }
        Step.Perms -> PermissionsScreen(missing, onFix, onDone)
    }
}

// ------------------------------------------------------------------ 1 welcome

@Composable
fun WelcomeScreen(onStart: () -> Unit, onRestore: () -> Unit) {
    val c = Tin.c
    Page {
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(start = 28.dp, end = 28.dp, top = 24.dp, bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                TinMark(32.dp)
                Text("Tinline", style = TinType.titleL.copy(fontSize = 20.sp), color = c.ink)
            }
            StringIllustration(Modifier.padding(top = 12.dp), height = 200.dp)
            Text("Call people directly.", style = TinType.display, color = c.ink)
            Lead("Tinline connects your phone straight to theirs. End-to-end encrypted. No phone number, no ads, no tracking.")
        }
        Column(Modifier.padding(start = 28.dp, end = 28.dp, bottom = 28.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            TinButton("Get started", onStart)
            TinButton("I have a recovery phrase", onRestore, style = BtnStyle.Text)
            Row(Modifier.padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.Top) {
                Icon(Icons.Rounded.WarningAmber, null, tint = c.ink2, modifier = Modifier.size(20.dp))
                Text(buildAnnotatedStringBold("Not for emergency calls.", "Use your phone’s dialer to reach emergency services."),
                    style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = c.ink2)
            }
        }
    }
}

// ------------------------------------------------------------------ 2 name

@Composable
fun NameScreen(name: String, onChange: (String) -> Unit, onBack: () -> Unit, progress: Int, onNext: () -> Unit) {
    StepFrame(progress, onBack, footer = { TinButton("Continue", onNext, enabled = name.isNotBlank()) }) {
        H1("What should people call you?")
        Lead("Your name is shown to people you add. It never leaves your phone otherwise.")
        TinField(name, onChange, "Your name", Modifier.padding(top = 8.dp),
            keyboard = androidx.compose.foundation.text.KeyboardOptions(capitalization = androidx.compose.ui.text.input.KeyboardCapitalization.Words,
                imeAction = androidx.compose.ui.text.input.ImeAction.Done),
            actions = androidx.compose.foundation.text.KeyboardActions(onDone = { if (name.isNotBlank()) onNext() }))
        Hint("You can change it any time in Settings.")
    }
}

// ------------------------------------------------------------------ 3 passphrase

/** One passphrase field of any length and the explainer; shared by onboarding, add and change. */
@Composable
fun NewPassphraseField(pass: String, onPass: (String) -> Unit, enabled: Boolean = true, onDone: () -> Unit = {}, label: String = "Passphrase", why: Boolean = true) {
    TinField(pass, onPass, label, mono = true, password = true, enabled = enabled,
        keyboard = KeyboardOptions(imeAction = ImeAction.Done), actions = KeyboardActions(onDone = { if (pass.isNotEmpty() && enabled) onDone() }))
    if (why) InfoCard(
        "Any length works; a few random words are easy to remember. Your phone remembers it so calls still ring after a restart — you’ll need it to see your recovery phrase.",
        icon = Icons.Rounded.Key,
    )
}

@Composable
fun PassphraseStep(
    pass: String, onPass: (String) -> Unit, busy: Boolean, error: String?,
    progress: Int?, title: String, cta: String, onBack: (() -> Unit)?, onNext: () -> Unit, onSkip: () -> Unit,
) {
    StepFrame(progress, onBack, footer = {
        if (busy) LinearProgressIndicator(Modifier.fillMaxWidth().padding(bottom = 8.dp), color = Tin.c.pr, trackColor = Tin.c.sf3)
        TinButton(if (busy) "Encrypting…" else cta, onNext, enabled = !busy && pass.isNotEmpty())
        TinButton("Skip for now", onSkip, style = BtnStyle.Text, enabled = !busy)
    }) {
        H1(title)
        Lead("Your account lives only on this phone. A passphrase encrypts it, so nobody holding your phone can copy it. Optional — you can add one later in Settings.")
        NewPassphraseField(pass, onPass, !busy, onDone = onNext)
        if (error != null) Text(error, style = TinType.bodyM, color = Tin.c.er)
    }
}

// ------------------------------------------------------------------ 4 recovery phrase

/** Two columns, column-major: words 1-12 on the left, 13-24 on the right. */
@Composable
fun WordGrid(phrase: String) {
    val c = Tin.c
    val words = phrase.trim().split(Regex("\\s+")).filter { it.isNotEmpty() }
    val half = (words.size + 1) / 2
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        for (r in 0 until half) Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            for (col in 0..1) {
                val i = r + col * half
                if (i < words.size) Row(
                    Modifier.weight(1f).heightIn(min = 32.dp).clip(RoundedCornerShape(8.dp)).background(c.sf).border(1.dp, c.ln, RoundedCornerShape(8.dp)).padding(horizontal = 12.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Text("${i + 1}", Modifier.width(18.dp), style = TinType.caption, color = c.ink2, textAlign = TextAlign.End)
                    Text(words[i], style = TinType.mono.copy(fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.Normal), color = c.ink, maxLines = 1)
                } else Spacer(Modifier.weight(1f))
            }
        }
    }
}

@Composable
fun PhraseScreen(phrase: String, onNext: () -> Unit) {
    StepFrame(3, null, footer = { TinButton("I’ve written them down", onNext) }) {
        H1("Write down these 24 words")
        Text(buildAnnotatedString {
            append("They "); pushStyle(SpanStyle(fontWeight = FontWeight.Bold, color = Tin.c.ink)); append("are"); pop()
            append(" your Tinline account. If you lose this phone, they bring it back. Anyone who has them can pretend to be you.")
        }, style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 22.sp), color = Tin.c.ink2)
        WordGrid(phrase)
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Rounded.Lock, null, tint = Tin.c.ink2, modifier = Modifier.size(16.dp))
            Hint("Paper is safest. Screenshots are blocked here.")
        }
    }
}

// ------------------------------------------------------------------ 5 terms

@Composable
fun TermsScreen(progress: Int?, onBack: (() -> Unit)?, onAgree: () -> Unit) {
    val c = Tin.c
    var agreed by rememberSaveable { mutableStateOf(false) }
    StepFrame(progress, onBack, footer = { TinButton("Agree and continue", onAgree, enabled = agreed) }) {
        H1("Before you start")
        Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            TermPoint(Icons.Rounded.VisibilityOff, "No ads, no tracking.", "We don’t collect your contacts, calls or location.")
            TermPoint(Icons.Rounded.Lock, "Calls are end-to-end encrypted.", "When a direct line isn’t possible, an encrypted relay passes the call along. It can’t hear it.")
            TermPoint(Icons.Rounded.Key, "You hold the keys.", "Lose your phone and your recovery phrase, and nobody — not even Osvauld — can get your account back.")
            TermPoint(Icons.Rounded.WarningAmber, "Not for emergency calls.", "")
        }
        Spacer(Modifier.weight(1f, fill = false).heightIn(min = 8.dp))
        val link = SpanStyle(color = c.pr, textDecoration = TextDecoration.Underline, fontWeight = FontWeight.SemiBold)
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(c.sf).border(1.dp, c.ln, RoundedCornerShape(14.dp))
                .clickable(role = Role.Checkbox) { agreed = !agreed }.padding(14.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.Top,
        ) {
            Checkbox(agreed, null, Modifier.size(22.dp), colors = CheckboxDefaults.colors(checkedColor = c.pr, checkmarkColor = c.onPr, uncheckedColor = c.ln2))
            Text(buildAnnotatedString {
                append("I agree to the ")
                withLink(LinkAnnotation.Url(URL_TERMS, TextLinkStyles(link))) { append("Terms of Use") }
                append(" and have read the ")
                withLink(LinkAnnotation.Url(URL_PRIVACY, TextLinkStyles(link))) { append("Privacy Policy") }
                append(".")
            }, style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 22.sp), color = c.ink)
        }
    }
}

@Composable
private fun TermPoint(icon: ImageVector, bold: String, rest: String) {
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.Top) {
        Icon(icon, null, tint = Tin.c.pr, modifier = Modifier.size(24.dp))
        Text(buildAnnotatedStringBold(bold, rest).let { if (rest.isEmpty()) androidx.compose.ui.text.AnnotatedString.Builder().also { b ->
            b.pushStyle(SpanStyle(fontWeight = FontWeight.Bold)); b.append(bold); b.pop() }.toAnnotatedString() else it },
            style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 22.sp), color = Tin.c.ink)
    }
}

// ------------------------------------------------------------------ 6 permissions

@Composable
fun PermissionsScreen(missing: List<Need>, onFix: (Need) -> Unit, onDone: () -> Unit) {
    StepFrame(null, null, footer = {
        TinButton("Done", onDone)
        TinButton("Skip for now", onDone, style = BtnStyle.Text)
    }) {
        H1("So calls can reach you")
        Text("Tinline works like a phone line: it stays open quietly in the background.", style = TinType.bodyL.copy(fontSize = 15.sp), color = Tin.c.ink2)
        PermRow(Icons.Rounded.Mic, "Microphone", "So they can hear you. Used only during a call.", Need.Mic !in missing) { onFix(Need.Mic) }
        PermRow(Icons.Rounded.Notifications, "Notifications", "So you see incoming calls.", Need.Notifications !in missing) { onFix(Need.Notifications) }
        PermRow(Icons.Rounded.PhoneLocked, "Ring over lock screen", "Shows incoming calls full screen, like the phone app.", Need.FullScreen !in missing) { onFix(Need.FullScreen) }
        PermRow(Icons.Rounded.BatteryChargingFull, "Run in background", "Android pauses idle apps to save battery. This keeps your line open, using a little battery.", Need.Battery !in missing) { onFix(Need.Battery) }
    }
}

@Composable
private fun PermRow(icon: ImageVector, title: String, desc: String, granted: Boolean, onAllow: () -> Unit) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(c.sf).border(1.dp, c.ln, RoundedCornerShape(14.dp)).padding(16.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.Top,
    ) {
        Box(Modifier.size(40.dp).clip(RoundedCornerShape(12.dp)).background(c.prc), contentAlignment = Alignment.Center) { Icon(icon, null, tint = c.onPrc) }
        Column(Modifier.weight(1f)) {
            Text(title, style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
            Text(desc, style = TinType.bodyM.copy(lineHeight = 19.sp), color = c.ink2)
        }
        if (granted) Row(Modifier.heightIn(min = 40.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Icon(Icons.Rounded.Check, null, tint = c.pr, modifier = Modifier.size(18.dp)); Text("On", style = TinType.label, color = c.pr)
        } else TinButton("Allow", onAllow, fill = false, height = 40.dp, textStyle = TinType.label)
    }
}
