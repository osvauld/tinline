package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.ZoneOffset

/** The block model the screens read (filled from Node.peerBlocks / blockedContacts in P2pApp). */
class BlockingTest {
    private fun blocks(views: Map<String, BlockView>, list: List<BlockedRow> = emptyList()) = Blocks.from(views, list)

    @Test fun noBlockMeansAnOpenComposer() {
        val b = blocks(mapOf("ann" to BlockView()))
        assertTrue(b.states.isEmpty())
        assertEquals(ComposerState.Open, b.composer("ann", contact = true))
        assertNull(b.status("ann"))
    }

    @Test fun blockingAContactKeepsItAndOffersUnblock() {
        val b = blocks(mapOf("ann" to BlockView(blockedByMe = true, blockedAtSecs = 100)), listOf(BlockedRow("ann", "Ann Lee", 100)))
        assertEquals(ComposerState.BlockedByMe, b.composer("ann", contact = true))
        assertFalse(b.canCall("ann", contact = true))
        assertEquals(R.string.common_blocked, b.statusRes("ann"))
        assertEquals(appString(R.string.common_blocked), b.status("ann"))
        assertEquals(listOf("ann"), b.list.map { it.did })
    }

    @Test fun theBlockedListIsNewestFirstAndCountsBlocksFromOtherDevices() {
        val b = blocks(emptyMap(), listOf(BlockedRow("ann", "Ann", 100), BlockedRow("bob", "Bob", 200)))
        assertEquals(listOf("bob", "ann"), b.list.map { it.did })
        // Not among the DIDs read (a block made on another device): still blocked here.
        assertTrue(b.blockedByMe("ann"))
        assertEquals(ComposerState.BlockedByMe, b.composer("ann", contact = true))
    }

    @Test fun beingBlockedClosesTheComposerOnTheKeptChat() {
        // Their notice deleted our contact entry for them (U20): not a contact, chat kept.
        val b = blocks(mapOf("meera" to BlockView(blockedMeAtSecs = 1_791_640_200)))
        assertEquals(ComposerState.BlockedMe, b.composer("meera", contact = false))
        assertFalse(b.canCall("meera", contact = false))
        assertEquals(R.string.blocking_row_blocked_you, b.statusRes("meera"))
        assertTrue(b.list.isEmpty())
    }

    @Test fun afterTheirUnblockTheKeptChatAsksToAddThemAgain() {
        val b = blocks(mapOf("meera" to BlockView()))
        assertEquals(ComposerState.NotContact, b.composer("meera", contact = false))
        assertFalse(b.canCall("meera", contact = false))
        assertNull(b.status("meera"))
    }

    @Test fun callPermissionsGateTheCallButton() {
        // Ann: a ticket contact (both ways). Priya: from a group chat request, message only.
        val b = Blocks.from(emptyMap(), emptyList(), allowed = setOf("ann"), tokens = setOf("ann"))
        assertEquals(CallGate.Ok, b.callGate("ann", contact = true))
        assertTrue(b.callsAllowed("ann"))
        assertEquals(CallGate.NotAllowed, b.callGate("priya", contact = true))
        assertFalse(b.canCall("priya", contact = true))
        assertFalse(b.callsAllowed("priya"))
        assertEquals(ComposerState.Open, b.composer("priya", contact = true))
        // A block closes calling whatever the tokens say.
        val blocked = Blocks.from(mapOf("ann" to BlockView(blockedByMe = true)), emptyList(), setOf("ann"), setOf("ann"))
        assertEquals(CallGate.Closed, blocked.callGate("ann", contact = true))
    }

    @Test fun blockDateFollowsTheLocale() {
        val en = BlockCopy.date(1_791_640_200, ZoneOffset.UTC, java.util.Locale.US)
        val de = BlockCopy.date(1_791_640_200, ZoneOffset.UTC, java.util.Locale.GERMANY)
        assertTrue(en, en.startsWith("Oct 10"))
        assertTrue(de, de.contains("10.") && de.contains("13:50"))
    }
}
