package com.osvauld.p2p

import org.junit.Assert.*
import org.junit.Test

class VideoQualityTest {
    @Test fun callChoiceBeatsSettingOnlyForThatCall() {
        assertEquals(false, effectivePreferHd(false, null))
        assertEquals(true, effectivePreferHd(true, null))
        assertEquals(true, effectivePreferHd(false, true))
        assertEquals(false, effectivePreferHd(true, false))
    }
    @Test fun hdPausedFollowsActualSendSize() {
        assertFalse(hdPaused(true, 0, 0))
        assertFalse(hdPaused(true, 960, 540))
        assertTrue(hdPaused(true, 640, 360))
        assertTrue(hdPaused(true, 360, 640))
        assertFalse(hdPaused(false, 320, 180))
        assertEquals("HD paused \u00b7 weak connection", hdLine(true, true))
        assertEquals("Sharper video off", hdLine(false, true))
    }
    @Test fun hdIsExplicitAndAutoKeepsBaseline() {
        assertEquals(listOf(CapturePreference.SD), capturePreferences(false))
        assertEquals(listOf(CapturePreference.HD, CapturePreference.SD), capturePreferences(true))
        assertEquals(1280, CapturePreference.HD.width)
        assertEquals(720, CapturePreference.HD.height)
    }
    @Test fun unsupportedHdFallsBackOnceToBaseline() {
        val attempted = mutableListOf<CapturePreference>()
        val result = bindCapture(capturePreferences(true)) {
            attempted += it
            if (it == CapturePreference.HD) throw IllegalArgumentException("unsupported")
        }
        assertEquals(CapturePreference.SD, result)
        assertEquals(listOf(CapturePreference.HD, CapturePreference.SD), attempted)
    }
    @Test fun supportedHdDoesNotBindSd() {
        var attempts = 0
        assertEquals(CapturePreference.HD, bindCapture(capturePreferences(true)) { attempts++ })
        assertEquals(1, attempts)
    }
    @Test fun permissionErrorIsNotHiddenByFallback() {
        var attempts = 0
        try {
            bindCapture(capturePreferences(true)) { attempts++; throw SecurityException("camera denied") }
            fail("must propagate camera denial")
        } catch (e: SecurityException) { assertEquals("camera denied", e.message) }
        assertEquals(1, attempts)
    }
    @Test fun failedFallbackDoesNotLoop() {
        var attempts = 0
        try {
            bindCapture(capturePreferences(true)) { attempts++; throw IllegalArgumentException("no stream") }
            fail("must propagate final failure")
        } catch (e: IllegalArgumentException) { assertEquals("no stream", e.message) }
        assertEquals(2, attempts)
    }
    @Test fun storedPreferenceIsSafeAndStable() {
        assertEquals(VideoQualityPreference.AUTO, VideoQualityPreference.fromStored(null))
        assertEquals(VideoQualityPreference.AUTO, VideoQualityPreference.fromStored("invalid"))
        assertEquals(VideoQualityPreference.AUTO, VideoQualityPreference.fromStored("AUTO"))
        assertEquals(VideoQualityPreference.PREFER_HD, VideoQualityPreference.fromStored("PREFER_HD"))
    }
    @Test fun hdLabelUsesActualDimensionsAndHandlesPortrait() {
        assertNull(videoResolutionLabel(0, 0))
        assertNull(videoResolutionLabel(-1, 720))
        assertEquals("HD · 1280×720", videoResolutionLabel(1280, 720))
        assertEquals("HD · 720×1280", videoResolutionLabel(720, 1280))
        assertEquals("640×480", videoResolutionLabel(640, 480))
        assertEquals("960×540", videoResolutionLabel(960, 540))
        assertEquals("240×180", videoResolutionLabel(240, 180))
    }
    private fun snap(call: String = "a", sent: ULong = 10uL, received: ULong = 8uL,
        decoded: ULong = 8uL, admitted: ULong = 1000uL, delivered: ULong = 800uL) =
        VideoRateSnapshot(call, sent, received, decoded, admitted, delivered)

    @Test fun initialObservationAndNewCallAreUnknownNotZero() {
        val meter = VideoRateMeter()
        assertNull(meter.sample(0, snap()))
        assertNull(meter.sample(1_000_000_000, snap(call = "b")))
    }
    @Test fun usesElapsedTimeAndCounterDeltasNotTargetRate() {
        val meter = VideoRateMeter()
        meter.sample(0, snap())
        val r = meter.sample(2_000_000_000, snap(sent = 70uL, received = 48uL,
            decoded = 38uL, admitted = 126000uL, delivered = 100800uL))!!
        assertEquals(30.0, r.admittedFps, 0.0001)
        assertEquals(20.0, r.receivedFps, 0.0001)
        assertEquals(15.0, r.decodedFps, 0.0001)
        assertEquals(500.0, r.admittedPayloadKbps, 0.0001)
        assertEquals(400.0, r.receivedPayloadKbps, 0.0001)
    }
    @Test fun idleWindowRemainsInDenominator() {
        val meter = VideoRateMeter(); meter.sample(0, snap())
        val r = meter.sample(3_000_000_000, snap())!!
        assertEquals(0.0, r.admittedFps, 0.0)
        assertEquals(0.0, r.receivedPayloadKbps, 0.0)
    }
    @Test fun equalClockDoesNotDiscardAccumulatedCounters() {
        val meter = VideoRateMeter(); meter.sample(10, snap())
        assertNull(meter.sample(10, snap(sent = 12uL)))
        assertEquals(2.0, meter.sample(1_000_000_010, snap(sent = 12uL))!!.admittedFps, 0.0)
    }
    @Test fun backwardClockAndCounterResetRebaseline() {
        val meter = VideoRateMeter(); meter.sample(100, snap())
        assertNull(meter.sample(99, snap()))
        assertNull(meter.sample(1_000_000_099, snap(sent = 0uL)))
        assertEquals(10.0, meter.sample(2_000_000_099, snap())!!.admittedFps, 0.0)
    }
    @Test fun noUnsignedWrapOnLongCounters() {
        val meter = VideoRateMeter()
        meter.sample(0, snap(admitted = ULong.MAX_VALUE - 20uL))
        val r = meter.sample(1_000_000_000, snap(admitted = ULong.MAX_VALUE))!!
        assertEquals(0.16, r.admittedPayloadKbps, 0.0001)
        meter.reset()
        assertNull(meter.sample(2_000_000_000, snap()))
    }
}
