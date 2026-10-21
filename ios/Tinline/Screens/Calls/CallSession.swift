import Foundation

/*
 * CallSession.kt on iOS. One call as the system sees it, and the audio that goes with it.
 *
 * The call is reported to the system's call UI (CallKit on iOS, behind [CallProvider]), which then owns the audio
 * session, interruptions and routing: Bluetooth/wired headsets with their names, headset buttons, cars and watches,
 * and hold when a phone call takes over. If the system refuses the call (no provider, a configuration error) the
 * [engine] takes routing itself instead ([CallAudioEngine.takeRouting]); the call itself never depends on CallKit.
 *
 * Only the display name (the peer's or group's name) is handed to the system, never a DID: it shows on the lock
 * screen, in cars and on watches.
 *
 * Lifecycle: create when the call starts ringing or dialing (`incoming`); [connected] when the core reports it active;
 * [end] exactly once when it is over. System operations run in order; anything issued before the system answers is
 * queued.
 */

/** Why a call ended, as the system wants it (Android's DisconnectCause codes; CXCallEndedReason on iOS). */
enum CallEndCause: String { case local, remote, rejected, missed }

/** Callbacks from the system's side of one call. Main actor. */
@MainActor
protocol SystemCallEvents: AnyObject {
    /** Answered from outside the app (headset button, car, watch, the lock screen). */
    func systemAnswered()
    /** Ended or rejected from outside the app. */
    func systemDisconnected()
    /** Put on hold (another call took over) or resumed. */
    func systemHeld(_ held: Bool)
    /** The routes on offer and the one in use changed. */
    func systemRoutes(_ st: RouteState)
    /** The system microphone mute changed (a car's or headset's mute button). */
    func systemMuted(_ muted: Bool)
}

/** The system's side of one registered call. Each operation returns false when the system refused it. */
@MainActor
protocol SystemCall: AnyObject {
    func answer(video: Bool) async -> Bool
    func setActive() async -> Bool
    func disconnect(_ cause: CallEndCause) async
    func select(_ route: AudioRoute) async -> Bool
}

/** Registers calls with the system call UI. Throws when this device cannot (the session routes in-app). */
@MainActor
protocol CallProvider: AnyObject {
    func addCall(name: String, incoming: Bool, video: Bool, events: SystemCallEvents) async throws -> SystemCall
}

struct CallProviderUnavailable: Error {}

/**
 * TODO(ios-device): CallKit. A CXProvider (CXProviderConfiguration: supportsVideo, maximumCallGroups 1,
 * supportedHandleTypes [.generic], no recents) and a CXCallController: an incoming call is
 * `reportNewIncomingCall(with:update:)` (CXCallUpdate with `localizedCallerName` = the name, a constant generic handle),
 * an outgoing one a CXStartCallAction; answer/end/hold/mute arrive as CXAnswerCallAction / CXEndCallAction /
 * CXSetHeldCallAction / CXSetMutedCallAction and map to [SystemCallEvents]; `provider(_:didActivate:)` is when the audio
 * engine may run. Routes come from AVAudioSession's current route (CallKit has no endpoint API; the route picker is
 * AVRoutePickerView). Incoming calls while suspended need PushKit (VoIP pushes), which the core does not send yet.
 * Until this is built and tried on a device, every call takes the in-app fallback, which is a complete path.
 */
@MainActor
final class CallKitProviderStub: CallProvider {
    func addCall(name: String, incoming: Bool, video: Bool, events: SystemCallEvents) async throws -> SystemCall {
        throw CallProviderUnavailable()
    }
}

@MainActor
final class CallSession: SystemCallEvents {
    private enum Mode { case pending, system, fallback, ended }

    private let engine: CallAudioEngine
    private let incoming: Bool
    /**
     * A video call from the start (placed with the camera on, or rung as one): the system gets it as a video call (so it
     * treats it as one: loudspeaker, no ear detection). A voice call that turns into video later keeps its system type
     * and moves to the speaker with [speakerIfEarpiece].
     */
    private let video: Bool
    /** Group and video calls: start on the loudspeaker when no headset is connected. */
    var preferSpeaker: Bool
    private let onAnswer: () -> Void
    private let onDisconnect: () -> Void
    private let onSystemMute: (Bool) -> Void

    /** Where the audio plays (from the system, or the fallback). */
    var onRoutesChanged: (RouteState) -> Void = { _ in }
    /** The system put this call on hold (e.g. a phone call was answered). */
    var onHeldChanged: (Bool) -> Void = { _ in }
    private(set) var routes = RouteState() { didSet { if routes != oldValue { onRoutesChanged(routes) } } }
    private(set) var held = false { didSet { if held != oldValue { onHeldChanged(held) } } }

    private var mode = Mode.pending
    private var audioOn = false
    /** The system has been told the call is answered/active (answer for incoming, setActive for outgoing). */
    private var activated = false
    private var endedHere = false
    private var placedSpeaker = false
    /** The system disconnected the call itself (car, watch, headset): no fallback, the core call is ending. */
    private var remoteEnded = false
    private var system: SystemCall?
    /** Operations issued before the system answered addCall, run in order once it has. */
    private var pending: [(SystemCall) async -> Void] = []
    private var opChain: Task<Void, Never>?
    private var opsClosed = false
    private var lastMuted: Bool?

    init(provider: CallProvider?, engine: CallAudioEngine, name: String, incoming: Bool, video: Bool = false,
         preferSpeaker: Bool, onAnswer: @escaping () -> Void, onDisconnect: @escaping () -> Void,
         onSystemMute: @escaping (Bool) -> Void) {
        self.engine = engine; self.incoming = incoming; self.video = video; self.preferSpeaker = preferSpeaker
        self.onAnswer = onAnswer; self.onDisconnect = onDisconnect; self.onSystemMute = onSystemMute
        engine.onRoutes = { [weak self] st in
            Task { @MainActor in if let self, self.mode == .fallback { self.routes = st } }
        }
        guard let provider else { fallback("call provider unavailable"); return }
        let display = callOr(name, "Tinline call")
        Task { [weak self] in
            do {
                guard let self else { return }
                let sc = try await provider.addCall(name: display, incoming: incoming, video: video, events: self)
                self.added(sc)
            } catch {
                self?.fallback("addCall failed: \(error)")
            }
        }
    }

    private func added(_ sc: SystemCall) {
        if mode == .ended { Task { await sc.disconnect(.local) }; return }
        if mode == .fallback || remoteEnded { Task { await sc.disconnect(.local) }; return }
        mode = .system
        system = sc
        CallTrace.add("system call added")
        let queued = pending; pending.removeAll()
        for op in queued { chain(op) }
    }

    private func chain(_ op: @escaping (SystemCall) async -> Void) {
        guard let sc = system else { return }
        let prev = opChain
        opChain = Task { await prev?.value; await op(sc) }
    }

    private func op(_ block: @escaping (SystemCall) async -> Void) {
        if opsClosed { return }
        if system != nil { chain(block) } else { pending.append(block) }
    }

    /** The core says the call is up: answer/activate it in the system and start the audio. */
    func connected() {
        if mode == .ended || audioOn { return }
        audioOn = true
        engine.start()
        if mode == .fallback { engine.takeRouting(preferSpeaker: preferSpeaker) }
        activate()
    }

    /** The user answered in the app: tell the system right away so the audio session is set before media flows. */
    func answering() { if incoming { activate() } }

    private func activate() {
        if activated { return }
        activated = true
        let video = video
        if incoming { op { sc in if !(await sc.answer(video: video)) { CallTrace.add("system answer failed") } } }
        else { op { sc in if !(await sc.setActive()) { CallTrace.add("system active failed") } } }
    }

    /** The call is over (any reason). Stops the audio and releases the system call; idempotent. */
    func end(_ cause: CallEndCause) {
        if mode == .ended { return }
        endedHere = true
        let wasSystem = mode == .system
        mode = .ended
        if wasSystem { chain { sc in await sc.disconnect(cause) } }
        CallTrace.add("system end (\(cause.rawValue)\(wasSystem ? "" : ", not in system"))")
        opsClosed = true
        pending.removeAll()
        engine.stop()
        audioOn = false
        held = false
    }

    func select(_ route: AudioRoute) {
        switch mode {
        case .system: op { sc in if !(await sc.select(route)) { CallTrace.add("system route failed") } }
        case .fallback: engine.selectRoute(route)
        default: break
        }
    }

    /** The speaker button: earpiece/headset <-> loudspeaker. */
    func toggleSpeaker() { if let r = CallRoutes.toggleTarget(routes) { select(r) } }

    /** Video: the phone leaves the ear, so the earpiece moves to the loudspeaker (headsets stay). */
    func speakerIfEarpiece() {
        preferSpeaker = true
        if routes.earpiece { toggleSpeaker() }
    }

    private func fallback(_ why: String) {
        if mode == .ended || mode == .fallback || remoteEnded { return }
        CallLog.warn("\(why); routing in-app"); CallTrace.add("system fallback")
        mode = .fallback
        opsClosed = true
        pending.removeAll()
        if audioOn { engine.takeRouting(preferSpeaker: preferSpeaker) }
    }

    // MARK: SystemCallEvents

    func systemAnswered() { activated = true; onAnswer() }

    func systemDisconnected() {
        CallTrace.add("system disconnected\(endedHere ? "" : " by system")")
        remoteEnded = true
        opsClosed = true
        if !endedHere { onDisconnect() }
    }

    func systemHeld(_ h: Bool) {
        CallLog.test("system \(h ? "hold" : "resume")")
        engine.held = h
        held = h
    }

    func systemRoutes(_ st: RouteState) {
        guard mode == .system else { return }
        routes = st
        // The system starts on a headset if there is one, else the earpiece; group and video calls want the speaker.
        if preferSpeaker && !placedSpeaker, st.current?.kind == .earpiece {
            placedSpeaker = true
            if let spk = st.available.first(where: { $0.kind == .speaker }) { select(spk) }
        }
    }

    func systemMuted(_ m: Bool) {
        // Only changes count: our own mute button does not go through the system.
        if let last = lastMuted, last != m { onSystemMute(m) }
        lastMuted = m
    }
}
