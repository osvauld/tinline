package com.osvauld.p2p

import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/*
 * Production adapters: P2pApp state -> the playground-ported Settings / Profile / Privacy pages. Kept out of MainActivity so its
 * edits stay a few route lines.
 */

/** Settings landing with the real accounts, profile and permission problems. Native-only rows (About, video, debug) follow the playground's. */
@Composable
fun SettingsRoute(
    app: P2pApp, missing: List<Need>, onBack: () -> Unit, onMe: () -> Unit, onSwitchPick: (String) -> Unit,
    onCreateAccount: () -> Unit, onRestoreAccount: () -> Unit, onLinkAccount: () -> Unit,
    onDevices: () -> Unit, onPrivacy: () -> Unit, onNotifications: () -> Unit, onSounds: () -> Unit,
    onAbout: () -> Unit, onDiagnostics: () -> Unit, onPreview: (String) -> Unit,
) {
    val ctx = LocalContext.current
    val store = remember(app) { ProfileBackend.of(app) }
    val acct by app.accountDid.collectAsState()
    val list by produceState(emptyList<AccountRow>(), acct) {
        val ticket = app.profiles.session.ticket() ?: return@produceState
        val result = withContext(Dispatchers.IO) { runCatching { app.profileWork(ticket) { node -> node.accounts().map { AccountRow(it.did, it.name, it.current) } } }.getOrNull() }
        if (app.profiles.session.accepts(ticket) && result != null) value = result
    }
    val fallback = profileFallbackName(app, acct)
    var adding by remember { mutableStateOf(false) }
    val previewOn by PreviewFlags.enabled(ctx).collectAsState()
    SettingsLanding(store, fallback, list, permProblems(missing), onBack, onMe, onSwitchPick, { adding = true },
        onDevices, onPrivacy, onNotifications, onSounds) {
        // Not in the playground: the app's About (licences must stay reachable), the video offer, and debug-only tools.
        var video by remember { mutableStateOf(app.videoEnabled) }
        PfSec {
            PfCard {
                act { PfAct("Video in calls", onClick = { video = !video; app.videoEnabled = video }, icon = Pg.Video, plain = true, role = androidx.compose.ui.semantics.Role.Switch,
                    sub = if (video) "You can turn your camera on in a call" else "Calls are voice only",
                    trailing = { PgSwitch(video, { video = it; app.videoEnabled = it }, "Video in calls") }) }
                act { PfAct("About Tinline", onClick = onAbout, icon = PgIcons.Info, chevron = true, sub = "Version, licences, terms, privacy") }
                if (BuildConfig.DEBUG) act { PfAct("Diagnostics", onClick = onDiagnostics, chevron = true, sub = "Test tone and core version (debug builds)") }
                if (PreviewFlags.available) act {
                    PfAct(PreviewFlags.LABEL, plain = true, onClick = { PreviewFlags.set(ctx, previewOn != true) }, sub = "Debug builds only. Screens for features the core doesn’t have yet",
                        trailing = { PgSwitch(previewOn == true, { PreviewFlags.set(ctx, it) }, PreviewFlags.LABEL) })
                }
                if (previewOn == true) {
                    act { PfAct("People, requests & blocking (preview)", onClick = { onPreview("chats") }, chevron = true) }
                    act { PfAct("Address changes (preview)", onClick = { onPreview("address") }, chevron = true) }
                }
            }
        }
    }
    // gate.beginAdd(): the playground's Choose step. The app picks the path before it begins adding.
    PgLayer(adding, { adding = false }, "Add another account") {
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)) {
            DlgTitle("Add another account")
            Column(Modifier.fillMaxWidth().padding(top = 10.dp).bleedX(12.dp)) {
                val shape = androidx.compose.foundation.shape.RoundedCornerShape(14.dp)
                PfAct("Create a new account", onClick = { adding = false; onCreateAccount() }, icon = PgIcons.Plus, padH = 12.dp, shape = shape)
                PfAct("Link to a device", onClick = { adding = false; onLinkAccount() }, icon = PgIcons.Devices, padH = 12.dp, shape = shape)
                PfAct("Restore from recovery phrase", onClick = { adding = false; onRestoreAccount() }, icon = PgIcons.Key, padH = 12.dp, shape = shape)
            }
            PfActs { PBtn("Cancel", { adding = false }, variant = PVariant.Quiet) }
        }
    }
}

@Composable
fun MyProfileRoute(app: P2pApp, onBack: () -> Unit) {
    val acct by app.accountDid.collectAsState()
    val fallback = profileFallbackName(app, acct)
    MyProfileScreen(remember(app) { ProfileBackend.of(app) }, fallback, onBack)
}

@Composable
fun PrivacyRoute(app: P2pApp, onBack: () -> Unit, onBlocked: () -> Unit, onCalls: () -> Unit, onPhrase: () -> Unit, onPassphrase: () -> Unit) {
    val blocks by app.blocks.collectAsState()
    val contacts by app.contacts.collectAsState()
    val acct by app.accountDid.collectAsState()
    val hasPass by produceState(true, acct) {
        val ticket = app.profiles.session.ticket() ?: return@produceState
        val result = withContext(Dispatchers.IO) { runCatching { app.profileWork(ticket) { it.hasPassphrase() } }.getOrNull() }
        if (app.profiles.session.accepts(ticket) && result != null) value = result
    }
    val msgOnly = contacts.count { blocks.of(it.did).blockedByMe.not() && !blocks.callsAllowed(it.did) }
    PrivacyLanding(blocks.list.size, msgOnly, hasPass, onBack, onBlocked, onCalls, onPhrase, onPassphrase)
}

@Composable
fun CallsAllowedRoute(app: P2pApp, onBack: () -> Unit) {
    val blocks by app.blocks.collectAsState()
    val contacts by app.contacts.collectAsState()
    val rows = contacts.filter { !blocks.of(it.did).blockedByMe }.map { CallsRow(it.did, it.display(), blocks.callsAllowed(it.did)) }
    CallsAllowedPage(rows, onBack) { did, on -> app.allowCalls(did, on) }
}

/** No blocking Node calls during composition; stale account results never become fallback copy. */
@Composable
private fun profileFallbackName(app: P2pApp, account: String?): String {
    val name by produceState("", account) {
        val ticket = app.profiles.session.ticket() ?: return@produceState
        val result = withContext(Dispatchers.IO) { runCatching { app.profileWork(ticket) { it.profile()?.name } }.getOrNull() }
        if (app.profiles.session.accepts(ticket)) value = result ?: ""
    }
    return name
}
