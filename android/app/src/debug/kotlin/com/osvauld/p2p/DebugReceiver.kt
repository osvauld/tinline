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
                if (!node.hasIdentity()) node.createIdentity(i.getStringExtra("name") ?: "phone", if (i.getBooleanExtra("nopass", false)) "" else i.getStringExtra("pass") ?: "test-passphrase")
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
            "clip" -> {
                val cm = app.getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
                cm.setPrimaryClip(android.content.ClipData.newPlainText("test", i.getStringExtra("text") ?: node.myTicket()))
                testLog("clip set")
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
            "lockstate" -> testLog("lockstate=${node.lockState()}")
            "stats" -> app.calls.logStats()
            "avail" -> {
                val on = i.getStringExtra("on") != "0"
                val mins = i.getStringExtra("mins")?.toLongOrNull()
                app.setAvailable(on, mins?.let { System.currentTimeMillis() / 1000 + it * 60 })
                testLog("availability=${app.availability.value.available} until=${app.availability.value.until}")
            }
            "history" -> node.recentCalls(50u).forEach {
                testLog("history peer=${it.peerName} incoming=${it.incoming} reason=${it.reason} missed=${it.missed} secs=${it.durationSecs}")
            }
            "safety" -> {
                val c = node.contacts().firstOrNull { it.did == i.getStringExtra("who") || it.name == i.getStringExtra("who") }
                testLog("safety=${c?.let { node.safetyNumber(it.did) }}")
            }
            "chat_fake" -> {
                ChatBackend.override = if (i.getStringExtra("on") == "0") null else ChatBackend.override ?: FakeChatSource(app)
                testLog("chat_fake=${ChatBackend.override != null}")
            }
            "chat_incoming" -> {
                val f = ChatBackend.override as? FakeChatSource ?: throw IllegalStateException("chat_fake is off")
                f.incoming(i.getStringExtra("who") ?: FakeChatSource.did("Arjun"), i.getStringExtra("text") ?: "Hello")
            }
            // Device linking without a camera: feed the other device's QR text into the scan path.
            // `new=1` = this phone is the new device (link_new_scan, label `label`).
            "link_scan" -> app.links.scan(i.getStringExtra("qr") ?: "", i.getStringExtra("new") == "1", i.getStringExtra("label") ?: "Test phone")
            "link_show" -> app.links.showQr(i.getStringExtra("new") == "1", i.getStringExtra("label") ?: "Test phone")
            "link_approve" -> { app.links.approve(i.getStringExtra("pass")); testLog("link_approve ok") }
            "link_cancel" -> app.links.cancel()
            "devices" -> node.linkedDevices().forEach { testLog("device label=${it.label} this=${it.thisDevice} removed=${it.removed} seen=${it.lastSeen}") }
            else -> Log.w("P2PTEST", "unknown cmd $cmd")
        }
    }
}
