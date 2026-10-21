package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.p2pcore.GroupDetail
import uniffi.p2pcore.GroupItemKind
import uniffi.p2pcore.GroupMember
import uniffi.p2pcore.GroupMessage
import uniffi.p2pcore.GroupPage
import uniffi.p2pcore.GroupRole
import uniffi.p2pcore.OnlineMember
import uniffi.p2pcore.GroupDelivery as CoreDelivery

/** Public groups on Android (docs/design/public-group.md §8): the words, the online order, and who may post. */
class GroupsPublicTest {
    private val me = "did:key:me"

    private fun member(did: String, owner: Boolean = false, admin: Boolean = owner) =
        GroupMember(did, did.removePrefix("did:key:"), did == me, owner, admin, false, 0u, 1uL, null, listOf("dev-$did"))

    private fun detail(role: GroupRole, open: Boolean = false, adminsOnly: Boolean = false) = GroupDetail("g", "Lobby", "did:key:own", role, false,
        listOf(member("did:key:own", owner = true), member(me, admin = role == GroupRole.ADMIN)), emptyList(), emptyList(), emptyList(), false, null,
        false, emptyList(), emptyList(), false, open = open, adminsOnly = adminsOnly)

    @Test fun theOnlineLabelSaysPlusAtTheCap() {
        assertEquals("0 online", GroupText.onlineLabel(0))
        assertEquals("12 online", GroupText.onlineLabel(12))
        assertEquals("499 online", GroupText.onlineLabel(499))
        // A host frame stops at 500 DIDs, so exactly 500 may already be cut short.
        assertEquals("500+ online", GroupText.onlineLabel(500))
        assertEquals("500+ online", GroupText.onlineLabel(731))
    }

    @Test fun onlyAnOpenGroupImInShowsWhoIsOnline() {
        assertTrue(GroupText.showsOnline(detail(GroupRole.MEMBER, open = true)))
        assertFalse(GroupText.showsOnline(detail(GroupRole.MEMBER)))
        assertFalse(GroupText.showsOnline(detail(GroupRole.LEFT, open = true)))
        assertFalse(GroupText.showsOnline(detail(GroupRole.REMOVED, open = true)))
    }

    @Test fun adminsComeFirstThenNamesIgnoringCase() {
        val list = listOf(OnlineMember("d4", "zoe", false), OnlineMember("d2", "Bea", true), OnlineMember("d3", "amir", false),
            OnlineMember("d1", "Ana", true), OnlineMember("d0", "amir", false))
        assertEquals(listOf("d1", "d2", "d0", "d3", "d4"), GroupText.onlineRows(list).map { it.did })
    }

    @Test fun onlyAMemberOfAnAdminsOnlyGroupLosesTheComposer() {
        assertTrue(GroupText.adminsOnlyLocked(detail(GroupRole.MEMBER, adminsOnly = true)))
        assertFalse(GroupText.adminsOnlyLocked(detail(GroupRole.ADMIN, adminsOnly = true)))
        assertFalse(GroupText.adminsOnlyLocked(detail(GroupRole.OWNER, adminsOnly = true)))
        assertFalse(GroupText.adminsOnlyLocked(detail(GroupRole.MEMBER)))
        // Left or removed members have their own footer.
        assertFalse(GroupText.adminsOnlyLocked(detail(GroupRole.LEFT, adminsOnly = true)))
    }

    @Test fun theCoresAdminsOnlyRefusalReadsLikeTheComposerLine() {
        assertEquals("Only admins post here.", GroupText.error(uniffi.p2pcore.Exception.Rejected("they refused: admins only")))
    }

    /** Only what these tests use; every other call fails loudly. */
    private open class Fake(var d: GroupDetail) : GroupApi by unused() {
        var onlineNow = listOf<OnlineMember>()
        var link = "tinline://g/open1"
        var posts = 0
        override fun groups() = emptyList<uniffi.p2pcore.GroupSummary>()
        override fun detail(id: String) = d
        override fun messages(id: String, before: String?, limit: Int) = GroupPage(emptyList(), null)
        override fun markRead(id: String) {}
        override fun deviceId() = "dev"
        override fun requestsIncoming() = emptyList<uniffi.p2pcore.GroupChatRequest>()
        override fun requestsOutgoing() = emptyList<uniffi.p2pcore.GroupChatRequest>()
        override fun invitationsIncoming() = emptyList<uniffi.p2pcore.GroupInvitation>()
        override fun invitationsOutgoing(id: String) = emptyList<uniffi.p2pcore.GroupInvitation>()
        override fun online(id: String) = onlineNow
        override fun openLink(id: String): String { d = d.copy(open = true); return link }
        override fun setOpen(id: String, open: Boolean) { d = d.copy(open = open) }
        override fun post(id: String, text: String, replyTo: String?): GroupMessage {
            posts++
            return GroupMessage("p$posts", id, GroupItemKind.POST, "did:key:me", "Me", "did:key:me", "Me", 1uL, text, replyTo, true, CoreDelivery.WAITING, false)
        }
    }

    private companion object {
        fun unused(): GroupApi = java.lang.reflect.Proxy.newProxyInstance(GroupApi::class.java.classLoader, arrayOf(GroupApi::class.java)) { _, m, _ ->
            throw UnsupportedOperationException(m.name)
        } as GroupApi
    }

    @Test fun presenceEventsRefreshTheOnlineListOfTheGroupOnScreen() {
        val api = Fake(detail(GroupRole.MEMBER, open = true))
        val ctl = GroupsController(api) { me }
        ctl.open("g")
        assertTrue(ctl.open.value!!.online.isEmpty())
        api.onlineNow = listOf(OnlineMember("did:key:own", "own", true))
        ctl.onGroupPresence("other")
        assertTrue(ctl.open.value!!.online.isEmpty())
        ctl.onGroupPresence("g")
        assertEquals(listOf("did:key:own"), ctl.open.value!!.online.map { it.did })
        // A private group has no list, whatever the core says.
        api.d = api.d.copy(open = false)
        ctl.refresh()
        assertTrue(ctl.open.value!!.online.isEmpty())
    }

    @Test fun aMemberOfAnAdminsOnlyGroupIsRefusedBeforeTheCore() {
        val api = Fake(detail(GroupRole.MEMBER, adminsOnly = true))
        val ctl = GroupsController(api) { me }
        ctl.open("g")
        assertEquals("Only admins post here.", ctl.post("g", "hi", null).exceptionOrNull()!!.message)
        assertEquals(0, api.posts)
        api.d = api.d.copy(adminsOnly = false)
        ctl.refresh()
        assertTrue(ctl.post("g", "hi", null).isSuccess)
    }

    @Test fun theOpenLinkIsKeptUntilTheGroupCloses() {
        val api = Fake(detail(GroupRole.ADMIN))
        val ctl = GroupsController(api) { me }
        ctl.open("g")
        assertEquals("tinline://g/open1", ctl.openLink("g").getOrThrow())
        assertEquals("g" to "tinline://g/open1", ctl.openLink.value)
        assertTrue(ctl.open.value!!.detail.open)
        assertTrue(ctl.closeLink("g").isSuccess)
        assertNull(ctl.openLink.value)
        assertFalse(ctl.open.value!!.detail.open)
    }
}
