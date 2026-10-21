import XCTest
@testable import Tinline

/** Port of android VideoQualityTest.kt, plus the I420 conversion. */
final class VideoQualityTests: XCTestCase {
    func testCallChoiceBeatsSettingOnlyForThatCall() {
        XCTAssertFalse(effectivePreferHd(setting: false, callChoice: nil))
        XCTAssertTrue(effectivePreferHd(setting: true, callChoice: nil))
        XCTAssertTrue(effectivePreferHd(setting: false, callChoice: true))
        XCTAssertFalse(effectivePreferHd(setting: true, callChoice: false))
    }

    func testHdPausedFollowsActualSendSize() {
        XCTAssertFalse(hdPaused(hdOn: true, sendWidth: 0, sendHeight: 0))
        XCTAssertFalse(hdPaused(hdOn: true, sendWidth: 960, sendHeight: 540))
        XCTAssertTrue(hdPaused(hdOn: true, sendWidth: 640, sendHeight: 360))
        XCTAssertTrue(hdPaused(hdOn: true, sendWidth: 360, sendHeight: 640))
        XCTAssertFalse(hdPaused(hdOn: false, sendWidth: 320, sendHeight: 180))
        XCTAssertEqual("HD paused \u{00b7} weak connection", hdLine(on: true, paused: true))
        XCTAssertEqual("Sharper video off", hdLine(on: false, paused: true))
    }

    func testHdIsExplicitAndAutoKeepsBaseline() {
        XCTAssertEqual([.sd], capturePreferences(preferHd: false))
        XCTAssertEqual([.hd, .sd], capturePreferences(preferHd: true))
        XCTAssertEqual(1280, CapturePreference.hd.width)
        XCTAssertEqual(720, CapturePreference.hd.height)
    }

    func testUnsupportedHdFallsBackOnceToBaseline() throws {
        var attempted: [CapturePreference] = []
        let result = try bindCapture(capturePreferences(preferHd: true)) {
            attempted.append($0)
            if $0 == .hd { throw CaptureUnsupported(message: "unsupported") }
        }
        XCTAssertEqual(.sd, result)
        XCTAssertEqual([.hd, .sd], attempted)
    }

    func testSupportedHdDoesNotBindSd() throws {
        var attempts = 0
        XCTAssertEqual(.hd, try bindCapture(capturePreferences(preferHd: true)) { _ in attempts += 1 })
        XCTAssertEqual(1, attempts)
    }

    func testPermissionErrorIsNotHiddenByFallback() {
        struct Denied: Error {}
        var attempts = 0
        XCTAssertThrowsError(try bindCapture(capturePreferences(preferHd: true)) { _ in attempts += 1; throw Denied() }) {
            XCTAssertTrue($0 is Denied)
        }
        XCTAssertEqual(1, attempts)
    }

    func testFailedFallbackDoesNotLoop() {
        var attempts = 0
        XCTAssertThrowsError(try bindCapture(capturePreferences(preferHd: true)) { _ in
            attempts += 1; throw CaptureUnsupported(message: "no stream")
        }) { XCTAssertEqual("no stream", ($0 as? CaptureUnsupported)?.message) }
        XCTAssertEqual(2, attempts)
    }

    func testStoredPreferenceIsSafeAndStable() {
        XCTAssertEqual(.auto, VideoQualityPreference.fromStored(nil))
        XCTAssertEqual(.auto, VideoQualityPreference.fromStored("invalid"))
        XCTAssertEqual(.auto, VideoQualityPreference.fromStored("AUTO"))
        XCTAssertEqual(.preferHd, VideoQualityPreference.fromStored("PREFER_HD"))
    }

    func testHdLabelUsesActualDimensionsAndHandlesPortrait() {
        XCTAssertNil(videoResolutionLabel(width: 0, height: 0))
        XCTAssertNil(videoResolutionLabel(width: -1, height: 720))
        XCTAssertEqual("HD · 1280×720", videoResolutionLabel(width: 1280, height: 720))
        XCTAssertEqual("HD · 720×1280", videoResolutionLabel(width: 720, height: 1280))
        XCTAssertEqual("640×480", videoResolutionLabel(width: 640, height: 480))
        XCTAssertEqual("960×540", videoResolutionLabel(width: 960, height: 540))
        XCTAssertEqual("240×180", videoResolutionLabel(width: 240, height: 180))
    }

    private func snap(_ call: String = "a", sent: UInt64 = 10, received: UInt64 = 8, decoded: UInt64 = 8,
                      admitted: UInt64 = 1000, delivered: UInt64 = 800) -> VideoRateSnapshot {
        VideoRateSnapshot(callId: call, admittedFrames: sent, receivedFrames: received, decodedFrames: decoded,
                          admittedPayloadBytes: admitted, receivedPayloadBytes: delivered)
    }

    func testInitialObservationAndNewCallAreUnknownNotZero() {
        let meter = VideoRateMeter()
        XCTAssertNil(meter.sample(nowNanos: 0, counters: snap()))
        XCTAssertNil(meter.sample(nowNanos: 1_000_000_000, counters: snap("b")))
    }

    func testUsesElapsedTimeAndCounterDeltasNotTargetRate() {
        let meter = VideoRateMeter()
        _ = meter.sample(nowNanos: 0, counters: snap())
        let r = meter.sample(nowNanos: 2_000_000_000, counters: snap(sent: 70, received: 48, decoded: 38, admitted: 126_000, delivered: 100_800))!
        XCTAssertEqual(30, r.admittedFps, accuracy: 0.0001)
        XCTAssertEqual(20, r.receivedFps, accuracy: 0.0001)
        XCTAssertEqual(15, r.decodedFps, accuracy: 0.0001)
        XCTAssertEqual(500, r.admittedPayloadKbps, accuracy: 0.0001)
        XCTAssertEqual(400, r.receivedPayloadKbps, accuracy: 0.0001)
    }

    func testIdleWindowRemainsInDenominator() {
        let meter = VideoRateMeter(); _ = meter.sample(nowNanos: 0, counters: snap())
        let r = meter.sample(nowNanos: 3_000_000_000, counters: snap())!
        XCTAssertEqual(0, r.admittedFps)
        XCTAssertEqual(0, r.receivedPayloadKbps)
    }

    func testEqualClockDoesNotDiscardAccumulatedCounters() {
        let meter = VideoRateMeter(); _ = meter.sample(nowNanos: 10, counters: snap())
        XCTAssertNil(meter.sample(nowNanos: 10, counters: snap(sent: 12)))
        XCTAssertEqual(2, meter.sample(nowNanos: 1_000_000_010, counters: snap(sent: 12))!.admittedFps)
    }

    func testBackwardClockAndCounterResetRebaseline() {
        let meter = VideoRateMeter(); _ = meter.sample(nowNanos: 100, counters: snap())
        XCTAssertNil(meter.sample(nowNanos: 99, counters: snap()))
        XCTAssertNil(meter.sample(nowNanos: 1_000_000_099, counters: snap(sent: 0)))
        XCTAssertEqual(10, meter.sample(nowNanos: 2_000_000_099, counters: snap())!.admittedFps)
    }

    func testNoUnsignedWrapOnLongCounters() {
        let meter = VideoRateMeter()
        _ = meter.sample(nowNanos: 0, counters: snap(admitted: UInt64.max - 20))
        let r = meter.sample(nowNanos: 1_000_000_000, counters: snap(admitted: UInt64.max))!
        XCTAssertEqual(0.16, r.admittedPayloadKbps, accuracy: 0.0001)
        meter.reset()
        XCTAssertNil(meter.sample(nowNanos: 2_000_000_000, counters: snap()))
    }

    func testHdMeteredNoteOncePerCallOnlyWhileSendingHd() {
        XCTAssertTrue(hdMeteredNow(hdOn: true, sending: true, metered: true, saidThisCall: false))
        XCTAssertFalse(hdMeteredNow(hdOn: true, sending: true, metered: true, saidThisCall: true))
        XCTAssertFalse(hdMeteredNow(hdOn: false, sending: true, metered: true, saidThisCall: false))
        XCTAssertFalse(hdMeteredNow(hdOn: true, sending: false, metered: true, saidThisCall: false))
        XCTAssertFalse(hdMeteredNow(hdOn: true, sending: true, metered: false, saidThisCall: false))
        XCTAssertEqual("On mobile data, HD uses more \u{00b7} tap HD", hdMeteredLine)
    }

    func testHdFollowsChipThenSettingThenCore() {
        XCTAssertFalse(effectivePreferHd(setting: false, callChoice: nil))
        XCTAssertTrue(effectivePreferHd(setting: true, callChoice: nil))
        XCTAssertFalse(effectivePreferHd(setting: true, callChoice: false))
        XCTAssertTrue(effectivePreferHd(setting: false, callChoice: true))
        XCTAssertGreaterThan(hdMeteredMs, hdNoteMs)
    }

    /** BT.601 limited range, fixed point as on Android: black, near white (254: x1192 >> 10) and greys stay grey. */
    func testI420ToBgraGreys() {
        // 2x2: Y = 16 (black), 235 (white), 126, 126; one chroma pair at 128 (no colour).
        let src = Data([16, 235, 126, 126, 128, 128])
        var out = [UInt32](repeating: 0, count: 4)
        VideoIo.i420ToBgra(src, 2, 2, &out)
        XCTAssertEqual(0xFF00_0000, out[0])
        XCTAssertEqual(0xFFFE_FEFE, out[1])
        XCTAssertEqual(out[2] & 0xFF, (out[2] >> 8) & 0xFF)
        XCTAssertEqual(out[2] & 0xFF, (out[2] >> 16) & 0xFF)
    }
}
