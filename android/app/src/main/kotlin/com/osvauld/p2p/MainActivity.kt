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
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import uniffi.p2pcore.LockState

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

private enum class Screen { Home, Add, Settings }

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        secureWindow()
        enableEdgeToEdge()
        val app = P2pApp.get(this)
        if (app.node.hasIdentity()) CoreService.ensureRunning(this)
        setContent {
            P2pTheme {
                Surface(Modifier.fillMaxSize()) { Root(app) }
            }
        }
    }
}

@Composable
private fun Root(app: P2pApp) {
    val ctx = LocalContext.current
    val has by app.hasIdentity.collectAsState()
    val lock by app.lockState.collectAsState()
    var forgot by rememberSaveable { mutableStateOf(false) }
    var screen by rememberSaveable { mutableStateOf(Screen.Home) }
    var onboarding by rememberSaveable { mutableStateOf(!app.node.hasIdentity()) }
    var missing by remember { mutableStateOf(Perms.missing(ctx)) }
    val prefs = remember { ctx.getSharedPreferences("perm_state", Context.MODE_PRIVATE) }
    val owner = LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val o = LifecycleEventObserver { _, e -> if (e == Lifecycle.Event.ON_RESUME) { missing = Perms.missing(ctx); app.refresh() } }
        owner.lifecycle.addObserver(o); onDispose { owner.lifecycle.removeObserver(o) }
    }
    val permLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        missing = Perms.missing(ctx)
    }
    val settingsLauncher = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        missing = Perms.missing(ctx)
    }
    // First run after onboarding: ask for the runtime permissions in one go.
    LaunchedEffect(onboarding, has, lock) {
        if (!onboarding && has && lock == LockState.UNLOCKED) {
            val rt = missing.mapNotNull { Perms.runtimePermission(it) }
            if (rt.isNotEmpty()) {
                rt.forEach { prefs.edit().putBoolean("asked_$it", true).apply() }
                permLauncher.launch(rt.toTypedArray())
            }
        }
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

    if (onboarding || !has) {
        OnboardingScreen(app) { onboarding = false; missing = Perms.missing(ctx) }
    } else if (lock == LockState.LOCKED) {
        if (forgot) OnboardingScreen(app, forgot = true) { forgot = false; missing = Perms.missing(ctx) }
        else UnlockScreen(app) { forgot = true }
    } else if (lock == LockState.NEEDS_PASSPHRASE) {
        SetPassphraseScreen(app)
    } else {
        BackHandler(screen != Screen.Home) { screen = Screen.Home }
        when (screen) {
            Screen.Home -> HomeScreen(app, missing, fix, { screen = Screen.Add }, { screen = Screen.Settings })
            Screen.Add -> AddContactScreen(app) { screen = Screen.Home }
            Screen.Settings -> SettingsScreen(app) { screen = Screen.Home }
        }
    }
}

private fun Context.findActivity(): android.app.Activity? {
    var c: Context? = this
    while (c is ContextWrapper) { if (c is android.app.Activity) return c; c = c.baseContext }
    return null
}
