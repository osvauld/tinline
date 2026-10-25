package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** docs/push.md: push is the default where the edition can do it; always-on otherwise. */
class PushPolicyTest {
    @Test fun pushIsTheDefaultWhenAvailable() {
        assertEquals(PushPolicy.Mode.PUSH, PushPolicy.mode(providerAvailable = true, alwaysOn = null))
        assertEquals(PushPolicy.Mode.PUSH, PushPolicy.mode(providerAvailable = true, alwaysOn = false))
    }

    @Test fun theUserCanChooseAlwaysOn() {
        assertEquals(PushPolicy.Mode.ALWAYS_ON, PushPolicy.mode(providerAvailable = true, alwaysOn = true))
    }

    @Test fun noProviderMeansAlwaysOnWhateverWasStored() {
        // Liberum, a Play build without Firebase settings, or a phone without Play services.
        for (stored in listOf(null, true, false))
            assertEquals(PushPolicy.Mode.ALWAYS_ON, PushPolicy.mode(providerAvailable = false, alwaysOn = stored))
    }

    @Test fun onlyAlwaysOnStartsTheServiceAtBootAndAppOpen() {
        assertTrue(PushPolicy.wantsAlwaysOnService(PushPolicy.Mode.ALWAYS_ON))
        assertFalse(PushPolicy.wantsAlwaysOnService(PushPolicy.Mode.PUSH))
    }
}
