package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
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
fun AccountSheet(app: P2pApp, onDismiss: () -> Unit, onPick: (String) -> Unit, actions: AccountActions? = null, atLogin: Boolean = false) {
    val c = Tin.c
    val status by app.status.collectAsState()
    // Re-read when the core says the accounts changed: a background account's unread count, or the open one.
    val version by app.accountsVersion.collectAsState()
    val list by produceState<List<AccountSummary>>(emptyList(), version) { value = withContext(Dispatchers.IO) { app.node.accounts() } }
    // With wake pushes the other accounts still ring and notify here (docs/multi-online.md); always-on and Liberum keep one.
    // Read once, as Settings does (answered from the push check's cache), so the hint never changes under the reader.
    val wakes = remember { PushMode.pushOn(app) }
    TinSheet(onDismiss) {
        Text(if (atLogin) stringResource(R.string.accounts_open_as) else stringResource(R.string.accounts_title), Modifier.padding(top = 4.dp, bottom = 8.dp), style = TinType.titleL, color = c.ink)
        list.forEach { a ->
            // At login nothing is open yet: say only whether picking it opens right away (key remembered here, or no
            // passphrase) or asks for its passphrase.
            val opensNow = !a.hasPassphrase || UnlockStore.exists(app, a.did)
            val sub = if (atLogin) (if (opensNow) stringResource(R.string.accounts_opens_right_away) else stringResource(R.string.accounts_asks_passphrase)) else when {
                a.current && status?.online == true -> stringResource(R.string.accounts_online_here)
                a.current -> stringResource(R.string.accounts_on_this_phone)
                // A woken account needs its key remembered here; a passphrase-only one waits until it is opened.
                wakes && UnlockStore.exists(app, a.did) -> stringResource(R.string.accounts_rings_here)
                else -> stringResource(R.string.accounts_locked_not_ringing)
            }
            val unread = if (a.current) 0 else a.unread.toInt()
            val unreadDesc = if (unread > 0) pluralStringResource(R.plurals.shell_badge_unread, unread, unread) else ""
            Row(
                Modifier.fillMaxWidth().heightIn(min = 64.dp).clip(SmoothCornerShape(12.dp)).clickable(role = Role.Button) { if (a.current) onDismiss() else onPick(a.did) }
                    .semantics(mergeDescendants = true) { if (unread > 0) stateDescription = unreadDesc },
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                if (a.current) SelfAvatar(a.name.ifBlank { "?" }, 44.dp) else Avatar(a.name.ifBlank { "?" }, a.did, 44.dp)
                Column(Modifier.weight(1f)) {
                    Text(a.name, style = TinType.bodyL.copy(fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold), color = c.ink, maxLines = 1)
                    Hint(sub)
                }
                if (a.current) Icon(PgIcons.Check, stringResource(R.string.accounts_current_desc), tint = c.pr)
                else if (unread > 0) CountBadge(unread, Modifier.padding(end = 10.dp))
            }
        }
        if (actions != null) {
            Spacer(Modifier.height(8.dp))
            SheetRow(PgIcons.Plus, stringResource(R.string.accounts_create), null, true) { actions.onCreate() }
            SheetRow(PgIcons.Link, stringResource(R.string.accounts_link), null, true) { actions.onLink() }
            SheetRow(PgIcons.Key, stringResource(R.string.accounts_restore), null, true) { actions.onRestore() }
        }
        if (!atLogin) Hint(stringResource(if (wakes) R.string.accounts_others_ring_hint else R.string.accounts_one_online_hint), Modifier.padding(top = 12.dp))
    }
}

@Composable
private fun SheetRow(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, sub: String?, enabled: Boolean, onClick: () -> Unit) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(SmoothCornerShape(12.dp)).clickable(enabled = enabled, role = Role.Button, onClick = onClick),
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
    val finishCallFirst = stringResource(R.string.accounts_finish_call_first)
    val target = remember(did) { app.node.accounts().firstOrNull { it.did == did } }
    val current = remember { app.node.profile()?.name ?: "" }
    // With wake pushes the account I leave sleeps but still rings and notifies, if its key is remembered here.
    val leftStillRings = remember { PushMode.pushOn(app) && app.accountDid.value?.let { UnlockStore.exists(ctx, it) } == true }
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
                if (it is uniffi.p2pcore.Exception.InCall) { onMessage(finishCallFirst); onBack() }
                else error = friendly(it)
            }
            r.onSuccess { pass = ""; onSwitched() }
        }
    }
    Page {
        TopBar(stringResource(R.string.accounts_switch_account), onBack)
        Column(
            Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 16.dp, bottom = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Avatar(target.name.ifBlank { "?" }, target.did, 72.dp)
            H1(stringResource(R.string.accounts_switch_to, target.name), align = TextAlign.Center)
            Lead(stringResource(if (leftStillRings) R.string.accounts_switch_lead_push else R.string.accounts_switch_lead, current), align = TextAlign.Center)
            if (needsPass) TinField(pass, { pass = it; error = null }, stringResource(R.string.accounts_passphrase_for, target.name), mono = true, password = true, enabled = !busy,
                state = if (error != null) FieldState.Error else FieldState.Normal, hint = error,
                keyboard = KeyboardOptions(imeAction = ImeAction.Done), actions = KeyboardActions(onDone = { if (pass.isNotEmpty() && !busy) go() }))
            else error?.let { Text(it, style = TinType.bodyM, color = c.er) }
            if (inCall == null) Hint(stringResource(R.string.accounts_not_on_call), align = TextAlign.Center)
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth(), color = c.pr, trackColor = c.sf3)
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            TinButton(if (busy) stringResource(R.string.accounts_switching) else if (needsPass) stringResource(R.string.accounts_unlock_and_switch) else stringResource(R.string.accounts_switch), ::go, enabled = !busy && (!needsPass || pass.isNotEmpty()))
            TinButton(stringResource(R.string.common_cancel), onBack, style = BtnStyle.Text, enabled = !busy)
        }
    }
}
