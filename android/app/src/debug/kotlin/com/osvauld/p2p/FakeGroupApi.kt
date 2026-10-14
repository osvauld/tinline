package com.osvauld.p2p

import uniffi.p2pcore.ChatRequestState
import uniffi.p2pcore.GroupChatRequest
import uniffi.p2pcore.GroupDetail
import uniffi.p2pcore.GroupHost
import uniffi.p2pcore.GroupInvitation
import uniffi.p2pcore.GroupInvitePeek
import uniffi.p2pcore.GroupItemKind
import uniffi.p2pcore.GroupMessage
import uniffi.p2pcore.GroupPage
import uniffi.p2pcore.GroupSummary
import uniffi.p2pcore.GroupDelivery as CoreDelivery
import uniffi.p2pcore.GroupMember as CoreMember
import uniffi.p2pcore.GroupRole as CoreRole

/**
 * Debug-only groups backend for the screen gallery (`--es screen group_chat`): the playground seed's "Weekend hike"
 * (seed.ts), so the real GroupChatLive renders with the same people and posts. Never shipped (debug source set).
 */
class FakeGroupApi(private val me: String, private val waiting: Boolean = false) : GroupApi {
    private fun did(id: String) = "did:key:z6Mkpg$id"
    private val names = mapOf("meera" to "Meera Nair", "arjun" to "Arjun Oommen", "sam" to "Sam Rao", "priya" to "Priya Menon", "kavya" to "Kavya Iyer")
    private fun at(h: Int, m: Int, daysAgo: Int): ULong {
        val z = java.time.ZoneId.systemDefault()
        return java.time.LocalDate.now(z).minusDays(daysAgo.toLong()).atTime(h, m).atZone(z).toInstant().toEpochMilli().toULong()
    }
    private var n = 0
    private fun post(from: String, at: ULong, text: String, kind: GroupItemKind = GroupItemKind.POST): GroupMessage {
        val mine = from == "me"
        val d = if (mine) me else did(from)
        val name = if (mine) "Maya Kurian" else names[from] ?: from
        return GroupMessage("hike-${n++}", "hike", kind, d, name, d, name, at, text, null, mine, if (mine && waiting) CoreDelivery.WAITING else CoreDelivery.DELIVERED, false)
    }
    private val msgs = mutableListOf(
        post("sam", at(18, 0, 8), "Trail map and the parking details are in the shared folder."),
        post("priya", at(9, 30, 5), "Anyone have a spare headlamp for Saturday? Mine died last time."),
        post("kavya", at(9, 2, 0), "Kavya joined", GroupItemKind.JOINED),
        post("sam", at(12, 30, 0), "Saturday 7am at the trailhead."),
        post("sam", at(12, 31, 0), "Bring rain jackets, the forecast says showers after 10."),
        post("sam", at(12, 31, 0) + 1u, "And snacks for the top."),
        post("me", at(12, 33, 0), "I can bring the stove."),
        post("priya", at(12, 38, 0), "Can someone give me a lift from the station?"),
        post("kavya", at(12, 52, 0), "First hike with you all. Is the loop fine for beginners?"),
        post("sam", at(12, 55, 0), "It is. Easy grade, one steep bit near the top."),
    )
    private val members = names.map { (id, name) -> CoreMember(did(id), name, false, id == "meera", id == "sam" || id == "priya", id == "arjun", 0u, 1uL, null, listOf("dev-$id")) } +
        CoreMember(me, "Maya Kurian", true, false, true, false, 0u, 1uL, null, listOf("dev-me"))
    private val host = GroupHost(did("arjun"), "Arjun Oommen", "a1b2", null, false, !waiting)
    private val detail = GroupDetail("hike", "Weekend hike", did("meera"), CoreRole.ADMIN, false, members, listOf(host), emptyList(), emptyList(), false, null, false, listOf(host), emptyList(), false)
    private var unread = 3u

    override fun groups() = listOf(GroupSummary("hike", "Weekend hike", CoreRole.ADMIN, members.size.toUInt(), 1u, false, !waiting, msgs.last(), unread))
    override fun detail(id: String) = detail
    override fun messages(id: String, before: String?, limit: Int) = GroupPage(msgs.toList(), null)
    override fun markRead(id: String) { unread = 0u }
    override fun create(name: String) = "new"
    override fun invite(id: String) = "tinline://g/$id"
    override fun peek(token: String): GroupInvitePeek? = null
    override fun join(token: String): String = "hike"
    override fun post(id: String, text: String, replyTo: String?): GroupMessage = post("me", (System.currentTimeMillis()).toULong(), text).also { msgs += it }
    override fun flag(id: String, did: String) {}
    override fun setAdmin(id: String, did: String, admin: Boolean) {}
    override fun leave(id: String) {}
    override fun appointHost(id: String, did: String, deviceId: String) {}
    override fun revokeHost(id: String, did: String, deviceId: String) {}
    override fun acceptHost(id: String, accept: Boolean) {}
    override fun deviceId() = "aa".repeat(32)
    override fun chatRequest(id: String, did: String) = GroupChatRequest("r0", id, "Weekend hike", did, "", true, ChatRequestState.PENDING, 1uL, false)
    override fun requestsIncoming() = emptyList<GroupChatRequest>()
    override fun requestsOutgoing() = emptyList<GroupChatRequest>()
    override fun chatAccept(requestId: String) = chatRequest("hike", "")
    override fun chatDecline(requestId: String) = chatRequest("hike", "")
    override fun block(did: String) {}
    override fun inviteContacts(id: String, dids: List<String>) = emptyList<GroupInvitation>()
    override fun invitationsIncoming() = emptyList<GroupInvitation>()
    override fun invitationsOutgoing(id: String) = emptyList<GroupInvitation>()
    override fun invitationAccept(invitationId: String): GroupInvitation = throw UnsupportedOperationException()
    override fun invitationDecline(invitationId: String): GroupInvitation = throw UnsupportedOperationException()
}
