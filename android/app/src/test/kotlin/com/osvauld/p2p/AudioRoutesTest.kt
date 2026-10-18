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

    @Test fun genericBluetoothNameCountsAsUnnamed() {
        val generic = bt.copy(name = Routes.GENERIC_BT)
        assertTrue(Routes.unnamedBluetooth(RouteState(generic, listOf(ear, spk, generic))))
        assertTrue(Routes.unnamedBluetooth(RouteState(ear, listOf(ear, bt.copy(name = "")))))
        assertFalse(Routes.unnamedBluetooth(RouteState(bt, listOf(ear, spk, bt))))
        assertFalse(Routes.unnamedBluetooth(RouteState(ear, listOf(ear, spk))))
        assertEquals("Bluetooth", Routes.label(generic))
    }

    @Test fun videoCallIsPlacedAsVideoThenAnyCameraOn() {
        // Ringing/dialling: how it was placed.
        assertTrue(Routes.isVideo(placedAsVideo = true, decided = false, negotiated = false, localCamera = false, remoteCamera = false))
        assertFalse(Routes.isVideo(placedAsVideo = false, decided = false, negotiated = false, localCamera = false, remoteCamera = false))
        // My camera on while it rings ("Video") decides nothing yet.
        assertFalse(Routes.isVideo(placedAsVideo = false, decided = false, negotiated = false, localCamera = true, remoteCamera = false))
        // Active: video runs and a camera is on.
        assertTrue(Routes.isVideo(placedAsVideo = true, decided = true, negotiated = true, localCamera = true, remoteCamera = false))
        assertFalse(Routes.isVideo(placedAsVideo = false, decided = true, negotiated = true, localCamera = false, remoteCamera = false))
        assertTrue(Routes.isVideo(placedAsVideo = false, decided = true, negotiated = true, localCamera = true, remoteCamera = false))
        assertTrue(Routes.isVideo(placedAsVideo = false, decided = true, negotiated = true, localCamera = false, remoteCamera = true))
        // Placed as video to a device without video: a voice call once that is known.
        assertFalse(Routes.isVideo(placedAsVideo = true, decided = true, negotiated = false, localCamera = true, remoteCamera = false))
    }

    @Test fun becomingVideoIsOncePerChange() {
        assertTrue(Routes.becameVideo(wasVideo = false, isVideo = true))
        // A call that already is video is left where the person put it.
        assertFalse(Routes.becameVideo(wasVideo = true, isVideo = true))
        assertFalse(Routes.becameVideo(wasVideo = false, isVideo = false))
        assertFalse(Routes.becameVideo(wasVideo = true, isVideo = false))
    }

    @Test fun proximityOnlyForVoiceOnEarpiece() {
        val onEar = RouteState(ear, listOf(ear, spk))
        assertTrue(Routes.proximityWanted(onEar, video = false))
        assertFalse(Routes.proximityWanted(onEar, video = true))
        assertFalse(Routes.proximityWanted(RouteState(spk, listOf(ear, spk)), video = false))
        assertFalse(Routes.proximityWanted(RouteState(), video = false))
    }

    @Test fun labels() {
        assertEquals("Phone", Routes.label(ear))
        assertEquals("Buds", Routes.label(bt))
        assertEquals("Bluetooth", Routes.label(bt.copy(name = "")))
        assertEquals("Headset", Routes.label(wired))
    }
}
