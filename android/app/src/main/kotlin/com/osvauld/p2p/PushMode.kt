package com.osvauld.p2p

import android.content.Context
import uniffi.p2pcore.BackgroundMode
import uniffi.p2pcore.PushTarget

/**
 * The seam between the app and a wake-up transport (docs/push.md). The `play` edition implements it
 * with FCM (src/play), `liberum` with nothing (src/liberum); UnifiedPush comes later.
 */
interface PushProvider {
    /**
     * First thing in `Application.onCreate`, on the main thread, so it must stay cheap: whatever the
     * transport's own entry points need before they run (a push can start the process). Slow checks
     * go to a background thread here, so [available] is usually answered from cache.
     */
    fun start(ctx: Context) {}
    /** This build and phone can receive wake pushes (library present, configured, services available). */
    fun available(ctx: Context): Boolean
    /** Turns the transport's own token handling on or off with the chosen mode. */
    fun setEnabled(ctx: Context, on: Boolean)
    /** The current wake target, or null when there is none. May answer on another thread. */
    fun fetchTarget(ctx: Context, done: (PushTarget?) -> Unit)
}

/** How this phone stays reachable. Pure, so the decision is unit tested. */
object PushPolicy {
    enum class Mode { PUSH, ALWAYS_ON }

    /**
     * Push is the default whenever the edition can do it; the user's "stay connected all the time"
     * choice (`alwaysOn`, null while never chosen) switches to the always-on service. With no push
     * transport the phone is always-on, whatever was stored.
     */
    fun mode(providerAvailable: Boolean, alwaysOn: Boolean?): Mode =
        if (!providerAvailable || alwaysOn == true) Mode.ALWAYS_ON else Mode.PUSH

    /** The plain always-on service start (boot, app open, account switch) is wanted only in always-on mode. */
    fun wantsAlwaysOnService(mode: Mode) = mode == Mode.ALWAYS_ON

    /**
     * Which other accounts the core may run (docs/multi-online.md): with wake pushes a sleeping account opens on a wake
     * for it; always-on (and Liberum, which has no push) keeps only the open account, as before.
     */
    fun backgroundMode(mode: Mode): BackgroundMode = if (mode == Mode.PUSH) BackgroundMode.ON_WAKE else BackgroundMode.OPEN_ONLY
}

/** The app's reachability mode: the edition's provider plus the stored Settings choice. */
object PushMode {
    private const val FILE = "push"
    private const val KEY_ALWAYS_ON = "always_on"
    private const val KEY_REFRESH_PENDING = "refresh_pending"

    private val provider: PushProvider by lazy { platformPushProvider() }
    internal fun provider() = provider

    fun start(c: Context) = provider.start(c)

    private fun prefs(c: Context) = c.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    /** Settings shows the choice only when the edition can wake by push. */
    fun canChoose(c: Context) = provider.available(c)

    fun alwaysOnChosen(c: Context): Boolean? =
        prefs(c).let { if (it.contains(KEY_ALWAYS_ON)) it.getBoolean(KEY_ALWAYS_ON, false) else null }

    fun mode(c: Context): PushPolicy.Mode = PushPolicy.mode(provider.available(c), alwaysOnChosen(c))

    fun pushOn(c: Context) = mode(c) == PushPolicy.Mode.PUSH

    /** May block on the first Play services check: not for the main thread. */
    fun backgroundMode(c: Context): BackgroundMode = PushPolicy.backgroundMode(mode(c))

    fun setAlwaysOn(c: Context, on: Boolean) { prefs(c).edit().putBoolean(KEY_ALWAYS_ON, on).apply() }

    /** `onNewToken` arrived while the account was locked: the token is re-read at the next start anyway. */
    fun setRefreshPending(c: Context, on: Boolean) { prefs(c).edit().putBoolean(KEY_REFRESH_PENDING, on).apply() }
    fun refreshPending(c: Context) = prefs(c).getBoolean(KEY_REFRESH_PENDING, false)
}
