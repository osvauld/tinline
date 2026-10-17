package com.osvauld.p2p

import android.util.Log
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Whether the always-on notification is the ongoing-call one (and the service holds the microphone
 * type and call locks), derived only from the current call state. [CoreService] re-derives on every
 * poke, so pokes carry no state and their order does not matter: the last one always wins with the
 * truth (a pushed in-call/idle flag could be left stuck by two pokes arriving out of order).
 */
object CallNotif {
    /** Where the 1:1 call stands, as far as the notification cares. An ended call is [NONE]. */
    enum class OneToOne { NONE, DIALING, RINGING, ANSWERED, ACTIVE }

    /** [title]: the ongoing notification's title, or null for the idle one; [why] goes to [CallTrace]. */
    data class Derived(val title: String?, val why: String)

    /** A group-call prepare that never settled stops counting after this (the join failed silently). */
    const val PREPARE_TIMEOUT_MS = 30_000L

    /**
     * Pure. [callName]/[groupName]: the titles to use. [groupInCall]: this device's user is in the
     * group call (joining, joined or reconnecting); hosting alone is not a call on this device.
     * [preparedAtMs]: when [GroupCallAudio.prepare] raised the service for a start/join not yet seen.
     */
    fun derive(
        call: OneToOne, callName: String,
        groupInCall: Boolean, groupHosting: Boolean, groupName: String,
        preparedAtMs: Long?, nowMs: Long,
    ): Derived {
        // The user answered (or End & answer): keep the microphone type from the tap on, as before.
        if (call == OneToOne.ACTIVE) return Derived(callName, "1:1 active")
        if (call == OneToOne.ANSWERED) return Derived(callName, "1:1 answered")
        if (groupInCall) return Derived(groupName, "group in call")
        if (preparedAtMs != null) {
            val age = nowMs - preparedAtMs
            if (age in 0 until PREPARE_TIMEOUT_MS) return Derived(groupName, "group prepared ${age / 1000}s")
        }
        val why = when {
            call == OneToOne.RINGING -> "1:1 ringing"
            call == OneToOne.DIALING -> "1:1 dialing"
            groupHosting -> "group hosting only"
            preparedAtMs != null -> "group prepare expired"
            else -> "no call"
        }
        return Derived(null, "idle: $why")
    }

    fun of(u: CallUi?): OneToOne = when {
        u == null -> OneToOne.NONE
        u.state is uniffi.p2pcore.CallState.Active -> OneToOne.ACTIVE
        u.state is uniffi.p2pcore.CallState.Ended -> OneToOne.NONE
        u.answered -> OneToOne.ANSWERED
        u.state is uniffi.p2pcore.CallState.Ringing -> OneToOne.RINGING
        else -> OneToOne.DIALING
    }

    /** The derivation for the app as it is now. */
    fun current(app: P2pApp): Derived {
        val u = app.calls.ui.value
        val g = app.groupCalls
        val here = g.here.value
        return derive(
            of(u), u?.info?.peerName?.ifBlank { null } ?: "Call",
            here?.inCall == true, here?.hosting == true, g.callName(),
            g.preparedAtMs, System.currentTimeMillis(),
        )
    }
}

/** First characters of a call id: enough to tell calls apart in [CallTrace], nothing more. */
fun shortId(id: String?): String = id?.take(8) ?: "-"

/**
 * Bounded in-memory log of call and notification transitions, kept in release builds too (and
 * mirrored to logcat under [TAG]) so a stuck notification can be explained after the fact.
 * Never put DIDs, names or other personal data in it: short call ids, states and reasons only.
 */
object CallTrace {
    const val TAG = "TinNotif"
    const val MAX = 200
    private val ring = ArrayDeque<String>()
    private val fmt = SimpleDateFormat("MM-dd HH:mm:ss.SSS", Locale.US)

    fun add(msg: String) {
        val line = synchronized(fmt) { fmt.format(Date()) } + " " + msg
        push(line)
        Log.i(TAG, msg)
    }

    /** Pure part of [add] (unit tests). */
    internal fun push(line: String) = synchronized(ring) {
        ring.addLast(line)
        while (ring.size > MAX) ring.removeFirst()
    }

    fun dump(): String = synchronized(ring) { ring.joinToString("\n") }

    internal fun clear() = synchronized(ring) { ring.clear() }
}
