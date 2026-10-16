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
            "contacts" -> node.contacts().forEach { testLog("contact name=${it.name} did=${it.did} alias=${it.alias}") }
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
            "video_bench" -> {
                // Codec timing inside the app process. `path` is relative to filesDir (push with
                // run-as); `prio` is an android.os.Process thread priority (e.g. -8 = display),
                // inherited by the encoder's worker threads; `pin` = fast cores only.
                val path = i.getStringExtra("path")?.let { java.io.File(app.filesDir, it).absolutePath }
                android.os.Process.setThreadPriority(i.getIntExtra("prio", 0))
                val out = uniffi.p2pcore.videoBench(path, i.getIntExtra("frames", 89).toUInt(), i.getIntExtra("threads", 4).toUInt(), i.getBooleanExtra("pin", false))
                out.lines().filter { it.isNotBlank() }.forEach { testLog("video_bench $it") }
                testLog("video_bench done")
            }
            // MediaCodec VP8 self-test (Rust, no camera, no call): synthetic frames through the
            // hardware encoder and decoder for `seconds` (30) at `w`x`h` (1280x720); one line per
            // second plus SUMMARY lines. `wait_ms` (int): how long the encoder
            // waits for its output (default 0). Own thread: it outlives the broadcast's time limit.
            "mediacodec_vp8" -> Thread {
                try {
                    val out = uniffi.p2pcore.mediacodecVp8Selftest(i.getIntExtra("seconds", 30).toUInt(), i.getIntExtra("w", 1280).toUInt(), i.getIntExtra("h", 720).toUInt(), i.getIntExtra("wait_ms", -1))
                    out.lines().filter { it.isNotBlank() }.forEach { testLog("mediacodec_vp8 $it") }
                } catch (e: Throwable) { testLog("mediacodec_vp8 error ${e.message}") }
                testLog("mediacodec_vp8 done")
            }.apply { name = "mediacodec-vp8-selftest" }.start()
            "stats" -> app.calls.logStats()
            // Call video: state and counters (also logged every second during a video call).
            "video_stats" -> testLog(app.video.statsLine(app.calls.renderer))
            // Explicit metadata inventory only: no codec creation, timing or benchmark.
            "video_hardware" -> VideoHardware.inventory().forEach { testLog("video_hardware $it") }
            // Camera on/off as the button does (permission must already be granted); `on=0` = off.
            "camera" -> {
                val on = i.getStringExtra("on") != "0"
                app.video.setCamera(on)
                if (on) app.calls.speakerForVideo()
                testLog("camera on=$on problem=${app.video.problem.value}")
            }
            // Frames through VideoJni (`jni=1`, default) or UniFFI (`jni=0`), to compare.
            "video_path" -> { app.video.io.jni = i.getStringExtra("jni") != "0"; testLog("video_path jni=${app.video.io.jni}") }
            // Push cost per path without a call (the planes are copied, then dropped): 640x480
            // NV21-shaped direct buffers like CameraX's, `n` pushes each way; median ms.
            "video_io_bench" -> Thread {
                val n = i.getStringExtra("n")?.toIntOrNull() ?: 200
                val (w, h) = 640 to 480
                val y = java.nio.ByteBuffer.allocateDirect(w * h)
                val vu = java.nio.ByteBuffer.allocateDirect(w * h / 2)
                val v = vu.duplicate().apply { limit(w * h / 2 - 1) }.slice()
                val u = vu.duplicate().apply { position(1) }.slice()
                val io = app.video.io
                val was = io.jni
                for (jni in listOf(false, true, false, true)) {
                    io.jni = jni
                    val t = LongArray(n)
                    for (k in 0 until n) {
                        val t0 = System.nanoTime()
                        io.pushPlanes(w, h, 90, y, w, u, v, w, 2)
                        t[k] = System.nanoTime() - t0
                    }
                    t.sort()
                    testLog("video_io_bench path=${if (jni) "jni" else "uniffi"} push_ms_median=${"%.3f".format(t[n / 2] / 1e6)} p90=${"%.3f".format(t[n * 9 / 10] / 1e6)} copy_ms=${"%.3f".format(io.packNs / 1e6)}")
                }
                io.jni = was
                testLog("video_io_bench done")
            }.start()
            "flip" -> { app.video.flip(); testLog("flip front=${app.video.front.value}") }
            "video_enabled" -> { app.videoEnabled = i.getStringExtra("on") != "0"; testLog("video_enabled=${app.videoEnabled}") }
            // Brings the call screen up (the app must be in the foreground to start activities).
            "callscreen" -> app.startActivity(android.content.Intent(app, CallActivity::class.java).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
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
            // Chat checks for the live tests: `who` = contact name or DID.
            "chat_send" -> testLog("chat_sent id=${node.sendText(contact(node, i).did, i.getStringExtra("text") ?: "hi", null).id}")
            "chat_msgs" -> {
                val did = contact(node, i).did
                var page: uniffi.p2pcore.DayPage? = node.chatDay(did, null)
                while (page != null) {
                    page.messages.forEach { testLog("msg id=${it.id} out=${it.outgoing} delivery=${it.delivery} text=${it.text}") }
                    page = page.olderDay?.let { node.chatDay(did, it) }
                }
                testLog("chat_msgs done")
            }
            "online" -> {
                val did = contact(node, i).did
                node.watchPresence(did)
                testLog("online=${node.contactOnline(did)}")
            }
            else -> Log.w("P2PTEST", "unknown cmd $cmd")
        }
    }

    private fun contact(node: uniffi.p2pcore.Node, i: Intent) = i.getStringExtra("who").let { who ->
        node.contacts().firstOrNull { it.did == who || it.name == who } ?: throw IllegalArgumentException("no such contact $who")
    }
}
