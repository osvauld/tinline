package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.AccountSummary

/** What the sheet's two "add" rows do; null (onboarding) hides them. */
class AccountActions(val onCreate: () -> Unit, val onRestore: () -> Unit, val onLink: () -> Unit)

/** The account switcher bottom sheet. [onPick] gets a non-current account; the current one is just ticked. */
@Composable
fun AccountSheet(app: P2pApp, onDismiss: () -> Unit, onPick: (String) -> Unit, actions: AccountActions? = null) {
    val c = Tin.c
    val status by app.status.collectAsState()
    val list by produceState<List<AccountSummary>>(emptyList()) { value = withContext(Dispatchers.IO) { app.node.accounts() } }
    TinSheet(onDismiss) {
        Text("Accounts", Modifier.padding(top = 4.dp, bottom = 8.dp), style = TinType.titleL, color = c.ink)
        list.forEach { a ->
            val sub = when {
                a.current && status?.online == true -> "Online on this phone"
                a.current -> "On this phone"
                else -> "Locked · not ringing here"
            }
            Row(
                Modifier.fillMaxWidth().heightIn(min = 64.dp).clip(RoundedCornerShape(12.dp)).clickable(role = Role.Button) { if (a.current) onDismiss() else onPick(a.did) },
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                if (a.current) SelfAvatar(a.name.ifBlank { "?" }, 44.dp) else Avatar(a.name.ifBlank { "?" }, a.did, 44.dp)
                Column(Modifier.weight(1f)) {
                    Text(a.name, style = TinType.bodyL.copy(fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold), color = c.ink, maxLines = 1)
                    Hint(sub)
                }
                if (a.current) Icon(Icons.Rounded.Check, "Current account", tint = c.pr)
            }
        }
        if (actions != null) {
            Spacer(Modifier.height(8.dp))
            SheetRow(Icons.Rounded.Add, "Create a new account", null, true) { actions.onCreate() }
            SheetRow(Icons.Rounded.Link, "Link an account from another device", null, true) { actions.onLink() }
            SheetRow(Icons.Rounded.Key, "Restore with recovery phrase", null, true) { actions.onRestore() }
        }
        Hint("One account is online at a time. Calls to a locked account ring only on your other devices.", Modifier.padding(top = 12.dp))
    }
}

@Composable
private fun SheetRow(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, sub: String?, enabled: Boolean, onClick: () -> Unit) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(RoundedCornerShape(12.dp)).clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Icon(icon, null, tint = if (enabled) c.pr else c.ink2, modifier = Modifier.padding(start = 10.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = TinType.bodyL, color = if (enabled) c.ink else c.ink2)
            if (sub != null) Hint(sub)
        }
    }
}

/** "Switch to X?" with the passphrase field only when X has one and this phone has no remembered key for it. */
@Composable
fun SwitchConfirmScreen(app: P2pApp, did: String, onBack: () -> Unit, onSwitched: () -> Unit, onMessage: (String) -> Unit) {
    val c = Tin.c
    val ctx = androidx.compose.ui.platform.LocalContext.current
    val scope = rememberCoroutineScope()
    val target = remember(did) { app.node.accounts().firstOrNull { it.did == did } }
    val current = remember { app.node.profile()?.name ?: "" }
    if (target == null) { LaunchedEffect(Unit) { onBack() }; return }
    val needsPass = target.hasPassphrase && !UnlockStore.exists(ctx, did)
    val inCall by app.calls.ui.collectAsState()
    var pass by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    fun go() {
        busy = true; error = null
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { app.switchTo(did, if (needsPass) pass else null) } }
            busy = false
            r.onFailure {
                if (it is uniffi.p2pcore.Exception.InCall) { onMessage("Finish your call first"); onBack() }
                else error = friendly(it)
            }
            r.onSuccess { pass = ""; onSwitched() }
        }
    }
    Page {
        TopBar("Switch account", onBack)
        Column(
            Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 16.dp, bottom = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Avatar(target.name.ifBlank { "?" }, target.did, 72.dp)
            H1("Switch to ${target.name}?", align = TextAlign.Center)
            Lead("$current goes offline on this phone. Calls to $current will ring only on your other linked devices until you switch back.", align = TextAlign.Center)
            if (needsPass) TinField(pass, { pass = it; error = null }, "Passphrase for ${target.name}", mono = true, password = true, enabled = !busy,
                state = if (error != null) FieldState.Error else FieldState.Normal, hint = error,
                keyboard = KeyboardOptions(imeAction = ImeAction.Done), actions = KeyboardActions(onDone = { if (pass.isNotEmpty() && !busy) go() }))
            else error?.let { Text(it, style = TinType.bodyM, color = c.er) }
            if (inCall == null) Hint("You’re not on a call. If you were, switching would end it.", align = TextAlign.Center)
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth(), color = c.pr, trackColor = c.sf3)
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            TinButton(if (busy) "Switching…" else if (needsPass) "Unlock and switch" else "Switch", ::go, enabled = !busy && (!needsPass || pass.isNotEmpty()))
            TinButton("Cancel", onBack, style = BtnStyle.Text, enabled = !busy)
        }
    }
}
