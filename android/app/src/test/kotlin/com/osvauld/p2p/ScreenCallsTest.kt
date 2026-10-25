package com.osvauld.p2p

import org.junit.Assert.*
import org.junit.Test

class ScreenCallsTest {
    @Test fun realPhaseFlagsChooseStatusWithoutInventingTiming() {
        assertEquals("Calling…", callStatus(false, false, false, 0))
        assertEquals("Ringing…", callStatus(false, true, false, 0))
        assertEquals("Waking their phone…", callStatus(false, false, false, 0, waking = true))
        assertEquals("Ringing…", callStatus(false, true, false, 0, waking = true))
        assertEquals("Reconnecting…", callStatus(true, false, true, 65))
    }
    @Test fun encryptionClaimIsConditionalOnConnection() {
        val connecting = callPrivacy(false, "Arjun")
        assertTrue(connecting.startsWith("Connecting to Arjun"))
        assertFalse(connecting.startsWith("End-to-end encrypted."))
        val connected = callPrivacy(true, "Arjun")
        assertTrue(connected.startsWith("End-to-end encrypted."))
        // The relay is explained in the card's connection row, only when the call is relayed.
        assertFalse(connected.contains("relay"))
    }
    @Test fun signalUsesRecentLossAndSharedThresholds() {
        val l = RecentLoss()
        l.push(0, 0); l.push(1000, 200)
        var last = 0.0
        for (i in 2..8) last = l.push(1000L * i, 200)
        assertEquals(0.0, last, 0.0)
        assertEquals(listOf("0", "0.2", "14"), listOf(lossText(0.0), lossText(.2), lossText(14.04)))
    }
    @Test fun liveMarkUsesFoundationCallPriority() {
        assertEquals(MarkState.Ringing, statusLook(CallPhase.Outgoing, conn = ConnState.Online).state)
        assertEquals(MarkState.Taut, statusLook(CallPhase.Active, conn = ConnState.Offline).state)
        assertEquals(MarkState.Ringing, statusLook(CallPhase.Reconnecting, conn = ConnState.Online).state)
    }
}
