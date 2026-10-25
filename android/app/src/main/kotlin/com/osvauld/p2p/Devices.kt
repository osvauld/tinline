package com.osvauld.p2p

import androidx.compose.ui.res.stringResource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.LinkedDevice

/** "2 min ago", "3 hours ago", "23 days ago" for a unix-seconds timestamp. */
fun agoText(secs: Long, nowSecs: Long = System.currentTimeMillis() / 1000): String = uniffi.present.ago(nowSecs, secs)

/** The registry of the account, re-read whenever the core says it changed. */
@Composable
private fun rememberDevices(app: P2pApp): State<List<LinkedDevice>?> {
    val ver by app.links.devicesVersion.collectAsState()
    return produceState<List<LinkedDevice>?>(null, ver) {
        value = withContext(Dispatchers.IO) { runCatching { app.node.linkedDevices() }.getOrDefault(emptyList()) }
    }
}

/** Settings › Devices › Linked devices. */
@Composable
fun LinkedDevicesScreen(app: P2pApp, onBack: () -> Unit, onLink: () -> Unit, onDetail: (String) -> Unit) {
    val devices by rememberDevices(app)
    val list = devices ?: emptyList()
    val active = list.filter { !it.removed }
    val removed = list.filter { it.removed }
    Page {
        TopBar(stringResource(R.string.devices_title), onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
            Hint(stringResource(R.string.devices_hint), Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            active.forEach { d -> DeviceRow(d, onClick = { onDetail(d.device) }) }
            Box(Modifier.padding(horizontal = 20.dp, vertical = 16.dp)) { TinButton(stringResource(R.string.devices_link), onLink, icon = PgIcons.Plus) }
            if (removed.isNotEmpty()) {
                SectionLabel(stringResource(R.string.devices_unlinked_label))
                removed.forEach { d -> DeviceRow(d, onClick = { onDetail(d.device) }) }
            }
            SectionLabel(stringResource(R.string.devices_sync_label))
            Hint(stringResource(R.string.devices_sync_hint), Modifier.padding(horizontal = 20.dp))
        }
    }
}

@Composable
private fun DeviceRow(d: LinkedDevice, onClick: () -> Unit) {
    val title = if (d.thisDevice) stringResource(R.string.devices_this_phone_named, d.label.ifBlank { stringResource(R.string.devices_unnamed) }) else d.label.ifBlank { stringResource(R.string.devices_unnamed_device) }
    val seen = d.lastSeen
    val sub = when {
        d.removed -> stringResource(R.string.devices_sub_unlinked)
        d.thisDevice -> stringResource(R.string.devices_sub_this)
        seen != null -> stringResource(R.string.devices_sub_synced, agoText(seen.toLong()))
        else -> stringResource(R.string.devices_sub_not_seen)
    }
    ListItem(title, onClick = onClick, sub = sub, icon = if (d.thisDevice) PgIcons.Smartphone else PgIcons.Devices,
        titleColor = if (d.removed) Tin.c.ink2 else Tin.c.ink)
}

/** One device: rename, sync now, unlink (with the board's confirm that says what unlinking cannot do). */
@Composable
fun DeviceDetailScreen(app: P2pApp, device: String, onBack: () -> Unit, onMessage: (String) -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val devices by rememberDevices(app)
    val d = devices?.firstOrNull { it.device == device }
    val hasPass = remember { app.node.hasPassphrase() }
    var renaming by remember { mutableStateOf(false) }
    var confirm by remember { mutableStateOf(false) }
    var pass by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    if (devices != null && d == null) { LaunchedEffect(Unit) { onBack() }; return }
    if (d == null) { Page { TopBar(stringResource(R.string.devices_device), onBack) }; return }
    val others = devices.orEmpty().count { !it.removed && !it.thisDevice }

    fun unlink() {
        busy = true; error = null
        scope.launch {
            val did = app.currentDid()
            val r = withContext(Dispatchers.IO) { runCatching { app.node.unlinkDevice(d.device, if (hasPass) pass else null) } }
            busy = false
            r.onFailure {
                error = if (it is uniffi.p2pcore.Exception.InCall) ctx.getString(R.string.devices_finish_call) else friendly(it)
                if (it !is uniffi.p2pcore.Exception.WrongPassphrase) { confirm = false; onMessage(error!!) }
            }
            r.onSuccess {
                pass = ""; confirm = false
                if (d.thisDevice && did != null) withContext(Dispatchers.IO) { app.accountGone(did, notify = false) }
                else { onMessage(if (d.label.isBlank()) ctx.getString(R.string.devices_unlinked_msg_unnamed) else ctx.getString(R.string.devices_unlinked_msg, d.label)); onBack() }
            }
        }
    }
    fun confirmed() {
        val act = ctx.findActivity()
        if (hasPass || act == null) unlink()
        else DeviceAuth.ask(act, if (d.thisDevice) ctx.getString(R.string.devices_remove_account) else ctx.getString(R.string.devices_unlink_device)) { ok -> if (ok) unlink() else error = ctx.getString(R.string.devices_lock_cancelled) }
    }

    Page {
        TopBar(if (d.thisDevice) stringResource(R.string.devices_this_phone) else d.label.ifBlank { stringResource(R.string.devices_device) }, onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
            ListItem(stringResource(R.string.devices_name), onClick = if (d.removed) null else ({ renaming = true }), sub = d.label.ifBlank { stringResource(R.string.devices_unnamed) }, icon = PgIcons.Edit, trailing = null)
            ListItem(stringResource(R.string.devices_last_synced), sub = if (d.thisDevice) stringResource(R.string.devices_this_device) else d.lastSeen?.let { agoText(it.toLong()) } ?: stringResource(R.string.devices_not_yet), icon = PgIcons.Retry, trailing = null)
            ListItem(stringResource(R.string.devices_rings), sub = if (d.removed) stringResource(R.string.devices_rings_no) else stringResource(R.string.devices_rings_yes), icon = PgIcons.Phone, trailing = null)
            if (!d.removed) {
                Column(Modifier.padding(horizontal = 20.dp, vertical = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (!d.thisDevice) TinButton(stringResource(R.string.devices_sync_now), { app.node.networkChanged(); onMessage(if (d.label.isBlank()) ctx.getString(R.string.devices_syncing_unnamed) else ctx.getString(R.string.devices_syncing, d.label)) }, style = BtnStyle.Outlined, icon = PgIcons.Retry)
                    TinButton(if (d.thisDevice) stringResource(R.string.devices_remove_account) else stringResource(R.string.devices_unlink_this), { pass = ""; error = null; confirm = true }, style = BtnStyle.Danger, icon = PgIcons.Off)
                }
            }
        }
    }
    if (renaming) {
        var text by remember { mutableStateOf(d.label) }
        TinDialog(if (d.thisDevice) stringResource(R.string.devices_name_this_phone) else stringResource(R.string.devices_rename), { renaming = false }, stringResource(R.string.devices_save), {
            val n = text.trim()
            renaming = false
            scope.launch {
                val r = withContext(Dispatchers.IO) { runCatching { app.node.renameDevice(d.device, n) } }
                r.onFailure { onMessage(friendly(it)) }
                r.onSuccess { app.links.onDevicesChanged() }
            }
        }, confirmEnabled = text.isNotBlank()) { TinField(text, { text = it }, stringResource(R.string.devices_name_field)) }
    }
    if (confirm) {
        TinDialog(if (d.thisDevice) stringResource(R.string.devices_confirm_remove_title) else (if (d.label.isBlank()) stringResource(R.string.devices_confirm_unlink_title_unnamed) else stringResource(R.string.devices_confirm_unlink_title, d.label)), { if (!busy) confirm = false },
            if (busy) stringResource(R.string.devices_working) else if (d.thisDevice) stringResource(R.string.common_remove) else stringResource(R.string.devices_unlink), ::confirmed, destructive = true, icon = PgIcons.Off,
            confirmEnabled = !busy && (!hasPass || pass.isNotEmpty())) {
            if (d.thisDevice) {
                DialogText(stringResource(R.string.devices_remove_body))
                if (others == 0) InfoCard(stringResource(R.string.devices_remove_only), icon = PgIcons.Warning, kind = BannerKind.Error)
                else DialogText(stringResource(R.string.devices_remove_others))
            } else {
                DialogText(stringResource(R.string.devices_unlink_body))
                InfoCard(stringResource(R.string.devices_unlink_warn),
                    icon = PgIcons.Warning, kind = BannerKind.Warn, bold = stringResource(R.string.devices_lost_stolen))
            }
            if (hasPass) TinField(pass, { pass = it; error = null }, stringResource(R.string.settings_your_passphrase), mono = true, password = true, enabled = !busy,
                state = if (error != null) FieldState.Error else FieldState.Normal, hint = error,
                keyboard = KeyboardOptions(imeAction = ImeAction.Done), actions = KeyboardActions(onDone = { if (pass.isNotEmpty() && !busy) confirmed() }))
            else error?.let { Text(it, style = TinType.bodyM, color = c.er) }
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth(), color = c.pr, trackColor = c.sf3)
        }
    }
}
