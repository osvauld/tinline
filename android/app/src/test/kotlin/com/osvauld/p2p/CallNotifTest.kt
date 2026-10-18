package com.osvauld.p2p

import com.osvauld.p2p.CallNotif.OneToOne
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The ongoing-call notification is derived from state only (field reports: it stayed on after a call ended). */
class CallNotifTest {
    private val now = 1_000_000L

    /** The derivation with defaults for an idle phone. */
    private fun d(
        call: OneToOne = OneToOne.NONE, groupIn: Boolean = false, hosting: Boolean = false, preparedAt: Long? = null, at: Long = now,
    ) = CallNotif.derive(call, "Ada", groupIn, hosting, "Book club", preparedAt, at)

    @Test fun idle() { assertNull(d().title); assertEquals("idle: no call", d().why) }

    @Test fun ringingIncomingIsNotACall() = assertNull(d(OneToOne.RINGING).title)

    @Test fun dialingIsNotACallYet() = assertNull(d(OneToOne.DIALING).title)

    @Test fun answeredIsACall() = assertEquals("Ada", d(OneToOne.ANSWERED).title)

    @Test fun activeIsACall() = assertEquals("Ada", d(OneToOne.ACTIVE).title)

    /** An ended call leaves [CallController.ui] (or is mapped to NONE): back to idle, whatever was pushed before. */
    @Test fun endedIsIdle() = assertNull(d(OneToOne.NONE).title)

    @Test fun groupJoined() = assertEquals("Book club", d(groupIn = true).title)

    @Test fun groupHostingOnlyIsNotACall() {
        val r = d(hosting = true)
        assertNull(r.title)
        assertEquals("idle: group hosting only", r.why)
    }

    /** A 1:1 call wins the title over a group call; a 1:1 ending keeps the group call's call mode. */
    @Test fun oneToOneThenGroup() {
        assertEquals("Ada", d(OneToOne.ACTIVE, groupIn = true).title)
        assertEquals("Book club", d(OneToOne.NONE, groupIn = true).title)
    }

    @Test fun preparedCountsUntilTimeout() {
        assertEquals("Book club", d(preparedAt = now).title)
        assertEquals("Book club", d(preparedAt = now - CallNotif.PREPARE_TIMEOUT_MS + 1).title)
        val r = d(preparedAt = now - CallNotif.PREPARE_TIMEOUT_MS)
        assertNull(r.title)
        assertEquals("idle: group prepare expired", r.why)
        // A clock that went backwards does not keep it either.
        assertNull(d(preparedAt = now + 5_000).title)
    }

    /**
     * Pokes carry no state: the service derives the same result whichever poke arrives last. The old
     * bug: IDLE (sent directly) overtook IN_CALL (queued in the system), leaving "in call" pushed.
     */
    @Test fun outOfOrderPokesDoNotMatter() {
        // A model of CoreService: a poke re-derives from the state as it is when the poke is handled.
        var state = OneToOne.NONE
        var group = false
        var title: String? = null
        val queued = ArrayDeque<String>()   // startForegroundService intents still in the system
        fun handle() { title = d(state, groupIn = group).title }

        // Answer and Active: their IN_CALL pokes are still queued...
        state = OneToOne.ANSWERED; queued.addLast("IN_CALL")
        state = OneToOne.ACTIVE; queued.addLast("IN_CALL")
        // ...when the call ends and IDLE is refused and handled directly first.
        state = OneToOne.NONE; handle()
        assertNull(title)
        while (queued.isNotEmpty()) { queued.removeFirst(); handle() }
        assertNull("late IN_CALL pokes must not bring the call back", title)

        // Same with a group call joined meanwhile: the late pokes keep it, the 1:1 ending doesn't drop it.
        group = true; state = OneToOne.ACTIVE; queued.addLast("IN_CALL")
        state = OneToOne.NONE; handle()
        assertEquals("Book club", title)
        queued.removeFirst(); handle()
        assertEquals("Book club", title)
        group = false; handle()
        assertNull(title)
    }

    @Test fun traceIsBounded() {
        CallTrace.clear()
        repeat(CallTrace.MAX + 50) { CallTrace.push("line $it") }
        val lines = CallTrace.dump().lines()
        assertEquals(CallTrace.MAX, lines.size)
        assertEquals("line 50", lines.first())
        assertEquals("line ${CallTrace.MAX + 49}", lines.last())
        CallTrace.clear()
    }
}
