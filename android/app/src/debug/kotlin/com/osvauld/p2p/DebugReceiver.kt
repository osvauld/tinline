package com.osvauld.p2p

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/** Test hook: `adb shell am broadcast -a com.osvauld.p2p.DEBUG -n com.osvauld.p2p/.DebugReceiver --es cmd ...` */
class DebugReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val pending = goAsync()
        val app = P2pApp.get(context)
        val cmd = intent.getStringExtra("cmd") ?: ""
        Thread {
            try { run(app, cmd, intent) } catch (e: Throwable) { testLog("error=$cmd: ${e.message}") } finally { pending.finish() }
        }.start()
    }

    private fun run(app: P2pApp, cmd: String, i: Intent) {
        val node = app.node
        when (cmd) {
            "create" -> {
                if (!node.hasIdentity()) node.createIdentity(i.getStringExtra("name") ?: "phone")
                app.identityReady()
                val p = node.profile()
                testLog("created did=${p?.did} name=${p?.name}")
            }
            "ticket" -> testLog("ticket=${node.myTicket()}")
            "add" -> {
                val c = node.addContact(i.getStringExtra("ticket") ?: "")
                app.refresh()
                testLog("added name=${c.name} did=${c.did}")
            }
            "contacts" -> node.contacts().forEach { testLog("contact name=${it.name} did=${it.did}") }
            "call" -> {
                val who = i.getStringExtra("who") ?: ""
                val c = node.contacts().firstOrNull { it.did == who || it.name == who }
                    ?: throw IllegalArgumentException("no such contact $who")
                val r = app.calls.place(c.did)
                r.onFailure { throw it }
            }
            "answer" -> app.calls.answer()
            "decline" -> app.calls.decline()
            "hangup" -> app.calls.hangup()
            "tone" -> {
                val hz = i.getStringExtra("hz")?.toFloatOrNull() ?: 0f
                app.testToneHz = if (hz > 0f) hz else null
                node.setTestTone(app.testToneHz)
                testLog("tone hz=$hz")
            }
            "status" -> {
                val s = node.status()
                testLog("status online=${s.online} relay=${s.relay} id=${s.endpointId}")
            }
            "stats" -> app.calls.logStats()
            else -> Log.w("P2PTEST", "unknown cmd $cmd")
        }
    }
}
