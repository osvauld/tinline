package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AudioRoutesTest {
    private val ear = AudioRoute(AudioRoute.Kind.Earpiece, "", "ear")
    private val spk = AudioRoute(AudioRoute.Kind.Speaker, "", "spk")
    private val bt = AudioRoute(AudioRoute.Kind.Bluetooth, "Buds", "bt")
    private val wired = AudioRoute(AudioRoute.Kind.Wired, "", "wired")

    @Test fun oneToOneStartsOnEarpieceWithoutHeadset() {
        assertEquals(ear, Routes.pickDefault(listOf(spk, ear), preferSpeaker = false))
    }

    @Test fun groupAndVideoStartOnSpeakerWithoutHeadset() {
        assertEquals(spk, Routes.pickDefault(listOf(ear, spk), preferSpeaker = true))
    }

    @Test fun headsetWinsOverSpeakerPreference() {
        assertEquals(bt, Routes.pickDefault(listOf(ear, spk, wired, bt), preferSpeaker = true))
        assertEquals(wired, Routes.pickDefault(listOf(ear, spk, wired), preferSpeaker = true))
    }

    @Test fun noRoutesPicksNothing() {
        assertNull(Routes.pickDefault(emptyList(), preferSpeaker = false))
    }

    @Test fun toggleGoesToSpeakerAndBackToHeadset() {
        val onBt = RouteState(bt, listOf(ear, spk, bt))
        assertEquals(spk, Routes.toggleTarget(onBt))
        val onSpk = RouteState(spk, listOf(ear, spk, bt))
        assertEquals(bt, Routes.toggleTarget(onSpk))
        assertEquals(ear, Routes.toggleTarget(RouteState(spk, listOf(ear, spk))))
    }

    @Test fun stateFlags() {
        assertTrue(RouteState(ear, listOf(ear, spk)).earpiece)
        assertFalse(RouteState(ear, listOf(ear, spk)).hasHeadset)
        assertTrue(RouteState(spk, listOf(ear, spk, bt)).hasHeadset)
        assertTrue(RouteState(spk, listOf(ear, spk)).speaker)
        assertFalse(RouteState().earpiece)
    }

    @Test fun labels() {
        assertEquals("Phone", Routes.label(ear))
        assertEquals("Buds", Routes.label(bt))
        assertEquals("Bluetooth", Routes.label(bt.copy(name = "")))
        assertEquals("Headset", Routes.label(wired))
    }
}
