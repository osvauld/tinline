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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// Five-try / 30-second lockout is a core gap, not a UI-only security policy.

/** Shown while the identity is Locked and no device key could unlock it. */
@Composable
fun UnlockScreen(app: P2pApp, onForgot: () -> Unit) {
    val scope = rememberCoroutineScope()
    var pass by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val name = remember { app.node.profile()?.name ?: "" }
    val wrongPassphrase = stringResource(R.string.unlock_wrong_passphrase)
    var sheet by remember { mutableStateOf(false) }
    val many = remember { app.node.accounts().size > 1 }
    val onSwitch: (() -> Unit)? = if (many) ({ sheet = true }) else null
    if (sheet) AccountSheet(app, atLogin = true, onDismiss = { sheet = false }, onPick = { did ->
        sheet = false; busy = true
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { app.switchTo(did, null) } }
            busy = false
            r.onFailure { error = friendly(it) }
        }
    })
    // No passphrase and the Keystore key is gone: the 24 words are the only way back.
    if (!app.node.hasPassphrase()) {
        KeyLostContent(name, busy, onRetry = {
            busy = true
            scope.launch { withContext(Dispatchers.IO) { app.tryAutoUnlock() }; busy = false; app.identityReady() }
        }, onRestore = onForgot, onSwitch = onSwitch)
        return
    }
    UnlockContent(
        name, pass, { pass = it; error = null }, busy, error, onForgot, onSwitch = onSwitch,
        onUnlock = {
            busy = true; error = null
            scope.launch {
                val r = withContext(Dispatchers.IO) { runCatching { app.node.unlock(pass) } }
                busy = false
                r.onFailure {
                    if (it is uniffi.p2pcore.Exception.WrongPassphrase) {
                        error = wrongPassphrase
                    } else error = friendly(it)
                }
                r.onSuccess { pass = ""; app.identityReady() }
            }
        },
    )
}

/** Failed.svelte (keylost): locked with no passphrase, this phone's own key is gone, so only the recovery phrase brings the account back. */
@Composable
fun KeyLostContent(name: String, busy: Boolean, onRetry: () -> Unit, onRestore: () -> Unit, onSwitch: (() -> Unit)? = null) {
    Page {
        GateHead(null, null)
        GateStep(
            title = if (name.isNotBlank()) stringResource(R.string.unlock_keylost_title_named, name) else stringResource(R.string.unlock_keylost_title),
            lead = stringResource(R.string.unlock_keylost_lead),
            center = true, anchor = true,
            hero = { Box(Modifier.padding(top = 12.dp)) { IconDisc(PgIcons.Key, 72.dp, 32.dp) } },
            foot = {
                GBtn(stringResource(R.string.unlock_keylost_restore), onRestore, enabled = !busy)
                GBtn(stringResource(R.string.common_try_again), onRetry, kind = GKind.Text, busy = busy)
                if (onSwitch != null) GBtn(stringResource(R.string.accounts_switch_account), onSwitch, kind = GKind.Text, enabled = !busy)
            },
        ) {
            GNotice(stringResource(R.string.unlock_keylost_notice), NKind.Warn, PgIcons.Info)
        }
    }
}

/**
 * Unlock.svelte: the avatar hangs above the heading, the field's message area is always 4.25 lines tall (error, the "deliberately
 * slow" hint and the lockout line all fit), so nothing below moves when any of them appears.
 */
@Composable
fun UnlockContent(name: String, pass: String, onPass: (String) -> Unit, busy: Boolean, error: String?, onForgot: () -> Unit, onUnlock: () -> Unit, onSwitch: (() -> Unit)? = null) {
    val c = Tin.c
    // Reserved error footprint remains stable; current core has no lockout countdown.
    // Identities have one nickname: the whole of it, never just the first word.
    val nick = name.trim()
    val submit = { if (pass.isNotEmpty() && !busy) onUnlock() }
    Page {
        GateHead(null, null)
        GateStep(
            title = if (nick.isNotBlank()) stringResource(R.string.unlock_welcome_back_named, nick) else stringResource(R.string.unlock_welcome_back),
            center = true, anchor = true,
            hero = { Box(Modifier.padding(top = 12.dp)) { SelfAvatar(name.ifBlank { "?" }, 72.dp) } },
            // With other identities here, the nickname in the title is the switcher (lab A, 2026-10-14).
            titleSlot = if (onSwitch != null && nick.isNotBlank()) ({ WelcomeSwitcher(nick, enabled = !busy, onSwitch) }) else null,
            foot = {
                GBtn(if (busy) stringResource(R.string.unlock_unlocking) else stringResource(R.string.unlock_unlock), submit, enabled = pass.isNotEmpty(), busy = busy)
                GBtn(stringResource(R.string.unlock_forgot), onForgot, kind = GKind.Text, enabled = !busy)
            },
        ) {
            GField(
                pass, onPass, stringResource(R.string.unlock_passphrase_label), secret = true, enabled = !busy, focus = rememberAutofocus(),
                error = error, hint = if (error != null) stringResource(R.string.unlock_slow_hint) else null,
                reserveLines = 4.25f, keyboard = KeyboardOptions(imeAction = ImeAction.Done), onDone = submit,
            )
        }
    }
}

/** "Welcome back, Maya ⌄": the nickname opens "Open as" (the identities on this phone). Same type as the title, a quiet
 *  chevron, a 48 dp target. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun WelcomeSwitcher(nick: String, enabled: Boolean, onSwitch: () -> Unit) {
    val c = Tin.c
    val switchDesc = stringResource(R.string.unlock_switch_identity_desc, nick)
    FlowRow(
        Modifier.fillMaxWidth().semantics { heading() },
        horizontalArrangement = Arrangement.Center, verticalArrangement = Arrangement.Center,
    ) {
        Text(stringResource(R.string.unlock_welcome_back_prefix) + " ", style = PgType.h1, color = c.ink, modifier = Modifier.align(Alignment.CenterVertically))
        Row(
            Modifier.heightIn(min = 48.dp).clip(SmoothCornerShape(10.dp))
                .clickable(enabled = enabled, role = Role.Button, onClickLabel = stringResource(R.string.unlock_open_as_label), onClick = onSwitch)
                .padding(horizontal = 4.dp).semantics { contentDescription = switchDesc },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(nick, style = PgType.h1, color = c.ink, maxLines = 1)
            Icon(PgIcons.Chev, null, Modifier.size(22.dp), tint = c.ink2)
        }
    }
}
