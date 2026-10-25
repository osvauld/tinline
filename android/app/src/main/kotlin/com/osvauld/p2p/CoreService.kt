package com.osvauld.p2p

import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.Network
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch

/** Always-on foreground service: keeps the process alive, the node started, and holds call locks. */
class CoreService : Service() {
    private lateinit var cm: ConnectivityManager
    private var wake: PowerManager.WakeLock? = null
    private var wifi: WifiManager.WifiLock? = null
    /** The ongoing call's title, else null (idle). Only ever set by [reevaluate]; main thread. */
    private var inCall: String? = null
    private var watch: Job? = null

    private val netCb = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) { changed() }
        override fun onLost(network: Network) { changed() }
        private fun changed() {
            val app = P2pApp.instance
            app.scope.launch { try { app.node.setConnectionMetered(cm.isActiveNetworkMetered); app.node.networkChanged() } catch (_: Exception) {} }
        }
    }

    override fun onCreate() {
        super.onCreate()
        cm = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        running = this
        try { cm.registerDefaultNetworkCallback(netCb) } catch (e: Exception) { Log.w(TAG, "netcb: $e") }
        try { P2pApp.get(this).node.setConnectionMetered(cm.isActiveNetworkMetered) } catch (_: Exception) {}
        // Self-heal: any change to the 1:1 or group call re-derives the notification, whatever pokes
        // were (or were not) delivered. Keyed on what the derivation reads, so stats ticks don't poke.
        val app = P2pApp.get(this)
        watch = app.scope.launch {
            launch {
                app.calls.ui.map { it?.info?.callId to CallNotif.of(it) }.distinctUntilChanged()
                    .collect { (id, st) -> CallTrace.add("watch 1:1 ${shortId(id)} $st"); poke("1:1 changed") }
            }
            app.groupCalls.here.map { Triple(it?.callId, it?.inCall, it?.hosting) }.distinctUntilChanged()
                .collect { (id, inc, host) -> CallTrace.add("watch group ${shortId(id)} in=$inc host=$host"); poke("group changed") }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val app = P2pApp.get(this)
        when (intent?.action) {
            ACTION_DECLINE -> app.calls.decline()
            // The ongoing notification's hang-up: the 1:1 call, else the group call (GA-A1). With
            // neither, the notification was stale: the re-derivation below puts it back to idle.
            ACTION_HANGUP -> when {
                app.calls.ui.value != null -> app.calls.hangup()
                app.groupCalls.active -> app.groupCalls.leave()
                else -> CallTrace.add("hangup tapped with no call: re-derive")
            }
            ACTION_GCALL_NOT_NOW -> app.groupCallUi.notNow()
            ACTION_GCALL_MUTE -> app.groupCallUi.toggleMute()
            ACTION_DECLINE_WAITING -> app.calls.declineWaiting()
            ACTION_END_ANSWER -> app.calls.endAndAnswer()
            // The wake ring's Decline (WakeRing); ACTION_WAKE only keeps the process up while a wake is handled.
            ACTION_WAKE_DECLINE -> WakeRing.decline(app)
            // ACTION_IN_CALL / ACTION_IDLE are plain pokes: the state is re-derived below, so
            // their order does not matter, and a 1:1 call ending keeps a group call's call mode.
        }
        reevaluate(intent?.action?.substringAfterLast('.') ?: "start")
        enterForeground()
        updateLocks()
        app.startNode()
        scheduleIdleStop()
        return START_STICKY
    }

    /**
     * Wake-by-push mode (docs/push.md): the service is only here for a wake, a ring or a call. Once nothing is going on
     * for [IDLE_HOLD_MS] it stops itself and the phone sleeps again; the next push starts it. Always-on mode never stops.
     * Main thread only.
     */
    private val idleStop = Runnable {
        if (!PushMode.pushOn(this)) return@Runnable
        if (inCall != null) return@Runnable      // [apply] schedules again when the call ends
        if (WakeRing.active) { scheduleIdleStop(); return@Runnable }
        CallTrace.add("service idle in push mode: stopping")
        try { stopForeground(STOP_FOREGROUND_REMOVE) } catch (_: Exception) {}
        stopSelf()
    }

    private fun scheduleIdleStop() {
        main.removeCallbacks(idleStop)
        if (inCall == null && PushMode.pushOn(this)) main.postDelayed(idleStop, IDLE_HOLD_MS)
    }

    /** Main thread only. Derives [inCall] from the call state ([CallNotif]); true when it changed. */
    private fun reevaluate(why: String): Boolean {
        val d = try { CallNotif.current(P2pApp.get(this)) } catch (e: Exception) {
            Log.w(TAG, "derive: $e"); CallNotif.Derived(null, "idle: derive failed")
        }
        val changed = d.title != inCall
        inCall = d.title
        CallTrace.add("service ${if (d.title != null) "in-call" else "idle"} (${d.why}) on $why${if (changed) ", changed" else ""}")
        return changed
    }

    /** Main thread only: re-derive and re-post; a change also redoes the service type and the locks. */
    private fun apply(why: String) {
        if (reevaluate(why)) { enterForeground(); updateLocks() } else repost()
        scheduleIdleStop()
    }

    private fun notifId() = if (inCall != null) Notifications.ID_CALL else Notifications.ID_SERVICE

    private fun repost() {
        try {
            (getSystemService(Context.NOTIFICATION_SERVICE) as android.app.NotificationManager)
                .notify(notifId(), Notifications.service(this, inCall))
        } catch (_: Exception) {}
    }

    /**
     * Android drops updates past a few per second per app; a call's end posts several at once,
     * so the last (idle) one could be the dropped one. Re-post the current state once it is quiet.
     */
    private val settle = Runnable { CallTrace.add("settle ${if (inCall != null) "in-call" else "idle"}"); repost() }

    private fun enterForeground() {
        main.removeCallbacks(settle)
        main.postDelayed(settle, SETTLE_MS)
        val id = notifId()
        val n = Notifications.service(this, inCall)
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                var type = ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
                // The microphone type needs RECORD_AUDIO (else SecurityException) and is only added
                // for a call the user started or answered (CallController asks for it then). The
                // manifest declares specialUse|microphone, so the type is allowed to apply.
                if (inCall != null && Perms.granted(this, android.Manifest.permission.RECORD_AUDIO))
                    type = type or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
                try {
                    startForeground(id, n, type)
                } catch (e: Exception) {
                    // Microphone type may be refused when started from the background (API 34+).
                    Log.w(TAG, "startForeground($type) refused: $e")
                    CallTrace.add("startForeground mic type refused (${e.javaClass.simpleName})")
                    startForeground(id, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
                }
            } else startForeground(id, n)
            // Android removes the previous id's notification when the id changes; make sure.
            val other = if (id == Notifications.ID_CALL) Notifications.ID_SERVICE else Notifications.ID_CALL
            try { (getSystemService(Context.NOTIFICATION_SERVICE) as android.app.NotificationManager).cancel(other) } catch (_: Exception) {}
        } catch (e: Exception) { Log.e(TAG, "startForeground failed: $e"); CallTrace.add("startForeground failed (${e.javaClass.simpleName})") }
    }

    /** Main thread only (onStartCommand / apply / onDestroy); idempotent: a second call finds the locks set. */
    private fun updateLocks() {
        if (inCall != null) {
            if (wake == null) {
                wake = (getSystemService(POWER_SERVICE) as PowerManager)
                    .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "p2p:call").apply { acquire(4 * 3600_000L) }
            }
            if (wifi == null) {
                @Suppress("DEPRECATION") val mode = if (Build.VERSION.SDK_INT >= 29) WifiManager.WIFI_MODE_FULL_LOW_LATENCY else WifiManager.WIFI_MODE_FULL_HIGH_PERF
                wifi = (applicationContext.getSystemService(WIFI_SERVICE) as WifiManager)
                    .createWifiLock(mode, "p2p:call").apply { acquire() }
            }
        } else {
            try { wake?.let { if (it.isHeld) it.release() } } catch (_: Exception) {}; wake = null
            try { wifi?.let { if (it.isHeld) it.release() } } catch (_: Exception) {}; wifi = null
        }
    }

    override fun onDestroy() {
        main.removeCallbacks(settle)
        main.removeCallbacks(idleStop)
        try { cm.unregisterNetworkCallback(netCb) } catch (_: Exception) {}
        watch?.cancel(); watch = null
        inCall = null; updateLocks()
        if (running === this) running = null
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        const val TAG = "CoreService"
        private const val SETTLE_MS = 1500L
        /** Wake-by-push mode: how long the service stays after the last wake, ring or call (the core's ring cap is 40 s). */
        private const val IDLE_HOLD_MS = 90_000L
        const val ACTION_DECLINE = "com.osvauld.p2p.DECLINE"
        const val ACTION_HANGUP = "com.osvauld.p2p.HANGUP"
        const val ACTION_DECLINE_WAITING = "com.osvauld.p2p.DECLINE_WAITING"
        const val ACTION_END_ANSWER = "com.osvauld.p2p.END_ANSWER"
        const val ACTION_GCALL_NOT_NOW = "com.osvauld.p2p.GCALL_NOT_NOW"
        const val ACTION_GCALL_MUTE = "com.osvauld.p2p.GCALL_MUTE"
        const val ACTION_IN_CALL = "com.osvauld.p2p.IN_CALL"
        const val ACTION_IDLE = "com.osvauld.p2p.IDLE"
        const val ACTION_WAKE = "com.osvauld.p2p.WAKE"
        const val ACTION_WAKE_DECLINE = "com.osvauld.p2p.WAKE_DECLINE"
        @Volatile var running: CoreService? = null
        private val main = android.os.Handler(android.os.Looper.getMainLooper())

        /** Re-derives and re-posts the always-on notification (availability, connectivity or call text changed). */
        fun refreshNotification() = poke("refresh")

        /** Re-derives the running service's call state on the main thread (no start command). */
        fun poke(why: String) {
            main.post { running?.apply(why) }
        }

        /**
         * Boot, app open, account switch: the always-on service only in always-on mode. In wake-by-push mode the node is
         * started without it (a push or a call raises the service when needed).
         */
        fun ensureAlwaysOn(ctx: Context) {
            if (PushPolicy.wantsAlwaysOnService(PushMode.mode(ctx))) ensureRunning(ctx) else P2pApp.get(ctx).startNode()
        }

        /**
         * Starts the service, or pokes it. [ACTION_IN_CALL] (sent while the app may still take the
         * microphone type) and [ACTION_IDLE] only ask for a re-derivation; they carry no state.
         */
        fun ensureRunning(ctx: Context, action: String? = null) {
            val i = Intent(ctx, CoreService::class.java).setAction(action)
            val tag = action?.substringAfterLast('.') ?: "start"
            try {
                ctx.startForegroundService(i)
                CallTrace.add("ensureRunning $tag")
            } catch (e: Exception) {
                Log.w(TAG, "startForegroundService refused: $e")
                CallTrace.add("ensureRunning $tag refused (${e.javaClass.simpleName}), direct${if (running == null) ", no service running" else ""}")
                // An already-running foreground service can still be poked directly, but only on
                // the main thread: onStartCommand touches notification and lock state.
                main.post { running?.let { s -> try { s.onStartCommand(i, 0, 0) } catch (_: Exception) {} } }
            }
        }
    }
}
