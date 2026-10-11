package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.p2pcore.GroupCallDeviceClass
import uniffi.p2pcore.GroupCallThermal

class HostCondTest {
    @Test fun batteryPercent() {
        assertEquals(57, HostCond.batteryPct(57, 100))
        assertEquals(50, HostCond.batteryPct(128, 256))
        assertEquals(100, HostCond.batteryPct(300, 100))
        // Unknown level or scale: treated as empty, so an unknown phone never advertises hosting.
        assertEquals(0, HostCond.batteryPct(-1, 100))
        assertEquals(0, HostCond.batteryPct(50, 0))
    }

    @Test fun pluggedFromSourceOrStatus() {
        assertFalse(HostCond.plugged(0, 3)) // discharging
        assertFalse(HostCond.plugged(0, 4)) // not charging, unplugged
        assertTrue(HostCond.plugged(1, 3)) // AC
        assertTrue(HostCond.plugged(2, -1)) // USB
        assertTrue(HostCond.plugged(4, -1)) // wireless
        assertTrue(HostCond.plugged(0, 2)) // charging
        assertTrue(HostCond.plugged(0, 5)) // full
    }

    @Test fun thermalStatus() {
        assertEquals(GroupCallThermal.NONE, HostCond.thermal(-1))
        assertEquals(GroupCallThermal.NONE, HostCond.thermal(0))
        assertEquals(GroupCallThermal.LIGHT, HostCond.thermal(1))
        assertEquals(GroupCallThermal.MODERATE, HostCond.thermal(2))
        assertEquals(GroupCallThermal.SEVERE, HostCond.thermal(3))
        assertEquals(GroupCallThermal.CRITICAL, HostCond.thermal(4))
        assertEquals(GroupCallThermal.CRITICAL, HostCond.thermal(5)) // emergency
        assertEquals(GroupCallThermal.CRITICAL, HostCond.thermal(6)) // shutdown
    }

    @Test fun meteredFromCapabilities() {
        assertTrue(HostCond.metered(notMetered = false, temporarilyNotMetered = false))
        assertFalse(HostCond.metered(notMetered = true, temporarilyNotMetered = false))
        assertFalse(HostCond.metered(notMetered = false, temporarilyNotMetered = true))
    }

    @Test fun conditionsAreAPhone() {
        val c = HostCond.conditions(250, plugged = true, metered = true, thermal = GroupCallThermal.SEVERE)
        assertEquals(GroupCallDeviceClass.PHONE, c.deviceClass)
        assertEquals(100.toUByte(), c.batteryPct)
        assertTrue(c.plugged)
        assertTrue(c.metered)
        assertFalse(c.meteredAllowed)
        assertEquals(GroupCallThermal.SEVERE, c.thermal)
        assertEquals(0.toUByte(), HostCond.conditions(-5, false, false, GroupCallThermal.NONE).batteryPct)
    }
}
