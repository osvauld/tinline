package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.p2pcore.GroupDetail
import uniffi.p2pcore.GroupImage
import uniffi.p2pcore.GroupImageState
import uniffi.p2pcore.GroupItemKind
import uniffi.p2pcore.GroupMember
import uniffi.p2pcore.GroupMessage
import uniffi.p2pcore.GroupPage
import uniffi.p2pcore.GroupRole
import uniffi.p2pcore.GroupDelivery as CoreDelivery

/** Image posts on Android (docs/design/public-group.md §5): the words, posting a picture, tap to load, and the image event. */
class GroupsImagesTest {
    private val me = "did:key:me"

    private fun member(did: String, owner: Boolean = false) =
        GroupMember(did, did.removePrefix("did:key:"), did == me, owner, owner, false, 0u, 1uL, null, listOf("dev-$did"))

    private fun detail(role: GroupRole, adminsOnly: Boolean = false) = GroupDetail("g", "Lobby", "did:key:own", role, false,
        listOf(member("did:key:own", owner = true), member(me)), emptyList(), emptyList(), emptyList(), false, null,
        false, emptyList(), emptyList(), false, open = true, adminsOnly = adminsOnly)

    private fun image(state: GroupImageState) = GroupImage(1080u, 720u, 200_000uL, "image/jpeg", ByteArray(36), state)

    private fun post(id: String, text: String, image: GroupImage?, outgoing: Boolean = false) =
        GroupMessage(id, "g", GroupItemKind.POST, if (outgoing) me else "did:key:sam", if (outgoing) "Me" else "Sam Lee",
            if (outgoing) me else "did:key:sam", "Sam Lee", 1uL, text, null, outgoing, CoreDelivery.DELIVERED, null, false, image)

    @Test fun aPictureReadsAsPhotoInOneLine() {
        assertEquals("Photo", GroupText.postLine(post("a", "", image(GroupImageState.READY))))
        assertEquals("Photo · the view", GroupText.postLine(post("a", "the view", image(GroupImageState.REMOTE))))
        assertEquals("hello", GroupText.postLine(post("a", "hello", null)))
    }

    private class Fake(var d: GroupDetail) : GroupApi by unused() {
        var msgs = listOf<GroupMessage>()
        val fetched = mutableListOf<String>()
        var posted: Pair<Int, String>? = null
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
        override fun postImage(id: String, bytes: ByteArray, caption: String): GroupMessage {
            posted = bytes.size to caption
            return GroupMessage("p1", id, GroupItemKind.POST, "did:key:me", "Me", "did:key:me", "Me", 2uL, caption, null, true, CoreDelivery.WAITING, null, false,
                GroupImage(1080u, 720u, 200_000uL, "image/jpeg", ByteArray(0), GroupImageState.READY))
        }
        override fun fetchImage(id: String, messageId: String) { fetched += messageId }
        override fun imagePath(id: String, messageId: String) = "/pictures/$messageId.jpg"
    }

    private companion object {
        fun unused(): GroupApi = java.lang.reflect.Proxy.newProxyInstance(GroupApi::class.java.classLoader, arrayOf(GroupApi::class.java)) { _, m, _ ->
            throw UnsupportedOperationException(m.name)
        } as GroupApi
    }

    @Test fun aPicturePostsWithItsCaptionAndShowsAtOnce() {
        val api = Fake(detail(GroupRole.MEMBER))
        val ctl = GroupsController(api) { me }
        ctl.open("g")
        val m = ctl.postImage("g", ByteArray(10), "  the view ").getOrThrow()
        assertEquals(10 to "the view", api.posted)
        assertEquals(listOf(m.id), ctl.open.value!!.messages.map { it.id })
        assertTrue(ctl.postImage("g", ByteArray(0), "").isFailure)
    }

    @Test fun aMemberOfAnAdminsOnlyGroupCannotPostAPicture() {
        val api = Fake(detail(GroupRole.MEMBER, adminsOnly = true))
        val ctl = GroupsController(api) { me }
        ctl.open("g")
        assertEquals("Only admins post here.", ctl.postImage("g", ByteArray(10), "").exceptionOrNull()!!.message)
        assertEquals(null, api.posted)
    }

    @Test fun aTapLoadsAndTheImageEventRereadsTheGroupOnScreen() {
        val api = Fake(detail(GroupRole.MEMBER))
        api.msgs = listOf(post("m1", "", image(GroupImageState.REMOTE)))
        val ctl = GroupsController(api) { me }
        ctl.open("g")
        assertTrue(ctl.fetchImage("g", "m1").isSuccess)
        assertEquals(listOf("m1"), api.fetched)
        api.msgs = listOf(post("m1", "", image(GroupImageState.READY)))
        ctl.onGroupImage("other", "m1")
        assertEquals(GroupImageState.REMOTE, ctl.open.value!!.messages.single().image!!.state)
        ctl.onGroupImage("g", "m1")
        assertEquals(GroupImageState.READY, ctl.open.value!!.messages.single().image!!.state)
        assertEquals("/pictures/m1.jpg", ctl.imagePath("g", "m1"))
    }
}
