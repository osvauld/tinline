import Combine
import Foundation

/**
 * Group-call audio (GA-A1): the same mic + speaker loop as a 1:1 call (its own [CallAudioEngine], so nothing on the 1:1
 * path stops it) while this device's user is in a group call ([CallNotif] derives it; hosting alone is not a call on
 * this device). Port of GroupCallAudio.kt; implements the core's `GroupCallEvents` (register with
 * `node.setGroupCallEvents`).
 *
 * Each event only triggers a re-read of the core's state (`groupCallHere`), one at a time in arrival order: the
 * callbacks run on separate core threads and can arrive out of order (a superseded call's Left after the winner's
 * Joined), and the re-read makes that harmless. `pushMicPcm16`/`pullSpeakerPcm16` reach the group call whenever no 1:1
 * call is active.
 *
 * While in the call it is registered with the system as a [CallSession] named after the group, so headsets, cars and
 * watches see it; it starts on the loudspeaker unless a headset is connected.
 */
@MainActor
final class GroupCallAudio: ObservableObject, GroupCallEvents {
    static var name: String { tr("call_group_call_name") }

    private let core: @Sendable () -> CallCore?
    private let platform: CallPlatform
    private let provider: CallProvider?
    private lazy var audio: CallAudioEngine = CallAudioEngine(core: core, onMicUnavailable: { [weak self] in
        Task { @MainActor in self?.micUnavailable() }
    })

    /** The group call this device is in or hosts, as of the last event. */
    @Published private(set) var here: GroupCallHere?
    /** Every group-call event, for the call UI. */
    let events = PassthroughSubject<GroupCallEvent, Never>()
    @Published private(set) var micProblem: String?
    /** Where the group call's audio plays (empty while not in a call). */
    @Published private(set) var routes = RouteState()
    private var session: CallSession?
    /**
     * When [prepare] marked a call that has not shown up yet, else nil. Read by [CallNotif]; counts for
     * [CallNotif.prepareTimeoutMs] at most, so a prepare that never settles cannot keep the call state.
     */
    private(set) var preparedAtMs: Int64?
    /** The group's name for [callName], cached by [reconcile]. */
    private var hereName = GroupCallAudio.name
    private var chain: Task<Void, Never>?

    init(core: @escaping @Sendable () -> CallCore?, platform: CallPlatform, provider: CallProvider?) {
        self.core = core; self.platform = platform; self.provider = provider
    }

    /** In a group call or hosting one. */
    var active: Bool { here != nil }

    nonisolated func onGroupCall(event: GroupCallEvent) {
        Task { @MainActor in self.handle(event) }
    }

    private func handle(_ event: GroupCallEvent) {
        CallLog.test("group-call kind=\(event.kind) group=\(event.groupId) call=\(event.callId)")
        // Why a start failed or a call ended for us: reasons only (Joined/Announced details are DIDs).
        let reasons: [GroupCallEventKind] = [.startFailed, .left, .hostingEnded, .ended]
        let why = reasons.contains(event.kind) ? " " + String(event.detail.prefix(70)) : ""
        CallTrace.add("group \(event.kind) \(shortId(event.callId))\(why)")
        events.send(event)
        reconcile(settles: event.kind == .startFailed || event.kind == .left)
    }

    /**
     * Call right before the user starts or joins a group call: marks this device as about to be in a call. Undone by
     * `StartFailed`/`Left` if the call does not come about, and by the timeout if nothing comes at all; call [unprepare]
     * when the start or join call itself throws.
     */
    func prepare() {
        let at = callNowMs()
        preparedAtMs = at
        CallTrace.add("group prepare")
        Task { [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(CallNotif.prepareTimeoutMs + 500) * 1_000_000)
            if self?.preparedAtMs == at { CallTrace.add("group prepare timed out") }
        }
    }

    func unprepare() { reconcile(settles: true) }

    func leave() {
        let core = core
        callBackground("leave") { core()?.groupCallLeave() }
    }

    /** Group calls start on the loudspeaker unless a headset is connected; this switches explicitly. */
    func setSpeaker(_ on: Bool) { if routes.speaker != on { session?.toggleSpeaker() } }

    func selectRoute(_ route: AudioRoute) { session?.select(route) }

    /** The group call's name for the in-call title: the group's, else "Group call". */
    func callName() -> String { hereName }

    private func startSession(_ name: String) {
        if session != nil { return }
        let s = CallSession(provider: provider, engine: audio, name: name, incoming: false, preferSpeaker: true,
                            onAnswer: {}, onDisconnect: { [weak self] in self?.leave() },
                            onSystemMute: { [weak self] m in self?.audio.muted = m })
        s.onRoutesChanged = { [weak self] st in self?.routes = st }
        session = s
        s.connected()
    }

    private func endSession() {
        guard let s = session else { audio.stop(); return }
        session = nil
        s.end(.local)
        routes = RouteState()
    }

    private func micUnavailable() {
        micProblem = platform.micGranted ? tr("call_mic_unavailable") : tr("call_mic_permission_needed")
    }

    /**
     * Start and stop are idempotent. `settles`: the event means a prepared start or join is over (succeeded or not).
     * Runs after the previous reconcile, so the core is read in event order.
     */
    private func reconcile(settles: Bool) {
        let prev = chain
        let core = core
        chain = Task { [weak self] in
            await prev?.value
            let read: (GroupCallHere?, String?)? = try? await callOffMain {
                guard let c = core() else { return (nil, nil) }
                let h = c.groupCallHere()
                let name = h.flatMap { h in (try? c.groups())?.first { $0.id == h.groupId }?.name }
                return (h, name)
            }
            self?.apply(read?.0 ?? nil, name: read?.1 ?? nil, settles: settles)
        }
    }

    private func apply(_ h: GroupCallHere?, name: String?, settles: Bool) {
        let was = here
        // The name first: whoever re-derives the in-call title reads it as soon as `here` changes.
        if let h, h.groupId != was?.groupId { hereName = callOr(name ?? "", GroupCallAudio.name) }
        // Hosting alone (not yet in it) does not settle a prepare: the join may still be on its way.
        if h?.inCall == true || settles { preparedAtMs = nil }
        here = h
        if h?.inCall == true {
            if was?.inCall != true { micProblem = nil }
            startSession(hereName)
        } else {
            endSession()
        }
    }
}
