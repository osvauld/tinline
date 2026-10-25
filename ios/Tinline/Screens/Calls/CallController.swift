import Combine
import Foundation

/**
 * Single place that reacts to 1:1 call events: UI state, ringing, alerts, audio. Each call is also registered with the
 * system call UI through a [CallSession], which owns routing. Port of CallController.kt; the Android foreground-service
 * pokes have no iOS counterpart (CallKit keeps a call alive in the background) and are left out.
 *
 * Main actor: core callbacks (`NodeEvents.onIncomingCall/onCallState`, implemented by the Shell) are forwarded here
 * with `Task { @MainActor in ... }`, so the state needs no locks; the blocking core calls run off the main actor.
 */
@MainActor
final class CallController: ObservableObject {
    @Published private(set) var ui: CallUi?
    /** Set when a call ends (after [ui] goes nil); cleared when the next call starts. */
    @Published private(set) var ended: EndedUi?
    /** A second call that arrived during an active one: it rings quietly over the call screen. */
    @Published private(set) var waiting: CallInfo?
    /** When I last pressed End (ms): the call screen plays End's glyph and stays [callEndHoldMs] after the call goes. */
    @Published private(set) var endTapMs: Int64 = 0

    /** Whether the call screen is on top; if not, the waiting call also gets an alert. */
    var callScreenShown = false
    /** Opens the call screen (the Shell presents [CallScreenRoot]). */
    var openCallScreen: () -> Void = {}

    private let core: @Sendable () -> CallCore?
    private let platform: CallPlatform
    private let alerts: CallAlerts
    private let directory: CallDirectory
    private let provider: CallProvider?
    /** The current call's video; set by [CallsEnv] (they refer to each other). */
    weak var video: CallVideo?
    private lazy var audio: CallAudioEngine = CallAudioEngine(core: core, onMicUnavailable: { [weak self] in
        Task { @MainActor in self?.micUnavailable() }
    })

    private var ringtone: CallRing?
    private var vibrating = false
    private var statsTask: Task<Void, Never>?
    private var ringTimeout: Task<Void, Never>?
    private var everActive = false
    private var waitingTone: Task<Void, Never>?
    /** Set by [endAndAnswer]: the call that takes over when the current one ends. */
    private var takeover: CallInfo?
    /** The current call is a video call now ([CallRoutes.isVideo]): loudspeaker, no proximity monitoring. */
    private var videoNow = false
    /** The current call's system registration and audio; replaced per call. */
    private var session: CallSession?
    /** Set when this side ends the call, so the system gets the right cause. */
    private var localEnd: CallEndCause?
    // Events for a call we are still placing arrive before core.call() has returned its id and before `ui` is set; they
    // are held here and replayed (the desktop keeps the same `early` list).
    private var early: [(String, CallState)] = []
    private var placing = false
    private var ringing = false

    /** The call screen's remote-video renderer while it shows video; read by the stats log. */
    weak var renderer: RemoteRenderer?

    init(core: @escaping @Sendable () -> CallCore?, platform: CallPlatform, alerts: CallAlerts, directory: CallDirectory,
         provider: CallProvider?) {
        self.core = core; self.platform = platform; self.alerts = alerts; self.directory = directory; self.provider = provider
    }

    /** The core supplier for the screens' once-a-second reads (video stats). */
    var coreForReads: @Sendable () -> CallCore? { core }

    /** My profile name, for the ended screens' pair of avatars. */
    func myName() -> String { core()?.profile()?.name ?? "" }

    /** Previews only: a made-up call to draw with (never called by the app). */
    func showForPreview(_ u: CallUi?, ended e: EndedUi? = nil, waiting w: CallInfo? = nil) { ui = u; ended = e; waiting = w }

    private func micUnavailable() {
        let msg = platform.micGranted ? tr("call_mic_unavailable") : tr("call_mic_permission_needed")
        ui?.micProblem = msg
    }

    func onIncoming(_ call: CallInfo) {
        if waiting?.callId == call.callId { clearWaiting() }  // it was promoted: an ordinary ring now
        if let cur = ui, cur.isActive, cur.info.callId != call.callId {
            CallLog.test("waiting id=\(call.callId)")
            CallTrace.add("waiting \(shortId(call.callId))")
            waiting = call
            startWaitingTone()
            if !callScreenShown { alerts.waiting(call) }
            return
        }
        CallLog.test("incoming id=\(call.callId)")
        let over = ui.map { " over \(shortId($0.info.callId)) \(CallNotif.of($0))" } ?? ""
        CallTrace.add("incoming \(shortId(call.callId))\(over)")
        everActive = false; ended = nil
        video?.reset()
        ui = CallUi(info: call, state: .ringing)
        newSession(call)
        // The app is open and unlocked on Home: the home header (RingHead) is the incoming call, so no system alert.
        if !(directory.ringHeadShowing && core()?.lockState() == .unlocked) { alerts.incoming(call) }
        startRinging()
        ringTimeout?.cancel()
        let id = call.callId
        ringTimeout = Task { [weak self] in
            // The core ends an unanswered call itself after 60 s (no_answer); this is only the backstop.
            try? await Task.sleep(nanoseconds: UInt64(ringTimeoutMs + 5_000) * 1_000_000)
            guard let self, !Task.isCancelled, let c = self.ui, c.info.callId == id, !c.isActive else { return }
            // Nobody answered: stop ringing for good. Ended then posts the "Missed call" note.
            self.stopRinging(); self.alerts.cancelIncoming()
            let core = self.core
            callBackground("ring timeout decline") { try core()?.decline(callId: id) }
        }
    }

    func onState(_ callId: String, _ state: CallState) {
        CallLog.test("state id=\(callId) state=\(describeCallState(state))")
        CallTrace.add("state \(shortId(callId)) \(describeCallState(state))")
        if let w = waiting, w.callId == callId {
            if case .ended(let reason) = state {
                // Unanswered (or the caller gave up): a missed call like any other.
                clearWaiting()
                if isMissedReason(reason: reason, incoming: true, answered: false) { alerts.missed(name: w.peerName) }
            }
            return
        }
        guard let c = ui, c.info.callId == callId else {
            if placing { early.append((callId, state)) }
            else { CallTrace.add("state \(shortId(callId)) ignored: current \(shortId(ui?.info.callId))") }
            return
        }
        switch state {
        case .active:
            everActive = true
            ringTimeout?.cancel()
            stopRinging(); alerts.cancelIncoming()
            ui?.state = state
            ui?.activeSinceMs = callNowMs()
            audio.muted = ui?.muted ?? false
            if let s = session { s.connected() } else { audio.start() }
            if let u = ui { updateProximity(CallRoutes.proximityWanted(u.routes, video: videoNow)) }
            startStats()
        case .ended(let reason):
            ringTimeout?.cancel()
            stopRinging(); alerts.cancelIncoming()
            statsTask?.cancel(); updateProximity(false)
            video?.reset()
            endSession(c, reason: reason)
            if !everActive && isMissedReason(reason: reason, incoming: c.info.incoming, answered: everActive) {
                alerts.missed(name: c.info.peerName)
            }
            let next = takeover; takeover = nil
            // Our outgoing call yielded to their simultaneous call: no ended screen, theirs rings.
            if next == nil && classifyEnd(reason: reason) != .superseded {
                let s = c.stats
                ended = EndedUi(
                    peerName: c.info.peerName, peerDid: c.info.peerDid, reason: reason, incoming: c.info.incoming,
                    wasActive: everActive, secs: c.activeSinceMs.map { (callNowMs() - $0) / 1000 } ?? 0,
                    direct: s?.direct, bars: (s != nil && c.lossPct != nil) ? qualityBars(rttMs: Int(s!.rttMs), lossPct: c.lossPct!) : nil)
            }
            // End & answer: straight on to the new call, no "call ended" screen in between.
            if let next {
                ui = CallUi(info: next, state: .ringing, answered: true)
                newSession(next)
            } else {
                ui = nil
            }
        default:
            ui?.state = state
        }
    }

    /**
     * Places a call. One press of the video button, one video call: never carried to a later call. The video button
     * makes a video call with my camera on from the start (the core drops frames until video runs, so the camera is
     * warm by then); without the camera permission it still rings as one, with my camera off until it is asked for
     * once video runs (`wantCamera`, for this call only). A voice call keeps the camera off: video off by default.
     */
    @discardableResult
    func place(did: String) async throws -> CallInfo {
        let video = self.video?.wantCamera ?? false
        self.video?.reset()
        if !platform.micGranted { throw MicPermissionNeeded() }
        let camera = video && platform.cameraGranted
        guard let core = core() else { throw CallCoreUnavailable() }
        early.removeAll(); placing = true
        let info: CallInfo
        do {
            info = try await callOffMain { try (video ? core.videoCall(did: did) : core.call(did: did)) }
        } catch {
            placing = false; early.removeAll()
            throw error
        }
        if info.video && !camera {
            callBackground("camera off") { try core.setCamera(on: false) }
            self.video?.wantCamera = true
        }
        everActive = false; ended = nil
        ui = CallUi(info: info, state: .dialing)
        newSession(info)
        placing = false
        let replay = early.filter { $0.0 == info.callId }
        early.removeAll()
        for (id, st) in replay { onState(id, st) }
        openCallScreen()
        return info
    }

    func answer() {
        guard let c = ui else { return }
        ringTimeout?.cancel()
        stopRinging(); alerts.cancelIncoming()
        CallTrace.add("answer \(shortId(c.info.callId))")
        // Marked answered first, so the in-call state derives from the tap on.
        let id = c.info.callId
        if ui?.info.callId == id { ui?.answered = true }
        session?.answering()
        let core = core
        Task { [weak self] in
            do { try await callOffMain { try core()?.answer(callId: id) } } catch {
                CallLog.warn("answer: \(error)")
                CallTrace.add("answer \(shortId(id)) failed")
                // Not answered after all: back to only ringing.
                if let self, self.ui?.info.callId == id, self.ui?.isActive == false { self.ui?.answered = false }
            }
        }
    }

    func decline() {
        guard let c = ui else { return }
        ringTimeout?.cancel()
        stopRinging(); alerts.cancelIncoming()
        CallTrace.add("decline \(shortId(c.info.callId))")
        localEnd = .rejected
        let core = core, id = c.info.callId
        callBackground("decline") { try core()?.decline(callId: id) }
    }

    /** Banner action: refuse the second call; its caller hears "busy" and the current call goes on. */
    func declineWaiting() {
        guard let w = waiting else { return }
        clearWaiting()
        let core = core, id = w.callId
        callBackground("decline waiting") { try core()?.decline(callId: id) }
    }

    /** Banner action: hang up the current call and answer the second one. */
    func endAndAnswer() {
        guard let w = waiting else { return }
        clearWaiting()
        takeover = w
        localEnd = .local
        CallTrace.add("end and answer \(shortId(w.callId))")
        // The current call is still Active (in call); its Ended hands over to `w` marked answered.
        everActive = false
        let core = core, id = w.callId
        Task { [weak self] in
            do { try await callOffMain { try core()?.endAndAnswer(callId: id) } } catch {
                self?.takeover = nil
                CallLog.warn("end and answer: \(error)")
            }
        }
    }

    private func clearWaiting() {
        waitingTone?.cancel(); waitingTone = nil
        alerts.cancelWaiting()
        waiting = nil
    }

    /** The quiet in-call "call waiting" beep, every 4 s, on the call's own audio so it does not disturb it. */
    private func startWaitingTone() {
        waitingTone?.cancel()
        if directory.quiet { return }   // Settings > Sounds > Quiet
        waitingTone = Task { [weak self] in
            while !Task.isCancelled {
                self?.platform.waitingBeep()
                try? await Task.sleep(nanoseconds: 4_000_000_000)
            }
        }
    }

    func hangup() {
        guard let c = ui else { CallTrace.add("hangup: no call"); return }
        CallTrace.add("hangup \(shortId(c.info.callId))")
        endTapMs = callNowMs()
        localEnd = .local
        let core = core, id = c.info.callId
        callBackground("hangup") { try core()?.hangup(callId: id) }
    }

    func toggleMute() {
        guard ui != nil else { return }
        ui?.muted.toggle()
        audio.muted = ui?.muted ?? false
    }

    /** The system microphone mute changed (a car or headset mute button): follow it. */
    private func systemMute(_ callId: String, _ m: Bool) {
        guard ui?.info.callId == callId else { return }
        ui?.muted = m
        audio.muted = m
    }

    /** Answer/hang-up from outside the app (headset button, car, watch). */
    private func systemAnswer(_ callId: String) {
        guard let c = ui else { return }
        if c.info.callId == callId && c.info.incoming && c.isRinging { answer() }
    }

    private func systemDisconnect(_ callId: String) {
        guard let c = ui, c.info.callId == callId else { return }
        if c.info.incoming && c.isRinging { decline() } else { hangup() }
    }

    /** Registers `info` with the system; the routes and hold state it reports feed [ui]. */
    private func newSession(_ info: CallInfo) {
        session?.end(.local)
        localEnd = nil
        let id = info.callId
        videoNow = info.video
        let s = CallSession(
            provider: provider, engine: audio, name: callOr(info.peerName, tr("call_system_call_name")), incoming: info.incoming,
            // A video call (either side pressed Video) is a system video call on the loudspeaker; a headset still wins.
            video: info.video, preferSpeaker: info.video,
            onAnswer: { [weak self] in self?.systemAnswer(id) },
            onDisconnect: { [weak self] in self?.systemDisconnect(id) },
            onSystemMute: { [weak self] m in self?.systemMute(id, m) })
        s.onHeldChanged = { [weak self] h in if self?.ui?.info.callId == id { self?.ui?.held = h } }
        s.onRoutesChanged = { [weak self] st in
            guard let self, self.ui?.info.callId == id else { return }
            self.ui?.routes = st
            if self.ui?.isActive == true { self.updateProximity(CallRoutes.proximityWanted(st, video: self.videoNow)) }
        }
        session = s
    }

    private func endSession(_ cur: CallUi?, reason: String) {
        guard let s = session else { audio.stop(); return }
        session = nil
        let cause: CallEndCause
        if let l = localEnd { cause = l }
        else if let cur, !everActive, cur.info.incoming, isMissedReason(reason: reason, incoming: true, answered: false) { cause = .missed }
        else { cause = .remote }
        localEnd = nil
        s.end(cause)
    }

    /** Video calls are held away from the ear: the camera going on moves the earpiece to the speaker (headsets stay). */
    func speakerForVideo() { session?.speakerIfEarpiece() }

    /**
     * The core's video state changed ([CallVideo.onVideoState]). A call that becomes a video call (either camera on,
     * e.g. a voice call turned video mid-call) moves from the earpiece to the loudspeaker, once; headsets and Bluetooth
     * stay. While it is one, no proximity monitoring (the screen must not go dark under the picture).
     */
    func onVideo(_ v: VideoInfo) {
        guard let u = ui, u.info.callId == v.callId else { return }
        // A VideoInfo can come while it rings (my camera on for "Video"): not decided until negotiated or active.
        let decided = v.negotiated || u.isActive
        let now = CallRoutes.isVideo(placedAsVideo: u.info.video, decided: decided, negotiated: v.negotiated,
                                     localCamera: v.localCamera, remoteCamera: v.remoteCamera)
        let was = videoNow
        videoNow = now
        if CallRoutes.becameVideo(wasVideo: was, isVideo: now) {
            CallLog.test("video_call id=\(v.callId) speaker")
            session?.speakerIfEarpiece()
        }
        if u.isActive { updateProximity(CallRoutes.proximityWanted(u.routes, video: now)) }
    }

    /** "Video" on a ringing video call: my camera on (the core takes it while ringing), then answer. */
    func answerWithVideo() {
        guard let c = ui, c.info.incoming, c.isRinging else { return }
        video?.setCamera(true)
        answer()
    }

    /** Earpiece/headset <-> loudspeaker. */
    func toggleSpeaker() { session?.toggleSpeaker() }

    /** Play the call through `route` (one of [CallUi.routes]). */
    func selectRoute(_ route: AudioRoute) { session?.select(route) }

    /** Screen off against the ear while a call is active on the earpiece. */
    private func updateProximity(_ want: Bool) { platform.setProximity(want) }

    func statsLine(_ s: CallStats) -> String {
        audioStatsLine(s: AudioStats(secs: s.secs, direct: s.direct, rttMs: s.rttMs, sent: s.sent, received: s.received,
                                     lost: s.lost, recovered: s.recovered, concealed: s.concealed, bufferedMs: s.bufferedMs,
                                     rxFreqHz: Double(s.rxFreqHz), rxRms: Double(s.rxRms)))
    }

    private func startStats() {
        statsTask?.cancel()
        let core = core
        statsTask = Task { [weak self] in
            let recent = RecentLoss()
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 1_000_000_000)
                guard let s = try? await callOffMain({ core()?.callStats() }), let self else { continue }
                CallLog.test(self.statsLine(s))
                if self.video?.info?.negotiated == true, let line = self.video?.statsLine(self.renderer) { CallLog.test(line) }
                let loss = recent.push(received: Int64(s.received), lost: Int64(s.lost))
                self.ui?.stats = s
                self.ui?.lossPct = loss
            }
        }
    }

    /** The app left the front (home, screen off, another app): a call still ringing gets its alert now. */
    func appLeftFront() {
        guard let c = ui else { return }
        if ringing && c.info.incoming && c.isRinging && !callScreenShown { alerts.incoming(c.info) }
    }

    private func startRinging() {
        stopRinging()
        ringing = true
        if !directory.callsAlertOn { return }   // Settings > Notifications > Calls off: no ringing
        if !directory.quiet { ringtone = platform.startRing() }
        // Vibrate unless the phone is fully silent; the ringtone itself follows the ring volume.
        platform.vibrate(true); vibrating = true
    }

    private func stopRinging() {
        ringing = false
        ringtone?.stop()
        ringtone = nil
        if vibrating { platform.vibrate(false); vibrating = false }
    }
}
