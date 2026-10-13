package com.osvauld.p2p
import org.junit.Assert.*
import org.junit.Test

class ProfileRecoveryTest {
    @Test fun busyCannotSilentlyRefuseCalls() {
        assertNull(ProfilePolicy.availabilityProblem(true))
        assertEquals(ProfilePolicy.BUSY_UNSUPPORTED, ProfilePolicy.availabilityProblem(false))
    }
    @Test fun accountRoundTripRejectsOldTicket() {
        val s = ProfileSession()
        s.select("account-a")
        val old = s.ticket()!!
        s.select("account-b")
        s.select("account-a")
        assertFalse(s.accepts(old))
        assertTrue(s.accepts(s.ticket()!!))
    }
    @Test fun lockInvalidatesAndSameAccountRefreshDoesNot() {
        val s = ProfileSession()
        s.select("account-a")
        val t = s.ticket()!!
        assertFalse(s.select("account-a"))
        assertTrue(s.accepts(t))
        s.invalidate()
        assertNull(s.ticket())
        assertFalse(s.accepts(t))
    }
}
