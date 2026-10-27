package com.osvauld.p2p

import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.util.Log
import androidx.core.app.NotificationCompat
import uniffi.p2pcore.AccountEvents
import uniffi.p2pcore.AccountRef
import java.util.concurrent.ConcurrentHashMap

/**
 * How a ring or a notice names an account of this phone that is not the open one (docs/multi-online.md).
 * Pure, so the shapes are unit tested. The separators are the design's ("Asha → Work", "Asha · Work").
 */
object AccountLabels {
    /** The ring's title: "Asha → Work" for a call to a background account, else the caller alone. */
    fun ringName(peer: String, account: String?): String =
        if (account.isNullOrBlank()) peer else "$peer → ${account.trim()}"

    /** A background account's message notification title: "Asha · Work" (the sender, then the account). */
    fun messageTitle(from: String, account: String): String =
        if (account.isBlank()) from else "$from · ${account.trim()}"

    /** The notification tag that keeps one account's notices apart from the open account's and from each other. */
    fun noticeTag(accountDid: String) = "bg:$accountDid"
}

/**
 * Messages to a background account (woken by a push, `BackgroundMode.ON_WAKE`): one notification per account and sender,
 * titled with both. It carries no message text (the core does not hand any over) and has no inline reply, which would
 * send from the open account. A tap switches to that account first, then opens the chat (MainActivity).
 */
object BackgroundNotifier {
    /** The account a tapped notification belongs to; with [ChatNotifier.EXTRA_PEER] for the chat to open there. */
    const val EXTRA_ACCOUNT = "chat_account"

    private fun nm(c: Context) = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    private fun id(peer: String) = peer.hashCode() and 0xffff
    /** Account DID -> the senders it has a notice up for, so opening the account clears them. */
    private val posted = ConcurrentHashMap<String, MutableSet<String>>()

    fun post(app: P2pApp, account: AccountRef, fromDid: String, fromName: String) {
        if (!AlertPrefs.messages(app)) return   // Settings › Notifications › Messages
        val from = fromName.trim().ifBlank { app.getString(R.string.common_someone) }
        val tag = AccountLabels.noticeTag(account.did)
        val open = PendingIntent.getActivity(
            app, (account.did + fromDid).hashCode(),
            Intent(app, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
                .putExtra(ChatNotifier.EXTRA_PEER, fromDid).putExtra(EXTRA_ACCOUNT, account.did),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val n = NotificationCompat.Builder(app, Notifications.CH_MESSAGES)
            .setSilent(AlertPrefs.quiet(app))   // the app plays no cue for another account, so the channel's tone does
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setColor(0xFF0B6B5B.toInt())
            .setContentTitle(AccountLabels.messageTitle(from, account.name))
            .setContentText(app.getString(R.string.notif_bg_message_text))
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setContentIntent(open)
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .build()
        posted.getOrPut(account.did) { ConcurrentHashMap.newKeySet() }.add(fromDid)
        try { nm(app).notify(tag, id(fromDid), n) } catch (_: SecurityException) { }
    }

    /** [accountDid] is the open account now: its chats show the messages, so its background notices go. */
    fun cancelFor(c: Context, accountDid: String) {
        val peers = posted.remove(accountDid) ?: return
        val tag = AccountLabels.noticeTag(accountDid)
        peers.forEach { try { nm(c).cancel(tag, id(it)) } catch (_: Exception) {} }
    }
}

/**
 * The core's [AccountEvents]: the remembered keys of the other accounts and their notices. Thin; the routing is the core's.
 * Never logs a DID or a key.
 */
class BackgroundAccounts(private val app: P2pApp) : AccountEvents {
    /**
     * Asked on the thread that called `onWake` (the push thread) or `start` (the lifecycle thread): read the Keystore
     * directly, never through the lifecycle executor (that would wait on itself). Null for a passphrase-only account.
     * The bytes go to the core as they are; UniFFI copies them, so they cannot be zeroed after the hand-over.
     */
    override fun accountKey(did: String): ByteArray? {
        if (!UnlockStore.exists(app, did)) return null
        return when (val l = UnlockStore.load(app, did)) {
            is UnlockStore.Loaded.Key -> l.bytes
            UnlockStore.Loaded.Gone -> { UnlockStore.clear(app, did); null }
            UnlockStore.Loaded.Transient -> null
        }
    }

    /** Core thread: post and return. */
    override fun onBackgroundMessage(account: AccountRef, fromDid: String, fromName: String) {
        try { BackgroundNotifier.post(app, account, fromDid, fromName) } catch (e: Exception) { Log.w(TAG, "notice: ${e.javaClass.simpleName}") }
    }

    /** Core thread: unread counts changed, or the core opened another account (answering its call). */
    override fun onAccountsChanged() = app.accountsChanged()

    private companion object { const val TAG = "BgAccounts" }
}
