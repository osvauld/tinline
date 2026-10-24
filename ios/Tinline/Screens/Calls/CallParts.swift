import SwiftUI

/*
 * The pieces every voice call screen shares (CallParts.kt): the top row, the five control slots, the focus layout on
 * the colour field, and About this call.
 */

/**
 * The live mark in the call's top row. The Components area owns `TinMark`; the call look (state, sway, label) is
 * computed here ([callMarkLook]) and passed on (Mark.kt `TinMark(size, state, live, sway, description)`).
 */
struct CallLiveMark: View {
    let look: CallMarkLook
    private var state: MarkState {
        switch look.state {
        case .slack: return .slack
        case .ringing: return .ringing
        case .taut: return .taut
        }
    }
    var body: some View {
        TinMark(size: 28, state: state, live: true, sway: look.swayPeriod.map { Sway(period: $0, amp: look.swayAmp ?? 0) },
                description: look.label)
            .frame(width: 28, height: 28)
    }
}

/** A 44 pt round icon button for the top row. */
struct CallTopButton: View {
    @Environment(\.tin) var tin
    let icon: CallIcon
    let label: String
    var tint: Color? = nil
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            icon.image.font(.system(size: 20, weight: .regular)).foregroundStyle(tint ?? tin.ink)
                .frame(width: 44, height: 44).contentShape(Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
    }
}

/**
 * Fixed, mirrored 44 pt action cells with the 28 pt live mark independently centred. The end cell is the signal
 * (SignalBars, where the "i" was): tapping it opens About this call.
 */
struct CallTop: View {
    @Environment(\.tin) var tin
    let active: Bool
    let ringing: Bool
    var onMinimise: (() -> Void)?
    var onInfo: (() -> Void)?
    var reconnecting = false
    var incoming = false
    var signal = 0
    var direct: Bool?
    /** Message (decided 2026-10-14, the in-call lab): leave the call screen for their chat; the call continues. */
    var onMessage: (() -> Void)?

    var body: some View {
        let phase: CallMarkPhase = reconnecting ? .reconnecting : active ? .active : incoming ? .incoming : .outgoing
        ZStack {
            CallLiveMark(look: callMarkLook(phase))
            HStack(spacing: 0) {
                // Back, like every other screen: to where I was; the call goes on in the strip.
                if let onMinimise { CallTopButton(icon: .back, label: "Back. The call continues", action: onMinimise) }
                Spacer(minLength: 0)
                if let onMessage { CallTopButton(icon: .chat, label: "Message. The call continues", action: onMessage) }
                if let onInfo {
                    let up = active && !reconnecting && signal > 0
                    let label = active && reconnecting ? "About this call. Reconnecting."
                        : up ? "About this call. \(callSignalWord(signal)) signal, \(direct == true ? "direct" : "relayed")."
                        : "About this call"
                    Button(action: onInfo) {
                        CallSignalBars(level: up ? signal : 0, color: tin.ink, wave: active && reconnecting)
                            .frame(width: 44, height: 44).contentShape(Circle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(label)
                }
            }
        }
        .frame(maxWidth: .infinity)
        .frame(height: 48)
    }
}

/** One of the five stable control slots: a round button (44-56 pt, as wide as the slot allows) and its caption. */
private struct CallControl<Glyph: View>: View {
    @Environment(\.tin) var tin
    let label: String
    let caption: String
    let bg: Color
    var enabled = true
    var selected: Bool?
    let action: () -> Void
    @ViewBuilder let glyph: () -> Glyph

    var body: some View {
        VStack(spacing: 8) {
            GeometryReader { geo in
                let d = min(max(geo.size.width, 44), 56)
                Button(action: action) {
                    glyph().frame(width: d, height: d).background(Circle().fill(bg)).contentShape(Circle())
                }
                .buttonStyle(.plain)
                .disabled(!enabled)
                .callDepth(.lift, dark: tin.dark)
                .frame(maxWidth: .infinity)
                .accessibilityLabel(label)
                .accessibilityAddTraits(selected == true ? .isSelected : [])
            }
            .frame(height: 56)
            Text(caption).font(CallFont.figtree(13)).foregroundStyle(tin.ink).lineLimit(1).truncationMode(.tail)
                .frame(minHeight: 18)
        }
        .frame(maxWidth: .infinity)
        .opacity(enabled ? 1 : 0.38)
    }
}

/** Five stable slots. End stays on the axis; Flip never vanishes when the camera turns off. */
struct CallControls: View {
    @Environment(\.tin) var tin
    let muted: Bool
    let speaker: Bool
    let camera: Bool
    let onMute: () -> Void
    let onSpeaker: () -> Void
    let onCamera: (() -> Void)?
    let onFlip: (() -> Void)?
    let onEnd: () -> Void
    /** With a headset connected the speaker slot becomes the route picker: its name and icon. */
    var routeCaption = "Speaker"
    var routeIcon: CallIcon = .speaker
    /** I pressed End: its glyph plays the cut string letting go ([CallHangupGlyph]). */
    var ending = false

    var body: some View {
        let fill = tin.callControlFill
        HStack(alignment: .top, spacing: 10) {
            CallControl(label: "Mute", caption: muted ? "Muted" : "Mute", bg: muted ? tin.ink : fill, selected: muted, action: onMute) {
                (muted ? CallIcon.micOff : .mic).image.font(.system(size: 22)).foregroundStyle(muted ? tin.bg : tin.ink)
            }
            CallControl(label: "Camera", caption: "Camera", bg: camera ? tin.ink : fill, enabled: onCamera != nil, selected: camera,
                        action: onCamera ?? {}) {
                (camera ? CallIcon.video : .videoOff).image.font(.system(size: 22)).foregroundStyle(camera ? tin.bg : tin.ink)
            }
            CallControl(label: "End call", caption: "End", bg: tin.callButtonEnd, action: onEnd) {
                CallHangupGlyph(gone: ending, tint: .white).frame(width: 28, height: 28)
            }
            CallControl(label: "Flip camera", caption: "Flip", bg: fill, enabled: camera && onFlip != nil, action: onFlip ?? {}) {
                CallIcon.flip.image.font(.system(size: 22)).foregroundStyle(tin.ink)
            }
            CallControl(label: routeCaption == "Speaker" ? "Speaker" : "Audio: \(routeCaption)", caption: routeCaption,
                        bg: speaker ? tin.ink : fill, selected: speaker, action: onSpeaker) {
                routeIcon.image.font(.system(size: 22)).foregroundStyle(speaker ? tin.bg : tin.ink)
            }
        }
        .frame(maxWidth: .infinity)
    }
}

/** Frames reported by [CallFocus]'s children in its own coordinate space. */
private struct FocusFrames: PreferenceKey {
    static let defaultValue: [String: CGRect] = [:]
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) { value.merge(nextValue()) { $1 } }
}

private extension View {
    func focusFrame(_ key: String) -> some View {
        background(GeometryReader { g in Color.clear.preference(key: FocusFrames.self, value: [key: g.frame(in: .named("callFocus"))]) })
    }
}

/**
 * Voice focus geometry on the call's colour field (Fog, as on the playground and desktop): the field state is mapped
 * as call_screen.rs does (incoming, outgoing while dialling, connecting while ringing, active, waiting, reconnecting),
 * anchored at the avatar, with calm patches behind the name/status block and the controls in dark mode.
 */
struct CallFocus<Controls: View>: View {
    @Environment(\.tin) var tin
    let name: String
    let did: String
    let active: Bool
    let ringing: Bool
    var secs: Int64 = 0
    var reconnecting = false
    var problem: String?
    var incoming = false
    var video = false
    var onMinimise: (() -> Void)?
    var onInfo: (() -> Void)?
    var signal = 0
    var direct: Bool?
    var onMessage: (() -> Void)?
    var waiting = false
    var muted = false
    var waitingCard: AnyView?
    @State var frames: [String: CGRect] = [:]
    @ViewBuilder let controls: () -> Controls

    private var fieldState: CallFieldState {
        if incoming { return .incoming }
        if active && waiting { return .waiting }
        if active && reconnecting { return .reconnecting }
        if active { return .active }
        if ringing { return .connecting }
        return .outgoing
    }

    var body: some View {
        GeometryReader { geo in
            let root = geo.size
            // The field fills the whole screen (under the bars); the frames are measured inside the safe area.
            let dx = geo.safeAreaInsets.leading, dy = geo.safeAreaInsets.top
            let full = CGSize(width: root.width + dx + geo.safeAreaInsets.trailing, height: root.height + dy + geo.safeAreaInsets.bottom)
            let avatar = frames["avatar"].map { CGPoint(x: $0.midX + dx, y: $0.midY + dy) } ?? CGPoint(x: full.width / 2, y: full.height * 0.3)
            let calm: [FieldCalm] = {
                guard let t = frames["text"], let c = frames["ctrl"], !t.isEmpty, !c.isEmpty else { return [] }
                return [FieldCalm(cx: t.midX + dx, cy: t.midY + dy, hw: min(t.width, 420) / 2, hh: t.height / 2),
                        FieldCalm(cx: c.midX + dx, cy: c.midY + dy, hw: min(c.width, 340) / 2, hh: c.height / 2)]
            }()
            ZStack(alignment: .bottom) {
                tin.bg.ignoresSafeArea()
                CallFieldBackground(state: fieldState, muted: muted, avatar: avatar, calm: calm).ignoresSafeArea()
                VStack(spacing: 0) {
                    CallTop(active: active, ringing: ringing, onMinimise: onMinimise, onInfo: onInfo, reconnecting: reconnecting,
                            incoming: incoming, signal: signal, direct: direct, onMessage: onMessage)
                    GeometryReader { sg in
                    ScrollView {
                        VStack(spacing: 0) {
                            TinAvatar(did: did, size: 112).focusFrame("avatar")
                            VStack(spacing: 0) {
                                Text(name).font(CallFont.figtree(28, .semibold, .title)).foregroundStyle(tin.ink)
                                    .multilineTextAlignment(.center).lineLimit(1).truncationMode(.tail)
                                    .frame(maxWidth: .infinity, minHeight: 34).padding(.top, 22)
                                let status = incoming ? (video ? "Tinline video call" : "Tinline voice call")
                                    : callStatus(active: active, ringing: ringing, reconnecting: reconnecting, secs: secs)
                                Text(status).font(CallFont.figtree(active && !reconnecting ? 20 : 17, .medium).monospacedDigit())
                                    .foregroundStyle(tin.ink).lineLimit(1)
                                    .frame(minHeight: 30).padding(.top, 6)
                                // The path, on the main screen: honest, and About this call explains it.
                                Text(active && !reconnecting ? "End-to-end encrypted" + (direct.map { $0 ? " · Direct" : " · Relayed" } ?? "") : " ")
                                    .font(CallFont.figtree(13)).foregroundStyle(tin.ink).lineLimit(1)
                                    .frame(minHeight: 18).padding(.top, 10)
                                // Keep a two-line error cell even when there is no microphone/video problem.
                                Text(problem ?? " ").font(CallFont.figtree(13)).foregroundStyle(tin.er)
                                    .multilineTextAlignment(.center).lineLimit(2, reservesSpace: true)
                                    .frame(maxWidth: .infinity, minHeight: 38).padding(.top, 8)
                                    .accessibilityHidden(problem == nil)
                            }
                            .frame(maxWidth: .infinity)
                            .focusFrame("text")
                        }
                        .padding(.bottom, 40)
                        .frame(maxWidth: .infinity, minHeight: sg.size.height, alignment: .center)
                    }
                    .scrollBounceBehavior(.basedOnSize)
                    }
                    controls().focusFrame("ctrl")
                }
                .padding(EdgeInsets(top: 8, leading: 20, bottom: 36, trailing: 20))
                // WaitingCard: its bottom edge 22 pt above the controls, growing upward, so the avatar and name never move.
                if let waitingCard {
                    let above = frames["ctrl"].map { root.height - $0.minY + 22 } ?? 150
                    waitingCard.padding(.horizontal, 20).padding(.bottom, above)
                }
            }
            .coordinateSpace(name: "callFocus")
            .onPreferenceChange(FocusFrames.self) { frames = $0 }
        }
    }
}

/**
 * CallScreen.svelte `.info`: a calm card over a 24 % scrim. Encryption first, then the connection row (bars + "Strong
 * signal · Direct" and one line, in a block that reserves the title + four lines so the card never grows when the
 * wording changes), then the numbers.
 */
struct CallInfoCard: View {
    @Environment(\.tin) var tin
    let name: String
    let active: Bool
    let onDismiss: () -> Void
    var reconnecting = false
    var signal = 0
    var direct: Bool?
    var rttMs: Int?
    var lossPct: Double?
    var video = false
    var sendCodec = ""
    var recvCodec = ""

    var body: some View {
        let first = callFirstName(name)
        let p = CallFont.figtree(13)
        CallScrimCard(onDismiss: onDismiss) {
            Text("About this call").font(CallFont.figtree(17, .semibold)).foregroundStyle(tin.ink)
            if active {
                (Text("End-to-end encrypted.").foregroundStyle(tin.ink).fontWeight(.semibold)
                 + Text(" \(video ? "Your voice and picture are" : "Your voice is") encrypted on this device and only decrypted on \(name)’s.")
                    .foregroundStyle(tin.ink2))
                    .font(p).padding(.top, 10)
                let up = !reconnecting && signal > 0
                HStack(alignment: .top, spacing: 12) {
                    CallSignalBars(level: up ? signal : 0, color: tin.ink, wave: reconnecting, size: 22).padding(.top, 1)
                    let headline = reconnecting ? "Reconnecting" : "\(callSignalWord(signal)) signal · \(direct == true ? "Direct" : "Relayed")"
                    let line = reconnecting ? "The connection dropped for a moment. Tinline is finding a way back to \(first)."
                        : direct == true ? "Your call goes straight between your device and \(first)’s."
                        : "Your devices can’t reach each other directly right now, so a relay passes the call along. It can’t \(video ? "listen or watch" : "listen")."
                    let weak = up && signal == 1 ? " The line is weak, so \(first)’s voice may break up." : ""
                    (Text(headline).foregroundStyle(tin.ink).fontWeight(.semibold) + Text("\n" + line + weak).foregroundStyle(tin.ink2))
                        .font(p)
                        .frame(maxWidth: .infinity, minHeight: 95, alignment: .topLeading)
                }
                .padding(.top, 12)
                if up, let rttMs {
                    Text("\(rttMs) ms round trip · \(lossText(lossPct ?? 0))% lost").font(CallFont.meta).foregroundStyle(tin.ink2)
                        .padding(.top, 6).padding(.leading, 34)
                }
                // Codec per direction while that camera is on. Both rows are always there on a video call (blank when
                // off), so the card never changes height.
                if video {
                    codecRow("Your video", sendCodec)
                    codecRow("Their video", recvCodec)
                }
            } else {
                Text(callPrivacy(active: false, name: name)).font(p).foregroundStyle(tin.ink2).padding(.top, 10)
            }
            CallButton("Got it", height: 44, action: onDismiss).padding(.top, 16)
        }
    }

    private func codecRow(_ who: String, _ what: String) -> some View {
        HStack(spacing: 12) {
            Text(who).font(CallFont.meta).foregroundStyle(tin.ink2).frame(width: 88, alignment: .leading)
            Text(what).font(CallFont.meta).foregroundStyle(tin.ink)
        }
        .frame(minHeight: 19).padding(.top, 6).padding(.leading, 34)
    }
}
