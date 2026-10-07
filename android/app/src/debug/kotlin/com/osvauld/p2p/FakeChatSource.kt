package com.osvauld.p2p

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import uniffi.p2pcore.Attachment
import uniffi.p2pcore.AttachmentKind
import uniffi.p2pcore.Chat
import uniffi.p2pcore.DayPage
import uniffi.p2pcore.DeliveryState
import uniffi.p2pcore.Message
import uniffi.p2pcore.TransferState
import java.io.File
import java.time.LocalDate
import java.time.ZoneId
import java.util.UUID

/**
 * In-memory [ChatSource] for design review and emulator runs (debug builds only). Seeded with the boards'
 * conversations; sending is delivered after a delay unless the contact's link is Offline, and downloads
 * advance by themselves. Never touches the Node.
 */
class FakeChatSource(private val app: P2pApp? = null, files: Boolean = false) : ChatSource {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val msgs = LinkedHashMap<String, MutableList<Message>>()
    private val unread = HashMap<String, Int>()
    private val names = LinkedHashMap<String, String>()
    private val _chats = MutableStateFlow<List<Chat>>(emptyList())
    override val chats: StateFlow<List<Chat>> = _chats
    // Replay lets a conversation opened later still see the seeded "sending 42%" progress.
    private val _events = MutableSharedFlow<ChatEvent>(replay = 8, extraBufferCapacity = 64)
    override val events: SharedFlow<ChatEvent> = _events
    private val _links = MutableStateFlow<Map<String, Link>>(emptyMap())
    override val links: StateFlow<Map<String, Link>> = _links

    companion object {
        fun did(name: String) = "did:key:fake-${name.substringBefore(' ').lowercase()}"
        val people = listOf("Arjun Oommen", "Dad", "Rosa Silva", "Jonas Krüger", "Joanna Ostrowska", "Lena Park", "Priya Nair")
    }

    private fun ms(daysAgo: Long, h: Int, m: Int) =
        LocalDate.now().minusDays(daysAgo).atTime(h, m).atZone(ZoneId.systemDefault()).toInstant().toEpochMilli().toULong()

    private fun msg(peer: String, out: Boolean, at: ULong, text: String, delivery: DeliveryState = DeliveryState.DELIVERED, replyTo: String? = null,
                    att: Attachment? = null, id: String = UUID.randomUUID().toString(), deleted: Boolean = false, edited: Boolean = false) =
        Message(id, peer, if (out) "did:key:me" else peer, out, at, if (deleted) "" else text, if (edited) at + 120_000UL else null, deleted, replyTo, att, if (out) delivery else DeliveryState.DELIVERED)

    private fun att(name: String, size: Long, mime: String, state: TransferState = TransferState.READY, transferred: Long = 0, kind: AttachmentKind = AttachmentKind.FILE,
                    ms: Int = 0, wave: List<Int> = emptyList()) =
        Attachment(UUID.randomUUID().toString().replace("-", ""), name, size.toULong(), mime, kind, ms.toUInt(), wave.map { it.toUByte() }, state, transferred.toULong())

    private fun add(peer: String, m: Message) = msgs.getOrPut(peer) { mutableListOf() }.add(m)

    private var photoHash = ""

    init {
        people.forEach { names[did(it)] = it }
        val ar = did("Arjun")
        add(ar, msg(ar, false, ms(1, 22, 10), "Did you get home ok?"))
        add(ar, msg(ar, true, ms(1, 22, 14), "Yes! Train was on time for once"))
        add(ar, msg(ar, false, ms(0, 12, 38), "Are we still on for the call about the flat?", id = "q1"))
        add(ar, msg(ar, false, ms(0, 12, 38), "I have the landlord’s numbers now"))
        add(ar, msg(ar, true, ms(0, 12, 40), "Yes — after 6 works for me", replyTo = "q1", edited = true))
        add(ar, msg(ar, false, ms(0, 12, 41), "Sure, call me after 6?"))
        add(ar, msg(ar, true, ms(0, 12, 42), "Perfect, talk then", id = "perfect"))
        unread[ar] = 2
        val dad = did("Dad")
        add(dad, msg(dad, false, ms(0, 10, 58), "Safe flight!"))
        add(dad, msg(dad, true, ms(0, 11, 20), "Landed, will call from home"))
        val ro = did("Rosa")
        add(ro, msg(ro, false, ms(0, 9, 5), "", att = att("kitchen.jpg", 1_800_000, "image/jpeg")))
        unread[ro] = 1
        val jo = did("Jonas")
        add(jo, msg(jo, true, ms(1, 17, 20), "The slides are in the file I sent"))
        val jn = did("Joanna")
        add(jn, msg(jn, false, ms(2, 18, 30), "Thanks for today!"))
        val le = did("Lena")
        add(le, msg(le, true, ms(3, 20, 0), "See you there", DeliveryState.PENDING))
        // Files and voice (the files boards): Rosa's phone is Offline so downloads sit paused; Jonas is online.
        if (files) {
            add(ro, msg(ro, false, ms(0, 16, 2), "", att = att("kitchen2.jpg", 2_400_000, "image/jpeg")))
            add(ro, msg(ro, false, ms(0, 16, 5), "", att = att("Lease draft v2.pdf", 8_000_000, "application/pdf", TransferState.DOWNLOADING, 3_100_000)))
            add(ro, msg(ro, false, ms(0, 16, 7), "", att = att("Walkthrough.mp4", 184_000_000, "video/mp4", TransferState.DOWNLOADING, 12_000_000)))
            add(ro, msg(ro, false, ms(0, 16, 8), "", att = att("Bank statement.pdf", 36_000_000, "application/pdf", TransferState.REMOTE)))
            add(ro, msg(ro, false, ms(0, 16, 10), "", att = voice(32_000)))
            add(jo, msg(jo, false, ms(0, 17, 20), "Can you send the floor plan?"))
            val photo = msg(jo, true, ms(0, 17, 22), "", DeliveryState.PENDING, att = att("living-room.jpg", 3_200_000, "image/jpeg"), id = "outphoto")
            add(jo, photo); photoHash = photo.attachment!!.hash
            add(jo, msg(jo, true, ms(0, 17, 22), "", att = att("Floor plan – 3rd floor.pdf", 2_400_000, "application/pdf")))
            add(jo, msg(jo, true, ms(0, 17, 23), "", att = voice(14_000)))
        }
        _links.value = mapOf(ar to Link.Connected, dad to Link.Relayed, jo to Link.Connected, ro to Link.Offline, le to Link.Relayed)
        if (files) _events.tryEmit(ChatEvent.Progress(jo, photoHash, 42, 100, true))
        rebuild()
    }

    private fun voice(durMs: Int) = att("Voice message.opus", 40_000, "audio/ogg", kind = AttachmentKind.VOICE, ms = durMs,
        wave = List(64) { (30 + 200 * kotlin.math.abs(kotlin.math.sin(it * 0.55) * kotlin.math.cos(it * 0.13))).toInt().coerceIn(0, 255) })

    /** The "older messages" board: a Monday with a deleted message on each side. */
    fun seedOlder(peer: String) {
        msgs[peer]?.clear()
        add(peer, msg(peer, false, ms(3, 19, 2), "Found a flat near the station, sending pics later"))
        add(peer, msg(peer, true, ms(3, 19, 5), "", deleted = true))
        add(peer, msg(peer, false, ms(3, 19, 6), "", deleted = true))
        add(peer, msg(peer, true, ms(3, 19, 7), "Nice! Which street?"))
        add(peer, msg(peer, false, ms(0, 12, 41), "Sure, call me after 6?"))
        unread.remove(peer); rebuild()
    }

    fun setLink(peer: String, link: Link) { _links.update { it + (peer to link) } }

    private fun previewOf(m: Message) = ChatNotifier.previewOf(m).let { if (m.attachment?.mime?.startsWith("image/") == true && m.text.isBlank()) "Photo" else it }

    private fun rebuild() {
        _chats.value = names.map { (d, n) ->
            val last = msgs[d]?.maxWithOrNull(compareBy({ it.at }, { it.id }))
            Chat(d, n, last?.let(::previewOf) ?: "", last?.outgoing ?: false, last?.delivery ?: DeliveryState.DELIVERED, last?.at ?: 0UL, (unread[d] ?: 0).toUInt())
        }.sortedByDescending { it.lastActivity }
    }

    private fun put(m: Message) {
        val l = msgs.getOrPut(m.peerDid) { mutableListOf() }
        val i = l.indexOfFirst { it.id == m.id }
        if (i >= 0) l[i] = m else l.add(m)
        rebuild()
    }
    private fun find(peer: String, id: String) = msgs[peer]?.firstOrNull { it.id == id } ?: throw IllegalArgumentException("no message")

    /** Debug hook: a message arrives from [peer] (also raises the notification when the chat is not open). */
    fun incoming(peer: String, text: String) {
        val m = msg(peer, false, System.currentTimeMillis().toULong(), text)
        unread[peer] = (unread[peer] ?: 0) + 1
        put(m)
        _events.tryEmit(ChatEvent.Added(m))
        app?.let { ChatNotifier.incoming(it, m) }
    }

    override fun refresh() { rebuild() }
    override fun day(peerDid: String, day: String?) = DayPage("today", (msgs[peerDid] ?: emptyList<Message>()).sortedBy { it.at }, null)
    override fun fetchOlder(peerDid: String, beforeDay: String) = 0

    private fun deliverLater(m: Message) {
        scope.launch {
            delay(1500)
            if (_links.value[m.peerDid] == Link.Offline) return@launch
            val d = find(m.peerDid, m.id).copy(delivery = DeliveryState.DELIVERED)
            put(d)
            _events.tryEmit(ChatEvent.Delivery(m.peerDid, m.id, DeliveryState.DELIVERED))
        }
    }

    override fun sendText(peerDid: String, text: String, replyTo: String?): Message {
        val m = msg(peerDid, true, System.currentTimeMillis().toULong(), text, DeliveryState.PENDING, replyTo)
        put(m); _events.tryEmit(ChatEvent.Added(m)); deliverLater(m)
        return m
    }

    override fun edit(peerDid: String, messageId: String, text: String): Message {
        val m = find(peerDid, messageId).copy(text = text, editedAt = System.currentTimeMillis().toULong())
        put(m); _events.tryEmit(ChatEvent.Changed(m)); return m
    }

    override fun delete(peerDid: String, messageId: String): Message {
        val m = find(peerDid, messageId).copy(text = "", deleted = true, attachment = null)
        put(m); _events.tryEmit(ChatEvent.Changed(m)); return m
    }

    override fun markRead(peerDid: String) { if (unread.remove(peerDid) != null) rebuild() }

    override fun sendFile(peerDid: String, path: String, mime: String, text: String?): Message {
        val f = File(path)
        val m = msg(peerDid, true, System.currentTimeMillis().toULong(), text ?: "", DeliveryState.PENDING, att = att(f.name, f.length(), mime))
        put(m); _events.tryEmit(ChatEvent.Added(m))
        scope.launch {
            for (p in 1..10) { delay(200); _events.tryEmit(ChatEvent.Progress(peerDid, m.attachment!!.hash, p * 10L, 100, true)) }
        }
        deliverLater(m)
        return m
    }

    override fun sendVoice(peerDid: String, path: String, durationMs: Int, waveform: ByteArray): Message {
        val m = msg(peerDid, true, System.currentTimeMillis().toULong(), "", DeliveryState.PENDING,
            att = att("Voice message.opus", File(path).length(), "audio/ogg", kind = AttachmentKind.VOICE, ms = durationMs, wave = waveform.map { it.toInt() and 0xff }))
        put(m); _events.tryEmit(ChatEvent.Added(m)); deliverLater(m)
        return m
    }

    override fun download(peerDid: String, messageId: String) {
        val m0 = find(peerDid, messageId)
        val a = m0.attachment ?: return
        put(m0.copy(attachment = a.copy(state = TransferState.DOWNLOADING)).also { _events.tryEmit(ChatEvent.Changed(it)) })
        scope.launch {
            val total = a.size.toLong()
            var done = a.transferred.toLong()
            while (done < total) {
                delay(300)
                if (_links.value[peerDid] == Link.Offline) continue
                done = (done + total / 8).coerceAtMost(total)
                _events.tryEmit(ChatEvent.Progress(peerDid, a.hash, done, total, false))
                val cur = find(peerDid, messageId)
                put(cur.copy(attachment = cur.attachment!!.copy(transferred = done.toULong())).also { _events.tryEmit(ChatEvent.Changed(it)) })
            }
            val cur = find(peerDid, messageId)
            put(cur.copy(attachment = cur.attachment!!.copy(state = TransferState.READY, transferred = 0UL)).also { _events.tryEmit(ChatEvent.Changed(it)) })
        }
    }

    override fun save(peerDid: String, messageId: String, destPath: String) {
        val a = find(peerDid, messageId).attachment ?: throw IllegalArgumentException("no attachment")
        val dest = File(destPath)
        if (a.mime.startsWith("image/")) {
            // A made-up picture: sky, a sun and two hills, tinted by the file name.
            val bmp = Bitmap.createBitmap(960, 720, Bitmap.Config.ARGB_8888)
            val c = Canvas(bmp)
            val p = Paint(Paint.ANTI_ALIAS_FLAG)
            val hue = (a.name.hashCode() and 0xff) / 255f * 360f
            p.color = android.graphics.Color.HSVToColor(floatArrayOf(hue, .25f, .95f)); c.drawRect(0f, 0f, 960f, 720f, p)
            p.color = 0xFFF5C451.toInt(); c.drawCircle(740f, 170f, 80f, p)
            p.color = android.graphics.Color.HSVToColor(floatArrayOf(hue, .55f, .55f)); c.drawCircle(280f, 900f, 480f, p)
            p.color = android.graphics.Color.HSVToColor(floatArrayOf(hue, .6f, .4f)); c.drawCircle(820f, 980f, 520f, p)
            dest.outputStream().use { bmp.compress(Bitmap.CompressFormat.PNG, 90, it) }
        } else dest.writeText("fake attachment ${a.name}\n")
    }
}
