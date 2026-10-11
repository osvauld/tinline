package com.osvauld.p2p

import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import uniffi.p2pcore.ChatRequestState
import uniffi.p2pcore.GroupChatRequest
import uniffi.p2pcore.GroupDetail
import uniffi.p2pcore.GroupRequestEvents
import uniffi.p2pcore.GroupEvents
import uniffi.p2pcore.GroupInvitePeek
import uniffi.p2pcore.GroupItemKind
import uniffi.p2pcore.GroupMessage
import uniffi.p2pcore.GroupPage
import uniffi.p2pcore.GroupSummary
import uniffi.p2pcore.Node
import uniffi.p2pcore.GroupDelivery as CoreDelivery
import uniffi.p2pcore.GroupMember as CoreMember
import uniffi.p2pcore.GroupRole as CoreRole

/*
 * Group chat on the core's real API (G1: crates/core/src/group/api.rs, docs/group-core.md).
 * [GroupsController] is what the screens in GroupsLive.kt read; every core call goes through [GroupApi]
 * so the flows are tested on the JVM with a stand-in (GroupsCoreTest). The placeholder preview
 * (GroupsSource.kt, FakeGroupsPrivacy.kt) stays only for the demo screens and the privacy preview.
 *
 * Decisions (rotation-open-decisions.md §0): 3 admin flags remove a member (U9/U15) and the removed member
 * sees the signed flags; a post written before a removal still shows, marked (U11); only hosts know member
 * endpoints (U7), so our posts honestly say "Waiting for a host" until a host acknowledges them. Chat
 * requests through the host (U3) are not in core yet and have no live UI.
 */

/** The core's group calls. Blocking ones (create, invite, join, accept host) are for IO threads. */
interface GroupApi {
    fun groups(): List<GroupSummary>
    fun detail(id: String): GroupDetail
    fun messages(id: String, before: String?, limit: Int): GroupPage
    fun markRead(id: String)
    fun create(name: String): String
    fun invite(id: String): String
    fun deviceToken(id: String): String
    fun peek(token: String): GroupInvitePeek?
    fun join(token: String): String
    fun post(id: String, text: String, replyTo: String?): GroupMessage
    fun flag(id: String, did: String)
    fun setAdmin(id: String, did: String, admin: Boolean)
    fun leave(id: String)
    fun appointHost(id: String, did: String, deviceId: String)
    fun revokeHost(id: String, did: String, deviceId: String)
    fun acceptHost(id: String, accept: Boolean)
    fun deviceId(): String
    // Chat requests through the group's hosts (G2, U3).
    fun chatRequest(id: String, did: String): GroupChatRequest
    fun requestsIncoming(): List<GroupChatRequest>
    fun requestsOutgoing(): List<GroupChatRequest>
    fun chatAccept(requestId: String): GroupChatRequest
    fun chatDecline(requestId: String): GroupChatRequest
    /** The personal block (P5), for "Block" on an incoming request. */
    fun block(did: String)
}

class NodeGroupApi(private val node: () -> Node) : GroupApi {
    override fun groups() = node().groups()
    override fun detail(id: String) = node().groupDetail(id)
    override fun messages(id: String, before: String?, limit: Int) = node().groupMessages(id, before, limit.toUInt())
    override fun markRead(id: String) = node().groupMarkRead(id)
    override fun create(name: String) = node().groupCreate(name)
    override fun invite(id: String) = node().groupInvite(id)
    override fun deviceToken(id: String) = node().groupDeviceToken(id)
    override fun peek(token: String) = node().groupPeekInvite(token)
    override fun join(token: String) = node().groupJoin(token)
    override fun post(id: String, text: String, replyTo: String?) = node().groupPost(id, text, replyTo)
    override fun flag(id: String, did: String) = node().groupFlag(id, did)
    override fun setAdmin(id: String, did: String, admin: Boolean) = node().groupSetAdmin(id, did, admin)
    override fun leave(id: String) = node().groupLeave(id)
    override fun appointHost(id: String, did: String, deviceId: String) = node().groupAppointHost(id, did, deviceId)
    override fun revokeHost(id: String, did: String, deviceId: String) = node().groupRevokeHost(id, did, deviceId)
    override fun acceptHost(id: String, accept: Boolean) = node().groupAcceptHost(id, accept)
    override fun deviceId() = node().groupDeviceId()
    override fun chatRequest(id: String, did: String) = node().groupChatRequest(id, did)
    override fun requestsIncoming() = node().groupChatRequestsIncoming()
    override fun requestsOutgoing() = node().groupChatRequestsOutgoing()
    override fun chatAccept(requestId: String) = node().groupChatAccept(requestId)
    override fun chatDecline(requestId: String) = node().groupChatDecline(requestId)
    override fun block(did: String) = node().block(did)
}

/** The group on screen: its detail and the loaded window of messages (oldest first). */
data class OpenGroup(val id: String, val detail: GroupDetail, val messages: List<GroupMessage>, val older: String?)

/** What a member's sheet offers us, in order. */
sealed interface MemberAction {
    /** Message privately: their chat if a contact, else a chat request (U3). */
    data object Message : MemberAction
    data object MakeAdmin : MemberAction
    data object RevokeAdmin : MemberAction
    data class Appoint(val deviceId: String) : MemberAction
    data class Revoke(val deviceId: String) : MemberAction
    data object Flag : MemberAction
}

object GroupText {
    const val FLAGS_TO_REMOVE = 3
    const val PAGE = 100

    fun active(r: CoreRole) = r == CoreRole.OWNER || r == CoreRole.ADMIN || r == CoreRole.MEMBER
    fun isAdmin(r: CoreRole) = r == CoreRole.OWNER || r == CoreRole.ADMIN

    private fun someone(name: String) = name.ifBlank { "Someone" }
    fun shortDev(dev: String) = dev.take(8)

    /** A system line in plain words. [me] is our DID. */
    fun sysText(m: GroupMessage, me: String): String {
        fun who(did: String, name: String) = if (did == me) "You" else someone(name)
        val by = who(m.authorDid, m.authorName)
        val sub = who(m.subjectDid, m.subjectName)
        val you = m.subjectDid == me
        return when (m.kind) {
            GroupItemKind.POST -> m.text
            GroupItemKind.JOINED -> "$sub joined"
            GroupItemKind.LEFT -> "$sub left"
            GroupItemKind.REMOVED -> if (you) "You were removed after $FLAGS_TO_REMOVE admin flags" else "$sub was removed after $FLAGS_TO_REMOVE admin flags"
            GroupItemKind.ADMIN_GRANTED -> if (you) "$by made you an admin" else "$by made $sub an admin"
            GroupItemKind.ADMIN_REVOKED -> if (you) "$by removed you as admin" else "$by removed $sub as admin"
            GroupItemKind.HOST_ADDED -> if (you) "A device of yours now hosts the group" else "A device of $sub’s now hosts the group"
            GroupItemKind.HOST_REMOVED -> if (you) "A device of yours stopped hosting the group" else "A device of $sub’s stopped hosting the group"
        }
    }

    /** The list row's second line. */
    fun preview(s: GroupSummary, me: String): String = when (s.myRole) {
        CoreRole.REMOVED -> "You were removed from this group"
        CoreRole.LEFT -> "You left this group"
        else -> when (val m = s.last) {
            null -> ""
            else -> when {
                m.kind != GroupItemKind.POST -> sysText(m, me)
                m.outgoing -> "You: ${m.text}"
                else -> "${firstName(someone(m.authorName))}: ${m.text}"
            }
        }
    }

    /** The header's sub-line. */
    fun status(d: GroupDetail): String {
        when (d.myRole) {
            CoreRole.REMOVED -> return "You were removed from this group"
            CoreRole.LEFT -> return "You left this group"
            else -> {}
        }
        val n = d.members.size
        val members = "$n member${if (n == 1) "" else "s"}"
        return when {
            d.amHost -> "$members · this device hosts the group"
            d.hosts.isEmpty() -> "$members · no host yet"
            d.hosts.none { it.online } -> "$members · waiting for a host"
            else -> "$members · ${d.hosts.size} host${if (d.hosts.size == 1) "" else "s"}"
        }
    }

    fun waitingForHost(d: GroupDetail) = active(d.myRole) && !d.amHost && d.hosts.none { it.online }

    /** The small line under one of our posts, or null when delivered. */
    fun deliveryWord(m: GroupMessage): String? =
        if (m.outgoing && m.kind == GroupItemKind.POST && m.delivery == CoreDelivery.WAITING) "Waiting for a host" else null

    /** A sent request's state line and whether it is bad news. A block is visible (U5 revised, P5). */
    fun sentLine(r: GroupChatRequest): Pair<String, Boolean> {
        val f = firstName(someone(r.peerName))
        return when (r.state) {
            ChatRequestState.PENDING -> "Waiting for $f · via ${r.groupName}" to false
            ChatRequestState.ACCEPTED -> (if (r.contactAdded) "$f accepted · messages only" else "$f accepted · connecting") to false
            ChatRequestState.DECLINED -> "$f declined" to true
            ChatRequestState.BLOCKED -> "$f blocked you" to true
        }
    }

    /** Who asked this device to host [d], or null when nobody did. */
    fun hostAsker(d: GroupDetail): String? {
        if (!d.hostRequestedHere || !active(d.myRole)) return null
        val by = d.hostRequests.firstOrNull { it.isMe }?.appointedBy
        return by?.let { b -> d.members.firstOrNull { it.did == b }?.name }?.ifBlank { null } ?: "An admin"
    }

    /** Unread posts in groups we are still in: the Groups tab's badge. */
    fun badge(list: List<GroupSummary>) = list.filter { active(it.myRole) }.sumOf { it.unread.toInt() }

    fun actions(d: GroupDetail, m: CoreMember): List<MemberAction> {
        if (m.isMe || !active(d.myRole)) return emptyList()
        val out = mutableListOf<MemberAction>(MemberAction.Message)
        if (d.myRole == CoreRole.OWNER && !m.isOwner) out += if (m.isAdmin) MemberAction.RevokeAdmin else MemberAction.MakeAdmin
        if (isAdmin(d.myRole)) {
            m.deviceIds.forEach { dev ->
                out += if (d.hosts.any { it.did == m.did && it.deviceId == dev }) MemberAction.Revoke(dev) else MemberAction.Appoint(dev)
            }
            if (!m.isOwner) out += MemberAction.Flag
        }
        return out
    }

    /** Which flag ours would be, and the dialog's sentence. */
    fun flagCopy(name: String, flagsSoFar: Int): Pair<Int, String> {
        val f = firstName(name)
        val yours = flagsSoFar + 1
        return yours to if (yours >= FLAGS_TO_REMOVE)
            "Yours will be flag $yours of $FLAGS_TO_REMOVE. $f is removed right away: hosts refuse their new messages and they see they were removed."
        else "Yours will be flag $yours of $FLAGS_TO_REMOVE. Any admin’s flags count, yours again too. At $FLAGS_TO_REMOVE flags $f is removed right away and sees they were removed."
    }

    /** Words for a failed group call (the core's flat errors carry "they refused: <reason>"). */
    fun error(e: Throwable): String {
        val msg = e.message.orEmpty()
        return when (e) {
            is uniffi.p2pcore.Exception.Rejected -> {
                val r = msg.removePrefix("they refused: ").trim()
                when (r) {
                    "removed" -> "You were removed from this group."
                    "not a member" -> "You’re not a member of this group."
                    "already a contact" -> "You already have them as a contact. Message them from Chats."
                    "you block them" -> "You block them. Unblock them first to ask for a chat."
                    "blocked" -> "They blocked you, so you can’t ask them for a chat."
                    "" -> "Refused."
                    else -> r.replaceFirstChar { it.uppercase() }.trimEnd('.') + "."
                }
            }
            is uniffi.p2pcore.Exception.Timeout -> "No host answered in time. Try again when a host is online."
            is uniffi.p2pcore.Exception.Locked -> "Unlock first."
            is uniffi.p2pcore.Exception.NotFound -> "That group isn’t on this phone."
            else -> msg.ifBlank { "Something went wrong" }
        }
    }
}

/** Inserts or replaces by id, keeping the core's (at, id) order. */
fun upsertGroupMessage(list: List<GroupMessage>, m: GroupMessage): List<GroupMessage> {
    val i = list.indexOfFirst { it.id == m.id }
    if (i >= 0) return list.toMutableList().also { it[i] = m }
    val at = list.indexOfFirst { it.at > m.at || (it.at == m.at && it.id > m.id) }.let { if (it < 0) list.size else it }
    return list.toMutableList().also { it.add(at, m) }
}

/**
 * The groups state the screens read, refreshed from the core on its [GroupEvents]. Reads are in-memory in
 * the core; actions return Result with what to tell the user, and must be called off the main thread.
 */
class GroupsController(private val api: GroupApi, private val me: () -> String?) : GroupEvents, GroupRequestEvents {
    private val _incoming = MutableStateFlow<List<GroupChatRequest>>(emptyList())
    /** Chat requests to us (blocked senders are never listed), newest first. */
    val incoming: StateFlow<List<GroupChatRequest>> = _incoming
    private val _outgoing = MutableStateFlow<List<GroupChatRequest>>(emptyList())
    val outgoing: StateFlow<List<GroupChatRequest>> = _outgoing
    private val _notice = MutableStateFlow<String?>(null)
    /** Something to tell the user once ("Leo accepted your request"); the UI clears it. */
    val notice: StateFlow<String?> = _notice
    fun clearNotice() { _notice.value = null }
    private val _list = MutableStateFlow<List<GroupSummary>>(emptyList())
    val list: StateFlow<List<GroupSummary>> = _list
    private val _open = MutableStateFlow<OpenGroup?>(null)
    val open: StateFlow<OpenGroup?> = _open
    @Volatile var myDevice: String = ""
        private set

    val myDid get() = me().orEmpty()

    @Synchronized fun refresh() {
        // A node without groups (or locked) answers with an error: an empty list.
        _list.value = try { api.groups() } catch (e: Exception) { emptyList() }
        if (myDevice.isEmpty()) myDevice = try { api.deviceId() } catch (_: Exception) { "" }
        reloadRequests()
        reloadOpen()
    }

    @Synchronized fun open(id: String) {
        _open.value = null
        try { api.markRead(id) } catch (_: Exception) {}
        load(id, GroupText.PAGE)
        _list.value = try { api.groups() } catch (e: Exception) { _list.value }
    }

    @Synchronized fun close(id: String) { if (_open.value?.id == id) _open.value = null }

    private fun load(id: String, want: Int) {
        _open.value = try {
            val d = api.detail(id)
            val p = api.messages(id, null, want)
            OpenGroup(id, d, p.messages, p.older)
        } catch (e: Exception) { null }
    }

    private fun reloadOpen() { _open.value?.let { load(it.id, maxOf(it.messages.size, GroupText.PAGE)) } }

    @Synchronized fun older() {
        val o = _open.value ?: return
        val before = o.older ?: return
        val p = try { api.messages(o.id, before, GroupText.PAGE) } catch (e: Exception) { return }
        val have = o.messages.map { it.id }.toSet()
        _open.value = o.copy(messages = p.messages.filter { it.id !in have } + o.messages, older = p.older)
    }

    fun peek(token: String): GroupInvitePeek? = if (token.isBlank()) null else try { api.peek(token.trim()) } catch (_: Exception) { null }

    fun post(id: String, text: String, replyTo: String?): Result<GroupMessage> {
        val t = text.trim()
        if (t.isEmpty()) return Result.failure(IllegalArgumentException("Type a message first."))
        return runCatching { api.post(id, t, replyTo) }.onSuccess { m -> synchronized(this) {
            _open.value?.let { o -> if (o.id == id) _open.value = o.copy(messages = upsertGroupMessage(o.messages, m)) }
            _list.value = try { api.groups() } catch (e: Exception) { _list.value }
        } }.mapError()
    }

    fun create(name: String): Result<String> {
        if (name.isBlank()) return Result.failure(IllegalArgumentException("Give the group a name."))
        return runCatching { api.create(name.trim()) }.also { refresh() }.mapError()
    }

    /** Blocks until joined (about a minute at most). */
    fun join(token: String): Result<String> = runCatching { api.join(token.trim()) }.also { refresh() }.mapError()

    fun invite(id: String): Result<String> = runCatching { api.invite(id) }.mapError()
    fun deviceToken(id: String): Result<String> = runCatching { api.deviceToken(id) }.mapError()

    fun flag(id: String, did: String) = act("Flag recorded. It is signed by you and the member can see it.") { api.flag(id, did) }
    fun setAdmin(id: String, did: String, admin: Boolean) = act(if (admin) "Made admin." else "No longer an admin.") { api.setAdmin(id, did, admin) }
    fun leave(id: String) = act("You left the group. Its messages stay here.") { api.leave(id) }
    fun appoint(id: String, did: String, dev: String) = act("Appointed. The device hosts once it agrees.") { api.appointHost(id, did, dev) }
    fun revoke(id: String, did: String, dev: String) = act("That device no longer hosts the group.") { api.revokeHost(id, did, dev) }
    fun acceptHost(id: String, accept: Boolean) = act(
        if (accept) "This phone agreed to host. It hosts as soon as an admin has appointed it." else "This phone stopped hosting the group."
    ) { api.acceptHost(id, accept) }

    private fun reloadRequests() {
        _incoming.value = try { api.requestsIncoming() } catch (_: Exception) { emptyList() }
        _outgoing.value = try { api.requestsOutgoing() } catch (_: Exception) { emptyList() }
    }

    /** "Message privately" for a member who is not a contact. */
    fun request(groupId: String, did: String): Result<String> =
        runCatching { api.chatRequest(groupId, did) }.map { "Request sent to ${firstName(it.peerName.ifBlank { "them" })}. It waits in Chats until they answer." }
            .also { reloadRequests() }.mapError()

    fun accept(requestId: String): Result<String> = runCatching { api.chatAccept(requestId) }
        .map { "Accepted. ${firstName(it.peerName.ifBlank { "They" })} is added for messages only and shows in Chats once their device connects; calls stay off until you allow them." }
        .also { reloadRequests() }.mapError()

    fun decline(requestId: String): Result<String> = runCatching { api.chatDecline(requestId) }
        .map { "Declined. ${firstName(it.peerName.ifBlank { "They" })} is told “declined”." }.also { reloadRequests() }.mapError()

    /** Block (P5: they are told) and decline. */
    fun blockRequester(requestId: String, did: String): Result<String> = runCatching { api.block(did); api.chatDecline(requestId) }
        .map { "${firstName(it.peerName.ifBlank { "They" })} is blocked and can’t send more requests." }.also { reloadRequests() }.mapError()

    override fun onChatRequest(request: GroupChatRequest) {
        reloadRequests()
        if (request.outgoing && request.state == ChatRequestState.ACCEPTED && request.contactAdded)
            _notice.value = "${firstName(request.peerName.ifBlank { "They" })} accepted your request. You can message each other now; calls stay off until one of you allows them."
    }

    private fun act(ok: String, f: () -> Unit): Result<String> = runCatching { f(); ok }.also { refresh() }.mapError()

    private fun <T> Result<T>.mapError(): Result<T> = fold({ Result.success(it) }, { Result.failure(GroupFailure(GroupText.error(it))) })

    // ---- core events (core threads: never block) ----

    override fun onGroupMessage(message: GroupMessage) {
        synchronized(this) {
            _open.value?.let { o ->
                if (o.id == message.groupId) {
                    _open.value = o.copy(messages = upsertGroupMessage(o.messages, message))
                    try { api.markRead(o.id) } catch (_: Exception) {}
                }
            }
            _list.value = try { api.groups() } catch (e: Exception) { _list.value }
        }
    }

    override fun onGroupChanged(groupId: String) {
        synchronized(this) {
            _list.value = try { api.groups() } catch (e: Exception) { _list.value }
            if (_open.value?.id == groupId) reloadOpen()
        }
    }

    fun attach(node: Node) {
        try { node.setGroupEvents(this); node.setGroupRequestEvents(this) } catch (e: Exception) { Log.w(P2pApp.TAG, "setGroupEvents: $e") }
    }
}

/** A failed group action: [message] is already in the user's words. */
class GroupFailure(message: String) : Exception(message)
