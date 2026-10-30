package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.p2pcore.GroupDetail
import uniffi.p2pcore.GroupFile
import uniffi.p2pcore.GroupFileHolder
import uniffi.p2pcore.GroupImageState
import uniffi.p2pcore.GroupItemKind
import uniffi.p2pcore.GroupMember
import uniffi.p2pcore.GroupMessage
import uniffi.p2pcore.GroupPage
import uniffi.p2pcore.GroupRole
import uniffi.p2pcore.GroupDelivery as CoreDelivery

/** File posts in groups (docs/design/group-files.md): the one-line words, the holder line, posting, download and delete. */
class GroupsFilesTest {
    private val me = "did:key:me"

    private fun member(did: String, owner: Boolean = false) =
        GroupMember(did, did.removePrefix("did:key:"), did == me, owner, owner, false, 0u, 1uL, null, listOf("dev-$did"))

    private fun detail(role: GroupRole, adminsOnly: Boolean = false) = GroupDetail("g", "Lobby", "did:key:own", role, false,
        listOf(member("did:key:own", owner = true), member(me)), emptyList(), emptyList(), emptyList(), false, null,
        false, emptyList(), emptyList(), false, open = false, adminsOnly = adminsOnly)

    private fun holder(did: String, name: String) = GroupFileHolder(did, name)

    private fun file(state: GroupImageState, holders: List<GroupFileHolder> = emptyList()) =
        GroupFile("Trip plan.pdf", 300_000uL, "application/pdf", state, 0uL, true, holders)

    private fun post(id: String, f: GroupFile?, outgoing: Boolean = false, caption: String = "") =
        GroupMessage(id, "g", GroupItemKind.POST, if (outgoing) me else "did:key:sam", if (outgoing) "Me" else "Sam Lee",
            if (outgoing) me else "did:key:sam", "Sam Lee", 1uL, caption, null, outgoing, CoreDelivery.DELIVERED, null, false, null, null, f)

    @Test fun aFilePostReadsAsFileAndItsNameInOneLine() {
        assertEquals(appString(R.string.groupsc_file_line, "Trip plan.pdf"), GroupText.postLine(post("a", file(GroupImageState.REMOTE))))
    }

    @Test fun theHolderLineNamesOthersNeverMe() {
        val name: (GroupFileHolder) -> String = { it.name }
        assertNull("only me", GroupText.holdersLine(file(GroupImageState.READY, listOf(holder(me, "Me"))), me, name))
        assertNull("nobody", GroupText.holdersLine(file(GroupImageState.READY), me, name))
        assertEquals(appString(R.string.groupsc_file_also_hosting, "Anoop"),
            GroupText.holdersLine(file(GroupImageState.READY, listOf(holder(me, "Me"), holder("did:key:a", "Anoop K"))), me, name))
        assertEquals(appPlural(R.plurals.groupsc_file_holders, 2, "Anoop", 2),
            GroupText.holdersLine(file(GroupImageState.REMOTE, listOf(holder("did:key:a", "Anoop K"), holder(me, "Me"), holder("did:key:b", "Bea"), holder("did:key:c", "Cy"))), me, name))
    }

    private class Fake(var d: GroupDetail) : GroupApi by unused() {
        var msgs = listOf<GroupMessage>()
        val calls = mutableListOf<String>()
        var posted: List<String>? = null
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
        override fun postFile(id: String, path: String, name: String, mime: String, caption: String): GroupMessage {
            posted = listOf(path, name, mime, caption)
            return GroupMessage("f1", id, GroupItemKind.POST, ME, "Me", ME, "Me", 2uL, caption, null, true, CoreDelivery.WAITING, null, false,
                null, null, GroupFile(name, 10uL, mime, GroupImageState.READY, 0uL, true, listOf(GroupFileHolder(ME, "Me"))))
        }
        override fun fetchFile(id: String, messageId: String) { calls += "fetch $messageId" }
        override fun cancelFile(id: String, messageId: String) { calls += "cancel $messageId" }
        override fun deleteFileCopy(id: String, messageId: String) { calls += "delete $messageId" }
        override fun filePath(id: String, messageId: String) = "/files/$messageId"
    }

    private companion object {
        const val ME = "did:key:me"
        fun unused(): GroupApi = java.lang.reflect.Proxy.newProxyInstance(GroupApi::class.java.classLoader, arrayOf(GroupApi::class.java)) { _, m, _ ->
            throw UnsupportedOperationException(m.name)
        } as GroupApi
    }

    @Test fun aPickedFilePostsWithItsNameAndCaptionAndShowsAtOnce() {
        val api = Fake(detail(GroupRole.MEMBER))
        val ctl = GroupsController(api) { me }
        ctl.open("g")
        val m = ctl.postFile("g", "/cache/chat_out/x/Trip_plan.pdf", "Trip plan.pdf", "application/pdf", "  the plan ").getOrThrow()
        assertEquals(listOf("/cache/chat_out/x/Trip_plan.pdf", "Trip plan.pdf", "application/pdf", "the plan"), api.posted)
        assertEquals(listOf(m.id), ctl.open.value!!.messages.map { it.id })
        assertEquals(GroupImageState.READY, ctl.open.value!!.messages.single().file!!.state)
    }

    @Test fun aMemberOfAnAdminsOnlyGroupCannotPostAFile() {
        val api = Fake(detail(GroupRole.MEMBER, adminsOnly = true))
        val ctl = GroupsController(api) { me }
        ctl.open("g")
        assertEquals(GroupText.ADMINS_ONLY_ERROR, ctl.postFile("g", "/f", "f.txt", "text/plain", "").exceptionOrNull()!!.message)
        assertNull(api.posted)
    }

    @Test fun downloadCancelDeleteGoToTheCoreAndTheImageEventRereadsTheCard() {
        val api = Fake(detail(GroupRole.MEMBER))
        api.msgs = listOf(post("m1", file(GroupImageState.REMOTE)))
        val ctl = GroupsController(api) { me }
        ctl.open("g")
        assertTrue(ctl.fetchFile("g", "m1").isSuccess)
        assertTrue(ctl.cancelFile("g", "m1").isSuccess)
        assertTrue(ctl.deleteFileCopy("g", "m1").isSuccess)
        assertEquals(listOf("fetch m1", "cancel m1", "delete m1"), api.calls)
        api.msgs = listOf(post("m1", file(GroupImageState.LOADING)))
        ctl.onGroupImage("g", "m1")
        assertEquals(GroupImageState.LOADING, ctl.open.value!!.messages.single().file!!.state)
        assertEquals("/files/m1", ctl.filePath("g", "m1"))
    }
}
