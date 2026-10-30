package com.osvauld.p2p

import androidx.compose.ui.geometry.Offset
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The thread-follow rules (ChatFollow.kt) behind "the newest message stays above the keyboard", and the viewer's zoom maths. */
class ChatFollowTest {
    private val slack = 120

    @Test fun atTheNewestMeansTheBottomRowIsTheNewestWithinTheSlack() {
        assertTrue(ChatFollow.atNewest(0, 0, slack))
        assertTrue(ChatFollow.atNewest(0, slack, slack))
        assertFalse(ChatFollow.atNewest(0, slack + 1, slack))
        assertFalse(ChatFollow.atNewest(1, 0, slack))
    }

    @Test fun theKeyboardRisingHoldsAReaderNearTheNewest() {
        // A few px short of the bottom when the list shrinks: pulled back to the newest.
        assertTrue(ChatFollow.holdOnShrink(before = 1800, after = 1100, wasAtNewest = true, firstIndex = 0, firstOffset = 30))
        // Already exactly there: nothing to do (the reversed list keeps it).
        assertFalse(ChatFollow.holdOnShrink(1800, 1100, true, 0, 0))
    }

    @Test fun aReaderUpInTheHistoryIsNeverMoved() {
        assertFalse(ChatFollow.holdOnShrink(1800, 1100, wasAtNewest = false, firstIndex = 14, firstOffset = 300))
    }

    @Test fun theListGrowingBackMovesNothing() {
        assertFalse(ChatFollow.holdOnShrink(1100, 1800, true, 0, 30))
    }

    @Test fun aNewMessageIsFollowedWhenMineOrWhenReadingTheBottom() {
        assertTrue(ChatFollow.followNew(mine = true, firstIndex = 30, theirs = true))
        assertTrue(ChatFollow.followNew(mine = false, firstIndex = 2, theirs = true))
        assertFalse(ChatFollow.followNew(mine = false, firstIndex = 3, theirs = true))
        // A search jump in progress: their messages don't pull the list away from it.
        assertFalse(ChatFollow.followNew(mine = false, firstIndex = 0, theirs = false))
    }

    @Test fun zoomKeepsThePointUnderTheFingersAndNeverPansPastTheEdge() {
        // Double tap 100 px right of centre at 1x -> 2.5x: that point stays under the finger.
        val pan = PhotoZoom.zoomAt(Offset.Zero, 1f, 2.5f, Offset(100f, 0f))
        assertEquals(-150f, pan.x, 0.001f)
        // At 1x nothing pans; at 2x a 1000 px box pans at most 500 px either way.
        val still = PhotoZoom.clamp(Offset(40f, -40f), 1f, 1000f, 800f)
        assertEquals(0f, still.x, 0.001f); assertEquals(0f, still.y, 0.001f)
        val edge = PhotoZoom.clamp(Offset(900f, -900f), 2f, 1000f, 800f)
        assertEquals(500f, edge.x, 0.001f); assertEquals(-400f, edge.y, 0.001f)
    }
}
