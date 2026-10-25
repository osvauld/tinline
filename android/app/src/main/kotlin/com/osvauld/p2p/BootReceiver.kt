package com.osvauld.p2p

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED && intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        val app = P2pApp.get(context)
        // Wake-by-push mode starts nothing at boot: the next push does (docs/push.md). Always-on, as before.
        if (app.node.hasIdentity() && PushPolicy.wantsAlwaysOnService(PushMode.mode(context))) CoreService.ensureRunning(context)
    }
}
