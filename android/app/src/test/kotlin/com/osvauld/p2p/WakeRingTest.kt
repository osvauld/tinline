package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.p2pcore.WakeOutcome

/** The wake ring's state changes and the plan a core answer maps to (docs/push.md "Calls"). Synthetic DIDs. */
class WakeRingTest {
    private val a = "did:key:zAlice"
    private val b = "did:key:zBob"

    @Test fun callIdIsWakeColonDid() {
        assertEquals("wake:$a", WakeRingLogic.callId(a))
        assertTrue(WakeRingLogic.isWakeCallId("wake:$a"))
        assertFalse(WakeRingLogic.isWakeCallId("c-123"))
    }

    @Test fun aRepeatWakeIsOneRingAndKeepsAnEarlyAnswer() {
        val first = WakeRingLogic.ring(null, a, "Maya")
        val answered = WakeRingLogic.answered(first)
        val repeat = WakeRingLogic.ring(answered, a, "Maya")
        assertEquals(WakeRingUi(a, "Maya", answered = true), repeat)
    }

    @Test fun theEndOfTheRingClearsOnlyItsOwnContact() {
        val ring = WakeRingLogic.ring(null, a, "Maya")
        assertEquals(ring, WakeRingLogic.ended(ring, WakeRingLogic.callId(b)))
        assertNull(WakeRingLogic.ended(ring, WakeRingLogic.callId(a)))
        assertNull(WakeRingLogic.ended(null, WakeRingLogic.callId(a)))
    }

    @Test fun theRealCallFromTheSamePeerTakesTheRingOver() {
        val ring = WakeRingLogic.ring(null, a, "Maya")
        assertEquals(WakeRingLogic.Handover(matched = true, answeredEarly = false), WakeRingLogic.handover(ring, a))
        assertEquals(WakeRingLogic.Handover(matched = true, answeredEarly = true), WakeRingLogic.handover(WakeRingLogic.answered(ring), a))
    }

    @Test fun aCallFromSomeoneElseOrWithNoRingIsAnOrdinaryRing() {
        val ring = WakeRingLogic.ring(null, a, "Maya")
        assertEquals(WakeRingLogic.Handover(false, false), WakeRingLogic.handover(ring, b))
        assertEquals(WakeRingLogic.Handover(false, false), WakeRingLogic.handover(null, a))
    }

    @Test fun answeringNothingStaysNothing() {
        assertNull(WakeRingLogic.answered(null))
    }

    @Test fun coreAnswersMapToPlans() {
        assertEquals(WakePlan.Nothing, WakePlan.of(WakeOutcome.Ignore))
        assertEquals(WakePlan.Nothing, WakePlan.of(WakeOutcome.EndCall))
        assertEquals(WakePlan.StartNode, WakePlan.of(WakeOutcome.DialedBack(a)))
        assertEquals(WakePlan.StartNode, WakePlan.of(WakeOutcome.CodeWake))
        assertEquals(WakePlan.Ring(a, "Maya"), WakePlan.of(WakeOutcome.Ring(a, "Maya")))
    }
}
