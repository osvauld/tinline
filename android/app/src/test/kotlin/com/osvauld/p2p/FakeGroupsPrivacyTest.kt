package com.osvauld.p2p

import com.osvauld.p2p.FakeGroupsPrivacy.Companion.ARJUN
import com.osvauld.p2p.FakeGroupsPrivacy.Companion.DAY
import com.osvauld.p2p.FakeGroupsPrivacy.Companion.DEV
import com.osvauld.p2p.FakeGroupsPrivacy.Companion.FAMILY
import com.osvauld.p2p.FakeGroupsPrivacy.Companion.HIKE
import com.osvauld.p2p.FakeGroupsPrivacy.Companion.HOUR
import com.osvauld.p2p.FakeGroupsPrivacy.Companion.LEO
import com.osvauld.p2p.FakeGroupsPrivacy.Companion.MEERA
import com.osvauld.p2p.FakeGroupsPrivacy.Companion.PRIYA
import com.osvauld.p2p.FakeGroupsPrivacy.Companion.SAM
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.time.ZoneOffset

/** The PLACEHOLDER groups & privacy state machine: deterministic under a fixed clock. */
class FakeGroupsPrivacyTest {
    private var now = 1_791_640_200_000L // 2026-10-10T14:30Z
    private lateinit var f: FakeGroupsPrivacy

    @Before fun setUp() { f = FakeGroupsPrivacy(clock = { now }, myName = "Maya", zone = ZoneOffset.UTC) }

    private fun hike() = f.groups.value.first { it.id == HIKE }
    private fun leo() = hike().members.first { it.did == LEO }
    private fun person(did: String) = f.people.value.first { it.did == did }

    // ---------------------------------------------------------------- seed

    @Test fun seedMatchesBoards() {
        assertEquals(listOf("Weekend hike", "Family", "Book club"), f.groups.value.map { it.name })
        assertEquals(8, hike().activeMembers.size)
        assertTrue(hike().hostReachable)
        assertFalse(f.groups.value.first { it.id == FAMILY }.hostReachable)
        assertNotNull(f.groups.value.first { it.id == FakeGroupsPrivacy.BOOK }.removal)
        assertEquals(1, leo().flags.size)
        assertEquals(11, f.address.value.contactsTold)
        assertEquals(12, f.address.value.contactsTotal)
        assertEquals(AddressInterval.ThreeDays, f.address.value.interval)
    }

    @Test fun twoFakesAreIdentical() {
        val other = FakeGroupsPrivacy(clock = { now }, myName = "Maya", zone = ZoneOffset.UTC)
        assertEquals(f.groups.value, other.groups.value)
        assertEquals(f.people.value, other.people.value)
        assertEquals(f.requests.value, other.requests.value)
    }

    // ---------------------------------------------------------------- flags

    @Test fun threeFlagsRemoveTheMember() {
        assertEquals(Outcome.Ok, f.flagMember(HIKE, LEO))
        assertEquals(2, leo().flags.size)
        assertFalse(leo().removed)
        // The same admin flagging again counts.
        assertEquals(Outcome.Ok, f.flagMember(HIKE, LEO))
        assertTrue(leo().removed)
        assertEquals(7, hike().activeMembers.size)
        val sys = hike().items.filterIsInstance<GroupItem.Sys>().last()
        assertEquals("Leo was removed after 3 admin flags", sys.text)
        // His offline-written message still arrives, labelled.
        val late = hike().items.last() as GroupItem.Msg
        assertEquals(LEO, late.authorDid)
        assertTrue(late.writtenBeforeAuthorRemoved)
        // No fourth flag on a removed member.
        assertTrue(f.flagMember(HIKE, LEO) is Outcome.Refused)
    }

    @Test fun onlyAdminsFlagAndNotThemselves() {
        assertTrue(f.flagMember(HIKE, FakeGroupsPrivacy.ME) is Outcome.Refused)
        // In Book club I was removed: no admin powers.
        assertTrue(f.flagMember(FakeGroupsPrivacy.BOOK, SAM) is Outcome.Refused)
    }

    @Test fun removedMemberCannotSend() {
        assertTrue(f.sendGroupText(FakeGroupsPrivacy.BOOK, "hi") is Outcome.Refused)
    }

    // ---------------------------------------------------------------- hosts

    @Test fun messagesWaitForAHostAndGoWhenOneReturns() {
        f.simulateHostsOnline(HIKE, false)
        assertFalse(hike().hostReachable)
        f.sendGroupText(HIKE, "Who has the tent?")
        assertEquals(GroupDelivery.WaitingForHost, (hike().items.last() as GroupItem.Msg).delivery)
        f.simulateHostsOnline(HIKE, true)
        assertEquals(GroupDelivery.Sent, (hike().items.last() as GroupItem.Msg).delivery)
    }

    @Test fun removingAHostAnnouncesTheMove() {
        assertEquals(Outcome.Ok, f.removeHost(HIKE, "h-meera"))
        assertEquals(listOf("h-arjun"), hike().hosts.map { it.id })
        assertFalse(GroupRole.Host in hike().members.first { it.did == MEERA }.roles)
        assertTrue((hike().items.last() as GroupItem.Sys).text.contains("moving to new addresses"))
    }

    @Test fun askToHostIsPendingAndDoesNotCountForDelivery() {
        f.simulateHostsOnline(HIKE, false)
        assertEquals(Outcome.Ok, f.askToHost(HIKE, LEO))
        assertTrue(hike().hosts.last().pending)
        assertFalse(hike().hostReachable)
        assertTrue(f.askToHost(HIKE, LEO) is Outcome.Refused)
    }

    @Test fun createGroupWithoutHostWaits() {
        val id = f.createGroup("Trip", listOf(ARJUN), useMyDesktop = false)
        val g = f.groups.value.first { it.id == id }
        assertFalse(g.hostReachable)
        assertTrue(GroupRole.Owner in g.me!!.roles)
        // Arjun got an invite card in our 1:1 chat.
        assertTrue(person(ARJUN).items.last() is DmItem.Invite)
        val id2 = f.createGroup("Trip 2", emptyList(), useMyDesktop = true)
        val withHost = f.groups.value.first { it.id == id2 }
        assertTrue(withHost.hostReachable)
    }

    @Test fun joinInviteThenExpiredInvite() {
        assertEquals(Outcome.Ok, f.joinInvite("inv-trek"))
        assertEquals(InviteState.Joined, f.invites.value.first().state)
        assertTrue(f.groups.value.any { it.name == "Monsoon trek" })
        f.reset()
        now += 7 * DAY
        assertTrue(f.joinInvite("inv-trek") is Outcome.Refused)
        assertEquals(InviteState.Expired, f.invites.value.first().state)
    }

    // ---------------------------------------------------------------- blocking

    @Test fun blockIsPersonalAndUnblockRestores() {
        val before = hike().members
        assertEquals(Outcome.Ok, f.block(ARJUN, "Arjun Oommen"))
        assertTrue(f.isBlocked(ARJUN))
        // Group membership and roles untouched.
        assertEquals(before, hike().members)
        assertEquals(CallCheck.IBlocked, f.callCheck(ARJUN))
        assertTrue(f.sendText(ARJUN, "hi") is Outcome.Refused)
        // Excluded from address updates: 11 of 12 becomes 10 of 11.
        assertEquals(10, f.address.value.contactsTold)
        assertEquals(11, f.address.value.contactsTotal)
        f.simulateBlockedAttempt(ARJUN); f.simulateBlockedAttempt(ARJUN)
        assertEquals(2, person(ARJUN).attemptsSinceBlock)

        f.unblock(ARJUN)
        assertFalse(f.isBlocked(ARJUN))
        assertEquals(0, person(ARJUN).attemptsSinceBlock)
        assertEquals(CallCheck.Allowed, f.callCheck(ARJUN))
        assertEquals(12, f.address.value.contactsTotal)
    }

    @Test fun unblockRestoresCallsOnlyIfAllowedBefore() {
        f.setCanCall(ARJUN, false)
        f.block(ARJUN, "Arjun Oommen")
        f.unblock(ARJUN)
        assertFalse(person(ARJUN).canCall)
        f.setCanCall(ARJUN, true)
        f.block(ARJUN, "Arjun Oommen")
        f.unblock(ARJUN)
        assertTrue(person(ARJUN).canCall)
    }

    @Test fun blockedMeMessagesAreNotDelivered() {
        assertEquals(CallCheck.BlockedMe, f.callCheck(MEERA))
        assertTrue(f.sendText(MEERA, "Hello again") is Outcome.Refused)
        assertEquals(DmTick.NotDeliveredBlocked, (person(MEERA).items.last() as DmItem.Text).tick)
        f.simulateBlockedMe(MEERA, false)
        assertEquals(Outcome.Ok, f.sendText(MEERA, "Thanks"))
        assertEquals(DmTick.Delivered, (person(MEERA).items.last() as DmItem.Text).tick)
    }

    @Test fun cannotBlockTwiceOrSelf() {
        assertTrue(f.block(DEV, "Dev T") is Outcome.Refused)
        assertTrue(f.block(FakeGroupsPrivacy.ME, "Me") is Outcome.Refused)
    }

    // ---------------------------------------------------------------- requests

    @Test fun acceptRequestMakesAMessagesOnlyContact() {
        val did = f.acceptRequest("in-priya")
        assertEquals(PRIYA, did)
        val p = person(PRIYA)
        assertEquals(ContactOrigin.GroupRequest, p.origin)
        assertTrue(p.canMessage)
        assertFalse(p.canCall)
        assertFalse(p.theyAllowMyCalls)
        assertFalse(p.callsChipDone)
        assertEquals(CallCheck.TheyHaveNotAllowed, f.callCheck(PRIYA))
        assertEquals(IncomingRequestState.Accepted, f.requests.value.incoming.first().state)
        // The note is the first message.
        assertTrue(p.items.any { it is DmItem.Text && !it.outgoing && it.text.startsWith("Hi, it’s Priya") })
        // Accepting twice does nothing.
        assertNull(f.acceptRequest("in-priya"))
    }

    @Test fun declineAndBlockRequester() {
        f.declineRequest("in-priya")
        assertEquals(IncomingRequestState.Declined, f.requests.value.incoming.first().state)
        f.reset()
        f.blockRequester("in-priya")
        assertEquals(IncomingRequestState.Blocked, f.requests.value.incoming.first().state)
        assertEquals(BlockVia.ChatRequest, f.blocked.value.first { it.did == PRIYA }.via)
    }

    @Test fun sentRequestRateLimitAndExpiry() {
        // Leo was asked 2 h ago: refused today.
        assertTrue(f.sendRequest(LEO, "Leo B", "Weekend hike", "again") is Outcome.Refused)
        // Contacts can't be asked: open the chat instead.
        assertTrue(f.sendRequest(ARJUN, "Arjun Oommen", "Weekend hike", "") is Outcome.Refused)
        // Blocked people can't be asked.
        assertTrue(f.sendRequest(DEV, "Dev T", "Weekend hike", "") is Outcome.Refused)
        now += DAY
        f.expireDue()
        assertEquals(SentRequestState.Waiting, f.requests.value.sent.first { it.toDid == LEO }.state)
        now += 6 * DAY
        f.expireDue()
        assertEquals(SentRequestState.Expired, f.requests.value.sent.first { it.toDid == LEO }.state)
        // A day later a new request may go out, replacing the old row.
        assertEquals(Outcome.Ok, f.sendRequest(LEO, "Leo B", "Weekend hike", "hi"))
        assertEquals(1, f.requests.value.sent.count { it.toDid == LEO })
    }

    @Test fun requestAcceptedByOtherSide() {
        f.simulateRequestAnswered("req-leo", accept = true)
        assertTrue(f.requests.value.sent.none { it.id == "req-leo" })
        val leo = person(LEO)
        assertFalse(leo.canCall)
        assertEquals(CallCheck.TheyHaveNotAllowed, f.callCheck(LEO))
        assertEquals(Outcome.Ok, f.askToAllowCalls(LEO))
        assertTrue(person(LEO).items.last() is DmItem.CallAsk)
    }

    @Test fun cancelAndDismissRequests() {
        f.cancelRequest("req-leo")
        f.dismissRequest("req-dev")
        assertEquals(listOf("req-ravi"), f.requests.value.sent.map { it.id })
    }

    // ---------------------------------------------------------------- permissions

    @Test fun permissionToggles() {
        f.acceptRequest("in-priya")
        f.setCanCall(PRIYA, true)
        assertTrue(person(PRIYA).canCall)
        assertTrue(person(PRIYA).callsChipDone)
        f.setCanMessage(PRIYA, false)
        assertFalse(person(PRIYA).canMessage)
        f.setCanCall(PRIYA, false)
        f.simulateCallAsk(PRIYA)
        val ask = person(PRIYA).items.last() as DmItem.CallAsk
        f.answerCallAsk(PRIYA, ask.id, allow = true)
        assertTrue(person(PRIYA).canCall)
        assertTrue((person(PRIYA).items.last() as DmItem.CallAsk).handled)
    }

    // ---------------------------------------------------------------- address / lost contact

    @Test fun intervalMovesNextChange() {
        val last = f.address.value.lastChangeMs
        f.setInterval(AddressInterval.Week)
        assertEquals(last + 7 * DAY, f.address.value.nextChangeMs)
        f.setInterval(AddressInterval.Day)
        assertEquals(last + DAY, f.address.value.nextChangeMs)
    }

    @Test fun lostContactWaitsThenReconnects() {
        assertEquals(CallCheck.Lost, f.callCheck(SAM))
        f.sendText(SAM, "Still there?")
        assertEquals(DmTick.WaitingToReach, (person(SAM).items.last() as DmItem.Text).tick)
        f.reconnect(SAM)
        assertFalse(person(SAM).lost)
        assertTrue(person(SAM).items.filterIsInstance<DmItem.Text>().none { it.tick == DmTick.WaitingToReach })
        // History and verification kept.
        assertNotNull(person(SAM).verifiedAtMs)
        assertEquals(12, f.address.value.contactsTold)
    }

    @Test fun resetRestoresSeed() {
        f.flagMember(HIKE, LEO); f.block(ARJUN, "Arjun Oommen"); now += HOUR
        f.reset()
        assertEquals(1, leo().flags.size)
        assertFalse(f.isBlocked(ARJUN))
    }
}
