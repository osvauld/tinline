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

    fun missing(c: Context): List<Need> = buildList {
        if (!granted(c, Manifest.permission.RECORD_AUDIO)) add(Need.Mic)
        if (Build.VERSION.SDK_INT >= 33 && !granted(c, Manifest.permission.POST_NOTIFICATIONS)) add(Need.Notifications)
        val pm = c.getSystemService(Context.POWER_SERVICE) as PowerManager
        if (!pm.isIgnoringBatteryOptimizations(c.packageName)) add(Need.Battery)
        if (Build.VERSION.SDK_INT >= 34) {
            val nm = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            if (!nm.canUseFullScreenIntent()) add(Need.FullScreen)
        }
    }

    fun settingsIntent(c: Context, need: Need): Intent? = when (need) {
        Need.Battery -> Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${c.packageName}"))
        Need.FullScreen -> if (Build.VERSION.SDK_INT >= 34)
            Intent(Settings.ACTION_MANAGE_APP_USE_FULL_SCREEN_INTENT, Uri.parse("package:${c.packageName}")) else null
        else -> null
    }

    fun runtimePermission(need: Need): String? = when (need) {
        Need.Mic -> Manifest.permission.RECORD_AUDIO
        Need.Notifications -> if (Build.VERSION.SDK_INT >= 33) Manifest.permission.POST_NOTIFICATIONS else null
        else -> null
    }
}
