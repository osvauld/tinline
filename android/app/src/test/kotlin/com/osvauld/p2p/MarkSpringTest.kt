package com.osvauld.p2p

import org.junit.Assert.*
import org.junit.Test

/** The Done / first-contact pull (LINE_SPRING) and the everyday recoil: parameters of the one model (Mark.kt). */
class MarkSpringTest {
    private fun pull(m: MarkModel): Pair<List<Double>, Double> {
        m.connect()
        var t = 0.0; val tugs = mutableListOf<Double>(); var seen = m.tugCount
        while (m.advance(1.0 / 600)) {
            t += 1.0 / 600
            if (m.tugCount > seen) { seen = m.tugCount; tugs += t }
            assertTrue("still moving after 3 s", t < 3.0)
        }
        return tugs to t
    }

    @Test fun lineSpringTugsTwiceOnTheCueGap() {
        val (tugs, _) = pull(markModelFor(96.0, MarkSpring.Line))
        assertEquals(2, tugs.size)
        val gap = tugs[1] - tugs[0]
        assertTrue("gap $gap", gap in 0.10..0.16)
    }

    @Test fun defaultPullRecoilsAndSettlesDeadStopDoesNot() {
        val (tugs, settled) = pull(markModelFor(96.0))
        assertTrue(tugs.size >= 2); assertTrue(settled < 1.2)
        val (dead, _) = pull(markModelFor(96.0, MarkSpring.DeadStop))
        assertEquals(1, dead.size)
    }

    @Test fun mountingTautCountsNoTug() {
        val m = markModelFor(96.0, MarkSpring.Line)
        m.set(MarkState.Taut)
        assertEquals(0, m.tugCount); assertTrue(m.tight)
    }
}
