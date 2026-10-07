package com.osvauld.p2p

import android.Manifest
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import androidx.core.content.ContextCompat

/** What the app needs to be reachable, and the intents that ask for each. */
enum class Need(val title: String, val why: String) {
    Mic("Microphone", "So the other person can hear you"),
    Notifications("Notifications", "To show incoming calls"),
    Battery("Run in background", "Exempt from battery optimisation so calls arrive when idle"),
    FullScreen("Full-screen calls", "Show incoming calls over the lock screen"),
}

object Perms {
    fun granted(c: Context, p: String) = ContextCompat.checkSelfPermission(c, p) == PackageManager.PERMISSION_GRANTED

    /** App-level notifications on, and the incoming-call channel not switched off (a call would be invisible). */
    fun notificationsOn(c: Context): Boolean {
        val nm = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (!nm.areNotificationsEnabled()) return false
        val ch = nm.getNotificationChannel(Notifications.CH_CALLS)
        return ch == null || ch.importance != NotificationManager.IMPORTANCE_NONE
    }

    fun missing(c: Context): List<Need> = buildList {
        if (!granted(c, Manifest.permission.RECORD_AUDIO)) add(Need.Mic)
        if (!notificationsOn(c)) add(Need.Notifications)
        val pm = c.getSystemService(Context.POWER_SERVICE) as PowerManager
        if (!pm.isIgnoringBatteryOptimizations(c.packageName)) add(Need.Battery)
        if (Build.VERSION.SDK_INT >= 34) {
            val nm = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            if (!nm.canUseFullScreenIntent()) add(Need.FullScreen)
        }
    }

    fun settingsIntent(c: Context, need: Need): Intent? = when (need) {
        Need.Battery -> Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${c.packageName}"))
        Need.Notifications -> Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, c.packageName)
        Need.Mic -> appDetails(c)
        Need.FullScreen -> if (Build.VERSION.SDK_INT >= 34)
            Intent(Settings.ACTION_MANAGE_APP_USE_FULL_SCREEN_INTENT, Uri.parse("package:${c.packageName}")) else null
        else -> null
    }

    fun appDetails(c: Context): Intent =
        Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${c.packageName}"))

    /** Starts a settings screen, falling back to this app's details page if no handler exists. */
    fun openSettings(c: Context, i: Intent) {
        try { c.startActivity(i) } catch (_: Exception) {
            try { c.startActivity(appDetails(c)) } catch (_: Exception) {}
        }
    }

    fun runtimePermission(need: Need): String? = when (need) {
        Need.Mic -> Manifest.permission.RECORD_AUDIO
        Need.Notifications -> if (Build.VERSION.SDK_INT >= 33) Manifest.permission.POST_NOTIFICATIONS else null
        else -> null
    }
}
