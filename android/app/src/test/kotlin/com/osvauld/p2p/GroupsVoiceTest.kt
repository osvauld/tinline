package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.p2pcore.GroupDetail
import uniffi.p2pcore.GroupImageState
import uniffi.p2pcore.GroupItemKind
import uniffi.p2pcore.GroupMember
import uniffi.p2pcore.GroupMessage
import uniffi.p2pcore.GroupPage
import uniffi.p2pcore.GroupRole
import uniffi.p2pcore.GroupVoice
import uniffi.p2pcore.GroupDelivery as CoreDelivery

/** Voice posts in groups: the words, posting a recording, tap to load, and the (shared) image event. */
class GroupsVoiceTest {
    private val me = "did:key:me"

    private fun member(did: String, owner: Boolean = false) =
        GroupMember(did, did.removePrefix("did:key:"), did == me, owner, owner, false, 0u, 1uL, null, listOf("dev-$did"))

    private fun detail(role: GroupRole, adminsOnly: Boolean = false) = GroupDetail("g", "Lobby", "did:key:own", role, false,
        listOf(member("did:key:own", owner = true), member(me)), emptyList(), emptyList(), emptyList(), false, null,
        false, emptyList(), emptyList(), false, open = true, adminsOnly = adminsOnly)

    private fun voice(state: GroupImageState) = GroupVoice(2_000u, ByteArray(64) { it.toByte() }, 6_000uL, state)

    private fun post(id: String, v: GroupVoice?, outgoing: Boolean = false) =
        GroupMessage(id, "g", GroupItemKind.POST, if (outgoing) me else "did:key:sam", if (outgoing) "Me" else "Sam Lee",
            if (outgoing) me else "did:key:sam", "Sam Lee", 1uL, "", null, outgoing, CoreDelivery.DELIVERED, null, false, null, v)

    @Test fun aVoicePostReadsAsVoiceMessageInOneLine() {
        assertEquals(appString(R.string.chat_preview_voice), GroupText.postLine(post("a", voice(GroupImageState.READY))))
        assertEquals(appString(R.string.chat_preview_voice), GroupText.postLine(post("a", voice(GroupImageState.REMOTE))))
    }

    private class Fake(var d: GroupDetail) : GroupApi by unused() {
        var msgs = listOf<GroupMessage>()
        val fetched = mutableListOf<String>()
        var posted: Pair<String, Int>? = null
        override fun groups() = emptyList<uniffi.p2pcore.GroupSummary>()
        override fun detail(id: String) = d
        override fun messages(id: String, before: String?, limit: Int) = GroupPage(msgs, null)
        override fun markRead(id: String) {}
        override fun deviceId() = "dev"
        override fun requestsIncoming() = emptyList<uniffi.p2pcore.GroupChatRequest>()
        override fun requestsOutgoing() = emptyList<uniffi.p2pcore.GroupChatRequest>()
        override fun invitationsIncoming() = emptyList<uniffi.p2pcore.GroupInvitation>()
        override fun invitationsOutgoing(id: String) = emptyList<uniffi.p2pcore.GroupInvitation>()
        override fun online(id: String) = emptyList<uniffi.p2pcore.OnlineMember>()
        override fun receipts(id: String, fromMs: ULong, toMs: ULong) = emptyList<uniffi.p2pcore.GroupReceiptStatus>()
        override fun postVoice(id: String, path: String, waveform: ByteArray): GroupMessage {
            posted = path to waveform.size
            return GroupMessage("v1", id, GroupItemKind.POST, "did:key:me", "Me", "did:key:me", "Me", 2uL, "", null, true, CoreDelivery.WAITING, null, false,
                null, GroupVoice(2_000u, waveform, 6_000uL, GroupImageState.READY))
        }
        override fun fetchVoice(id: String, messageId: String) { fetched += messageId }
        override fun voicePath(id: String, messageId: String) = "/voice/$messageId.ogg"
    }

    private companion object {
        fun unused(): GroupApi = java.lang.reflect.Proxy.newProxyInstance(GroupApi::class.java.classLoader, arrayOf(GroupApi::class.java)) { _, m, _ ->
            throw UnsupportedOperationException(m.name)
        } as GroupApi
    }

    @Test fun aRecordingPostsAndShowsAtOnce() {
        val api = Fake(detail(GroupRole.MEMBER))
        val ctl = GroupsController(api) { me }
        ctl.open("g")
        val m = ctl.postVoice("g", "/cache/voice/rec.opus", ByteArray(64)).getOrThrow()
        assertEquals("/cache/voice/rec.opus" to 64, api.posted)
        assertEquals(listOf(m.id), ctl.open.value!!.messages.map { it.id })
        assertEquals(GroupImageState.READY, ctl.open.value!!.messages.single().voice!!.state)
    }

    @Test fun aMemberOfAnAdminsOnlyGroupCannotPostAVoiceMessage() {
        val api = Fake(detail(GroupRole.MEMBER, adminsOnly = true))
        val ctl = GroupsController(api) { me }
        ctl.open("g")
        assertEquals(GroupText.ADMINS_ONLY_ERROR, ctl.postVoice("g", "/r.opus", ByteArray(0)).exceptionOrNull()!!.message)
        assertEquals(null, api.posted)
    }

    @Test fun aTapLoadsAndTheImageEventRereadsTheVoicePost() {
        val api = Fake(detail(GroupRole.MEMBER))
        api.msgs = listOf(post("m1", voice(GroupImageState.REMOTE)))
        val ctl = GroupsController(api) { me }
        ctl.open("g")
        assertTrue(ctl.fetchVoice("g", "m1").isSuccess)
        assertEquals(listOf("m1"), api.fetched)
        api.msgs = listOf(post("m1", voice(GroupImageState.READY)))
        ctl.onGroupImage("g", "m1")
        assertEquals(GroupImageState.READY, ctl.open.value!!.messages.single().voice!!.state)
        assertEquals("/voice/m1.ogg", ctl.voicePath("g", "m1"))
    }
}
