import Combine
import Foundation

/*
 * Group calls on the phone (GroupCallScreen.svelte, gcall.svelte.ts): start or join a group's call, the roster, mute,
 * leave, speaker, minimise to the strip, and the calls running in my groups. The core calls and [GroupCallAudio] runs
 * the mic and speaker; this is the presentation state, driven by the same core events. Port of GroupCallUi.kt (the
 * views are in GroupCallViews.swift).
 *
 * Not shown yet because the core doesn't expose them (SCREENS-PORT-STATUS.md): who is speaking, other people's mute,
 * each person's line to the host (§ 6h), how many joined a call I'm not in, and what kind of device the host is.
 */

enum GcPhase { case idle, starting, connecting, active, reconnecting, ended, failed }

/** GroupCallAnnounce.svelte: "Sam started a call in Weekend hike". A quiet banner that settles after [bannerHoldMs]. */
struct GcAnnounce: Equatable {
    var gid: String
    var callId: String
    var by: String
    var bannerOn = true
    var dismissed = false
    /** Rings like an incoming call (up to `groupCallRingMs`): Join / Not now. Then the quiet banner stays. */
    var ringing = false
}

/** gcall.svelte.ts `GCALL_MS.bannerHold`. */
let bannerHoldMs: Int64 = 9000

struct GcState: Equatable {
    var gid: String?
    var callId: String?
    var phase = GcPhase.idle
    /** Everyone in the call (DIDs, me included). */
    var parts: [String] = []
    var sinceMs: Int64?
    var muted = false
    var minimized = false
    var about = false
    /** The holder of the call host device (from `Joined`), and whether it is this phone. */
    var host: String?
    var hosting = false
    var reason = ""
    var note: String?
    /** Calls running in my groups. */
    var live: [String: GroupCall] = [:]
    var endedAt: Int64 = 0
    var announce: GcAnnounce?

    var active: Bool { phase != .idle }
    var connected: Bool { phase == .active || phase == .reconnecting }
    /** gcall.svelte.ts `statusText`. */
    var status: String {
        switch phase {
        case .starting: tr("call_status_finding_host")
        case .connecting: tr("call_status_connecting")
        case .reconnecting: tr("call_status_reconnecting")
        case .ended, .failed: reason
        default: ""
        }
    }
    /** The call is live on this phone (the minimise button and Leave apply). */
    var live4: Bool { [.starting, .connecting, .active, .reconnecting].contains(phase) }
}

func clockOf(_ secs: Int64) -> String { String(format: "%02d:%02d", secs / 60, secs % 60) }

@MainActor
final class GroupCallUi: ObservableObject {
    @Published private(set) var state = GcState()
    /** A call or join waiting for the microphone permission: the Shell asks, then calls [call] again. */
    @Published private(set) var needMic: String?

    private let core: @Sendable () -> CallCore?
    private let audio: GroupCallAudio
    private let platform: CallPlatform
    private let alerts: CallAlerts
    private let directory: CallDirectory
    /** Whether a 1:1 call is on (one call at a time). */
    var oneToOneBusy: () -> Bool = { false }
    /** The app is in front: a group ring posts its alert only when it is not. */
    var appInFront: () -> Bool = { true }
    private var bag: AnyCancellable?
    private var ringtone: CallRing?
    private var ringTask: Task<Void, Never>?
    private lazy var ringMs: Int64 = core().map { Int64($0.groupCallRingMs()) } ?? 30_000

    init(core: @escaping @Sendable () -> CallCore?, audio: GroupCallAudio, platform: CallPlatform, alerts: CallAlerts,
         directory: CallDirectory) {
        self.core = core; self.audio = audio; self.platform = platform; self.alerts = alerts; self.directory = directory
        bag = audio.events.receive(on: DispatchQueue.main).sink { [weak self] e in self?.onEvent(e) }
    }

    /** Previews only: show a given state without a call. */
    func showForPreview(_ s: GcState) { state = s }

    func micAsked() { needMic = nil }

    func refreshLive(_ gid: String) async {
        let core = core
        let calls = try? await callOffMain { try core()?.groupCalls(groupId: gid) }
        let c = (calls ?? nil)?.max { $0.startedAt < $1.startedAt }
        if let c { state.live[gid] = c } else { state.live.removeValue(forKey: gid) }
    }

    /** The group header's call button: start a call, join the running one, or return to mine. */
    func call(_ gid: String, onError: @escaping (String) -> Void) {
        let cur = state
        // Microphone first, as for a 1:1 call: without it the call would join silent with no way to fix it.
        if !(cur.gid == gid && cur.active) && !platform.micGranted { needMic = gid; return }
        if cur.announce?.gid == gid { stopRing() }
        if state.announce?.gid == gid { state.announce = nil }
        if cur.gid == gid && cur.active { state.minimized = false; return }
        if cur.active || oneToOneBusy() { onError(tr("call_group_already_in_call")); return }
        let core = core
        Task {
            await refreshLive(gid)
            let live = state.live[gid]
            state = GcState(gid: gid, callId: live?.callId, phase: live != nil ? .connecting : .starting,
                            sinceMs: live.map { Int64($0.startedAt) }, live: state.live)
            audio.prepare()
            do {
                let id: String = try await callOffMain {
                    guard let c = core() else { throw CallCoreUnavailable() }
                    if let live { try c.groupCallJoin(groupId: gid, callId: live.callId); return live.callId }
                    return try c.groupCallStart(groupId: gid)
                }
                if state.phase == .starting { state.callId = id; state.phase = .connecting }
            } catch {
                audio.unprepare()
                end(failed: true, reason: startFailure("\(error)"), holdMs: 2600)
            }
        }
    }

    func leave() {
        audio.leave()
        end(failed: false, reason: tr("call_group_ended"), holdMs: 0)
    }

    func toggleMute() {
        let m = !state.muted
        state.muted = m
        let core = core
        callBackground("group mute") { core()?.groupCallSetMuted(muted: m) }
    }

    func minimize() { if state.live4 { state.minimized = true; state.about = false } }
    func restore() { state.minimized = false }
    func dismissAnnounce() { stopRing(); state.announce?.dismissed = true }
    /** "Not now": the ring and its alert stop; the quiet "ongoing" pill in the group stays. */
    func notNow() { dismissAnnounce() }
    func about(_ on: Bool) { state.about = on }
    func noteOk() { state.note = nil }

    // MARK: the ring of a started group call

    func groupLabel(_ gid: String) -> String { directory.groupName(gid) ?? tr("call_group_fallback") }

    private func startRing(_ gid: String, _ callId: String) {
        stopRingSound()
        if !directory.quiet { ringtone = platform.startRing() }
        if !appInFront() { postRingAlert() }
        ringTask?.cancel()
        let ms = ringMs
        ringTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(ms) * 1_000_000)
            if !Task.isCancelled, self?.state.announce?.callId == callId { self?.stopRing() }
        }
    }

    private func postRingAlert() {
        guard let a = state.announce, a.ringing else { return }
        alerts.groupRing(gid: a.gid, group: groupLabel(a.gid), by: callFirstName(gcName(a.by)), ringMs: ringMs)
    }

    /** The app left the front while a group call is still ringing: the alert takes over. */
    func appLeftFront() { postRingAlert() }

    private func stopRingSound() { ringtone?.stop(); ringtone = nil }

    /** The ring ends (joined, Not now, ended, or the ring time): the quiet banner / pill stays. */
    func stopRing() {
        ringTask?.cancel(); ringTask = nil
        stopRingSound()
        alerts.cancelGroupRing()
        if state.announce?.ringing == true { state.announce?.ringing = false }
    }

    private func end(failed: Bool, reason: String, holdMs: Int64) {
        let at = callNowMs()
        state.phase = failed ? .failed : .ended
        state.reason = reason
        state.about = false
        state.endedAt = at
        Task { [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(holdMs) * 1_000_000)
            guard let self else { return }
            if self.state.endedAt == at && (self.state.phase == .ended || self.state.phase == .failed) { self.state = GcState(live: self.state.live) }
        }
    }

    private func onEvent(_ e: GroupCallEvent) {
        let s = state
        let mine = s.gid == e.groupId && s.active
        if e.kind == .ended || e.kind == .left { directory.refreshGroupLog() }
        switch e.kind {
        case .announced:
            Task {
                let fresh = state.live[e.groupId]?.callId != e.callId
                await refreshLive(e.groupId)
                guard let c = state.live[e.groupId] else { return }
                // Someone else's call, in a group I'm not on the line in: it rings (the core says whether: not blocked,
                // no group call here; a 1:1 call or "Calls" off in Settings keeps it quiet), then the banner stays.
                guard fresh, c.callId == e.callId, !c.joined, c.starterDid != directory.currentDid, !state.active else { return }
                let core = core
                let ringsCore = (try? await callOffMain { core()?.groupCallRings(groupId: e.groupId, callId: e.callId) ?? false }) ?? false
                let rings = !oneToOneBusy() && directory.callsAlertOn && ringsCore
                state.announce = GcAnnounce(gid: e.groupId, callId: e.callId, by: callOr(e.detail, c.starterDid), ringing: rings)
                if rings { startRing(e.groupId, e.callId) }
                try? await Task.sleep(nanoseconds: UInt64(bannerHoldMs + (rings ? ringMs : 0)) * 1_000_000)
                if state.announce?.callId == e.callId { state.announce?.bannerOn = false }
            }
        case .ended:
            if state.announce?.gid == e.groupId { stopRing() }
            state.live.removeValue(forKey: e.groupId)
            if state.announce?.gid == e.groupId { state.announce = nil }
            if mine && s.callId == e.callId { end(failed: false, reason: tr("call_group_ended"), holdMs: 900) }
        case .startFailed:
            if mine { end(failed: true, reason: startFailure(e.detail), holdMs: 2600) }
        case .joined:
            if mine {
                let me = directory.currentDid
                let blank = e.detail.trimmingCharacters(in: .whitespaces).isEmpty
                state.callId = e.callId
                state.phase = .active
                state.sinceMs = state.sinceMs ?? callNowMs()
                state.host = blank ? nil : e.detail
                state.hosting = blank || e.detail == me
                Task { await refreshLive(e.groupId) }
            }
        case .rosterChanged:
            if mine { state.parts = e.participants }
        case .reconnecting:
            if mine { state.phase = .reconnecting }
        case .left:
            // The call may be over for everyone (or still going without me): re-read, so Join never lingers or lies.
            if !e.groupId.isEmpty { Task { await refreshLive(e.groupId) } }
            if mine && s.phase != .ended && s.phase != .failed {
                end(failed: false, reason: e.detail.contains("Lost") ? tr("call_group_ended_host_lost") : tr("call_group_ended"), holdMs: 900)
            }
        case .hostingStarted:
            state.hosting = true
        case .hostingEnded:
            state.hosting = false
        case .hostingLimited:
            if mine && s.hosting {
                state.note = tr("call_group_battery_limited")
            }
        case .hostingUnlimited:
            state.note = nil
        default:
            break
        }
    }

    /** The core's "no call host available: <why>" shows its why (Rust words); else a short line. */
    private func startFailure(_ d: String) -> String {
        let marker = "no call host available: "
        if let r = d.range(of: marker) { return String(d[r.upperBound...]) }
        if d.lowercased().contains("host") { return tr("call_group_no_host") }
        return tr("call_group_connect_failed")
    }

    /** A person in a group call: "You", their name in the open group or contacts, else "Someone". */
    func gcName(_ did: String) -> String {
        if did == directory.currentDid { return tr("call_person_you") }
        return directory.personName(did) ?? tr("call_person_someone")
    }

    /// Who carries the call, as a case: each has its own whole sentence, so no name or noun is slotted into a translated one.
    enum GcHostKind { case thisPhone, yourOtherDevice, member(String), unknownMember, none }

    func gcHostKind(_ s: GcState) -> GcHostKind {
        if s.hosting { return .thisPhone }
        guard let h = s.host else { return .none }
        if h == directory.currentDid { return .yourOtherDevice }
        guard let name = directory.personName(h), !name.trimmingCharacters(in: .whitespaces).isEmpty else { return .unknownMember }
        return .member(callFirstName(name))
    }

    func groupName(_ gid: String?) -> String { gid.flatMap(directory.groupName) ?? tr("call_group_call_name") }

    func isMe(_ did: String) -> Bool { did == directory.currentDid }

    /** My own name for my roster tile. */
    func myName() -> String { core()?.profile()?.name ?? tr("call_person_you") }
}
