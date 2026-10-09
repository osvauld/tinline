package com.osvauld.p2p

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.*
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
fun agoText(secs: Long, nowSecs: Long = System.currentTimeMillis() / 1000): String {
    val d = (nowSecs - secs).coerceAtLeast(0)
    fun n(v: Long, unit: String) = "$v $unit${if (v == 1L) "" else "s"} ago"
    return when {
        d < 60 -> "just now"
        d < 3600 -> n(d / 60, "min").replace("mins", "min")
        d < 86400 -> n(d / 3600, "hour")
        else -> n(d / 86400, "day")
    }
}

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
        TopBar("Linked devices", onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
            Hint("Calls ring on all of these. People you call never see these names.", Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            active.forEach { d -> DeviceRow(d, onClick = { onDetail(d.device) }) }
            Box(Modifier.padding(horizontal = 20.dp, vertical = 16.dp)) { TinButton("Link a device", onLink, icon = Icons.Rounded.Add) }
            if (removed.isNotEmpty()) {
                SectionLabel("UNLINKED")
                removed.forEach { d -> DeviceRow(d, onClick = { onDetail(d.device) }) }
            }
            SectionLabel("SYNC")
            Hint("Devices sync directly with each other whenever two are online at once. There is no copy on any server.", Modifier.padding(horizontal = 20.dp))
        }
    }
}

@Composable
private fun DeviceRow(d: LinkedDevice, onClick: () -> Unit) {
    val title = if (d.thisDevice) "This phone — ${d.label.ifBlank { "Unnamed" }}" else d.label.ifBlank { "Unnamed device" }
    val seen = d.lastSeen
    val sub = when {
        d.removed -> "Unlinked"
        d.thisDevice -> "This device · Android"
        seen != null -> "Synced ${agoText(seen.toLong())}"
        else -> "Not seen yet"
    }
    ListItem(title, onClick = onClick, sub = sub, icon = if (d.thisDevice) Icons.Rounded.PhoneAndroid else Icons.Rounded.Devices,
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
    if (d == null) { Page { TopBar("Device", onBack) }; return }
    val others = devices.orEmpty().count { !it.removed && !it.thisDevice }

    fun unlink() {
        busy = true; error = null
        scope.launch {
            val did = app.currentDid()
            val r = withContext(Dispatchers.IO) { runCatching { app.node.unlinkDevice(d.device, if (hasPass) pass else null) } }
            busy = false
            r.onFailure {
                error = if (it is uniffi.p2pcore.Exception.InCall) "Finish your call first" else friendly(it)
                if (it !is uniffi.p2pcore.Exception.WrongPassphrase) { confirm = false; onMessage(error!!) }
            }
            r.onSuccess {
                pass = ""; confirm = false
                if (d.thisDevice && did != null) withContext(Dispatchers.IO) { app.accountGone(did, notify = false) }
                else { onMessage("Unlinked ${d.label.ifBlank { "device" }}"); onBack() }
            }
        }
    }
    fun confirmed() {
        val act = ctx.findActivity()
        if (hasPass || act == null) unlink()
        else DeviceAuth.ask(act, if (d.thisDevice) "Remove account from this phone" else "Unlink device") { ok -> if (ok) unlink() else error = "Screen lock check cancelled" }
    }

    Page {
        TopBar(if (d.thisDevice) "This phone" else d.label.ifBlank { "Device" }, onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
            ListItem("Name", onClick = if (d.removed) null else ({ renaming = true }), sub = d.label.ifBlank { "Unnamed" }, icon = Icons.Rounded.Edit, trailing = null)
            ListItem("Last synced", sub = if (d.thisDevice) "This device" else d.lastSeen?.let { agoText(it.toLong()) } ?: "Not yet", icon = Icons.Rounded.Sync, trailing = null)
            ListItem("Rings for calls", sub = if (d.removed) "No, it was unlinked" else "Yes, when it’s on", icon = Icons.Rounded.Call, trailing = null)
            if (!d.removed) {
                Column(Modifier.padding(horizontal = 20.dp, vertical = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (!d.thisDevice) TinButton("Sync now", { app.node.networkChanged(); onMessage("Syncing with ${d.label.ifBlank { "the device" }}…") }, style = BtnStyle.Outlined, icon = Icons.Rounded.Sync)
                    TinButton(if (d.thisDevice) "Remove account from this phone" else "Unlink this device", { pass = ""; error = null; confirm = true }, style = BtnStyle.Danger, icon = Icons.Rounded.LinkOff)
                }
            }
        }
    }
    if (renaming) {
        var text by remember { mutableStateOf(d.label) }
        TinDialog(if (d.thisDevice) "Name this phone" else "Rename device", { renaming = false }, "Save", {
            val n = text.trim()
            renaming = false
            scope.launch {
                val r = withContext(Dispatchers.IO) { runCatching { app.node.renameDevice(d.device, n) } }
                r.onFailure { onMessage(friendly(it)) }
                r.onSuccess { app.links.onDevicesChanged() }
            }
        }, confirmEnabled = text.isNotBlank()) { TinField(text, { text = it }, "Device name") }
    }
    if (confirm) {
        val name = d.label.ifBlank { "this device" }
        TinDialog(if (d.thisDevice) "Remove your account from this phone?" else "Unlink $name?", { if (!busy) confirm = false },
            if (busy) "Working…" else if (d.thisDevice) "Remove" else "Unlink", ::confirmed, destructive = true, icon = Icons.Rounded.LinkOff,
            confirmEnabled = !busy && (!hasPass || pass.isNotEmpty())) {
            if (d.thisDevice) {
                DialogText("This phone stops syncing and ringing, and your account, contacts, chats and call history are deleted from it.")
                if (others == 0) InfoCard("This is the only device with your account. Without your recovery phrase nothing can bring it back.", icon = Icons.Rounded.Warning, kind = BannerKind.Error)
                else DialogText("Your other devices keep everything.")
            } else {
                DialogText("It stops syncing and stops ringing for your calls. If it’s online, it removes your account from itself.")
                InfoCard("Unlinking can’t lock it from here. Anyone who can unlock it can still use your account. The only full cut-off is a new account.",
                    icon = Icons.Rounded.Warning, kind = BannerKind.Warn, bold = "Lost or stolen?")
            }
            if (hasPass) TinField(pass, { pass = it; error = null }, "Your passphrase", mono = true, password = true, enabled = !busy,
                state = if (error != null) FieldState.Error else FieldState.Normal, hint = error,
                keyboard = KeyboardOptions(imeAction = ImeAction.Done), actions = KeyboardActions(onDone = { if (pass.isNotEmpty() && !busy) confirmed() }))
            else error?.let { Text(it, style = TinType.bodyM, color = c.er) }
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth(), color = c.pr, trackColor = c.sf3)
        }
    }
}
