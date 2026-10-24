import AVFoundation
import Foundation

/*
 * AudioEngine.kt on iOS. Bridges the device audio to the core: mic -> pushMicPcm16, pullSpeakerPcm16 -> speaker.
 * Both loops are paced by a 20 ms clock so they behave identically when no audio device exists (simulator, or the
 * device stub below): the call stays up, silence goes out, and received frames are still drained.
 *
 * `core` is a supplier because the app can replace its Node (restore over a locked vault); the engine must never hold
 * on to an old one. Each start() gets a new generation; threads of an older generation exit on their own, so a late
 * stop()/start() pair can never leave two engines running.
 *
 * Routing, the audio session and interruptions normally belong to the system call (CallKit, [CallSession]). Only when
 * the call could not be registered there does [takeRouting] make the engine do it itself through [CallAudioRouting].
 */

/** An opened microphone: 48 kHz mono PCM16. `read` fills up to `count` bytes and returns how many (<0 = error). */
protocol CallMicInput: AnyObject {
    func read(into buf: UnsafeMutableRawBufferPointer) -> Int
    func release()
}

/** An opened speaker: 48 kHz mono PCM16. `write` returns bytes taken (<0 = error). */
protocol CallSpeakerOutput: AnyObject {
    func write(_ pcm: Data) -> Int
    func release()
}

/** Opens the device's mic and speaker for voice (echo cancelled). Nil = unavailable: silence goes out / audio is dropped. */
protocol CallAudioDevice: AnyObject, Sendable {
    func openMic() -> CallMicInput?
    func openSpeaker() -> CallSpeakerOutput?
}

/**
 * TODO(ios-device): an AVAudioEngine with voice processing (`inputNode.setVoiceProcessingEnabled(true)`), a tap on the
 * input converted to 48 kHz mono Int16 into a small ring the mic loop reads, and an AVAudioPlayerNode (or
 * AVAudioSourceNode) fed from the speaker loop. Needs the session category `.playAndRecord` / mode `.voiceChat`, which
 * CallKit activates (`provider(_:didActivate:)`). Until then both are unavailable and the loops send silence and drain
 * the received frames, exactly as on an emulator without audio.
 */
final class CallAudioDeviceStub: CallAudioDevice, @unchecked Sendable {
    func openMic() -> CallMicInput? { nil }
    func openSpeaker() -> CallSpeakerOutput? { nil }
}

/** The in-app routing used only when CallKit does not own the call (AudioEngine.kt fallback). */
protocol CallAudioRouting: AnyObject {
    /** Take the audio session for a voice call; route changes are reported through `onChange`. */
    func takeOver(onChange: @escaping () -> Void)
    func routes() -> RouteState
    func apply(_ route: AudioRoute)
    func release()
}

/**
 * The fallback routing through AVAudioSession: the built-in receiver and speaker plus whatever headset the current
 * route has. TODO(ios-device): verify Bluetooth HFP selection (`setPreferredInput` with a `.bluetoothHFP` port) and
 * the route-change notifications on a device; the calls below are the documented ones but have not run here.
 */
final class SessionAudioRouting: CallAudioRouting {
    private var observer: NSObjectProtocol?
    private var speakerOn = false

    func takeOver(onChange: @escaping () -> Void) {
        let s = AVAudioSession.sharedInstance()
        do {
            try s.setCategory(.playAndRecord, mode: .voiceChat, options: [.allowBluetooth])
            try s.setActive(true)
        } catch { CallLog.warn("audio session: \(error)") }
        observer = NotificationCenter.default.addObserver(forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main) { _ in onChange() }
    }

    func routes() -> RouteState {
        let s = AVAudioSession.sharedInstance()
        var avail = [AudioRoute(kind: .earpiece, name: "", key: "earpiece"), AudioRoute(kind: .speaker, name: "", key: "speaker")]
        for p in s.availableInputs ?? [] {
            switch p.portType {
            case .bluetoothHFP, .bluetoothLE: avail.append(AudioRoute(kind: .bluetooth, name: p.portName, key: "in:\(p.uid)"))
            case .headsetMic, .usbAudio: avail.append(AudioRoute(kind: .wired, name: p.portName, key: "in:\(p.uid)"))
            default: break
            }
        }
        let port = s.currentRoute.outputs.first?.portType
        let kind: AudioRoute.Kind
        if port == .builtInSpeaker { kind = .speaker }
        else if port == .builtInReceiver { kind = .earpiece }
        else if port == .bluetoothHFP || port == .bluetoothLE || port == .bluetoothA2DP { kind = .bluetooth }
        else if port == .headphones || port == .headsetMic || port == .usbAudio { kind = .wired }
        else { kind = speakerOn ? .speaker : .earpiece }
        return RouteState(current: avail.first { $0.kind == kind } ?? avail.first, available: avail)
    }

    func apply(_ route: AudioRoute) {
        let s = AVAudioSession.sharedInstance()
        do {
            switch route.kind {
            case .speaker:
                speakerOn = true
                try s.overrideOutputAudioPort(.speaker)
            case .bluetooth, .wired:
                speakerOn = false
                try s.overrideOutputAudioPort(.none)
                let uid = route.key.hasPrefix("in:") ? String(route.key.dropFirst(3)) : ""
                if let port = s.availableInputs?.first(where: { $0.uid == uid }) { try s.setPreferredInput(port) }
            default:
                speakerOn = false
                try s.overrideOutputAudioPort(.none)
                if let builtIn = s.availableInputs?.first(where: { $0.portType == .builtInMic }) { try s.setPreferredInput(builtIn) }
            }
        } catch { CallLog.warn("applyRoute failed: \(error)") }
    }

    func release() {
        if let o = observer { NotificationCenter.default.removeObserver(o) }
        observer = nil
        speakerOn = false
        try? AVAudioSession.sharedInstance().overrideOutputAudioPort(.none)
        try? AVAudioSession.sharedInstance().setActive(false, options: [.notifyOthersOnDeactivation])
    }
}

final class CallAudioEngine: @unchecked Sendable {
    static let rate = 48_000
    static let frameBytes = 1920
    static let frameNs: UInt64 = 20_000_000
    static let maxErrors = 10

    private let core: @Sendable () -> CallCore?
    private let device: CallAudioDevice
    private let routing: CallAudioRouting
    private let onMicUnavailable: @Sendable () -> Void
    private let lock = NSLock()
    private var running = false
    private var generation = 0
    private var _muted = false
    private var _held = false
    private var micThread: Thread?
    private var spkThread: Thread?

    // Fallback routing ([takeRouting]); untouched while CallKit owns the call. Main thread.
    /** Fallback route changes; set by the owner before [takeRouting]. */
    var onRoutes: (RouteState) -> Void = { _ in }
    private var owns = false
    private var preferSpeaker = false

    init(core: @escaping @Sendable () -> CallCore?, device: CallAudioDevice = CallAudioDeviceStub(),
         routing: CallAudioRouting = SessionAudioRouting(), onMicUnavailable: @escaping @Sendable () -> Void = {}) {
        self.core = core; self.device = device; self.routing = routing; self.onMicUnavailable = onMicUnavailable
    }

    var muted: Bool {
        get { lock.lock(); defer { lock.unlock() }; return _muted }
        set { lock.lock(); _muted = newValue; lock.unlock() }
    }
    /** The system put the call on hold (another call took over): send and play silence. */
    var held: Bool {
        get { lock.lock(); defer { lock.unlock() }; return _held }
        set { lock.lock(); _held = newValue; lock.unlock() }
    }

    private func live(_ gen: Int) -> Bool { lock.lock(); defer { lock.unlock() }; return running && generation == gen }

    /** Starts the mic and speaker loops. Routing stays with the system call unless [takeRouting] is called. */
    func start() {
        lock.lock(); defer { lock.unlock() }
        if running { return }
        running = true
        generation += 1
        let gen = generation
        let mic = Thread { [weak self] in self?.micLoop(gen) }
        mic.name = "p2p-mic"; mic.qualityOfService = .userInteractive
        let spk = Thread { [weak self] in self?.speakerLoop(gen) }
        spk.name = "p2p-spk"; spk.qualityOfService = .userInteractive
        micThread = mic; spkThread = spk
        mic.start(); spk.start()
    }

    func stop() {
        lock.lock()
        if running {
            running = false
            generation += 1
            // The loops wake at least every 20 ms and release their device on the way out; a following start() gets a
            // new generation, so old threads never race the new ones for the core's audio.
        }
        micThread = nil; spkThread = nil
        _held = false
        lock.unlock()
        releaseRouting()
    }

    /**
     * The system is not handling this call: take the audio session and routing ourselves and start on
     * [CallRoutes.pickDefault]. Idempotent; undone by [stop]. Main thread.
     */
    func takeRouting(preferSpeaker: Bool) {
        if owns { return }
        owns = true
        self.preferSpeaker = preferSpeaker
        CallLog.test("taking routing (no CallKit), preferSpeaker=\(preferSpeaker)")
        routing.takeOver { [weak self] in self?.routeChanged() }
        applyRoute(CallRoutes.pickDefault(routing.routes().available, preferSpeaker: preferSpeaker))
    }

    /** A headset came or went mid-call: move to it, as a phone call would; a route that left falls back to the default. */
    private func routeChanged() {
        guard owns else { return }
        let st = routing.routes()
        if let cur = st.current, st.available.contains(where: { $0.key == cur.key }) { publish() }
        else { applyRoute(CallRoutes.pickDefault(st.available, preferSpeaker: preferSpeaker)) }
    }

    private func releaseRouting() {
        let release = { [self] in
            guard owns else { return }
            owns = false
            routing.release()
        }
        if Thread.isMainThread { release() } else { DispatchQueue.main.async(execute: release) }
    }

    /** Fallback routing only: play through `route` (one of the last published routes). */
    func selectRoute(_ route: AudioRoute) {
        guard owns else { return }
        applyRoute(routing.routes().available.first { $0.key == route.key })
    }

    private func applyRoute(_ route: AudioRoute?) {
        if let route {
            routing.apply(route)
            CallLog.test("route -> \(route.kind) \(route.name)")
        }
        publish()
    }

    private func publish() { onRoutes(routing.routes()) }

    /** Sleeps until the next 20 ms slot; resyncs if we fell far behind (a blocking device read). */
    private struct Pacer {
        var next = DispatchTime.now().uptimeNanoseconds
        mutating func tick() {
            next += CallAudioEngine.frameNs
            let now = DispatchTime.now().uptimeNanoseconds
            if next > now { Thread.sleep(forTimeInterval: Double(next - now) / 1e9) }
            else if now - next > 200_000_000 { next = now }
        }
    }

    private func micLoop(_ gen: Int) {
        var mic = device.openMic()
        if mic == nil { CallLog.test("audio mic unavailable"); onMicUnavailable() }
        var buf = Data(count: Self.frameBytes)
        let zeros = Data(count: Self.frameBytes)
        var pacer = Pacer()
        var errors = 0
        while live(gen) {
            var filled = false
            if let m = mic {
                var off = 0
                while off < Self.frameBytes && live(gen) {
                    let n = buf.withUnsafeMutableBytes { p in
                        m.read(into: UnsafeMutableRawBufferPointer(rebasing: p[off..<Self.frameBytes]))
                    }
                    if n <= 0 { if n < 0 { errors += 1 }; break }
                    off += n
                    errors = 0
                }
                filled = off == Self.frameBytes
                if errors >= Self.maxErrors {
                    CallLog.warn("mic read failing, reopening")
                    m.release(); mic = device.openMic(); errors = 0
                    if mic == nil { onMicUnavailable() }
                }
            }
            let send = filled && !muted && !held
            core()?.pushMicPcm16(pcm: send ? buf : zeros)
            pacer.tick()
        }
        mic?.release()
    }

    private func speakerLoop(_ gen: Int) {
        var out = device.openSpeaker()
        if out == nil { CallLog.test("audio speaker unavailable") }
        var pacer = Pacer()
        var errors = 0
        while live(gen) {
            if let pcm = core()?.pullSpeakerPcm16(), let o = out {
                // On hold the frames are still drained, but the other call has the speaker.
                let n = o.write(held ? Data(count: pcm.count) : pcm)
                if n < 0 { errors += 1 } else { errors = 0 }
                if errors >= Self.maxErrors {
                    CallLog.warn("speaker write failing, reopening")
                    o.release(); out = device.openSpeaker(); errors = 0
                }
            }
            // The device write paces a real loop; the pacer only matters when it returns early.
            pacer.tick()
        }
        out?.release()
    }
}
