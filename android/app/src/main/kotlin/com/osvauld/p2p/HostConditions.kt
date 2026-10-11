package com.osvauld.p2p

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.BatteryManager
import android.os.Build
import android.os.PowerManager
import android.util.Log
import androidx.core.content.ContextCompat
import kotlinx.coroutines.launch
import uniffi.p2pcore.GroupCallConditions
import uniffi.p2pcore.GroupCallDeviceClass
import uniffi.p2pcore.GroupCallThermal

/**
 * The phone's call-hosting conditions (GA-A1, group-call-spec §2.4): battery level and charging
 * (ACTION_BATTERY_CHANGED), thermal status (PowerManager listener, API 29+; none before), metered
 * data (the default network's capabilities). Pushed to the core on start and on every change;
 * the core advertises no call hosting before the first push.
 */
class HostConditions(private val app: P2pApp) {
    @Volatile private var batteryPct = 0
    @Volatile private var plugged = false
    @Volatile private var thermal = GroupCallThermal.NONE
    @Volatile private var metered = false
    @Volatile private var last: GroupCallConditions? = null

    private val battery = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) { onBattery(i); push() }
    }

    private val net = object : ConnectivityManager.NetworkCallback() {
        override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
            metered = HostCond.metered(
                caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED),
                Build.VERSION.SDK_INT >= 30 && caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_TEMPORARILY_NOT_METERED),
            )
            push()
        }
        override fun onLost(network: Network) {
            metered = cm()?.isActiveNetworkMetered ?: false
            push()
        }
    }

    fun start() {
        try {
            val sticky = ContextCompat.registerReceiver(app, battery, IntentFilter(Intent.ACTION_BATTERY_CHANGED), ContextCompat.RECEIVER_NOT_EXPORTED)
            sticky?.let(::onBattery)
        } catch (e: Exception) { Log.w(TAG, "battery: $e") }
        try {
            val cm = cm()
            metered = cm?.isActiveNetworkMetered ?: false
            cm?.registerDefaultNetworkCallback(net)
        } catch (e: Exception) { Log.w(TAG, "network: $e") }
        if (Build.VERSION.SDK_INT >= 29) {
            try {
                val pm = app.getSystemService(Context.POWER_SERVICE) as PowerManager
                thermal = HostCond.thermal(pm.currentThermalStatus)
                pm.addThermalStatusListener(ContextCompat.getMainExecutor(app)) { s -> thermal = HostCond.thermal(s); push() }
            } catch (e: Exception) { Log.w(TAG, "thermal: $e") }
        }
        push()
    }

    private fun cm() = app.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager

    private fun onBattery(i: Intent) {
        batteryPct = HostCond.batteryPct(i.getIntExtra(BatteryManager.EXTRA_LEVEL, -1), i.getIntExtra(BatteryManager.EXTRA_SCALE, -1))
        plugged = HostCond.plugged(i.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0), i.getIntExtra(BatteryManager.EXTRA_STATUS, -1))
    }

    /** Pushes when something changed (battery broadcasts also come for voltage and temperature). */
    @Synchronized
    private fun push() {
        val c = HostCond.conditions(batteryPct, plugged, metered, thermal)
        if (c == last) return
        last = c
        testLog("host-conditions battery=${c.batteryPct} plugged=${c.plugged} metered=${c.metered} thermal=${c.thermal}")
        app.scope.launch { try { app.node.groupCallSetConditions(c) } catch (e: Exception) { Log.w(TAG, "set conditions: $e") } }
    }

    companion object { const val TAG = "HostConditions" }
}

/** Platform readings -> the core's [GroupCallConditions]; pure, so it runs in JVM unit tests. */
object HostCond {
    // BatteryManager / PowerManager constants, spelled out so no android.jar stub runs in tests.
    private const val STATUS_CHARGING = 2 // BATTERY_STATUS_CHARGING
    private const val STATUS_FULL = 5 // BATTERY_STATUS_FULL

    /** EXTRA_LEVEL / EXTRA_SCALE -> 0..100. Unknown counts as empty: an unknown phone does not host. */
    fun batteryPct(level: Int, scale: Int): Int =
        if (level < 0 || scale <= 0) 0 else (level * 100 / scale).coerceIn(0, 100)

    /** EXTRA_PLUGGED (any source) or a charging/full status. */
    fun plugged(pluggedExtra: Int, status: Int): Boolean =
        pluggedExtra != 0 || status == STATUS_CHARGING || status == STATUS_FULL

    /** PowerManager.THERMAL_STATUS_*: 0 none, 1 light, 2 moderate, 3 severe, 4 critical, 5 emergency, 6 shutdown. */
    fun thermal(status: Int): GroupCallThermal = when {
        status <= 0 -> GroupCallThermal.NONE
        status == 1 -> GroupCallThermal.LIGHT
        status == 2 -> GroupCallThermal.MODERATE
        status == 3 -> GroupCallThermal.SEVERE
        else -> GroupCallThermal.CRITICAL
    }

    /** The default network is metered unless it says it is not (also temporarily, API 30+). */
    fun metered(notMetered: Boolean, temporarilyNotMetered: Boolean): Boolean = !(notMetered || temporarilyNotMetered)

    fun conditions(batteryPct: Int, plugged: Boolean, metered: Boolean, thermal: GroupCallThermal) = GroupCallConditions(
        deviceClass = GroupCallDeviceClass.PHONE,
        plugged = plugged,
        batteryPct = batteryPct.coerceIn(0, 100).toUByte(),
        metered = metered,
        meteredAllowed = false,
        thermal = thermal,
    )
}
