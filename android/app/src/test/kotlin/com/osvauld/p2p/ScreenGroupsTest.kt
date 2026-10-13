package com.osvauld.p2p

import org.junit.Assert.*
import org.junit.Test
import uniffi.p2pcore.GroupDetail
import uniffi.p2pcore.GroupMember
import uniffi.p2pcore.GroupRole
import uniffi.p2pcore.GroupChatRequest
import uniffi.p2pcore.ChatRequestState

class ScreenGroupsTest {
    private fun member(owner: Boolean = false, me: Boolean = false) =
        GroupMember(if (owner) "owner" else "member", "Sam", me, owner, owner, false, 0u, 1uL, null, listOf("device"))
    private fun detail(role: GroupRole) = GroupDetail("g", "Hike", "owner", role, false,
        listOf(member(owner = true), member()), emptyList(), emptyList(), emptyList(), false, null,
        false, emptyList(), emptyList(), false)

    @Test fun rolesExposeOnlySupportedActions() {
        assertTrue(MemberAction.MakeAdmin in GroupText.actions(detail(GroupRole.OWNER), member()))
        assertFalse(MemberAction.MakeAdmin in GroupText.actions(detail(GroupRole.ADMIN), member()))
        assertFalse(MemberAction.Flag in GroupText.actions(detail(GroupRole.ADMIN), member(owner = true)))
        assertTrue(GroupText.actions(detail(GroupRole.REMOVED), member()).isEmpty())
        assertTrue(GroupText.actions(detail(GroupRole.OWNER), member(me = true)).isEmpty())
    }

    @Test fun noOnlineHostDoesNotPromiseDelivery() {
        assertTrue(GroupText.waitingForHost(detail(GroupRole.MEMBER)))
        assertFalse(GroupText.waitingForHost(detail(GroupRole.REMOVED)))
        assertTrue(GroupText.status(detail(GroupRole.MEMBER)).contains("no host"))
    }

    @Test fun flagConfirmationExplainsFinalFlag() {
        val (next, copy) = GroupText.flagCopy("Sam Rao", 2)
        assertEquals(3, next)
        assertTrue(copy.contains("removed right away"))
    }

    @Test fun declinedAndBlockedRequestsAreNotPresentedAsPending() {
        val r = GroupChatRequest("r", "g", "Hike", "sam", "Sam Rao", true, ChatRequestState.DECLINED, 1uL, false)
        assertEquals("Sam declined" to true, GroupText.sentLine(r))
        assertEquals("Sam blocked you" to true, GroupText.sentLine(r.copy(state = ChatRequestState.BLOCKED)))
    }
}
