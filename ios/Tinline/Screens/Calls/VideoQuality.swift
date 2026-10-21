import Foundation

/* VideoQuality.kt: the HD choice, its short lines, capture fallback and the measured video rates. Pure. */

/** Requested sizes, NOT promises about the camera's negotiated buffers or encoded resolution. */
enum CapturePreference: CaseIterable {
    case sd, hd
    var width: Int { self == .hd ? 1280 : 640 }
    var height: Int { self == .hd ? 720 : 480 }
    var wide: Bool { self == .hd }
}

/** Device-local choice. Auto preserves the existing SD capture/adaptation policy. */
enum VideoQualityPreference: String, CaseIterable {
    case auto = "AUTO", preferHd = "PREFER_HD"
    var label: String { self == .auto ? "Auto" : "Prefer HD" }
    static func fromStored(_ value: String?) -> VideoQualityPreference { value.flatMap(Self.init(rawValue:)) ?? .auto }
}

func capturePreferences(preferHd: Bool) -> [CapturePreference] { preferHd ? [.hd, .sd] : [.sd] }

/** This call's HD chip choice wins over Settings > Video "Sharper video" (which the chip never changes). */
func effectivePreferHd(setting: Bool, callChoice: Bool?) -> Bool { callChoice ?? setting }

/** Shortest side of the picture the phone aims for with sharper video (540p). */
let hdTargetShortPhone = 540

/**
 * Sharper video is on but the encoder is sending below the HD size: the sender's own ladder stepped down (weak link, a
 * throttled phone or a lower cap on the peer's side; the core does not say which). No frame sent yet is not paused.
 */
func hdPaused(hdOn: Bool, sendWidth: Int, sendHeight: Int) -> Bool {
    hdOn && sendWidth > 0 && sendHeight > 0 && min(sendWidth, sendHeight) < hdTargetShortPhone
}

/** The HD chip's short line (shown for 2.6 s in the line under the header). */
func hdLine(on: Bool, paused: Bool) -> String {
    if !on { return "Sharper video off" }
    if paused { return "HD paused \u{00b7} weak connection" }
    return "Sharper video on"
}

/**
 * How long the mobile-data note stays in the short line (ms): longer than the chip's 2.6 s echo, since it offers a
 * decision and the person needs time to read it and reach the chip.
 */
let hdMeteredMs: Int64 = 6000

/** The chip's own echo of a tap (ms). */
let hdNoteMs: Int64 = 2600

/** Sharper video is on while the phone is on mobile data: said once per call, calmly; the chip turns it off. */
let hdMeteredLine = "On mobile data, HD uses more \u{00b7} tap HD"

/**
 * Whether to say [hdMeteredLine] now: HD on, my camera sending, the active network metered, not said yet in this call.
 * The core's HD choice itself ignores the network type.
 */
func hdMeteredNow(hdOn: Bool, sending: Bool, metered: Bool, saidThisCall: Bool) -> Bool {
    hdOn && sending && metered && !saidThisCall
}

/** Actual encoded/decoded dimensions, not a capture preference or a bitrate-based guess. */
func videoResolutionLabel(width: Int, height: Int) -> String? {
    if width <= 0 || height <= 0 { return nil }
    let hd = max(width, height) >= 1280 && min(width, height) >= 720
    return "\(hd ? "HD · " : "")\(width)×\(height)"
}

/** The capture setup refused this size or use case: the only failure [bindCapture] retries. */
struct CaptureUnsupported: Error { var message: String }

/** Only unsupported-resolution/use-case failures retry; never hide permission/lifecycle errors. */
func bindCapture(_ preferences: [CapturePreference], bind: (CapturePreference) throws -> Void) throws -> CapturePreference {
    precondition(!preferences.isEmpty)
    for (index, preference) in preferences.enumerated() {
        do { try bind(preference); return preference }
        catch let e as CaptureUnsupported { if index == preferences.count - 1 { throw e } }
    }
    fatalError("unreachable")
}

struct VideoRateSnapshot: Equatable {
    var callId: String
    var admittedFrames: UInt64
    var receivedFrames: UInt64
    var decodedFrames: UInt64
    var admittedPayloadBytes: UInt64
    var receivedPayloadBytes: UInt64
    var counters: [UInt64] { [admittedFrames, receivedFrames, decodedFrames, admittedPayloadBytes, receivedPayloadBytes] }
}

struct VideoRates: Equatable {
    var admittedFps: Double
    var receivedFps: Double
    var decodedFps: Double
    var admittedPayloadKbps: Double
    var receivedPayloadKbps: Double
}

/** One bounded previous sample. No oracle capacity/target bitrate; idle time counts. */
final class VideoRateMeter {
    private var previous: (nanos: Int64, counters: VideoRateSnapshot)?
    private let lock = NSLock()

    func reset() { lock.lock(); previous = nil; lock.unlock() }

    func sample(nowNanos: Int64, counters: VideoRateSnapshot) -> VideoRates? {
        lock.lock(); defer { lock.unlock() }
        guard let old = previous, old.counters.callId == counters.callId, nowNanos >= old.nanos,
              !zip(counters.counters, old.counters.counters).contains(where: { $0 < $1 }) else {
            previous = (nowNanos, counters)
            return nil
        }
        if nowNanos == old.nanos { return nil } // Keep accumulated deltas for the next valid window.
        let seconds = Double(nowNanos - old.nanos) / 1e9
        if seconds <= 0 { previous = (nowNanos, counters); return nil }
        let before = old.counters
        previous = (nowNanos, counters)
        func rate(_ now: UInt64, _ old: UInt64) -> Double { Double(now - old) / seconds }
        return VideoRates(
            admittedFps: rate(counters.admittedFrames, before.admittedFrames),
            receivedFps: rate(counters.receivedFrames, before.receivedFrames),
            decodedFps: rate(counters.decodedFrames, before.decodedFrames),
            admittedPayloadKbps: rate(counters.admittedPayloadBytes, before.admittedPayloadBytes) * 8 / 1000,
            receivedPayloadKbps: rate(counters.receivedPayloadBytes, before.receivedPayloadBytes) * 8 / 1000)
    }
}
