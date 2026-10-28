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
    /** The group conversation in front of the user (its posts don't notify). */
    @Volatile var group: String? = null
    /** A tap on a message notification asks the UI to open this conversation. */
    val request = MutableStateFlow<String?>(null)
    /** A tap on a group invitation notification asks Home to show the Groups tab. */
    val groupsTab = MutableStateFlow(false)
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
                a.kind == AttachmentKind.VOICE -> appString(R.string.chat_preview_voice)
                a.mime.startsWith("image/") -> appString(R.string.chat_preview_photo)
                else -> a.name
            }
            else -> m.text
        }
    }

    fun incoming(app: P2pApp, m: Message) {
        if (m.deleted || OpenChat.peer == m.peerDid) return
        if (!AlertPrefs.messages(app)) return   // Settings › Notifications › Messages
        val text = previewOf(m).ifBlank { return }
        lines.getOrPut(m.peerDid) { mutableListOf() }.let { synchronized(it) { it.add(Line(text, m.at.toLong(), false)); while (it.size > KEEP) it.removeAt(0) } }
        post(app, m.peerDid, alert = true)
    }

    private fun nameOf(app: P2pApp, peer: String): String =
        app.contacts.value.firstOrNull { it.did == peer }?.display()
            ?: ChatBackend.current(app).chats.value.firstOrNull { it.peerDid == peer }?.peerName?.ifBlank { null } ?: app.getString(R.string.chat_notif_message)

    private fun post(c: Context, peer: String, alert: Boolean) {
        val app = P2pApp.get(c)
        val name = nameOf(app, peer)
        val them = Person.Builder().setName(name).setKey(peer).build()
        val me = Person.Builder().setName(c.getString(R.string.common_you)).build()
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
        val action = NotificationCompat.Action.Builder(0, c.getString(R.string.chat_notif_reply), reply)
            .addRemoteInput(RemoteInput.Builder(KEY_REPLY).setLabel(c.getString(R.string.chat_notif_message)).build())
            .setSemanticAction(NotificationCompat.Action.SEMANTIC_ACTION_REPLY).setShowsUserInterface(false).build()
        val n = NotificationCompat.Builder(c, Notifications.CH_MESSAGES)
            .setSilent(!alert || AlertPrefs.quiet(c) || AppVisible.resumed)   // in front, the app plays the `received` cue itself
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setColor(0xFF0B6B5B.toInt())
            .setStyle(style)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setContentIntent(open)
            .setAutoCancel(true)
            // New incoming messages re-alert the existing conversation notification;
            // displaying our inline reply only updates its history.
            .setOnlyAlertOnce(!alert)
            .addAction(action)
            .build()
        try { nm(c).notify(id(peer), n) } catch (_: SecurityException) { }
    }

    /** After an inline reply: show it in the thread so the notification stops spinning. */
    fun replied(c: Context, peer: String, text: String) {
        lines.getOrPut(peer) { mutableListOf() }.let { synchronized(it) { it.add(Line(text, System.currentTimeMillis(), true)); while (it.size > KEEP) it.removeAt(0) } }
        post(c, peer, alert = false)
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

/**
 * Settings › Notifications › Groups ("Posts, invites and requests"): a post from someone else in a group that isn't in
 * front of the user, one notification per group (MessagingStyle, the group's name as the title). Mute is per chat in the
 * playground and the core has none yet, so every group alerts while the switch is on.
 */
object GroupNotifier {
    private class Line(val who: String, val text: String, val at: Long)
    private val lines = java.util.concurrent.ConcurrentHashMap<String, MutableList<Line>>()
    private fun nm(c: Context) = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    private fun id(gid: String) = 3000 + (gid.hashCode() and 0xffff)

    fun post(app: P2pApp, m: uniffi.p2pcore.GroupMessage, groupName: String?) {
        if (m.outgoing || m.kind != uniffi.p2pcore.GroupItemKind.POST) return
        Cues.incoming(app, true, m.authorDid)   // in the app: the `received` tone (the notification below is then silent)
        if (OpenChat.group == m.groupId) return
        if (!AlertPrefs.groups(app)) return
        if (app.blocks.value.blockedByMe(m.authorDid)) return
        val who = m.authorName.trim().substringBefore(' ').ifBlank { app.getString(R.string.common_someone) }
        lines.getOrPut(m.groupId) { mutableListOf() }.let { synchronized(it) { it.add(Line(who, GroupText.postLine(m), m.at.toLong())); while (it.size > 6) it.removeAt(0) } }
        val me = Person.Builder().setName(app.getString(R.string.common_you)).build()
        val style = NotificationCompat.MessagingStyle(me).setConversationTitle(groupName ?: app.getString(R.string.chat_notif_group)).setGroupConversation(true)
        lines[m.groupId]?.let { synchronized(it) { it.forEach { l -> style.addMessage(l.text, l.at, Person.Builder().setName(l.who).build()) } } }
        val open = PendingIntent.getActivity(app, id(m.groupId), Intent(app, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val n = NotificationCompat.Builder(app, Notifications.CH_GROUPS)
            .setSilent(AlertPrefs.quiet(app) || AppVisible.resumed)   // in front, the app plays the `received` cue itself
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setColor(0xFF0B6B5B.toInt())
            .setStyle(style)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setContentIntent(open)
            .setAutoCancel(true)
            .setOnlyAlertOnce(false)   // each new post can alert an existing group notification
            .build()
        try { nm(app).notify(id(m.groupId), n) } catch (_: SecurityException) { }
    }

    /** "Book club · Priya invited you to the group". A tap opens the Groups tab, where the invitation waits. */
    fun invitation(app: P2pApp, i: uniffi.p2pcore.GroupInvitation) {
        val by = i.inviterName.trim().substringBefore(' ').ifBlank { app.getString(R.string.common_someone) }
        val open = PendingIntent.getActivity(app, 5000 + (i.id.hashCode() and 0xfff),
            Intent(app, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP).putExtra(Notifications.EXTRA_GROUPS, true),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val n = NotificationCompat.Builder(app, Notifications.CH_GROUPS)
            .setSilent(AlertPrefs.quiet(app))
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setColor(0xFF0B6B5B.toInt())
            .setContentTitle(i.groupName.ifBlank { app.getString(R.string.chat_notif_a_group) })
            .setContentText(app.getString(R.string.chat_notif_invited, by))
            .setContentIntent(open)
            .setAutoCancel(true)
            .build()
        try { nm(app).notify(4000 + (i.id.hashCode() and 0xffff), n) } catch (_: SecurityException) { }
    }

    fun cancel(c: Context, gid: String) {
        lines.remove(gid)
        nm(c).cancel(id(gid))
    }
}
