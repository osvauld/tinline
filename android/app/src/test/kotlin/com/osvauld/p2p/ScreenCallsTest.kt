package com.osvauld.p2p

import org.junit.Assert.*
import org.junit.Test

class ScreenCallsTest {
    @Test fun realPhaseFlagsChooseStatusWithoutInventingTiming() {
        assertEquals("Calling…", callStatus(false, false, false, 0))
        assertEquals("Ringing…", callStatus(false, true, false, 0))
        assertEquals("01:05", callStatus(true, false, false, 65))
        assertEquals("Reconnecting…", callStatus(true, false, true, 65))
    }
    @Test fun encryptionClaimIsConditionalOnConnection() {
        val connecting = callPrivacy(false, "Arjun")
        assertTrue(connecting.startsWith("Connecting to Arjun"))
        assertFalse(connecting.startsWith("End-to-end encrypted."))
        val connected = callPrivacy(true, "Arjun")
        assertTrue(connected.startsWith("End-to-end encrypted."))
        assertTrue(connected.contains("relay"))
        assertFalse(connected.contains("straight to"))
    }
    @Test fun liveMarkUsesFoundationCallPriority() {
        assertEquals(MarkState.Ringing, statusLook(CallPhase.Outgoing, conn = ConnState.Online).state)
        assertEquals(MarkState.Taut, statusLook(CallPhase.Active, conn = ConnState.Offline).state)
        assertEquals(MarkState.Ringing, statusLook(CallPhase.Reconnecting, conn = ConnState.Online).state)
    }
}
