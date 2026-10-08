package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ContentPaste
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** The app icon as a rounded square, for the Unlock screen and About. */
@Composable
fun AppIconBadge(size: androidx.compose.ui.unit.Dp) {
    Box(Modifier.size(size).clip(RoundedCornerShape(size * 0.28f)).background(Color(0xFF0B6B5B)), contentAlignment = Alignment.Center) {
        TinMark(size * 0.62f, can = Color(0xFFF5F6F3), string = Color(0xFFE8B04A))
    }
}

/** Shown while the identity is Locked and no device key could unlock it. */
@Composable
fun UnlockScreen(app: P2pApp, onForgot: () -> Unit) {
    val scope = rememberCoroutineScope()
    var pass by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val name = remember { app.node.profile()?.name ?: "" }
    // No passphrase and the Keystore key is gone: the 24 words are the only way back.
    if (!app.node.hasPassphrase()) {
        KeyLostContent(name, busy, onRetry = {
            busy = true
            scope.launch { withContext(Dispatchers.IO) { app.tryAutoUnlock() }; busy = false; app.identityReady() }
        }, onRestore = onForgot)
        return
    }
    UnlockContent(
        name, pass, { pass = it; error = null }, busy, error, onForgot,
        onUnlock = {
            busy = true; error = null
            scope.launch {
                val r = withContext(Dispatchers.IO) { runCatching { app.node.unlock(pass) } }
                busy = false
                r.onFailure { error = friendly(it) }
                r.onSuccess { pass = ""; app.identityReady() }
            }
        },
    )
}

/** Locked with no passphrase: this phone's own key is gone, so only the recovery phrase brings the account back. */
@Composable
fun KeyLostContent(name: String, busy: Boolean, onRetry: () -> Unit, onRestore: () -> Unit) {
    val c = Tin.c
    Page {
        Column(
            Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 72.dp, bottom = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            AppIconBadge(72.dp)
            H1(if (name.isNotBlank()) "Can’t open $name’s account" else "Can’t open your account", align = TextAlign.Center)
            Lead("This phone no longer has the key that opens your account. That happens after a screen-lock reset or a restored backup. Calls can’t reach you until you restore.", align = TextAlign.Center)
            InfoCard("You didn’t set a passphrase, so the 24-word recovery phrase is the only way back. Your contacts stay.", icon = Icons.Rounded.Info, kind = BannerKind.Warn)
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth(), color = c.pr, trackColor = c.sf3)
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            TinButton("Restore with recovery phrase", onRestore, enabled = !busy)
            TinButton("Try again", onRetry, style = BtnStyle.Text, enabled = !busy)
        }
    }
}

@Composable
fun UnlockContent(name: String, pass: String, onPass: (String) -> Unit, busy: Boolean, error: String?, onForgot: () -> Unit, onUnlock: () -> Unit) {
    val c = Tin.c
    Page {
        Column(
            Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 72.dp, bottom = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            AppIconBadge(72.dp)
            H1(if (name.isNotBlank()) "Welcome back, $name" else "Welcome back", align = TextAlign.Center)
            Lead("Enter your passphrase to open Tinline. Calls can’t reach you until you do.", align = TextAlign.Center)
            TinField(
                pass, onPass, "Passphrase", Modifier.padding(top = 12.dp), mono = true, password = true, enabled = !busy,
                state = if (error != null) FieldState.Error else FieldState.Normal, hint = error,
                keyboard = KeyboardOptions(imeAction = ImeAction.Done),
                actions = KeyboardActions(onDone = { if (pass.isNotEmpty() && !busy) onUnlock() }),
            )
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth(), color = c.pr, trackColor = c.sf3)
            TinButton(if (busy) "Unlocking…" else "Unlock", onUnlock, enabled = pass.isNotEmpty() && !busy)
            if (error != null) Hint("Checking takes a second — it’s deliberately slow to stop guessing.", align = TextAlign.Center)
        }
        if (error != null) Column(
            Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp).fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(c.sf2).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text("Forgot it?", style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
            Text("Your recovery phrase can restore this account and set a new passphrase.", style = TinType.bodyM, color = c.ink2)
            TinButton("Restore with recovery phrase", onForgot, style = BtnStyle.Text, fill = false, height = 40.dp)
        } else Box(Modifier.padding(horizontal = 24.dp, vertical = 24.dp)) { TinButton("Forgot passphrase?", onForgot, style = BtnStyle.Text, enabled = !busy) }
    }
}

// ------------------------------------------------------------------ restore: 24 words

/**
 * Type the 24 words into a 3-column grid; the active cell is a real text field, the others are
 * tappable. Words from the BIP-39 English list are suggested as you type; pasting a whole phrase
 * fills the grid. [onSubmit] gets the words joined by spaces; the core validates the checksum.
 */
@Composable
fun RestoreScreen(onBack: () -> Unit, error: String?, busy: Boolean, warning: String, onSubmit: (String) -> Unit) {
    val ctx = LocalContext.current
    val c = Tin.c
    val list = remember { Wordlist.get(ctx) }
    val set = remember(list) { list.toHashSet() }
    val words = remember { mutableStateListOf<String>().apply { repeat(24) { add("") } } }
    var active by remember { mutableIntStateOf(0) }
    var draft by remember { mutableStateOf("") }
    val focus = remember { FocusRequester() }

    fun commit(token: String, idx: Int) { words[idx] = token.lowercase() }
    fun fill(text: String) {
        val toks = text.lowercase().split(Regex("[^a-z]+")).filter { it.isNotEmpty() }
        if (toks.isEmpty()) return
        var i = active
        toks.forEach { if (i < 24) { words[i] = it; i++ } }
        active = minOf(i, 23); draft = if (i <= 23) words[active] else ""
        if (i <= 23 && words[active].isNotEmpty()) draft = words[active]
    }
    fun advance() {
        if (draft.isNotBlank()) commit(draft.trim(), active)
        if (active < 23) { active++; draft = words[active] }
    }
    LaunchedEffect(active) { runCatching { focus.requestFocus() } }

    val allValid = words.all { it in set }
    val sugg = remember(draft) { Wordlist.suggestions(ctx, draft.trim()).filter { it != draft.trim() || false } }

    Page {
        TopBar("Restore account", onBack) {
            TinButton("Paste", { clipboardText(ctx)?.let { fill(it) } }, style = BtnStyle.Text, icon = Icons.Rounded.ContentPaste, fill = false, height = 40.dp, textStyle = TinType.label)
        }
        Column(
            Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 4.dp, bottom = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Type your 24 words in order. They stay on this phone.", style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 22.sp), color = c.ink2)
            for (r in 0 until 8) Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                for (col in 0..2) {
                    val i = r * 3 + col
                    val isActive = i == active
                    val w = words[i]
                    val bad = w.isNotEmpty() && w !in set && !isActive
                    Row(
                        Modifier.weight(1f).heightIn(min = 38.dp).clip(RoundedCornerShape(8.dp)).background(c.sf)
                            .border(if (isActive) 2.dp else 1.dp, if (isActive) c.pr else if (bad) c.er else c.ln, RoundedCornerShape(8.dp))
                            .clickable(enabled = !isActive, role = Role.Button) { if (draft.isNotBlank()) commit(draft.trim(), active); active = i; draft = words[i] }
                            .padding(horizontal = 8.dp),
                        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Text("${i + 1}", Modifier.width(16.dp), style = TinType.caption.copy(fontSize = 11.sp), color = c.ink2, textAlign = TextAlign.End)
                        val st = TinType.mono.copy(fontSize = 14.sp, lineHeight = 18.sp)
                        if (isActive) BasicTextField(
                            draft, { v ->
                                if (v.any { it.isWhitespace() || it == ',' }) { if (v.trim().contains(Regex("[\\s,]+")) && v.trim().split(Regex("[\\s,]+")).size > 1) fill(v) else { draft = v.trim(); advance() } }
                                else draft = v.lowercase()
                            },
                            Modifier.fillMaxWidth().focusRequester(focus).onPreviewKeyEvent { e ->
                                if (e.type == KeyEventType.KeyDown && e.key == Key.Backspace && draft.isEmpty() && active > 0) {
                                    active--; draft = words[active]; true
                                } else false
                            },
                            singleLine = true, textStyle = st.copy(color = c.ink), cursorBrush = SolidColor(c.pr),
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false, imeAction = ImeAction.Next),
                            keyboardActions = KeyboardActions(onNext = { advance() }, onDone = { advance() }),
                        ) else Text(w, style = st, color = if (bad) c.er else c.ink, maxLines = 1)
                    }
                }
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                Hint("Word ${active + 1}:")
                sugg.forEach { s ->
                    Box(
                        Modifier.heightIn(min = 36.dp).clip(RoundedCornerShape(50)).background(c.prc).clickable(role = Role.Button) {
                            commit(s, active); if (active < 23) { active++; draft = words[active] } else draft = s
                        }.padding(horizontal = 14.dp),
                        contentAlignment = Alignment.Center,
                    ) { Text(s, style = TinType.mono.copy(fontSize = 14.sp, lineHeight = 18.sp), color = c.onPrc) }
                }
            }
            if (error != null) Text(error, style = TinType.bodyM, color = c.er)
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp, top = 4.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            InfoCard(warning, icon = Icons.Rounded.Info, kind = BannerKind.Warn)
            TinButton("Restore", {
                if (draft.isNotBlank()) commit(draft.trim(), active)
                onSubmit(words.joinToString(" "))
            }, enabled = !busy && (allValid || (words.count { it.isNotEmpty() } == 23 && draft.trim() in set)))
        }
    }
}

/** Legacy installs: usable already, but the identity should be sealed, with a passphrase if the user wants one. */
@Composable
fun SetPassphraseScreen(app: P2pApp) {
    val scope = rememberCoroutineScope()
    var pass by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    fun seal(p: String) {
        busy = true; error = null
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { app.node.setPassphrase(null, p) } }
            busy = false
            r.onFailure { error = friendly(it) }
            r.onSuccess { pass = ""; app.identityReady() }
        }
    }
    StepFrame(null, null, footer = {
        if (busy) LinearProgressIndicator(Modifier.fillMaxWidth().padding(bottom = 8.dp), color = Tin.c.pr, trackColor = Tin.c.sf3)
        TinButton(if (busy) "Encrypting…" else "Set passphrase", { seal(pass) }, enabled = pass.isNotEmpty() && !busy)
        TinButton("Skip for now", { seal("") }, style = BtnStyle.Text, enabled = !busy)
    }) {
        H1("Add a passphrase")
        Lead("Your recovery phrase is currently stored unprotected on this phone. Encrypt it now, with a passphrase if you like. Calls keep working while you do this.")
        NewPassphraseField(pass, { pass = it; error = null }, !busy, onDone = { seal(pass) }, why = false)
        error?.let { Text(it, style = TinType.bodyM, color = Tin.c.er) }
    }
}
