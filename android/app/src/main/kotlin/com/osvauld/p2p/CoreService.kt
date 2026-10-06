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
import kotlinx.coroutines.launch

/** Always-on foreground service: keeps the process alive, the node started, and holds call locks. */
class CoreService : Service() {
    private lateinit var cm: ConnectivityManager
    private var wake: PowerManager.WakeLock? = null
    private var wifi: WifiManager.WifiLock? = null
    private var inCall: String? = null

    private val netCb = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) { changed() }
        override fun onLost(network: Network) { changed() }
        private fun changed() {
            val app = P2pApp.instance
            app.scope.launch { try { app.node.networkChanged() } catch (_: Exception) {} }
        }
    }

    override fun onCreate() {
        super.onCreate()
        cm = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        running = this
        try { cm.registerDefaultNetworkCallback(netCb) } catch (e: Exception) { Log.w(TAG, "netcb: $e") }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val app = P2pApp.get(this)
        when (intent?.action) {
            ACTION_DECLINE -> app.calls.decline()
            ACTION_HANGUP -> app.calls.hangup()
            ACTION_IN_CALL -> inCall = intent.getStringExtra("name") ?: "Call"
            ACTION_IDLE -> inCall = null
        }
        enterForeground()
        updateLocks()
        app.scope.launch { app.startNode() }
        return START_STICKY
    }

    private fun enterForeground() {
        val n = Notifications.service(this, inCall)
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                var type = ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
                if (inCall != null) type = type or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
                try {
                    startForeground(Notifications.ID_SERVICE, n, type)
                } catch (e: Exception) {
                    // Microphone type may be refused when started from the background (API 34+).
                    Log.w(TAG, "startForeground($type) refused: $e")
                    startForeground(Notifications.ID_SERVICE, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
                }
            } else startForeground(Notifications.ID_SERVICE, n)
        } catch (e: Exception) { Log.e(TAG, "startForeground failed: $e") }
    }

    private fun updateLocks() {
        if (inCall != null) {
            if (wake == null) {
                wake = (getSystemService(POWER_SERVICE) as PowerManager)
                    .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "p2p:call").apply { acquire(4 * 3600_000L) }
            }
            if (wifi == null) {
                val mode = if (Build.VERSION.SDK_INT >= 29) WifiManager.WIFI_MODE_FULL_LOW_LATENCY else WifiManager.WIFI_MODE_FULL_HIGH_PERF
                wifi = (applicationContext.getSystemService(WIFI_SERVICE) as WifiManager)
                    .createWifiLock(mode, "p2p:call").apply { acquire() }
            }
        } else {
            wake?.let { if (it.isHeld) it.release() }; wake = null
            wifi?.let { if (it.isHeld) it.release() }; wifi = null
        }
    }

    override fun onDestroy() {
        try { cm.unregisterNetworkCallback(netCb) } catch (_: Exception) {}
        inCall = null; updateLocks()
        if (running === this) running = null
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        const val TAG = "CoreService"
        const val ACTION_DECLINE = "com.osvauld.p2p.DECLINE"
        const val ACTION_HANGUP = "com.osvauld.p2p.HANGUP"
        const val ACTION_IN_CALL = "com.osvauld.p2p.IN_CALL"
        const val ACTION_IDLE = "com.osvauld.p2p.IDLE"
        @Volatile var running: CoreService? = null

        fun ensureRunning(ctx: Context, action: String? = null, name: String? = null) {
            val i = Intent(ctx, CoreService::class.java).setAction(action)
            if (name != null) i.putExtra("name", name)
            try { ctx.startForegroundService(i) } catch (e: Exception) {
                Log.w(TAG, "startForegroundService refused: $e")
                // Already running (foreground) services can still be poked directly.
                running?.let { s -> try { s.onStartCommand(i, 0, 0) } catch (_: Exception) {} }
            }
        }
    }
}
