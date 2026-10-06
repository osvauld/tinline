package com.osvauld.p2p

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val app = P2pApp.get(context)
        if (app.node.hasIdentity()) CoreService.ensureRunning(context)
    }
}
