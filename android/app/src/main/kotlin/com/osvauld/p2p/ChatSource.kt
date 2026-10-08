package com.osvauld.p2p

import android.util.Log
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import uniffi.p2pcore.Chat
import uniffi.p2pcore.ChatEvents
import uniffi.p2pcore.DayPage
import uniffi.p2pcore.DeliveryState
import uniffi.p2pcore.Message
import java.util.concurrent.ConcurrentHashMap

/** What the core pushes while a conversation or the chat list is on screen. */
sealed interface ChatEvent {
    data class Added(val message: Message) : ChatEvent
    data class Changed(val message: Message) : ChatEvent
    data class Delivery(val peerDid: String, val messageId: String, val delivery: DeliveryState) : ChatEvent
    data class Progress(val peerDid: String, val hash: String, val done: Long, val total: Long, val outgoing: Boolean) : ChatEvent
}

/**
 * How we reach a contact right now. The core has no per-peer "connected" call yet, so the real
 * source only knows [Unknown] until traffic from that peer proves a link; the UI never claims
 * "online" without proof.
 */
enum class Link { Connected, Relayed, Offline, Unknown }

/**
 * Everything the chat screens need. [CoreChatSource] is backed by the [uniffi.p2pcore.Node]; the debug
 * build can swap in an in-memory fake through [ChatBackend]. The blocking calls are for IO threads and
 * throw on failure (the core reports "not implemented" until chat lands: callers show an empty state).
 */
interface ChatSource {
    /** Conversations, most recent activity first. Empty when the core cannot answer. */
    val chats: StateFlow<List<Chat>>
    val events: SharedFlow<ChatEvent>
    /** Per contact; a contact missing from the map is [Link.Unknown]. */
    val links: StateFlow<Map<String, Link>>
    /** Re-reads [chats] from the core, off the caller's thread. */
    fun refresh()

    fun day(peerDid: String, day: String?): DayPage
    fun fetchOlder(peerDid: String, beforeDay: String): Int
    fun sendText(peerDid: String, text: String, replyTo: String?): Message
    fun edit(peerDid: String, messageId: String, text: String): Message
    fun delete(peerDid: String, messageId: String): Message
    fun markRead(peerDid: String)
    fun sendFile(peerDid: String, path: String, mime: String, text: String?): Message
    fun sendVoice(peerDid: String, path: String, durationMs: Int, waveform: ByteArray): Message
    fun download(peerDid: String, messageId: String)
    /** Stops a download (also one waiting to retry or failed); the attachment is REMOTE again. */
    fun cancel(peerDid: String, messageId: String)
    fun save(peerDid: String, messageId: String, destPath: String)
}

/** Where the app gets its [ChatSource]. Release builds never set [override]; the debug gallery does. */
object ChatBackend {
    @Volatile var override: ChatSource? = null
    fun current(app: P2pApp): ChatSource = override ?: app.realChat
}

/** The real thing: the [uniffi.p2pcore.Node] plus its [ChatEvents] callbacks (core threads: never block here). */
class CoreChatSource(private val app: P2pApp) : ChatSource, ChatEvents {
    private val node get() = app.node
    private val _chats = MutableStateFlow<List<Chat>>(emptyList())
    override val chats: StateFlow<List<Chat>> = _chats
    private val _events = MutableSharedFlow<ChatEvent>(extraBufferCapacity = 128)
    override val events: SharedFlow<ChatEvent> = _events
    private val _links = MutableStateFlow<Map<String, Link>>(emptyMap())
    override val links: StateFlow<Map<String, Link>> = _links
    private val expiry = ConcurrentHashMap<String, Job>()

    /** Registers for callbacks on the current node; call again after the node is replaced. */
    fun attach() { try { node.setChatEvents(this) } catch (e: Exception) { Log.w(P2pApp.TAG, "setChatEvents: $e") } }

    /** Traffic from a peer proves a link for a minute. */
    private fun touch(peerDid: String) {
        _links.update { it + (peerDid to Link.Connected) }
        expiry.remove(peerDid)?.cancel()
        expiry[peerDid] = app.scope.launch { delay(60_000); _links.update { it - peerDid } }
    }

    override fun refresh() {
        app.scope.launch {
            _chats.value = try { node.chats() } catch (e: Exception) { Log.d(P2pApp.TAG, "chats: $e"); emptyList() }
        }
    }

    override fun onMessageAdded(message: Message) {
        if (!message.outgoing) touch(message.peerDid)
        _events.tryEmit(ChatEvent.Added(message))
        if (!message.outgoing) ChatNotifier.incoming(app, message)
    }
    override fun onMessageChanged(message: Message) { _events.tryEmit(ChatEvent.Changed(message)) }
    override fun onChatChanged(chat: Chat) {
        _chats.value = (listOf(chat) + _chats.value.filter { it.peerDid != chat.peerDid }).sortedByDescending { it.lastActivity }
    }
    override fun onDeliveryChanged(peerDid: String, messageId: String, delivery: DeliveryState) {
        touch(peerDid)
        _events.tryEmit(ChatEvent.Delivery(peerDid, messageId, delivery))
    }
    override fun onTransferProgress(peerDid: String, hash: String, done: ULong, total: ULong, outgoing: Boolean) {
        touch(peerDid)
        _events.tryEmit(ChatEvent.Progress(peerDid, hash, done.toLong(), total.toLong(), outgoing))
    }

    override fun day(peerDid: String, day: String?) = node.chatDay(peerDid, day)
    override fun fetchOlder(peerDid: String, beforeDay: String) = node.fetchOlderHistory(peerDid, beforeDay).toInt()
    override fun sendText(peerDid: String, text: String, replyTo: String?) = node.sendText(peerDid, text, replyTo)
    override fun edit(peerDid: String, messageId: String, text: String) = node.editMessage(peerDid, messageId, text)
    override fun delete(peerDid: String, messageId: String) = node.deleteMessage(peerDid, messageId)
    override fun markRead(peerDid: String) = node.markRead(peerDid)
    override fun sendFile(peerDid: String, path: String, mime: String, text: String?) = node.sendFile(peerDid, path, mime, text)
    override fun sendVoice(peerDid: String, path: String, durationMs: Int, waveform: ByteArray) =
        node.sendVoice(peerDid, path, durationMs.toUInt(), waveform)
    override fun download(peerDid: String, messageId: String) = node.downloadAttachment(peerDid, messageId)
    override fun cancel(peerDid: String, messageId: String) = node.cancelDownload(peerDid, messageId)
    override fun save(peerDid: String, messageId: String, destPath: String) = node.saveAttachment(peerDid, messageId, destPath)
}
