package com.osvauld.p2p

import org.junit.Assert.*
import org.junit.Test
import java.io.File

/**
 * Parity with the playground's model.ts through crates/desktop/src/call_field_golden.txt (frames produced by the
 * playground's own model, tools/call-field/golden.mjs). Same scenarios and the same tolerance (1e-7 relative to
 * max(|golden|, 1)) as crates/desktop/src/call_field_tests.rs. The golden file is read from the repo, not copied.
 */
class CallFieldTest {
    private val golden: List<String> by lazy {
        var d: File? = File("").absoluteFile
        while (d != null && !File(d, "crates/desktop/src/call_field_golden.txt").exists()) d = d.parentFile
        checkNotNull(d) { "call_field_golden.txt not found above ${File("").absolutePath}" }
        File(d, "crates/desktop/src/call_field_golden.txt").readLines()
    }

    private data class Step(val at: Double, val state: CallFieldState, val delay: Double)

    private fun scenario(name: String): List<Step> = when (name) {
        "out" -> listOf(Step(0.0, CallFieldState.Outgoing, 0.0), Step(2600.0, CallFieldState.Connecting, 0.0), Step(3300.0, CallFieldState.Active, 97.0),
            Step(9000.0, CallFieldState.Waiting, 0.0), Step(12000.0, CallFieldState.Active, 0.0), Step(14000.0, CallFieldState.Reconnecting, 0.0),
            Step(16000.0, CallFieldState.Active, 97.0), Step(18000.0, CallFieldState.Ended, 0.0))
        "inc" -> listOf(Step(0.0, CallFieldState.Incoming, 0.0), Step(4000.0, CallFieldState.Connecting, 0.0), Step(4700.0, CallFieldState.Active, 97.0),
            Step(8000.0, CallFieldState.Failed, 0.0))
        else -> error(name)
    }

    private fun run(name: String, theme: FieldTheme, reduced: Boolean, muteAt: Double?): List<Pair<Long, List<Double>>> {
        val f = CallField(theme, reduced)
        f.setView(390.0, 844.0, 195.0, 300.0)
        val sc = scenario(name.substringBefore('-'))
        var si = 0
        val out = ArrayList<Pair<Long, List<Double>>>()
        var t = 0L
        while (t <= 20000) {
            while (si < sc.size && sc[si].at <= t) {
                val s = sc[si]; si++
                f.setState(s.state, s.at, s.at + s.delay)
            }
            if (muteAt != null && t >= muteAt) f.setMute(true)
            val fr = f.update(t.toDouble())
            if (t % 160 < 16 || (t > 3200 && t < 4200)) {
                val v = ArrayList<Double>()
                v.add(fr.tau); v.add(fr.warp)
                for (l in fr.lobes) { v.addAll(listOf(l.x, l.y, l.sigma, l.alpha, l.lab[0], l.lab[1], l.lab[2])) }
                out.add(t to v)
            }
            t += if (t % 1000 == 500L) 7 else 16
        }
        return out
    }

    private fun check(name: String, theme: FieldTheme, reduced: Boolean, muteAt: Double?) {
        val want = golden.filter { it.startsWith("$name ") }.map { l ->
            val it = l.trim().split(Regex("\\s+")).drop(1)
            it[0].toLong() to it.drop(1).map(String::toDouble)
        }
        val got = run(name, theme, reduced, muteAt)
        assertTrue("$name: no golden frames", want.isNotEmpty())
        assertEquals("$name: frame count", want.size, got.size)
        var worst = 0.0
        for ((w, g) in want.zip(got)) {
            assertEquals(w.first, g.first)
            assertEquals("golden frame shape", 2 + FieldMath.K * 7, w.second.size)
            assertEquals("native frame shape", w.second.size, g.second.size)
            for ((i, ab) in w.second.zip(g.second).withIndex()) {
                val err = Math.abs(ab.first - ab.second) / Math.max(Math.abs(ab.first), 1.0)
                worst = Math.max(worst, err)
                assertTrue("$name t=${w.first} value $i: playground ${ab.first}, kotlin ${ab.second}", err < 1e-7)
            }
        }
        println("$name: ${got.size} frames, worst relative error $worst")
    }

    @Test fun matchesThePlaygroundOutgoingCallLight() = check("out-light", FieldTheme.Light, false, null)
    @Test fun matchesThePlaygroundOutgoingCallDarkMuted() = check("out-dark", FieldTheme.Dark, false, 10000.0)
    @Test fun matchesThePlaygroundIncomingCallThatFails() = check("inc-light", FieldTheme.Light, false, null)
    @Test fun matchesThePlaygroundReducedMotion() = check("out-reduced", FieldTheme.Light, true, null)

    @Test fun endedFieldFadesAndStopsAnimating() {
        val f = CallField(FieldTheme.Light, false)
        f.setState(CallFieldState.Active, 0.0)
        f.update(1000.0)
        f.setState(CallFieldState.Ended, 1000.0)
        var t = 1000.0
        while (f.update(t).animating) { t += 16.0; assertTrue("the end fade takes 300 ms", t < 2000.0) }
        assertTrue(f.update(t).lobes.all { it.alpha == 0.0 })
    }

    @Test fun reducedMotionSleepsUntilTheNextNote() {
        val f = CallField(FieldTheme.Light, true)
        f.setState(CallFieldState.Incoming, 0.0, 0.0)
        var t = 0.0
        var fr = f.update(t)
        while (fr.animating) { t += 16.0; fr = f.update(t) }
        val wake = checkNotNull(fr.wakeAt) { "a looping ring always has a next note" }
        assertTrue("wakes at the next ring note, got $wake", wake > t && wake <= 3200.0)
    }

    @Test fun rngIsBitExactWithMulberry32() {
        val r = FieldRng(7)
        assertEquals(listOf(0.011704753153026104, 0.06195825757458806, 0.97690763277933), List(3) { r.next() })
    }
}
