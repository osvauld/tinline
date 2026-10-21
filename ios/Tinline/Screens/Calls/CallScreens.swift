import SwiftUI

/*
 * The 1:1 call screens (CallScreens.kt): the root the Shell presents full screen, incoming (always dark), calling / in
 * call on the colour field, the video call, the call-waiting card, the route sheet, and the ended / couldn't reach /
 * microphone screens.
 */

private let heldLine = "On hold while your other call is on"

/**
 * The call screen (CallActivity.kt). The Shell presents it full screen while [CallController.ui] is set or an ended
 * call is fresh, and dismisses it on `onClose`. `onOpenChat`: Message, their chat in the app (the call screen steps
 * aside to the strip).
 */
struct CallScreenRoot: View {
    @ObservedObject var calls: CallsEnv
    @ObservedObject var controller: CallController
    let onClose: () -> Void
    let onOpenChat: (String) -> Void
    @Environment(\.scenePhase) var scenePhase

    @State var seen = false
    /** The call as it was: my own End holds the screen [callEndHoldMs] after the call goes, so End's glyph is seen. */
    @State var last: CallUi?
    @State var callSeenAt: Int64 = callNowMs()
    @State var tick = 0
    @State var meName = ""

    init(calls: CallsEnv, onClose: @escaping () -> Void, onOpenChat: @escaping (String) -> Void) {
        self.calls = calls; self.controller = calls.controller; self.onClose = onClose; self.onOpenChat = onOpenChat
    }

    var body: some View {
        let ui = controller.ui
        let e = controller.ended
        let tap = controller.endTapMs
        let sinceTap = callNowMs() - tap
        // End pressed during this call (a tap from an earlier call is older than this call's first sight).
        let ending = tap >= callSeenAt
        let holding = ui == nil && last != nil && ending && Double(sinceTap) < callEndHoldMs
        // An outgoing call that fails at once is already over when this screen opens: still show why.
        let showEnded = ui == nil && e != nil && (seen || callNowMs() - e!.atMs < 10_000)
        let _ = tick  // re-read the clock once the hold is over
        ZStack {
            if let u = ui, u.info.incoming, u.isRinging {
                // Their call arrived while ours was open (ours yielded): ring it here.
                IncomingCallContent(name: u.info.peerName, did: u.info.peerDid, video: u.info.video,
                                    onDecline: { controller.decline() },
                                    onAnswer: { calls.platform.requestMic { controller.answer() } },
                                    onVideo: { calls.platform.requestMicAndCamera { controller.answerWithVideo() } })
            } else if let shown = ui ?? (holding ? last : nil) {
                InCallScreen(ui: shown, calls: calls, video: calls.video, onCameraOn: cameraOn, onMinimise: onClose,
                             onMessage: { onOpenChat(shown.info.peerDid) }, ending: ending)
            } else if showEnded, let e {
                EndedRoute(e: e, meName: meName, blocked: calls.directory.blockedByMe(e.peerDid), onClose: onClose,
                           onAgain: { Task { try? await controller.place(did: e.peerDid) } })
            } else {
                Color.clear
            }
        }
        .onChange(of: ui) { _, new in
            if let new {
                if last?.info.callId != new.info.callId { callSeenAt = callNowMs() }
                seen = true; last = new
            }
            closeIfDone()
        }
        .onChange(of: tap) { scheduleHoldEnd() }
        .onAppear {
            if let ui { seen = true; last = ui; callSeenAt = callNowMs() }
            meName = controller.myName()
            controller.callScreenShown = true
            calls.video.screenShown()
        }
        .onDisappear {
            controller.callScreenShown = false
            // No capture in the background: the camera pauses with the screen (and resumes with it).
            calls.video.screenHidden()
        }
        .onChange(of: scenePhase) { _, p in
            if p == .background { calls.video.screenHidden() } else if p == .active { calls.video.screenShown() }
        }
        .task {
            // Opened with nothing to show (no call, and no recent ended call): do not sit there blank.
            try? await Task.sleep(nanoseconds: 1_500_000_000)
            let en = controller.ended
            if controller.ui == nil && (en == nil || callNowMs() - en!.atMs > 10_000) { onClose() }
        }
        .onChange(of: tick) { closeIfDone() }
    }

    private func scheduleHoldEnd() {
        let wait = max(0, callEndHoldMs - Double(callNowMs() - controller.endTapMs))
        Task { try? await Task.sleep(nanoseconds: UInt64(wait * 1_000_000)); tick += 1 }
    }

    private func closeIfDone() {
        let ui = controller.ui
        let ending = controller.endTapMs >= callSeenAt
        let holding = ui == nil && last != nil && ending && Double(callNowMs() - controller.endTapMs) < callEndHoldMs
        let e = controller.ended
        let showEnded = ui == nil && e != nil && (seen || callNowMs() - e!.atMs < 10_000)
        if ui == nil && seen && !showEnded && !holding { onClose() }
    }

    private func cameraOn() {
        calls.platform.requestCamera { ok in
            if ok { calls.video.setCamera(true); controller.speakerForVideo() } else { calls.video.cameraDenied() }
        }
    }
}

// MARK: - incoming (always dark)

/** RoundBtn + LabeledRound (Components.kt): a 64 pt round action with its caption under it, 96 wide. */
struct CallRoundAction<Glyph: View>: View {
    @Environment(\.tin) var tin
    let label: String
    let desc: String
    let bg: Color
    let action: () -> Void
    @ViewBuilder let glyph: () -> Glyph
    var body: some View {
        VStack(spacing: 8) {
            Button(action: action) {
                glyph().frame(width: 64, height: 64).background(Circle().fill(bg)).contentShape(Circle())
            }
            .buttonStyle(.plain)
            .callDepth(.lift, dark: tin.dark)
            .accessibilityLabel(desc)
            Text(label).font(CallFont.figtree(13, .medium)).foregroundStyle(tin.ink).multilineTextAlignment(.center)
                .accessibilityHidden(true)
        }
        .frame(width: 96)
    }
}

struct IncomingCallContent: View {
    let name: String
    let did: String
    var video = false
    let onDecline: () -> Void
    let onAnswer: () -> Void
    var onVideo: (() -> Void)?

    var body: some View {
        IncomingCallBody(name: name, did: did, video: video, onDecline: onDecline, onAnswer: onAnswer, onVideo: onVideo)
            .environment(\.tin, .darkTheme)
            .environment(\.colorScheme, .dark)
    }
}

private struct IncomingCallBody: View {
    @Environment(\.tin) var tin
    let name: String
    let did: String
    let video: Bool
    let onDecline: () -> Void
    let onAnswer: () -> Void
    let onVideo: (() -> Void)?
    var body: some View {
        CallFocus(name: callOr(name, "Unknown"), did: did, active: false, ringing: true, incoming: true, video: video) {
            HStack(spacing: 36) {
                CallRoundAction(label: "Decline", desc: "Decline", bg: tin.callButtonEnd, action: onDecline) {
                    CallHangupGlyph(gone: false, tint: .white).frame(width: 30, height: 30)
                }
                CallRoundAction(label: "Answer", desc: "Answer", bg: tin.callButtonAnswer, action: onAnswer) {
                    CallIcon.phone.image.font(.system(size: 26)).foregroundStyle(.white)
                }
                // A video call can be answered with my camera on; Answer keeps it off.
                if video, let onVideo {
                    CallRoundAction(label: "Video", desc: "Answer with video", bg: tin.callButtonAnswer, action: onVideo) {
                        CallIcon.video.image.font(.system(size: 26)).foregroundStyle(.white)
                    }
                }
            }
            .frame(maxWidth: .infinity)
        }
    }
}

// MARK: - call waiting

/**
 * WaitingCard.svelte: someone else is calling during my call (core: Live::waiting). A calm card just above the
 * controls: avatar 40, "<name> is calling" over "during your call", Decline and End & answer as two equal 44 pt pills,
 * and what End & answer does. No hold: End & answer ends this call. It settles in 12 pt from above (200 ms) and leaves
 * up 8 pt (140 ms).
 */
struct WaitingCard: View {
    @Environment(\.tin) var tin
    @Environment(\.accessibilityReduceMotion) var reduce
    let waiting: CallInfo?
    let currentName: String
    let onDecline: () -> Void
    let onEndAnswer: () -> Void

    var body: some View {
        ZStack(alignment: .bottom) {
            if let l = waiting {
                card(l).transition(.asymmetric(
                    insertion: .opacity.combined(with: .offset(y: reduce ? 0 : -12)).animation(CallMotion.easeOut(200)),
                    removal: .opacity.combined(with: .offset(y: reduce ? 0 : -8)).animation(CallMotion.cubicIn(140))))
            }
        }
        .animation(CallMotion.easeOut(200), value: waiting?.callId)
    }

    private func card(_ l: CallInfo) -> some View {
        let who = callOr(l.peerName, "Unknown")
        let first = callFirstName(currentName)
        return VStack(spacing: 12) {
            HStack(spacing: 12) {
                TinAvatar(did: l.peerDid, size: 40)
                VStack(alignment: .leading, spacing: 0) {
                    Text("\(who) is calling").font(CallFont.figtree(15, .semibold)).foregroundStyle(tin.ink).lineLimit(1)
                    Text("during your call").font(CallFont.sub).foregroundStyle(tin.ink2)
                }
                Spacer(minLength: 0)
            }
            HStack(spacing: 8) {
                pill("Decline", bg: Color.callLerp(tin.sf, tin.ink, 0.09), fg: tin.ink, desc: "Decline", action: onDecline)
                pill("End & answer", bg: tin.callButtonAnswer, fg: .white, desc: "End and answer", action: onEndAnswer)
            }
            HStack(spacing: 4) {
                Text("Ends your call with").lineLimit(1)
                Text(first).lineLimit(1).truncationMode(.tail)
            }
            .font(CallFont.figtree(12)).foregroundStyle(tin.ink2)
            .frame(maxWidth: .infinity)
        }
        .padding(EdgeInsets(top: 14, leading: 14, bottom: 12, trailing: 14))
        .frame(maxWidth: 360)
        .background(SmoothRoundedRectangle(cornerRadius: 22).fill(tin.sfRaised))
        .overlay(SmoothRoundedRectangle(cornerRadius: 22).stroke(tin.ln, lineWidth: 1))
        .callDepth(.pop, dark: tin.dark)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(who) is calling during your call")
    }

    private func pill(_ label: String, bg: Color, fg: Color, desc: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(label).font(CallFont.figtree(13, .bold)).foregroundStyle(fg).lineLimit(1)
                .frame(maxWidth: .infinity, minHeight: 44)
                .background(SmoothRoundedRectangle(cornerRadius: 22).fill(bg))
                .contentShape(SmoothRoundedRectangle(cornerRadius: 22))
        }
        .buttonStyle(.plain)
        .accessibilityLabel(desc)
    }
}

// MARK: - calling / in call

/** Live call state, bound to the controller and video: chooses the voice or video layout. */
struct InCallScreen: View {
    let ui: CallUi
    @ObservedObject var calls: CallsEnv
    @ObservedObject var video: CallVideo
    let onCameraOn: () -> Void
    let onMinimise: () -> Void
    var onMessage: (() -> Void)?
    /** I pressed End: End's glyph plays while the screen holds. */
    var ending = false

    @State var sendSize: (Int, Int)?
    @State var sendLabel: String?
    @State var recvLabel: String?
    @State var codecs: (String, String)?
    @State var wasPaused = false

    var body: some View {
        let controller = calls.controller
        let s = ui.stats
        let loss = s.map { _ in ui.lossPct ?? 0 }
        let active = ui.isActive
        let v = video.info.flatMap { active && $0.negotiated && $0.callId == ui.info.callId ? $0 : nil }
        let hdOn = video.hdOn()
        let inVideo = v.map { $0.localCamera || $0.remoteCamera } ?? false
        let hdIsPaused = hdPaused(hdOn: hdOn, sendWidth: sendSize?.0 ?? 0, sendHeight: sendSize?.1 ?? 0)
        let name = callOr(ui.info.peerName, "Unknown")
        let bars = s.flatMap { st in loss.map { qualityBars(rttMs: Int(st.rttMs), lossPct: $0) } }
        let onCamera = { if v?.localCamera == true { video.setCamera(false) } else { onCameraOn() } }
        let problem = ui.held ? heldLine : (ui.micProblem ?? video.problem)
        let waitingCard = AnyView(WaitingCard(waiting: controller.waiting, currentName: name,
                                              onDecline: { controller.declineWaiting() }, onEndAnswer: { controller.endAndAnswer() }))
        TimelineView(.periodic(from: .now, by: 0.5)) { tl in
            let now = Int64(tl.date.timeIntervalSince1970 * 1000)
            let secs = max(0, (now - (ui.activeSinceMs ?? now)) / 1000)
            if let v, v.localCamera || v.remoteCamera {
                let quality = [v.localCamera ? (video.hdNote?.text ?? sendLabel ?? "Starting video…") : nil, recvLabel]
                    .compactMap { $0 }.joined(separator: "   ")
                VideoCallContent(
                    name: name, did: ui.info.peerDid, secs: secs, direct: s?.direct, bars: bars, reconnecting: s?.reconnecting == true,
                    muted: ui.muted, speaker: ui.speaker, localCamera: v.localCamera, remoteCamera: v.remoteCamera, problem: problem,
                    onMute: { controller.toggleMute() }, onSpeaker: { controller.toggleSpeaker() }, onCamera: onCamera,
                    onFlip: { video.flip() }, onEnd: { controller.hangup() }, onMinimise: onMinimise,
                    remote: AnyView(RemoteVideoView(calls: calls)), selfView: AnyView(CallSelfView(video: video)),
                    routes: ui.routes, onRoute: { controller.selectRoute($0) }, onMessage: onMessage, waitingCard: waitingCard,
                    videoQuality: quality.isEmpty ? nil : quality, sendCodec: codecs?.0 ?? "", recvCodec: codecs?.1 ?? "",
                    hd: hdOn, hdPaused: hdIsPaused, onHd: { video.setHd(!hdOn, paused: hdIsPaused) },
                    ending: ending, rttMs: s.map { Int($0.rttMs) }, lossPct: loss)
                    .environment(\.tin, .darkTheme)
                    .environment(\.colorScheme, .dark)
            } else {
                InCallContent(
                    name: name, did: ui.info.peerDid, active: active, ringing: ui.isRinging, secs: secs, direct: s?.direct, bars: bars,
                    muted: ui.muted, speaker: ui.speaker, micProblem: problem,
                    onMute: { controller.toggleMute() }, onSpeaker: { controller.toggleSpeaker() }, onEnd: { controller.hangup() },
                    onMinimise: onMinimise, reconnecting: s?.reconnecting == true,
                    onCamera: v != nil ? onCamera : nil, routes: ui.routes, onRoute: { controller.selectRoute($0) },
                    rttMs: s.map { Int($0.rttMs) }, lossPct: loss, onMessage: onMessage,
                    waiting: controller.waiting != nil, waitingCard: waitingCard, ending: ending)
            }
        }
        .onChange(of: inVideo, initial: true) { _, on in budget(on) }
        .onDisappear { stopBudget?() ; stopBudget = nil }
        // What I send (once a frame of this camera start is out) and the core's HD choice, once a second.
        .task(id: SendKey(callId: ui.info.callId, local: v?.localCamera == true, hd: hdOn)) {
            sendSize = nil; sendLabel = nil
            guard v?.localCamera == true else { return }
            var initialFrames: UInt64?
            let core = calls.controller.coreForReads
            while !Task.isCancelled {
                if let st = try? await callOffMain({ core()?.videoStats() }) {
                    video.hdTick(st)
                    if initialFrames == nil { initialFrames = st.framesSent }
                    // Don't label cached dimensions from before this camera start as current HD.
                    if st.framesSent > initialFrames! {
                        sendSize = (Int(st.sendWidth), Int(st.sendHeight))
                        let l = videoSendLabel(stats: st)
                        sendLabel = l.isEmpty ? nil : l
                    }
                }
                try? await Task.sleep(nanoseconds: 1_000_000_000)
            }
        }
        // What I receive ("Their video · 540p · 24 fps") and the codec per direction, whether or not my camera is on.
        .task(id: RecvKey(callId: ui.info.callId, negotiated: v?.negotiated == true)) {
            recvLabel = nil; codecs = nil
            guard v?.negotiated == true else { return }
            let core = calls.controller.coreForReads
            while !Task.isCancelled {
                if let st = try? await callOffMain({ core()?.videoStats() }) {
                    let r = videoRecvLabel(stats: st)
                    recvLabel = r.isEmpty ? nil : r
                    codecs = (videoCodecLabel(codec: st.sendCodec, hardware: st.sendHardware),
                              videoCodecLabel(codec: st.recvCodec, hardware: st.recvHardware))
                }
                try? await Task.sleep(nanoseconds: 1_000_000_000)
            }
        }
        // Tell the core what the camera captures (its HD rule looks at a 640x480 capture only).
        .onChange(of: SyncKey(hd: hdOn, negotiated: v?.negotiated == true, callId: ui.info.callId), initial: true) { _, k in
            if k.negotiated { video.syncHd(k.hd) }
        }
        // The ladder stepping below HD while HD is on: say so once, when it happens.
        .onChange(of: hdIsPaused) { _, p in
            if p && !wasPaused { video.say(hdLine(on: true, paused: true)) }
            wasPaused = p
        }
        .task(id: video.hdNote?.id) {
            guard let n = video.hdNote else { return }
            try? await Task.sleep(nanoseconds: UInt64(n.ms) * 1_000_000)
            video.noteShown(n.id)
        }
    }

    @State var stopBudget: (() -> Void)?
    private func budget(_ on: Bool) {
        stopBudget?(); stopBudget = nil
        if on { stopBudget = video.watchBudget() }
    }

    private struct SendKey: Equatable { let callId: String; let local: Bool; let hd: Bool }
    private struct RecvKey: Equatable { let callId: String; let negotiated: Bool }
    private struct SyncKey: Equatable { let hd: Bool; let negotiated: Bool; let callId: String }
}

/** The voice call screen: calling, ringing and in call, on the colour field. */
struct InCallContent: View {
    let name: String
    let did: String
    let active: Bool
    let ringing: Bool
    let secs: Int64
    let direct: Bool?
    let bars: Int?
    let muted: Bool
    let speaker: Bool
    let micProblem: String?
    let onMute: () -> Void
    let onSpeaker: () -> Void
    let onEnd: () -> Void
    let onMinimise: () -> Void
    var reconnecting = false
    /** Set when the call has video: the camera button (the camera is off on this screen). */
    var onCamera: (() -> Void)?
    /** The live routes; with a headset connected the speaker slot opens the route sheet instead of toggling. */
    var routes: RouteState?
    var onRoute: (AudioRoute) -> Void = { _ in }
    /** The numbers About this call shows (round trip, recent loss). */
    var rttMs: Int?
    var lossPct: Double?
    var onMessage: (() -> Void)?
    /** A second caller is waiting over this call (the field shows its waiting state). */
    var waiting = false
    /** The call-waiting card, placed just above the controls (it grows upward; nothing else moves). */
    var waitingCard: AnyView?
    var ending = false
    var startWithSheet = false
    var startWithInfo = false

    @State var routeSheet = false
    @State var info = false

    var body: some View {
        let slot = CallRoutes.slot(routes)
        ZStack {
            CallFocus(name: callOr(name, "Unknown"), did: did, active: active, ringing: ringing, secs: secs, reconnecting: reconnecting,
                      problem: micProblem, onMinimise: onMinimise, onInfo: { info = true }, signal: bars ?? 0, direct: direct,
                      onMessage: onMessage, waiting: waiting, muted: muted, waitingCard: waitingCard) {
                CallControls(muted: muted, speaker: speaker, camera: false, onMute: onMute,
                             onSpeaker: { if routes?.hasHeadset == true { routeSheet = true } else { onSpeaker() } },
                             onCamera: onCamera, onFlip: nil, onEnd: onEnd, routeCaption: slot.caption, routeIcon: slot.icon, ending: ending)
            }
            if info {
                CallInfoCard(name: name, active: active, onDismiss: { info = false }, reconnecting: reconnecting, signal: bars ?? 0,
                             direct: direct, rttMs: rttMs, lossPct: lossPct)
            }
        }
        .onAppear { if startWithSheet { routeSheet = true }; if startWithInfo { info = true } }
        .sheet(isPresented: $routeSheet) {
            AudioRouteSheet(speaker: speaker, routes: routes, relayed: active && direct == false,
                            onPick: { wantSpeaker in if wantSpeaker != speaker { onSpeaker() }; routeSheet = false },
                            onRoute: { onRoute($0); routeSheet = false })
        }
    }
}

/** "Play call through": the routes on offer, earpiece first, then speaker, then headsets. */
struct AudioRouteSheet: View {
    @Environment(\.tin) var tin
    let speaker: Bool
    let routes: RouteState?
    let relayed: Bool
    let onPick: (Bool) -> Void
    let onRoute: (AudioRoute) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Play call through").font(CallFont.figtree(20, .bold)).foregroundStyle(tin.ink)
                .padding(EdgeInsets(top: 12, leading: 8, bottom: 8, trailing: 0))
            if let routes, !routes.available.isEmpty {
                // Earpiece first, then speaker, then headsets, as a phone's own call screen orders them.
                ForEach(routes.available.sorted { $0.kind < $1.kind }, id: \.key) { r in
                    RouteOption(icon: CallRoutes.icon(r.kind), label: CallRoutes.label(r), on: r.key == routes.current?.key) { onRoute(r) }
                }
            } else {
                RouteOption(icon: .phone, label: "Phone", on: !speaker) { onPick(false) }
                RouteOption(icon: .vol, label: "Speaker", on: speaker) { onPick(true) }
            }
            if relayed {
                CallHint(text: "Relayed: a direct line wasn’t possible on this network, so an encrypted relay is passing the call along. It can’t hear you.")
                    .padding(EdgeInsets(top: 12, leading: 8, bottom: 0, trailing: 8))
            }
        }
        .padding(EdgeInsets(top: 12, leading: 24, bottom: 24, trailing: 24))
        .frame(maxWidth: .infinity, alignment: .leading)
        .presentationDetents([.medium])
        .presentationDragIndicator(.visible)
        .presentationBackground(tin.sfRaised)
        .presentationCornerRadius(24)
    }
}

private struct RouteOption: View {
    @Environment(\.tin) var tin
    let icon: CallIcon
    let label: String
    let on: Bool
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            HStack(spacing: 16) {
                icon.image.font(.system(size: 20)).foregroundStyle(on ? tin.onPrc : tin.ink).padding(.leading, 8)
                Text(label).font(TinType.bodyL).foregroundStyle(on ? tin.onPrc : tin.ink).frame(maxWidth: .infinity, alignment: .leading)
                if on { CallIcon.check.image.foregroundStyle(tin.onPrc).padding(.trailing, 8).accessibilityLabel("Selected") }
            }
            .padding(.horizontal, 8)
            .frame(minHeight: 60)
            .background(SmoothRoundedRectangle(cornerRadius: 14).fill(on ? tin.prc : .clear))
            .contentShape(SmoothRoundedRectangle(cornerRadius: 14))
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(on ? [.isSelected] : [])
    }
}

// MARK: - video call

/**
 * In-call screen with video: the peer's picture full screen (their avatar while their camera is off), our camera small
 * in the corner, controls over a scrim. Always dark.
 */
struct VideoCallContent: View {
    @Environment(\.tin) var tin
    let name: String
    let did: String
    let secs: Int64
    let direct: Bool?
    let bars: Int?
    let reconnecting: Bool
    let muted: Bool
    let speaker: Bool
    let localCamera: Bool
    let remoteCamera: Bool
    let problem: String?
    let onMute: () -> Void
    let onSpeaker: () -> Void
    let onCamera: () -> Void
    let onFlip: () -> Void
    let onEnd: () -> Void
    let onMinimise: () -> Void
    let remote: AnyView
    let selfView: AnyView
    var routes: RouteState?
    var onRoute: (AudioRoute) -> Void = { _ in }
    var onMessage: (() -> Void)?
    var waitingCard: AnyView?
    var videoQuality: String?
    var sendCodec = ""
    var recvCodec = ""
    var hd = false
    var hdPaused = false
    var onHd: () -> Void = {}
    var ending = false
    var rttMs: Int?
    var lossPct: Double?

    @State var info = false
    @State var routeSheet = false
    @State var barH: CGFloat = 0

    var body: some View {
        ZStack {
            callVideoBg.ignoresSafeArea()
            if remoteCamera { remote.ignoresSafeArea() }
            else {
                VStack(spacing: 12) {
                    TinAvatar(did: did, size: 120)
                    Text("\(name)’s camera is off").font(CallFont.figtree(17)).foregroundStyle(tin.ink2)
                }
            }
            VStack(spacing: 0) {
                top
                Spacer(minLength: 0)
            }
            if localCamera {
                // Self-view: our camera (the front one mirrored), top right under the bar.
                VStack {
                    HStack {
                        Spacer()
                        ZStack(alignment: .topLeading) {
                            selfView
                            HdChip(on: hd, paused: hdPaused, action: onHd)
                        }
                        .frame(width: 104, height: 140)
                        .background(Color.black)
                        .clipShape(SmoothRoundedRectangle(cornerRadius: 16))
                        .overlay(SmoothRoundedRectangle(cornerRadius: 16).stroke(Color.white.opacity(0.25), lineWidth: 1))
                    }
                    .padding(.top, 72).padding(.trailing, 12)
                    Spacer()
                }
            }
            VStack {
                Spacer()
                bottom
                    .background(GeometryReader { g in Color.clear.onAppear { barH = g.size.height }.onChange(of: g.size.height) { barH = g.size.height } })
            }
            // WaitingCard: 22 pt above the controls bar, growing upward.
            if let waitingCard {
                VStack { Spacer(); waitingCard.padding(.horizontal, 20).padding(.bottom, barH + 22) }
            }
            if info {
                CallInfoCard(name: name, active: true, onDismiss: { info = false }, reconnecting: reconnecting, signal: bars ?? 0,
                             direct: direct, rttMs: rttMs, lossPct: lossPct, video: true,
                             sendCodec: localCamera ? sendCodec : "", recvCodec: remoteCamera ? recvCodec : "")
            }
        }
        .sheet(isPresented: $routeSheet) {
            AudioRouteSheet(speaker: speaker, routes: routes, relayed: direct == false,
                            onPick: { wantSpeaker in if wantSpeaker != speaker { onSpeaker() }; routeSheet = false },
                            onRoute: { onRoute($0); routeSheet = false })
        }
    }

    /** Top: scrim, back, name and time, message, the signal (About this call). */
    private var top: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 6) {
                CallTopButton(icon: .back, label: "Back. The call continues", tint: .white, action: onMinimise)
                VStack(alignment: .leading, spacing: 0) {
                    Text(name).font(TinType.titleM).foregroundStyle(.white).lineLimit(1)
                    Text(reconnecting ? "Reconnecting…" : callClock(secs)).font(CallFont.mono(14)).foregroundStyle(.white.opacity(0.8))
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                if let onMessage { CallTopButton(icon: .chat, label: "Message. The call continues", tint: .white, action: onMessage) }
                Button { info = true } label: {
                    CallSignalBars(level: reconnecting ? 0 : bars ?? 0, color: .white, wave: reconnecting)
                        .frame(width: 44, height: 44).contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(bars != nil && !reconnecting
                    ? "About this call. \(callSignalWord(bars!)) signal, \(direct == true ? "direct" : "relayed")." : "About this call")
            }
            .padding(EdgeInsets(top: 8, leading: 4, bottom: 0, trailing: 12))
            // One line, always: a longer line (a suggestion) is cut, never wrapped, so nothing below it moves.
            if let videoQuality {
                Text(videoQuality).font(CallFont.mono(12)).foregroundStyle(.white.opacity(0.8)).lineLimit(1).truncationMode(.tail)
                    .padding(.leading, 52).padding(.trailing, 12)
            }
        }
        .padding(.bottom, 24)
        .background(LinearGradient(colors: [.black.opacity(0.55), .clear], startPoint: .top, endPoint: .bottom).ignoresSafeArea(edges: .top))
    }

    /** Bottom: problem line and controls. */
    private var bottom: some View {
        let slot = CallRoutes.slot(routes)
        return VStack(spacing: 16) {
            Text(problem ?? " ").font(TinType.bodyM).foregroundStyle(tin.er).multilineTextAlignment(.center)
                .lineLimit(2, reservesSpace: true).frame(maxWidth: .infinity, minHeight: 38).padding(.horizontal, 24)
            CallControls(muted: muted, speaker: speaker, camera: localCamera, onMute: onMute,
                         onSpeaker: { if routes?.hasHeadset == true { routeSheet = true } else { onSpeaker() } },
                         onCamera: onCamera, onFlip: onFlip, onEnd: onEnd, routeCaption: slot.caption, routeIcon: slot.icon, ending: ending)
                .padding(.horizontal, 20)
        }
        .padding(.top, 32).padding(.bottom, 28)
        .background(LinearGradient(colors: [.clear, .black.opacity(0.6)], startPoint: .top, endPoint: .bottom).ignoresSafeArea(edges: .bottom))
    }
}

/** The peer's decoded frames, pulled on the display clock by a [RemoteRenderer] while shown. */
struct RemoteVideoView: View {
    @ObservedObject var calls: CallsEnv
    @State var frame: CGImage?
    @State var renderer: RemoteRenderer?
    // Its own id: the call screen and the strip's tile can both be shown, and the core asks for the largest one shown
    // (one reported hidden must not hide the other).
    @State var viewId: UInt32 = 0
    @State var size: CGSize = .zero
    @Environment(\.displayScale) var scale

    var body: some View {
        GeometryReader { g in
            ZStack {
                if let frame {
                    // Fill, cropping the picture's overflow and keeping its aspect.
                    Image(decorative: frame, scale: 1).resizable().aspectRatio(contentMode: .fill)
                        .frame(width: g.size.width, height: g.size.height).clipped()
                } else {
                    Text("Connecting video…").font(TinType.bodyL).foregroundStyle(.white.opacity(0.7))
                        .frame(width: g.size.width, height: g.size.height)
                }
            }
            .onAppear { size = g.size; start() }
            .onChange(of: g.size) { _, s in size = s; report(true) }
        }
        .onDisappear { stop() }
    }

    private func start() {
        if viewId == 0 { viewId = calls.video.newViewId() }
        let r = RemoteRenderer(io: calls.video.io) { img in frame = img }
        renderer = r
        calls.controller.renderer = r
        report(true)
    }

    private func stop() {
        renderer?.stop()
        if calls.controller.renderer === renderer { calls.controller.renderer = nil }
        renderer = nil
        report(false)
    }

    private func report(_ visible: Bool) {
        calls.video.setView(viewId, width: Int(size.width * scale), height: Int(size.height * scale), visible: visible)
    }
}

/**
 * The HD chip (playground HdChip.svelte): "HD", 24 pt tall, radius 7, 1.5 pt outline. Off = outline, on = filled (white
 * over video), paused = on but dimmed to 55 % while the sender is below the HD size. The 48 pt square is the touch target.
 */
struct HdChip: View {
    let on: Bool
    let paused: Bool
    let action: () -> Void
    var body: some View {
        let label = paused ? "Sharper video on, paused while the connection is weak" : on ? "Sharper video on, up to 540p" : "Sharper video off"
        let a = paused ? 0.55 : 1.0
        Button(action: action) {
            Text("HD").font(CallFont.mono(12, .bold)).kerning(0.5)
                .foregroundStyle(on ? Color(hex: 0x111111).opacity(a) : .white.opacity(a))
                .padding(.horizontal, 7)
                .frame(minWidth: 36, minHeight: 24, maxHeight: 24)
                .background(RoundedRectangle(cornerRadius: 7).fill(on ? .white.opacity(a) : .clear))
                .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(.white.opacity(a), lineWidth: 1.5))
                .frame(width: 48, height: 48)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
        .accessibilityValue(on ? "On" : "Off")
        .accessibilityAddTraits(.isToggle)
    }
}

/** Our camera: the preview (mirrored for the front camera) plus the feed to the core, while shown. */
struct CallSelfView: View {
    @ObservedObject var video: CallVideo
    @State var layer: CALayer?
    var body: some View {
        // A new HD choice (the chip's or the core's) restarts the capture at once (the picture warms up for a moment).
        CallPreviewLayerView(layer: layer)
            .task(id: SelfKey(front: video.front, hd: video.hdOn())) {
                video.startCamera()
                layer = video.camera.previewLayer
            }
            .onDisappear { video.stopCamera(); layer = nil }
    }
    private struct SelfKey: Equatable { let front: Bool; let hd: Bool }
}

// MARK: - ended / couldn't reach / mic needed

struct EndedRoute: View {
    let e: EndedUi
    let meName: String
    let blocked: Bool
    let onClose: () -> Void
    let onAgain: () -> Void
    var body: some View {
        if classifyEnd(reason: e.reason) == .unreachable && !e.wasActive {
            UnreachableContent(name: e.peerName, did: e.peerDid, meName: meName, onAgain: onAgain, onClose: onClose)
        } else {
            EndedContent(name: e.peerName, did: e.peerDid, meName: meName,
                         reasonText: endReasonText(reason: e.reason, name: e.peerName, incoming: e.incoming, blockedByMe: blocked) ?? "Call ended",
                         secs: e.wasActive ? e.secs : nil, direct: e.direct, bars: e.bars, onAgain: onAgain, onClose: onClose)
        }
    }
}

/** Me and them joined by the amber line (Components.kt PairAvatars): mine 64, theirs 96. */
private struct CallPairAvatars: View {
    @Environment(\.tin) var tin
    let meName: String
    let did: String
    var lineOpacity = 1.0
    var theirs: AnyView?
    var body: some View {
        HStack(spacing: 0) {
            Circle().fill(tin.prc).frame(width: 64, height: 64)
                .overlay(Text(initials(name: meName)).font(CallFont.figtree(22, .semibold)).foregroundStyle(tin.onPrc))
                .accessibilityHidden(true)
            Capsule().fill(tin.thread.opacity(lineOpacity)).frame(width: 36, height: 3)
            theirs ?? AnyView(TinAvatar(did: did, size: 96))
        }
    }
}

struct EndedContent: View {
    @Environment(\.tin) var tin
    let name: String
    let did: String
    let meName: String
    let reasonText: String
    let secs: Int64?
    let direct: Bool?
    let bars: Int?
    let onAgain: () -> Void
    let onClose: () -> Void
    var autoClose = true

    var body: some View {
        VStack(spacing: 0) {
            CallCentredScroll {
                VStack(spacing: 10) {
                    CallPairAvatars(meName: callOr(meName, "?"), did: did)
                    Text("Call ended").font(CallFont.figtree(30, .bold, .title)).foregroundStyle(tin.ink).padding(.top, 16)
                    Text(reasonText).font(CallFont.figtree(17)).foregroundStyle(tin.ink2).multilineTextAlignment(.center)
                    if let secs {
                        HStack(spacing: 8) {
                            CallBadge(text: callClock(secs), kind: .mono)
                            if direct == true { CallBadge(text: "Direct", kind: .direct) }
                            if direct == false { CallBadge(text: "Relayed", kind: .relayed) }
                            if let bars { CallBadge(text: "\(callSignalWord(bars)) signal", kind: .neutral) }
                        }
                        .padding(.top, 8)
                    }
                }
                .padding(.horizontal, 24)
            }
            VStack(spacing: 8) {
                CallButton("Call again", style: .outlined, action: onAgain)
                CallButton("Close", style: .text, action: onClose)
                if autoClose { CallHint(text: "Closes by itself after 4 seconds.", align: .center) }
            }
            .padding(EdgeInsets(top: 0, leading: 24, bottom: 48, trailing: 24))
        }
        .background(tin.bg.ignoresSafeArea())
        .task {
            guard autoClose else { return }
            try? await Task.sleep(nanoseconds: UInt64(endedAutoCloseMs) * 1_000_000)
            if !Task.isCancelled { onClose() }
        }
    }
}

struct UnreachableContent: View {
    @Environment(\.tin) var tin
    let name: String
    let did: String
    let meName: String
    let onAgain: () -> Void
    let onClose: () -> Void

    var body: some View {
        let blank = name.trimmingCharacters(in: .whitespaces).isEmpty
        let who = blank ? "them" : name
        VStack(spacing: 0) {
            CallCentredScroll {
                VStack(spacing: 12) {
                    CallPairAvatars(meName: callOr(meName, "?"), did: did, lineOpacity: 0.5, theirs: AnyView(
                        Circle().fill(tin.sf3).frame(width: 96, height: 96)
                            .overlay(Text(initials(name: name)).font(CallFont.figtree(34, .bold)).foregroundStyle(tin.ink2))))
                    Text("Couldn’t reach \(who)").font(TinType.h1).foregroundStyle(tin.ink).multilineTextAlignment(.center).padding(.top, 16)
                    Text("\(blank ? "They" : name) may be offline, out of signal, or not taking calls right now. Tinline can’t leave messages.")
                        .font(TinType.bodyL).foregroundStyle(tin.ink2).multilineTextAlignment(.center)
                }
                .padding(.horizontal, 28)
            }
            VStack(spacing: 8) {
                CallButton("Try again", action: onAgain)
                CallButton("Close", style: .text, action: onClose)
            }
            .padding(EdgeInsets(top: 0, leading: 24, bottom: 48, trailing: 24))
        }
        .background(tin.bg.ignoresSafeArea())
    }
}

/** Before the first call without the microphone: say why and send the user to the permission page. */
struct MicNeededScreen: View {
    @Environment(\.tin) var tin
    let onOpenSettings: () -> Void
    let onNotNow: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            CallCentredScroll {
                VStack(spacing: 12) {
                    Circle().fill(tin.erc).frame(width: 96, height: 96)
                        .overlay(CallIcon.micOff.image.font(.system(size: 40)).foregroundStyle(tin.er))
                        .accessibilityHidden(true)
                    Text("Tinline can’t use the microphone").font(TinType.h1).foregroundStyle(tin.ink).multilineTextAlignment(.center).padding(.top, 12)
                    Text("Without it, they won’t hear you. Tinline only listens during calls — never in the background.")
                        .font(TinType.bodyL).foregroundStyle(tin.ink2).multilineTextAlignment(.center)
                }
                .padding(.horizontal, 28)
            }
            VStack(spacing: 8) {
                CallButton("Open settings", action: onOpenSettings)
                CallHint(text: "Then go to Settings › Tinline › Microphone and allow it.", align: .center)
                CallButton("Not now", style: .text, action: onNotNow)
            }
            .padding(EdgeInsets(top: 0, leading: 24, bottom: 48, trailing: 24))
        }
        .background(tin.bg.ignoresSafeArea())
    }
}

/** Page body that centres its content vertically and scrolls when Dynamic Type makes it taller than the screen. */
private struct CallCentredScroll<Content: View>: View {
    @ViewBuilder let content: () -> Content
    var body: some View {
        GeometryReader { g in
            ScrollView { content().frame(maxWidth: .infinity, minHeight: g.size.height) }
                .scrollBounceBehavior(.basedOnSize)
        }
    }
}

// MARK: - previews (synthetic)

private let previewInfo = CallInfo(callId: "c0ffee00", peerDid: "did:key:z6MkPreviewArjun", peerName: "Arjun Mehta", incoming: false, video: false)

#Preview("Calling") {
    InCallContent(name: "Arjun Mehta", did: previewInfo.peerDid, active: false, ringing: false, secs: 0, direct: nil, bars: nil,
                  muted: false, speaker: false, micProblem: nil, onMute: {}, onSpeaker: {}, onEnd: {}, onMinimise: {})
        .tinTheme()
}

#Preview("In call") {
    InCallContent(name: "Arjun Mehta", did: previewInfo.peerDid, active: true, ringing: false, secs: 125, direct: true, bars: 4,
                  muted: true, speaker: false, micProblem: nil, onMute: {}, onSpeaker: {}, onEnd: {}, onMinimise: {})
        .tinTheme()
}

#Preview("Incoming video") {
    IncomingCallContent(name: "Arjun Mehta", did: previewInfo.peerDid, video: true, onDecline: {}, onAnswer: {}, onVideo: {})
}

#Preview("Ended") {
    EndedContent(name: "Arjun Mehta", did: previewInfo.peerDid, meName: "Sam Rivera", reasonText: "Arjun ended the call",
                 secs: 312, direct: false, bars: 3, onAgain: {}, onClose: {}, autoClose: false)
        .tinTheme()
}

#Preview("Couldn't reach") {
    UnreachableContent(name: "Arjun Mehta", did: previewInfo.peerDid, meName: "Sam Rivera", onAgain: {}, onClose: {}).tinTheme()
}
