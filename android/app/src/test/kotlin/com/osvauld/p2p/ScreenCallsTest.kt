package com.osvauld.p2p

import org.junit.Assert.*
import org.junit.Test

class ScreenCallsTest {
    @Test fun realPhaseFlagsChooseStatusWithoutInventingTiming() {
        assertEquals(appString(R.string.call_status_calling), callStatusText(false, false, false, 0))
        assertEquals(appString(R.string.call_status_ringing), callStatusText(false, true, false, 0))
        assertEquals(appString(R.string.call_reconnecting), callStatusText(true, false, true, 65))
    }
    @Test fun encryptionClaimIsConditionalOnConnection() {
        // Resources are `string#<id>` on the JVM: the two states must pick different strings, and only a connected call claims encryption.
        assertEquals(appString(R.string.call_privacy_connecting), callPrivacyText(false, "Arjun"))
        assertEquals(appString(R.string.call_privacy_active), callPrivacyText(true, "Arjun"))
        assertNotEquals(callPrivacyText(false, "Arjun"), callPrivacyText(true, "Arjun"))
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
