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
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
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
        setContent { TinlineTheme { Root(app) } }
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        intent.getStringExtra(ChatNotifier.EXTRA_PEER)?.let { OpenChat.request.value = it }
    }
}

private sealed interface Route {
    data object Home : Route
    data class Add(val scan: Boolean) : Route
    data class Contact(val did: String) : Route
    data class Verify(val did: String) : Route
    data object Settings : Route
    data object History : Route
    data class Chat(val did: String) : Route
    data object NewChat : Route
    data object Battery : Route
    data object Passphrase : Route
    data object PhraseGate : Route
    data class PhraseShown(val phrase: String) : Route
    data object About : Route
    data object Licences : Route
    data object Diagnostics : Route
    data object MicNeeded : Route
}

@Composable
private fun Root(app: P2pApp) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val has by app.hasIdentity.collectAsState()
    val lock by app.lockState.collectAsState()
    val contacts by app.contacts.collectAsState()
    val history by app.history.collectAsState()
    var forgot by rememberSaveable { mutableStateOf(false) }
    var onboarding by rememberSaveable { mutableStateOf(!app.node.hasIdentity()) }
    var termsOk by remember { mutableStateOf(LegalStore.accepted(ctx)) }
    var missing by remember { mutableStateOf(Perms.missing(ctx)) }
    var callError by remember { mutableStateOf<String?>(null) }
    var pendingCall by remember { mutableStateOf<String?>(null) }
    // Back stack. Not saved: the recovery phrase may sit in it, and Home is a fine place to restart.
    val stack = remember { mutableStateListOf<Route>(Route.Home) }
    fun pop() { if (stack.size > 1) stack.removeAt(stack.lastIndex) }
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

    fun place(did: String) {
        callError = null
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

    when {
        onboarding || !has -> OnboardingFlow(app, missing, fix) { onboarding = false; termsOk = LegalStore.accepted(ctx); missing = Perms.missing(ctx) }
        lock == LockState.LOCKED ->
            if (forgot) OnboardingFlow(app, missing, fix, forgot = true, onCancelForgot = { forgot = false }) { forgot = false; missing = Perms.missing(ctx) }
            else UnlockScreen(app) { forgot = true }
        lock == LockState.NEEDS_PASSPHRASE -> SetPassphraseScreen(app)
        !termsOk -> TermsScreen(progress = null, onBack = null) { LegalStore.accept(ctx); termsOk = true }
        else -> {
            BackHandler(stack.size > 1) { pop() }
            when (val r = stack.last()) {
                Route.Home -> HomeScreen(
                    app, missing, fix, onAdd = { stack.add(Route.Add(it)) }, onSettings = { stack.add(Route.Settings) },
                    onContact = { stack.add(Route.Contact(it.did)) }, onCall = { call(it.did) }, callError = callError,
                    onChat = { stack.add(Route.Chat(it)) }, onNewChat = { stack.add(Route.NewChat) },
                )
                Route.NewChat -> {
                    val chats by app.chat.chats.collectAsState()
                    val last = remember(chats) { chats.filter { it.lastActivity > 0UL }.associate { it.peerDid to "Last message ${listTime(it.lastActivity.toLong(), System.currentTimeMillis())}" } }
                    NewChatScreen(contacts, last, onBack = ::pop, onPick = { pop(); stack.add(Route.Chat(it.did)) }, onAdd = { pop(); stack.add(Route.Add(true)) })
                }
                is Route.Chat -> {
                    val chats by app.chat.chats.collectAsState()
                    val status by app.status.collectAsState()
                    val c = contacts.firstOrNull { it.did == r.did }
                    val name = c?.display() ?: chats.firstOrNull { it.peerDid == r.did }?.peerName?.ifBlank { null } ?: "Unknown"
                    ConversationScreen(app.chat, r.did, name, status?.online == true, onBack = ::pop, onCall = { call(r.did) })
                }
                Route.History -> HistoryScreen(app, onBack = ::pop, onContact = { stack.add(Route.Contact(it.did)) })
                is Route.Add -> AddContactScreen(app, r.scan, onClose = ::pop,
                    onCall = { c -> pop(); call(c.did) }, onVerify = { c -> pop(); stack.add(Route.Contact(c.did)); stack.add(Route.Verify(c.did)) })
                is Route.Contact -> {
                    val c = contacts.firstOrNull { it.did == r.did }
                    if (c == null) LaunchedEffect(Unit) { pop() }
                    else ContactScreen(c, onBack = ::pop, onCall = { call(c.did) }, onMessage = { stack.add(Route.Chat(c.did)) }, onVerify = { stack.add(Route.Verify(c.did)) },
                        history = remember(history, contacts) { val now = System.currentTimeMillis() / 1000; val by = contacts.associateBy { it.did }; history.filter { it.peerDid == c.did }.map { it.toRecent(by, now) } },
                        onRename = { a -> scope.launch(Dispatchers.IO) { runCatching { app.node.renameContact(c.did, a) }; app.refresh() } }, onRemove = {
                        pop()
                        scope.launch(Dispatchers.IO) { runCatching { app.node.removeContact(c.did) }; app.refresh() }
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
                        }, onRemove = {
                            pop(); pop()
                            scope.launch(Dispatchers.IO) { runCatching { app.node.removeContact(c.did) }; app.refresh() }
                        })
                    }
                }
                Route.Settings -> SettingsScreen(
                    app, missing, onBack = ::pop, onBattery = { stack.add(Route.Battery) }, onPassphrase = { stack.add(Route.Passphrase) },
                    onPhrase = { stack.add(Route.PhraseGate) }, onAbout = { stack.add(Route.About) }, onDiagnostics = { stack.add(Route.Diagnostics) },
                )
                Route.Battery -> BatteryScreen(missing, onBack = ::pop, onFix = fix)
                Route.Passphrase -> ChangePassphraseScreen(app, onBack = ::pop)
                Route.PhraseGate -> PhraseGateScreen(app, onBack = ::pop, onPhrase = { p -> pop(); stack.add(Route.PhraseShown(p)) })
                is Route.PhraseShown -> PhraseShownScreen(r.phrase, onHide = ::pop)
                Route.About -> AboutScreen(onBack = ::pop, onLicences = { stack.add(Route.Licences) })
                Route.Licences -> LicencesScreen(::pop)
                Route.Diagnostics -> DiagnosticsScreen(app, ::pop)
                Route.MicNeeded -> MicNeededScreen(onOpenSettings = { Perms.openSettings(ctx, Perms.appDetails(ctx)) }, onNotNow = ::pop)
            }
        }
    }
}

private fun Context.findActivity(): android.app.Activity? {
    var c: Context? = this
    while (c is ContextWrapper) { if (c is android.app.Activity) return c; c = c.baseContext }
    return null
}
