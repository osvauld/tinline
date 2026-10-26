package com.osvauld.p2p

import androidx.compose.ui.res.stringResource
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
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
    onVideo: () -> Unit, onStorage: () -> Unit, onAbout: () -> Unit, onDiagnostics: () -> Unit, onLanguage: () -> Unit,
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
    SettingsLanding(store, fallback, list, permProblems(missing), onBack, onMe, onSwitchPick, { adding = true },
        onDevices, onPrivacy, onNotifications, onSounds, onVideo, onStorage, onAbout) {
        // Not in the playground: the app's language (older phones have no per-app language page in the system).
        PfSec {
            PfCard {
                val tag = remember { AppLanguage.current(ctx) }
                act { PfAct(stringResource(R.string.settings_language), onClick = onLanguage, icon = PgIcons.Globe, chevron = true,
                    sub = if (tag.isEmpty()) stringResource(R.string.settings_language_match) else AppLanguage.nativeName(tag)) }
            }
        }
        // Not in the playground: the app's "Video in calls" offer, and developer tools.
        var video by remember { mutableStateOf(app.videoEnabled) }
        PfSec {
            PfCard {
                act { PfAct(stringResource(R.string.settings_video_in_calls), onClick = { video = !video; app.videoEnabled = video }, icon = Pg.Video, plain = true, role = androidx.compose.ui.semantics.Role.Switch,
                    sub = if (video) stringResource(R.string.settings_video_in_calls_on) else stringResource(R.string.settings_video_in_calls_off),
                    trailing = { PgSwitch(video, { video = it; app.videoEnabled = it }, stringResource(R.string.settings_video_in_calls)) }) }
                if (BuildConfig.DEBUG || BuildConfig.VIDEO_TUNING) act { PfAct("Diagnostics", onClick = onDiagnostics, chevron = true, sub = "Test tone, video tuning, core version") }
            }
        }
    }
    // gate.beginAdd(): the playground's Choose step. The app picks the path before it begins adding.
    PgLayer(adding, { adding = false }, stringResource(R.string.settings_add_account)) {
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)) {
            DlgTitle(stringResource(R.string.settings_add_account))
            Column(Modifier.fillMaxWidth().padding(top = 10.dp).bleedX(12.dp)) {
                val shape = androidx.compose.foundation.shape.RoundedCornerShape(14.dp)
                PfAct(stringResource(R.string.settings_add_create), onClick = { adding = false; onCreateAccount() }, icon = PgIcons.Plus, padH = 12.dp, shape = shape)
                PfAct(stringResource(R.string.settings_add_link), onClick = { adding = false; onLinkAccount() }, icon = PgIcons.Devices, padH = 12.dp, shape = shape)
                PfAct(stringResource(R.string.settings_add_restore), onClick = { adding = false; onRestoreAccount() }, icon = PgIcons.Key, padH = 12.dp, shape = shape)
            }
            PfActs { PBtn(stringResource(R.string.common_cancel), { adding = false }, variant = PVariant.Quiet) }
        }
    }
}

@Composable
internal fun VideoSettingsRoute(app: P2pApp, onBack: () -> Unit) {
    var quality by remember { mutableStateOf(app.videoQualityPreference) }
    VideoSettingsPage(quality, { quality = it; app.videoQualityPreference = it }, onBack)
}

@Composable
fun StorageRoute(app: P2pApp, onBack: () -> Unit) {
    val ctx = LocalContext.current
    var tick by remember { mutableIntStateOf(0) }
    var cleared by remember { mutableStateOf(false) }
    var download by remember { mutableStateOf(uniffi.p2pcore.PictureDownload.ALWAYS) }
    val sizes by produceState<List<Long>>(listOf(0L, 0L, 0L, 0L), tick) {
        value = withContext(Dispatchers.IO) {
            val u = runCatching { app.node.storageUsage() }.getOrNull()
            runCatching { download = app.node.pictureDownload() }
            listOf(u?.pictures?.toLong() ?: 0L, u?.files?.toLong() ?: 0L, u?.messages?.toLong() ?: 0L, ChatMedia.size(ctx))
        }
    }
    StoragePage(sizes[0], sizes[1], sizes[2], sizes[3], download, cleared,
        { d -> download = d; app.scope.launch(Dispatchers.IO) { runCatching { app.node.setPictureDownload(d) } } },
        { ChatMedia.clear(ctx); cleared = true; tick++ }, onBack)
}

@Composable
fun MyProfileRoute(app: P2pApp, onBack: () -> Unit, onEdit: () -> Unit, onShare: () -> Unit, onRecovery: () -> Unit) {
    val acct by app.accountDid.collectAsState()
    key(acct) {
        AccountProfileScreen(remember(app) { ProfileBackend.of(app) }, profileFallbackName(app, acct), onBack, onEdit, onShare, onRecovery)
    }
}

@Composable
fun EditMyProfileRoute(app: P2pApp, onBack: () -> Unit) {
    val acct by app.accountDid.collectAsState()
    key(acct) { MyProfileScreen(remember(app) { ProfileBackend.of(app) }, profileFallbackName(app, acct), onBack, title = stringResource(R.string.settings_edit_profile)) }
}

@Composable
fun MyContactCodeRoute(app: P2pApp, onBack: () -> Unit) {
    val acct by app.accountDid.collectAsState()
    val status by app.status.collectAsState()
    key(acct) {
        var retry by remember { mutableIntStateOf(0) }
        val loaded by produceState<Pair<String?, Boolean>>(null to false, status?.online, retry) {
            value = null to false
            val ticket = app.profiles.session.ticket() ?: return@produceState
            val result = withContext(Dispatchers.IO) { runCatching { app.profileWork(ticket) { it.myCode().code } }.getOrNull() }
            if (app.profiles.session.accepts(ticket)) value = result to (result == null)
        }
        MyContactCodeScreen(remember(app) { ProfileBackend.of(app) }, profileFallbackName(app, acct), loaded.first, loaded.second, onBack, { retry++ })
    }
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
    var reads by remember(acct) { mutableStateOf(true) }
    LaunchedEffect(acct) {
        withContext(Dispatchers.IO) { runCatching { app.node.readReceipts() }.getOrNull() }?.let { reads = it }
    }
    // "Show me as online in public groups": per install in the group core (default on).
    var shown by remember(acct) { mutableStateOf(true) }
    LaunchedEffect(acct) {
        withContext(Dispatchers.IO) { runCatching { app.node.groupPresenceVisible() }.getOrNull() }?.let { shown = it }
    }
    // Group pictures (public group §5): per install in the group core (default Wi-Fi only).
    var pictures by remember(acct) { mutableStateOf(uniffi.p2pcore.PictureDownload.WIFI) }
    LaunchedEffect(acct) {
        withContext(Dispatchers.IO) { runCatching { app.node.groupImageAutoDownload() }.getOrNull() }?.let { pictures = it }
    }
    PrivacyLanding(blocks.list.size, msgOnly, hasPass, onBack, onBlocked, onCalls, onPhrase, onPassphrase, reads,
        { on -> reads = on; app.scope.launch(Dispatchers.IO) { runCatching { app.node.setReadReceipts(on) } } },
        shown, { on -> shown = on; app.scope.launch(Dispatchers.IO) { runCatching { app.node.groupSetPresenceVisible(on) } } },
        pictures, { d -> pictures = d; app.scope.launch(Dispatchers.IO) { runCatching { app.node.groupSetImageAutoDownload(d) } } })
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
