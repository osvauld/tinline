package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.ZoneOffset

/** The block model the screens read (filled from Node.blockState / blockedContacts in P2pApp). */
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
        assertEquals("Blocked", b.status("ann"))
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
        assertEquals("Blocked you", b.status("meera"))
        assertTrue(b.list.isEmpty())
        assertEquals("Not delivered: Meera blocked you", BlockCopy.notDelivered("Meera K"))
        assertEquals("Meera blocked you", BlockCopy.blockedYouTitle("Meera K"))
    }

    @Test fun afterTheirUnblockTheKeptChatAsksToAddThemAgain() {
        val b = blocks(mapOf("meera" to BlockView()))
        assertEquals(ComposerState.NotContact, b.composer("meera", contact = false))
        assertFalse(b.canCall("meera", contact = false))
        assertNull(b.status("meera"))
        assertTrue(BlockCopy.notContact("Meera K").contains("Add them again"))
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
        assertEquals("Priya hasn’t allowed calls from you. You can still message them.", BlockCopy.cantCall("Priya N", ComposerState.Open))
        assertEquals("Priya hasn’t allowed calls from you", endReasonText("calls not allowed", "Priya N"))
        // A block closes calling whatever the tokens say.
        val blocked = Blocks.from(mapOf("ann" to BlockView(blockedByMe = true)), emptyList(), setOf("ann"), setOf("ann"))
        assertEquals(CallGate.Closed, blocked.callGate("ann", contact = true))
    }

    @Test fun theBlockDialogSaysWhatHappens() {
        assertEquals("Block Arjun?", BlockCopy.dialogTitle("Arjun Oommen"))
        val all = BlockCopy.dialogLines("Arjun Oommen").joinToString(" ")
        assertTrue(all.contains("will see that you blocked them"))
        assertTrue(all.contains("removes you from their contacts"))
        assertTrue(all.contains("chat history with Arjun stays"))
        assertTrue(all.contains("has to add you again"))
        assertTrue(all.contains("Groups you share are not affected"))
        assertFalse("nothing is deleted by a block", all.lowercase().contains("delete"))
    }

    @Test fun callWordsSayWhichSideBlocked() {
        assertEquals("You blocked Ann", endReasonText("blocked", "Ann Lee", blockedByMe = true))
        assertEquals("Ann blocked you", endReasonText("blocked", "Ann Lee"))
        assertEquals("Can’t call: Ann blocked you", BlockCopy.cantCall("Ann Lee", ComposerState.BlockedMe))
        assertEquals("Oct 10, 13:50", BlockCopy.date(1_791_640_200, ZoneOffset.UTC))
    }
}
