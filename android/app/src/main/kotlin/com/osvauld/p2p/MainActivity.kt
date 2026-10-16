package com.osvauld.p2p

import android.content.Context
import android.content.ContextWrapper
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.enableEdgeToEdge
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import android.content.Intent
import androidx.compose.runtime.*
import uniffi.p2pcore.CallState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.unit.dp
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import uniffi.p2pcore.LockState
import androidx.compose.runtime.snapshots.SnapshotStateList
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * FLAG_SECURE keeps the recovery phrase, passphrases and the call screen out of screenshots, screen
 * recording and the recents thumbnail. Release builds only: debug builds stay capturable so the
 * emulator test scripts and developers can take screenshots.
 */
fun androidx.activity.ComponentActivity.secureWindow() {
    if (BuildConfig.DEBUG) return
    window.setFlags(android.view.WindowManager.LayoutParams.FLAG_SECURE, android.view.WindowManager.LayoutParams.FLAG_SECURE)
    if (android.os.Build.VERSION.SDK_INT >= 33) setRecentsScreenshotEnabled(false)
}

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        secureWindow()
        enableEdgeToEdge()
        val app = P2pApp.get(this)
        if (app.node.hasIdentity()) CoreService.ensureRunning(this)
        intent?.getStringExtra(ChatNotifier.EXTRA_PEER)?.let { OpenChat.request.value = it }
        if (intent?.getBooleanExtra(Notifications.EXTRA_GROUPS, false) == true) OpenChat.groupsTab.value = true
        groupCallFrom(intent)
        if (savedInstanceState == null) { CardInbox.take(intent); GroupLinkInbox.take(intent); requestFrom(intent) }
        Appearance.load(this)
        setContent {
            val chosen by Appearance.dark.collectAsState()
            TinlineTheme(dark = chosen ?: androidx.compose.foundation.isSystemInDarkTheme()) { Root(app) }
        }
    }

    /** A tap on "Maya wants to add you as a contact": open the request sheet for it, if it is still waiting. */
    private fun requestFrom(intent: android.content.Intent?) {
        val did = intent?.getStringExtra(Notifications.EXTRA_REQUEST) ?: return
        val app = P2pApp.get(this)
        app.requestsIn.value.firstOrNull { it.did == did }?.let { app.incoming.value = IncomingSheet(it.did, it.name, joined = false, theirs = false) }
    }

    /** The group-call notification: Join takes the ringing call; a tap on the ongoing one returns to its screen. */
    private fun groupCallFrom(intent: android.content.Intent?) {
        val app = P2pApp.get(this)
        intent?.getStringExtra(Notifications.EXTRA_GCALL_JOIN)?.let { intent.removeExtra(Notifications.EXTRA_GCALL_JOIN); app.groupCallUi.call(it) {} }
        if (intent?.getBooleanExtra(Notifications.EXTRA_GCALL_OPEN, false) == true) { intent.removeExtra(Notifications.EXTRA_GCALL_OPEN); app.groupCallUi.restore() }
    }

    override fun onResume() { super.onResume(); AppVisible.resumed = true }
    override fun onPause() { AppVisible.resumed = false; P2pApp.get(this).calls.appLeftFront(); P2pApp.get(this).groupCallUi.appLeftFront(); super.onPause() }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        requestFrom(intent)
        groupCallFrom(intent)
        intent.getStringExtra(ChatNotifier.EXTRA_PEER)?.let { OpenChat.request.value = it }
        if (intent.getBooleanExtra(Notifications.EXTRA_GROUPS, false)) OpenChat.groupsTab.value = true
        CardInbox.take(intent)
        GroupLinkInbox.take(intent)
    }
}

private sealed interface Route {
    data object Home : Route
    data class Add(val scan: Boolean, val ticket: String? = null, val paste: Boolean = false) : Route
    data class Contact(val did: String) : Route
    data class Verify(val did: String) : Route
    data object Settings : Route
    data object History : Route
    data class Chat(val did: String, val searchHit: uniffi.p2pcore.MessageHit? = null) : Route
    data object NewChat : Route
    data object Battery : Route
    data object Passphrase : Route
    data object PhraseGate : Route
    data class PhraseShown(val phrase: String) : Route
    data object About : Route
    data object Licences : Route
    data object Diagnostics : Route
    data object MicNeeded : Route
    data class SwitchConfirm(val did: String) : Route
    data object Devices : Route
    data class DeviceDetail(val device: String) : Route
    data object LinkDevice : Route
    /** Settings › Blocked people (real data). */
    data object Blocked : Route
    /** Group chat on the core (GroupsLive.kt). */
    data class Group(val id: String) : Route
    data class GroupInfo(val id: String) : Route
    /** The invite link with its QR (G3). */
    data class GroupToken(val id: String) : Route
    /** Pick contacts to invite (G3). */
    data class AddMembers(val id: String) : Route
    data class GroupRemoval(val id: String) : Route
    data object NewGroup : Route
    /** [token]: a tapped tinline://g/ link, prefilled. */
    data class JoinGroup(val token: String? = null) : Route
    /** Full-screen search (Search.kt). */
    data object Search : Route
    /** Playground-ported settings/profile pages (SettingsRoutes.kt). */
    data object Me : Route
    data object EditMe : Route
    data object MyCode : Route
    data object Notifications : Route
    data object Sounds : Route
    data object Video : Route
    data object Storage : Route
    data object Privacy : Route
    data object CallsAllowed : Route
}

@Composable
private fun Root(app: P2pApp) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val has by app.hasIdentity.collectAsState()
    val lock by app.lockState.collectAsState()
    val contacts by app.contacts.collectAsState()
    val history by app.history.collectAsState()
    val adding by app.adding.collectAsState()
    val acct by app.accountDid.collectAsState()
    LaunchedEffect(acct, lock) { if (lock != LockState.UNLOCKED) { ConversationDrafts.clear(); SearchJump.pending.value = null } }
    DisposableEffect(acct) { onDispose { ConversationDrafts.clear(); SearchJump.pending.value = null } }
    val snack = remember { androidx.compose.material3.SnackbarHostState() }
    var forgot by rememberSaveable { mutableStateOf(false) }
    var onboarding by rememberSaveable { mutableStateOf(!app.node.hasIdentity()) }
    var termsOk by remember { mutableStateOf(LegalStore.accepted(ctx)) }
    var missing by remember { mutableStateOf(Perms.missing(ctx)) }
    var callError by remember { mutableStateOf<String?>(null) }
    var pendingCall by remember { mutableStateOf<String?>(null) }
    // Back stack. Not saved: the recovery phrase may sit in it, and Home is a fine place to restart.
    val stack = remember { mutableStateListOf<Route>(Route.Home) }
    // Home's tab survives opening a chat or group and coming back.
    val homeTab = rememberSaveable { mutableStateOf(HomeTab.Chats) }
    fun pop() { if (stack.size > 1) stack.removeAt(stack.lastIndex) }
    fun home() { stack.clear(); stack.add(Route.Home) }
    fun say(msg: String) { scope.launch { snack.currentSnackbarData?.dismiss(); snack.showSnackbar(msg) } }
    /** Create / restore: leave the current account and run the add-account onboarding. */
    fun addAccount(restore: Boolean, link: Boolean = false) {
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { app.beginAdding(restore, link) } }
            r.onFailure { say(friendly(it)) }
            r.onSuccess { home(); onboarding = true }
        }
    }
    val prefs = remember { ctx.getSharedPreferences("perm_state", Context.MODE_PRIVATE) }
    val owner = LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val o = LifecycleEventObserver { _, e ->
            if (e == Lifecycle.Event.ON_RESUME) {
                missing = Perms.missing(ctx); app.refresh()
                if (stack.last() == Route.MicNeeded && Perms.granted(ctx, android.Manifest.permission.RECORD_AUDIO)) pop()
            }
        }
        owner.lifecycle.addObserver(o); onDispose { owner.lifecycle.removeObserver(o) }
    }

    // A tap on a message notification opens that conversation once the app is past onboarding and unlock.
    val openReq by OpenChat.request.collectAsState()
    LaunchedEffect(openReq, has, lock, termsOk) {
        val did = openReq ?: return@LaunchedEffect
        if (has && lock == LockState.UNLOCKED && termsOk && !onboarding) {
            OpenChat.request.value = null
            if ((stack.last() as? Route.Chat)?.did != did) stack.add(Route.Chat(did))
        }
    }

    // A tap on a group invitation: Home on the Groups tab, where the invitation waits.
    val openGroups by OpenChat.groupsTab.collectAsState()
    LaunchedEffect(openGroups, has, lock, termsOk) {
        if (openGroups && has && lock == LockState.UNLOCKED && termsOk && !onboarding) {
            OpenChat.groupsTab.value = false
            while (stack.size > 1) stack.removeAt(stack.lastIndex)
            homeTab.value = HomeTab.Groups
        }
    }

    fun place(did: String) {
        callError = null
        // Either side blocking, or someone no longer a contact: no call (the core refuses it too).
        val isContact = contacts.any { it.did == did }
        val bs = app.blocks.value.composer(did, isContact)
        if (app.blocks.value.callGate(did, isContact) != CallGate.Ok) {
            val name = contacts.firstOrNull { it.did == did }?.display() ?: app.chat.chats.value.firstOrNull { it.peerDid == did }?.peerName ?: ""
            say(BlockCopy.cantCall(name, bs)); return
        }
        scope.launch {
            val r = withContext(Dispatchers.IO) { app.calls.place(did) }
            r.onFailure { callError = it.message ?: "Couldn\u2019t start the call" }
        }
    }
    val permLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        missing = Perms.missing(ctx)
        pendingCall?.let { did ->
            pendingCall = null
            if (Perms.granted(ctx, android.Manifest.permission.RECORD_AUDIO)) place(did) else stack.add(Route.MicNeeded)
        }
    }
    val settingsLauncher = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        missing = Perms.missing(ctx)
    }
    val fix: (Need) -> Unit = { n ->
        val p = Perms.runtimePermission(n)
        val i = Perms.settingsIntent(ctx, n)
        when {
            p != null -> {
                // After a permanent denial the system shows no dialog and "Allow" would do nothing:
                // asked before + no rationale to show = go to the settings page instead.
                val act = ctx.findActivity()
                val blocked = Perms.granted(ctx, p) ||
                    (prefs.getBoolean("asked_$p", false) && act != null &&
                        !androidx.core.app.ActivityCompat.shouldShowRequestPermissionRationale(act, p))
                if (blocked) Perms.openSettings(ctx, i ?: Perms.appDetails(ctx))
                else { prefs.edit().putBoolean("asked_$p", true).apply(); permLauncher.launch(arrayOf(p)) }
            }
            i != null -> try { settingsLauncher.launch(i) } catch (_: Exception) { Perms.openSettings(ctx, Perms.appDetails(ctx)) }
        }
    }
    /** Tap on a call button: microphone first (system dialog once, then the explainer screen), then dial. */
    val call: (String) -> Unit = { did ->
        val mic = android.Manifest.permission.RECORD_AUDIO
        if (Perms.granted(ctx, mic)) place(did)
        else {
            val act = ctx.findActivity()
            val blocked = prefs.getBoolean("asked_$mic", false) && act != null &&
                !androidx.core.app.ActivityCompat.shouldShowRequestPermissionRationale(act, mic)
            if (blocked) stack.add(Route.MicNeeded)
            else { pendingCall = did; prefs.edit().putBoolean("asked_$mic", true).apply(); permLauncher.launch(arrayOf(mic)) }
        }
    }

    // A tapped tinline://g/ link opens Join with it (G3).
    val groupLink by GroupLinkInbox.link.collectAsState()
    LaunchedEffect(groupLink, has, lock) {
        val l = groupLink ?: return@LaunchedEffect
        if (has && lock == LockState.UNLOCKED) { GroupLinkInbox.link.value = null; stack.add(Route.JoinGroup(l)) }
    }
    // "Leo accepted your request" and the like, once.
    val groupNotice by app.groups.notice.collectAsState()
    LaunchedEffect(groupNotice) { groupNotice?.let { say(it); app.groups.clearNotice() } }
    val unlinkedName by app.unlinked.collectAsState()
    // The account went away (unlinked here or from another device): leave whatever screen was open.
    LaunchedEffect(has) { if (!has) home() }
    // A minimised call's strip sits in flow at the top (CallStrip.kt), under the status bar, and takes that inset from
    // the app below it; the group call screen lies over all.
    val stripOn = callStripOn(app)
    Box(Modifier.fillMaxSize()) {
    Column(Modifier.fillMaxSize()) {
    CallStrips(app)
    Box(Modifier.weight(1f).fillMaxWidth()
        .then(if (stripOn) Modifier.consumeWindowInsets(WindowInsets.statusBars) else Modifier)) {
    when {
        unlinkedName != null -> UnlinkedScreen(unlinkedName!!, onOk = { app.dismissUnlinked(); home() },
            onLinkAgain = { app.dismissUnlinked(); if (!app.node.hasIdentity()) onboarding = true else addAccount(false, link = true) })
        onboarding || !has || adding != null -> key(adding) { OnboardingFlow(app, missing, fix,
            // Done "Add my first contact": Chats (its empty hero) with Add contact open over it, on By code.
            onAddFirst = { onboarding = false; home(); homeTab.value = HomeTab.Chats; termsOk = LegalStore.accepted(ctx); missing = Perms.missing(ctx); stack.add(Route.Add(false)) },
        ) { onboarding = false; home(); homeTab.value = HomeTab.Chats; termsOk = LegalStore.accepted(ctx); missing = Perms.missing(ctx) } }
        lock == LockState.LOCKED ->
            if (forgot) OnboardingFlow(app, missing, fix, forgot = true, onCancelForgot = { forgot = false }) { forgot = false; missing = Perms.missing(ctx) }
            else key(acct) { UnlockScreen(app) { forgot = true } }
        !termsOk -> TermsScreen(progress = null, onBack = null) { LegalStore.accept(ctx); termsOk = true }
        else -> {
            BackHandler(stack.size > 1) { pop() }
            val inCall by app.calls.ui.collectAsState()
            val top = stack.last()
            // RingHead.svelte: a call ringing while the app is open turns the home header into the caller, in place. Over
            // my own answered call (call minimised to the strip) a second caller does the same, with Decline / End & answer.
            val waitingCall by app.calls.waiting.collectAsState()
            LaunchedEffect(inCall, waitingCall) {
                val u = inCall; val w = waitingCall
                RingHead.ring.value = when {
                    u != null && w != null && u.state is CallState.Active -> RingHead.Ring(
                        w.peerDid, w.peerName.ifBlank { "Unknown" }, "Calling during your call", waiting = true,
                        onOpen = { ctx.startActivity(Intent(ctx, CallActivity::class.java)) },
                        onDecline = { app.calls.declineWaiting() }, onAnswer = { app.calls.endAndAnswer() })
                    u != null && u.info.incoming && u.state is CallState.Ringing -> RingHead.Ring(
                        u.info.peerDid, u.info.peerName.ifBlank { "Unknown" }, "Incoming call",
                        onOpen = { ctx.startActivity(Intent(ctx, IncomingCallActivity::class.java)) },
                        onDecline = { app.calls.decline() },
                        onAnswer = { ctx.startActivity(Intent(ctx, CallActivity::class.java).putExtra(CallActivity.EXTRA_ANSWER, true)) })
                    else -> null
                }
            }
            DisposableEffect(Unit) { onDispose { RingHead.ring.value = null } }
            val onHomeOrAdd = top is Route.Home || top is Route.Add
            // Someone used my code, or accepted my request while the sheet was closed: the sheet opens by itself over
            // Home (my own open Add contact sheet turns into it), never over a call or another screen; else it's dropped
            // and the request waits in Contacts.
            val inc by app.incoming.collectAsState()
            LaunchedEffect(inc) {
                inc ?: return@LaunchedEffect
                when (stack.last()) {
                    is Route.Add -> {}
                    Route.Home -> if (app.calls.ui.value == null) stack.add(Route.Add(false)) else app.incoming.value = null
                    else -> app.incoming.value = null
                }
            }
            CardPrompts(app, clipboardOk = onHomeOrAdd && inCall == null, shareOk = true, showOnScreen = inCall == null, onChat = { p -> if (top is Route.Add) pop(); stack.add(Route.Chat(p.did)) }) { p ->
                if (stack.last() is Route.Add) pop()
                stack.add(Route.Add(false, ticket = p.ticket))
            }
            val homeScreen: @Composable () -> Unit = { HomeScreen(
                    app, missing, fix, onAdd = { stack.add(Route.Add(it)) }, onPaste = { stack.add(Route.Add(false, paste = true)) }, onSettings = { stack.add(Route.Settings) },
                    onContact = { stack.add(Route.Contact(it.did)) }, onCall = { call(it.did) }, callError = callError,
                    onChat = { stack.add(Route.Chat(it)) }, onNewChat = { stack.add(Route.NewChat) },
                    homeTab = homeTab,
                    say = ::say, onGroup = { stack.add(Route.Group(it)) }, onGroupInfo = { stack.add(Route.GroupInfo(it)) }, onNewGroup = { stack.add(Route.NewGroup) }, onJoinGroup = { stack.add(Route.JoinGroup()) },
                    onSearch = { stack.add(Route.Search) },
                ) }
            when (val r = stack.last()) {
                Route.Home -> homeScreen()
                Route.Search -> SearchScreen(app, onBack = ::pop, onChat = { did ->
                    val hit = SearchJump.pending.value?.takeIf { it.conv == did && it.kind == uniffi.p2pcore.SearchConvKind.DM }
                    SearchJump.pending.value = null
                    stack.add(Route.Chat(did, hit))
                }, onGroup = { SearchJump.pending.value = null; stack.add(Route.Group(it)) },
                    onContact = { SearchJump.pending.value = null; stack.add(Route.Contact(it)) })
                is Route.Group -> GroupChatLive(app, r.id, onBack = ::pop, onInfo = { stack.add(Route.GroupInfo(r.id)) }, onRemoval = { stack.add(Route.GroupRemoval(r.id)) }, say = ::say)
                is Route.GroupInfo -> GroupInfoLive(app, r.id, onBack = ::pop, onLink = { stack.add(Route.GroupToken(r.id)) }, onAddMembers = { stack.add(Route.AddMembers(r.id)) }, onRemoval = { stack.add(Route.GroupRemoval(r.id)) }, say = ::say,
                    onChat = { stack.add(Route.Chat(it)) })
                is Route.GroupToken -> GroupTokenLive(app, r.id, onBack = ::pop)
                is Route.AddMembers -> AddMembersLive(app, r.id, onBack = ::pop, say = ::say)
                is Route.GroupRemoval -> GroupRemovalLive(app, r.id, onBack = ::pop)
                Route.NewGroup -> NewGroupLive(app, onBack = ::pop, onCreated = { pop(); stack.add(Route.Group(it)) })
                is Route.JoinGroup -> JoinGroupLive(app, r.token, onBack = ::pop, onJoined = { pop(); stack.add(Route.Group(it)); say("You joined the group.") })
                Route.NewChat -> {
                    val chats by app.chat.chats.collectAsState()
                    val last = remember(chats) { chats.filter { it.lastActivity > 0UL }.associate { it.peerDid to "Last message ${listTime(it.lastActivity.toLong(), System.currentTimeMillis())}" } }
                    NewChatScreen(contacts, last, onBack = ::pop, onPick = { pop(); stack.add(Route.Chat(it.did)) }, onAdd = { pop(); stack.add(Route.Add(true)) })
                }
                is Route.Chat -> {
                    val chats by app.chat.chats.collectAsState()
                    val status by app.status.collectAsState()
                    val c = contacts.firstOrNull { it.did == r.did }
                    val self = r.did == app.currentDid()
                    val name = if (self) "You" else c?.display() ?: chats.firstOrNull { it.peerDid == r.did }?.peerName?.ifBlank { null } ?: "Unknown"
                    val blocks by app.blocks.collectAsState()
                    ConversationScreen(app.chat, r.did, name, status?.online == true, onBack = ::pop, onCall = { call(r.did) }, addedAtSecs = c?.addedAt?.toLong(), isSelf = self,
                        block = if (self) null else ConvBlock(blocks.of(r.did), blocks.composer(r.did, c != null), blocks.callGate(r.did, c != null)),
                        onUnblock = { app.unblock(r.did) }, onAddContact = { stack.add(Route.Add(true)) },
                        notDelivered = { id -> app.notDeliveredReason(r.did, id) },
                        onInfo = if (c != null) ({ stack.add(Route.Contact(r.did)) }) else null,
                        searchHit = r.searchHit,
                        calls = app.history.collectAsState().value.filter { it.peerDid == r.did },
                        onVideo = { app.video.wantCamera = true; call(r.did) })
                }
                Route.History -> HistoryScreen(app, onBack = ::pop, onContact = { stack.add(Route.Contact(it.did)) })
                // Add contact is a sheet over Chats (the first screen's hero shows through and pulls tight behind it).
                is Route.Add -> { homeScreen(); AddContactSheet(app, r.scan, r.ticket, r.paste, onClose = { app.incoming.value = null; pop() },
                    onMessage = { did -> app.incoming.value = null; pop(); stack.add(Route.Chat(did)) }, onProfile = { did -> pop(); stack.add(Route.Contact(did)) }) }
                is Route.Contact -> {
                    val c = contacts.firstOrNull { it.did == r.did }
                    if (c == null) LaunchedEffect(Unit) { pop() }
                    else ContactScreen(c, onBack = ::pop, onCall = { call(c.did) }, onMessage = { stack.add(Route.Chat(c.did)) }, onVerify = { stack.add(Route.Verify(c.did)) },
                        block = app.blocks.collectAsState().value.of(c.did), onUnblock = { app.unblock(c.did) },
                        callGate = app.blocks.collectAsState().value.callGate(c.did, true),
                        callsAllowed = app.blocks.collectAsState().value.callsAllowed(c.did), onAllowCalls = { on -> app.allowCalls(c.did, on) },
                        history = remember(history, contacts) { val now = System.currentTimeMillis() / 1000; val by = contacts.associateBy { it.did }; history.filter { it.peerDid == c.did }.map { it.toRecent(by, now) } },
                        onRename = { a -> scope.launch(Dispatchers.IO) { runCatching { app.node.renameContact(c.did, a) }; app.refresh() } }, onBlock = {
                        // U19/U24: the contact and the chat stay; the page shows Blocked and Unblock.
                        app.block(c.did); say("${firstName(c.display())} is told you blocked them")
                    })
                }
                is Route.Verify -> {
                    val c = contacts.firstOrNull { it.did == r.did }
                    if (c == null) LaunchedEffect(Unit) { pop() }
                    else {
                        val groups = remember(c.did, c.device) { runCatching { app.node.safetyNumber(c.did).trim().split(Regex("\\s+")) }.getOrNull() }
                        VerifyScreen(app.node.profile()?.name ?: "", c, groups, onBack = ::pop, onMatch = {
                            scope.launch(Dispatchers.IO) { runCatching { app.node.setVerified(c.did, true) }; app.refresh() }
                            pop()
                        }, onBlock = {
                            pop()
                            app.block(c.did); say("${firstName(c.display())} is told you blocked them")
                        })
                    }
                }
                Route.Settings -> SettingsRoute(
                    app, missing, onBack = ::pop, onMe = { stack.add(Route.Me) }, onSwitchPick = { stack.add(Route.SwitchConfirm(it)) },
                    onCreateAccount = { addAccount(false) }, onRestoreAccount = { addAccount(true) }, onLinkAccount = { addAccount(false, link = true) },
                    onDevices = { stack.add(Route.Devices) }, onPrivacy = { stack.add(Route.Privacy) }, onNotifications = { stack.add(Route.Notifications) },
                    onSounds = { stack.add(Route.Sounds) }, onVideo = { stack.add(Route.Video) }, onStorage = { stack.add(Route.Storage) }, onAbout = { stack.add(Route.About) }, onDiagnostics = { stack.add(Route.Diagnostics) },
                )
                Route.Me -> MyProfileRoute(app, onBack = ::pop, onEdit = { stack.add(Route.EditMe) }, onShare = { stack.add(Route.MyCode) }, onRecovery = { stack.add(Route.PhraseGate) })
                Route.EditMe -> EditMyProfileRoute(app, onBack = ::pop)
                Route.MyCode -> MyContactCodeRoute(app, onBack = ::pop)
                Route.Notifications -> NotificationsPage(missing, onBack = ::pop, fix = fix)
                Route.Sounds -> SoundsPage(onBack = ::pop)
                Route.Video -> VideoSettingsRoute(app, onBack = ::pop)
                Route.Storage -> StorageRoute(app, onBack = ::pop)
                Route.Privacy -> PrivacyRoute(app, onBack = ::pop, onBlocked = { stack.add(Route.Blocked) }, onCalls = { stack.add(Route.CallsAllowed) },
                    onPhrase = { stack.add(Route.PhraseGate) }, onPassphrase = { stack.add(Route.Passphrase) })
                Route.CallsAllowed -> CallsAllowedRoute(app, onBack = ::pop)
                Route.Blocked -> BlockedPeopleScreen(app, onBack = ::pop, say = ::say)
                is Route.SwitchConfirm -> SwitchConfirmScreen(app, r.did, onBack = ::pop, onSwitched = { home() }, onMessage = ::say)
                Route.Devices -> LinkedDevicesScreen(app, ::pop, onLink = { stack.add(Route.LinkDevice) }, onDetail = { stack.add(Route.DeviceDetail(it)) })
                is Route.DeviceDetail -> DeviceDetailScreen(app, r.device, onBack = ::pop, onMessage = ::say)
                Route.LinkDevice -> LinkSession(app, newDevice = false, label = "", onClose = ::pop)
                Route.Battery -> BatteryScreen(missing, onBack = ::pop, onFix = fix)
                Route.Passphrase -> ChangePassphraseScreen(app, onBack = ::pop)
                Route.PhraseGate -> PhraseGateScreen(app, onBack = ::pop, onPhrase = { p -> pop(); stack.add(Route.PhraseShown(p)) })
                is Route.PhraseShown -> PhraseShownScreen(r.phrase, onHide = ::pop, onRelock = {
                    // Back to the gate (replacing this screen) so the words leave memory and the next look needs the check again.
                    if (stack.lastOrNull() is Route.PhraseShown) { pop(); stack.add(Route.PhraseGate) }
                })
                Route.About -> AboutScreen(onBack = ::pop, onLicences = { stack.add(Route.Licences) })
                Route.Licences -> LicencesScreen(::pop)
                Route.Diagnostics -> DiagnosticsScreen(app, ::pop)
                Route.MicNeeded -> MicNeededScreen(onOpenSettings = { Perms.openSettings(ctx, Perms.appDetails(ctx)) }, onNotNow = ::pop)
            }
        }
    }
    // MiniVideo: their picture floats over the app while a video call is minimised.
    if (has && lock == LockState.UNLOCKED) MiniVideo(app, Modifier.align(androidx.compose.ui.Alignment.BottomEnd).padding(end = 12.dp, bottom = 92.dp))
    // "Sam started a call in Weekend hike": over the app, only once it is open and unlocked.
    if (has && lock == LockState.UNLOCKED && termsOk && !onboarding && adding == null && unlinkedName == null) {
        GroupCallAnnounce(app, openGid = (stack.last() as? Route.Group)?.id) { stack.add(Route.Group(it)) }
    }
    androidx.compose.material3.SnackbarHost(snack, Modifier.align(androidx.compose.ui.Alignment.BottomCenter).navigationBarsPadding().padding(16.dp))
    }
    }
    GroupCallLayer(app)
    }
}

fun Context.findActivity(): android.app.Activity? {
    var c: Context? = this
    while (c is ContextWrapper) { if (c is android.app.Activity) return c; c = c.baseContext }
    return null
}
