package com.osvauld.p2p

import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.Person
import androidx.core.app.RemoteInput
import kotlinx.coroutines.flow.MutableStateFlow
import uniffi.p2pcore.AttachmentKind
import uniffi.p2pcore.Message
import java.util.concurrent.ConcurrentHashMap

/** Which conversation is on screen (resumed), so its own messages do not notify. */
object OpenChat {
    @Volatile var peer: String? = null
    /** A tap on a message notification asks the UI to open this conversation. */
    val request = MutableStateFlow<String?>(null)
}

/** One MessagingStyle notification per conversation, with an inline reply. Hooked to `on_message_added`. */
object ChatNotifier {
    const val EXTRA_PEER = "chat_peer"
    const val ACTION_REPLY = "com.osvauld.p2p.CHAT_REPLY"
    private const val KEY_REPLY = "reply_text"
    private const val KEEP = 6

    private class Line(val text: String, val at: Long, val mine: Boolean)
    private val lines = ConcurrentHashMap<String, MutableList<Line>>()

    private fun nm(c: Context) = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    private fun id(peer: String) = 1000 + (peer.hashCode() and 0xffff)

    fun previewOf(m: Message): String {
        val a = m.attachment
        return when {
            m.deleted -> ""
            a != null && m.text.isBlank() -> when {
                a.kind == AttachmentKind.VOICE -> "Voice message"
                a.mime.startsWith("image/") -> "Photo"
                else -> a.name
            }
            else -> m.text
        }
    }

    fun incoming(app: P2pApp, m: Message) {
        if (m.deleted || OpenChat.peer == m.peerDid) return
        val text = previewOf(m).ifBlank { return }
        lines.getOrPut(m.peerDid) { mutableListOf() }.let { synchronized(it) { it.add(Line(text, m.at.toLong(), false)); while (it.size > KEEP) it.removeAt(0) } }
        post(app, m.peerDid)
    }

    private fun nameOf(app: P2pApp, peer: String): String =
        app.contacts.value.firstOrNull { it.did == peer }?.display()
            ?: ChatBackend.current(app).chats.value.firstOrNull { it.peerDid == peer }?.peerName?.ifBlank { null } ?: "Message"

    private fun post(c: Context, peer: String) {
        val app = P2pApp.get(c)
        val name = nameOf(app, peer)
        val them = Person.Builder().setName(name).setKey(peer).build()
        val me = Person.Builder().setName("You").build()
        val style = NotificationCompat.MessagingStyle(me)
        lines[peer]?.let { synchronized(it) { it.forEach { l -> style.addMessage(l.text, l.at, if (l.mine) me else them) } } }
        val flags = PendingIntent.FLAG_UPDATE_CURRENT
        val open = PendingIntent.getActivity(
            c, id(peer), Intent(c, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP).putExtra(EXTRA_PEER, peer),
            flags or PendingIntent.FLAG_IMMUTABLE,
        )
        // Inline reply needs a mutable intent so the system can fill in the typed text.
        val replyIntent = Intent(c, ReplyReceiver::class.java).setAction(ACTION_REPLY).putExtra(EXTRA_PEER, peer)
        val reply = PendingIntent.getBroadcast(c, id(peer), replyIntent, flags or PendingIntent.FLAG_MUTABLE)
        val action = NotificationCompat.Action.Builder(0, "Reply", reply)
            .addRemoteInput(RemoteInput.Builder(KEY_REPLY).setLabel("Message").build())
            .setSemanticAction(NotificationCompat.Action.SEMANTIC_ACTION_REPLY).setShowsUserInterface(false).build()
        val n = NotificationCompat.Builder(c, Notifications.CH_MESSAGES)
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setColor(0xFF0B6B5B.toInt())
            .setStyle(style)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setContentIntent(open)
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .addAction(action)
            .build()
        try { nm(c).notify(id(peer), n) } catch (_: SecurityException) { }
    }

    /** After an inline reply: show it in the thread so the notification stops spinning. */
    fun replied(c: Context, peer: String, text: String) {
        lines.getOrPut(peer) { mutableListOf() }.let { synchronized(it) { it.add(Line(text, System.currentTimeMillis(), true)); while (it.size > KEEP) it.removeAt(0) } }
        post(c, peer)
    }

    fun cancel(c: Context, peer: String) {
        lines.remove(peer)
        nm(c).cancel(id(peer))
    }
}

/** Handles the notification's inline reply; the send runs on a worker thread. */
class ReplyReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val peer = intent.getStringExtra(ChatNotifier.EXTRA_PEER) ?: return
        val text = RemoteInput.getResultsFromIntent(intent)?.getCharSequence("reply_text")?.toString()?.trim().orEmpty()
        if (text.isEmpty()) return
        val pending = goAsync()
        val app = P2pApp.get(context)
        Thread {
            try {
                app.chat.sendText(peer, text, null)
                ChatNotifier.replied(app, peer, text)
            } catch (e: Exception) {
                Log.w(P2pApp.TAG, "reply: $e")
                ChatNotifier.cancel(app, peer)
            } finally { pending.finish() }
        }.start()
    }
}
