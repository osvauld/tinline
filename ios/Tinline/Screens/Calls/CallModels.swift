import Foundation
import os

/*
 * Plain call models and rules, ported from AudioRoutes.kt, CallController.kt (CallUi, EndedUi), CallParts.kt
 * (status copy), SignalBars.kt (RecentLoss) and Support.kt. No UI, no platform: unit-tested in TinlineTests/Calls.
 */

// MARK: - audio routes (AudioRoutes.kt)

/**
 * Where call audio plays (and, for headsets, where the mic is). The system call ([CallSession]) or the fallback in
 * [CallAudioEngine] fills these in; `key` is only meaningful to whichever made it.
 */
struct AudioRoute: Equatable, Hashable {
    enum Kind: Int, Comparable { case earpiece, speaker, bluetooth, wired, other
        static func < (a: Kind, b: Kind) -> Bool { a.rawValue < b.rawValue }
    }
    var kind: Kind
    var name: String
    var key: String
}

/** The routes on offer right now and the one in use (nil until known). */
struct RouteState: Equatable {
    var current: AudioRoute? = nil
    var available: [AudioRoute] = []
    var speaker: Bool { current?.kind == .speaker }
    /** Held to the ear: the proximity sensor may switch the screen off. */
    var earpiece: Bool { current?.kind == .earpiece }
    /** More than phone and speaker to choose from: the route button opens a list instead of toggling. */
    var hasHeadset: Bool { available.contains { $0.kind == .bluetooth || $0.kind == .wired } }
}

enum CallRoutes {
    /**
     * The route a call starts on: a connected headset first (Bluetooth, then wired), else the loudspeaker for group
     * and video calls (`preferSpeaker`) or the earpiece for 1:1 calls.
     */
    static func pickDefault(_ available: [AudioRoute], preferSpeaker: Bool) -> AudioRoute? {
        available.first { $0.kind == .bluetooth }
            ?? available.first { $0.kind == .wired }
            ?? available.first { $0.kind == (preferSpeaker ? .speaker : .earpiece) }
            ?? available.first { $0.kind == .earpiece || $0.kind == .speaker }
            ?? available.first
    }

    /** The speaker button: earpiece/headset <-> speaker. Back from speaker goes to the default non-speaker route. */
    static func toggleTarget(_ state: RouteState) -> AudioRoute? {
        state.speaker
            ? pickDefault(state.available.filter { $0.kind != .speaker }, preferSpeaker: false)
            : state.available.first { $0.kind == .speaker }
    }

    /**
     * A video call right now (the phone is held away from the ear). Until video is `decided` for the call (the core's
     * first negotiated VideoInfo, or the call active without one) that is how it was placed or rung
     * (`CallInfo.video`); from then on, video runs and either camera is on, so a voice call turns into a video call
     * when a camera goes on, and a video call to a device without video is a voice call.
     */
    static func isVideo(placedAsVideo: Bool, decided: Bool, negotiated: Bool, localCamera: Bool, remoteCamera: Bool) -> Bool {
        !decided ? placedAsVideo : negotiated && (localCamera || remoteCamera)
    }

    /**
     * The call just became a video call: time to move to the loudspeaker ([CallSession.speakerIfEarpiece]: only from
     * the earpiece; a headset or Bluetooth stays). Once per change, so a route picked during the video call is kept.
     */
    static func becameVideo(wasVideo: Bool, isVideo: Bool) -> Bool { isVideo && !wasVideo }

    /** Proximity monitoring (screen off at the ear): only for a voice call on the earpiece, never in a video call. */
    static func proximityWanted(_ state: RouteState, video: Bool) -> Bool { state.earpiece && !video }

    /** Android core-telecom's name for a Bluetooth device it may not read; kept so routes from either side compare. */
    static let genericBluetooth = "Bluetooth Device"

    /** A Bluetooth route shows without its own name. */
    static func unnamedBluetooth(_ st: RouteState) -> Bool {
        st.available.contains { $0.kind == .bluetooth && ($0.name.trimmingCharacters(in: .whitespaces).isEmpty || $0.name == genericBluetooth) }
    }

    /** Short label for the route button and sheet. */
    static func label(_ r: AudioRoute) -> String {
        let blank = r.name.trimmingCharacters(in: .whitespaces).isEmpty
        switch r.kind {
        case .earpiece: return "Phone"
        case .speaker: return "Speaker"
        case .wired: return blank ? "Headset" : r.name
        case .bluetooth: return blank || r.name == genericBluetooth ? "Bluetooth" : r.name
        case .other: return blank ? "Other" : r.name
        }
    }

    /** The speaker slot's caption and icon: "Speaker" until a headset is connected, then the route in use. */
    static func slot(_ routes: RouteState?) -> (caption: String, icon: CallIcon) {
        if let routes, routes.hasHeadset, let cur = routes.current, cur.kind != .speaker {
            return (label(cur), icon(cur.kind))
        }
        return ("Speaker", .speaker)
    }

    static func icon(_ k: AudioRoute.Kind) -> CallIcon {
        switch k {
        case .earpiece: .phone
        case .speaker, .wired, .other: .vol
        case .bluetooth: .bluetooth
        }
    }
}

// MARK: - call state for the UI (CallController.kt)

struct CallUi: Equatable {
    var info: CallInfo
    var state: CallState
    var muted = false
    /** Where the audio plays and what else is available (from the system call, or the in-app fallback). */
    var routes = RouteState()
    /** On hold because another call (e.g. a phone call) took over; both directions are silent. */
    var held = false
    var stats: CallStats? = nil
    /** Loss over the last few seconds ([RecentLoss]), for the signal bars. */
    var lossPct: Double? = nil
    var activeSinceMs: Int64? = nil
    /** Shown on the call screen when the mic cannot be used (denied permission or init failure). */
    var micProblem: String? = nil
    /** I answered (or ended the other call to answer this one): in call from the tap, before Active. */
    var answered = false

    var speaker: Bool { routes.speaker }
    var isActive: Bool { if case .active = state { true } else { false } }
    var isRinging: Bool { if case .ringing = state { true } else { false } }
    var isEnded: Bool { if case .ended = state { true } else { false } }
}

/** What the "Call ended" / "Couldn't reach" screens show once the call is gone. */
struct EndedUi: Equatable {
    var peerName: String
    var peerDid: String
    var reason: String
    var incoming: Bool
    var wasActive: Bool
    var secs: Int64
    var direct: Bool?
    var bars: Int?
    var atMs: Int64 = callNowMs()
}

/** Thrown by [CallController.place] when the microphone permission is missing. */
struct MicPermissionNeeded: Swift.Error, LocalizedError {
    var errorDescription: String? { "Microphone permission needed to place a call" }
}

/** Wall clock in ms, as `System.currentTimeMillis()`. */
func callNowMs() -> Int64 { Int64((Date().timeIntervalSince1970 * 1000).rounded()) }

/** Ended screens stay up this long (the "Call ended" board: "Closes by itself after 4 seconds"). */
let endedAutoCloseMs: Int64 = 4000

/** The core ends an unanswered call itself after 60 s (no_answer); the controller's timer is only the backstop. */
let ringTimeoutMs: Int64 = 60_000

func describeCallState(_ s: CallState) -> String {
    switch s {
    case .ended(let reason): "Ended(\(reason))"
    case .dialing: "Dialing"
    case .ringing: "Ringing"
    case .active: "Active"
    }
}

// MARK: - copy (CallParts.kt)

/** CallScreen.svelte's copy: never claim encryption while dialling or ringing. */
func callStatus(active: Bool, ringing: Bool, reconnecting: Bool, secs: Int64) -> String {
    if active && reconnecting { return "Reconnecting…" }
    if active { return clock(secs: secs) }
    if ringing { return "Ringing…" }
    return "Calling…"
}

func callPrivacy(active: Bool, name: String) -> String {
    active
        ? "End-to-end encrypted. Your voice is encrypted on this device and only decrypted on \(name)’s."
        : "Connecting to \(name). Once the call is connected, it is end-to-end encrypted."
}

/** First word of a name ("Arjun Mehta" -> "Arjun"), as Kotlin's `trim().substringBefore(' ')`. */
func callFirstName(_ name: String) -> String {
    let t = name.trimmingCharacters(in: .whitespaces)
    return t.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init) ?? t
}

/** `ifBlank { fallback }`. */
func callOr(_ s: String, _ fallback: String) -> String {
    s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? fallback : s
}

// MARK: - signal (SignalBars.kt, Support.kt)

/**
 * Loss over the last few seconds, not the whole call, or one bad minute would colour the rest of it.
 * Fed the running `CallStats.received / lost` totals about once a second.
 */
final class RecentLoss {
    private let window: Int
    private var seen: [(Int64, Int64)] = []
    init(window: Int = 5) { self.window = window }

    func push(received: Int64, lost: Int64) -> Double {
        if let last = seen.last, last.0 > received || last.1 > lost { seen.removeAll() }
        seen.append((received, lost))
        while seen.count > window + 1 { seen.removeFirst() }
        let (r0, l0) = seen[0]
        let r = received - r0, l = lost - l0
        return r + l == 0 ? 0 : Double(l) * 100 / Double(r + l)
    }
}

/** The card's loss number: "0", "0.2", "14". */
func lossText(_ pct: Double) -> String {
    let r = (pct * 10).rounded() / 10
    if r.truncatingRemainder(dividingBy: 1) == 0 || r >= 10 { return String(Int64(r.rounded())) }
    return String(format: "%.1f", locale: Locale(identifier: "en_US_POSIX"), r)
}

/** Bars (1-4) from round-trip time and loss (thresholds and words: present's `callQuality`). */
func qualityBars(rttMs: Int, lossPct: Double) -> Int {
    Int(callQuality(rttMs: UInt32(max(0, rttMs)), lossPct: lossPct).bars)
}

/** link.svelte.ts `SIGNAL_WORD`, by strength 1..4. */
func callSignalWord(_ level: Int) -> String { signalWord(bars: UInt8(clamping: level)) }

/** present's `clock` for a seconds count ("1:05"). */
func callClock(_ secs: Int64) -> String { clock(secs: secs) }

// MARK: - the live mark (Mark.kt statusLook, the call part)

/** The mark's three looks (Mark.kt `MarkState`). */
enum CallMarkState { case slack, ringing, taut }
enum CallMarkPhase { case incoming, outgoing, connecting, active, reconnecting }

/** Mark.kt `MarkLook` for a call: sway period (s) and amplitude (deg) are locked to the cue patterns. */
struct CallMarkLook: Equatable {
    var state: CallMarkState
    var swayPeriod: Double? = nil
    var swayAmp: Double? = nil
    var label: String
}

/** The call branch of `statusLook()`: the call phase wins over everything else, so connection does not matter here. */
func callMarkLook(_ phase: CallMarkPhase) -> CallMarkLook {
    switch phase {
    case .incoming: CallMarkLook(state: .ringing, swayPeriod: 1.6, swayAmp: 12, label: "Incoming call")
    case .outgoing: CallMarkLook(state: .ringing, swayPeriod: 2.0, swayAmp: 9, label: "Calling")
    case .connecting: CallMarkLook(state: .ringing, swayPeriod: 1.2, swayAmp: 6, label: "Connecting")
    case .active: CallMarkLook(state: .taut, label: "In a call")
    case .reconnecting: CallMarkLook(state: .ringing, swayPeriod: 2.0, swayAmp: 4, label: "Reconnecting")
    }
}

// MARK: - logs

/** First characters of a call id: enough to tell calls apart in [CallTrace], nothing more. */
func shortId(_ id: String?) -> String { id.map { String($0.prefix(8)) } ?? "-" }

/** The Android `testLog` lines (device tests grep them); here they go to the unified log. */
enum CallLog {
    static let logger = Logger(subsystem: "com.osvauld.tinline", category: "call")
    static func test(_ msg: String) { logger.debug("\(msg, privacy: .public)") }
    static func warn(_ msg: String) { logger.warning("\(msg, privacy: .public)") }
}

/**
 * Bounded in-memory log of call and notification transitions, kept in release builds too (and mirrored to a file) so a
 * stuck call can be explained after the fact. Never put DIDs, names or other personal data in it: short call ids,
 * states and reasons only.
 */
enum CallTrace {
    static let max = 200
    private static let lock = NSLock()
    private static var ring: [String] = []
    private static var file: URL?
    /** Lines appended since the file was last cut back to [max]; the process can live for days. */
    private static var appended = 0
    private static let fmt: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "MM-dd HH:mm:ss.SSS"
        return f
    }()

    /** Process start: reload the last [max] lines of the previous processes, then keep appending. */
    static func start(dir: URL) {
        let f = dir.appendingPathComponent("calltrace.log")
        let old = ((try? String(contentsOf: f, encoding: .utf8)) ?? "").split(separator: "\n").suffix(max).map(String.init)
        lock.lock(); ring.removeAll(); old.forEach(pushLocked); lock.unlock()
        try? old.map { $0 + "\n" }.joined().write(to: f, atomically: true, encoding: .utf8)
        file = f
        add("--- process start")
    }

    static func add(_ msg: String) {
        let line: String = { lock.lock(); defer { lock.unlock() }; return fmt.string(from: Date()) }() + " " + msg
        lock.lock()
        pushLocked(line)
        if let f = file {
            appended += 1
            if appended > max {
                try? ring.map { $0 + "\n" }.joined().write(to: f, atomically: true, encoding: .utf8)
                appended = 0
            } else if let h = try? FileHandle(forWritingTo: f) {
                h.seekToEndOfFile(); h.write(Data((line + "\n").utf8)); try? h.close()
            }
        }
        lock.unlock()
        CallLog.logger.info("\(msg, privacy: .public)")
    }

    /** Pure part of [add] (unit tests). */
    static func push(_ line: String) { lock.lock(); pushLocked(line); lock.unlock() }

    private static func pushLocked(_ line: String) {
        ring.append(line)
        while ring.count > max { ring.removeFirst() }
    }

    static func dump() -> String { lock.lock(); defer { lock.unlock() }; return ring.joined(separator: "\n") }

    static func clear() { lock.lock(); ring.removeAll(); lock.unlock() }
}
