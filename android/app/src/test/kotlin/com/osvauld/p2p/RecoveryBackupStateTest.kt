package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class RecoveryBackupStateTest {
    @Test fun startsHiddenWithKeySelected() {
        val s = RecoveryBackupState()
        assertTrue(s.asKey)
        assertFalse(s.visible(0))
        assertFalse(s.canCopyKey(0))
    }

    @Test fun revealHidesAfterThirtySeconds() {
        val s = RecoveryBackupState().reveal(1_000)
        assertEquals(30_000L, RecoveryBackupState.REVEAL_MS)
        assertTrue(s.visible(1_000))
        assertTrue(s.visible(30_999))
        assertFalse(s.visible(31_000))
        assertFalse(s.canCopyKey(31_000))
    }

    @Test fun hideClearsDeadlineAndKeepsMode() {
        val s = RecoveryBackupState(asKey = false).reveal(0).hide()
        assertNull(s.revealUntil)
        assertFalse(s.asKey)
        assertFalse(s.visible(1))
    }

    @Test fun copyOnlyForKeyWhileVisible() {
        val key = RecoveryBackupState().reveal(0)
        assertTrue(key.canCopyKey(10))
        val words = RecoveryBackupState(asKey = false).reveal(0)
        assertTrue(words.visible(10))
        assertFalse(words.canCopyKey(10))
    }

    @Test fun switchingTabHidesAgain() {
        val s = RecoveryBackupState().reveal(0).select(false)
        assertFalse(s.visible(1))
        assertFalse(s.asKey)
        assertFalse(s.select(true).visible(1))
    }
}
