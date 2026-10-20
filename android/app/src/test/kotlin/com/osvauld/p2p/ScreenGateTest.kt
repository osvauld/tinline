package com.osvauld.p2p

import org.junit.Assert.*
import org.junit.Test

class ScreenGateTest {
    @Test fun failureCopyDoesNotClaimSuccess() {
        assertEquals("The code expired. Try again.", linkFailText("expired", null))
        assertEquals("Cancelled on the other device.", linkFailText("cancelled", null))
        assertEquals("This account is already on this phone.", linkFailText("account_exists", null))
        assertEquals("Core error", linkFailText("error", "Core error"))
    }
}
