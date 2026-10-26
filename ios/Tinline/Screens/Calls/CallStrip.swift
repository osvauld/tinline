import SwiftUI
import UIKit

/*
 * The minimised call (CallStrip.svelte / GroupCallStrip.svelte): at the TOP of the app, under the status bar (user,
 * 2026-10-16: at the bottom it stacked with the search dock and the tab bar). In flow: the app makes room for it. No
 * End (2026-10-14 in-call lab): it sits by the ear when I come back to the app mid-call, so ending is on the call
 * screen and in the system call UI. While the keyboard is up it stays (I may be writing to someone else) but slims to
 * a 26 pt line.
 */

extension CallsEnv {
    /** A 1:1 call I'm in, outside its call screen (an incoming call still ringing has its own screen). */
    var stripCall: CallUi? {
        guard let u = controller.ui, !(u.info.incoming && u.isRinging), !u.isEnded else { return nil }
        return u
    }

    /** Whether a strip shows: the Shell gives up the status bar inset to it while true. */
    var callStripOn: Bool { stripCall != nil || (groupUi.state.active && groupUi.state.minimized) }
}

/**
 * The strip(s) for the top of the app. Place it above the app's content, which then drops its own top safe-area
 * padding while [CallsEnv.callStripOn]; the strip extends its accent under the status bar.
 */
struct CallStripView: View {
    @ObservedObject var calls: CallsEnv
    /** Opens the 1:1 call screen. */
    let onOpenCall: () -> Void
    @Environment(\.tin) var tin
    @State var keyboard = false

    var body: some View {
        let one = calls.stripCall
        let g = calls.groupUi.state
        let group = g.active && g.minimized
        VStack(spacing: 0) {
            if let c = one {
                TimelineView(.periodic(from: .now, by: 0.5)) { tl in
                    let now = Int64(tl.date.timeIntervalSince1970 * 1000)
                    let secs = max(0, (now - (c.activeSinceMs ?? now)) / 1000)
                    let vi = calls.video.info
                    let video = vi.map { $0.callId == c.info.callId && ($0.localCamera || $0.remoteCamera) } ?? false
                    let name = callOr(c.info.peerName, tr("call_unknown_name"))
                    let status = callStatus(active: c.isActive, ringing: c.isRinging, reconnecting: c.stats?.reconnecting == true, secs: secs, waking: c.stats?.waking == true)
                    StripRow(icon: video ? .video : .phone, name: name, status: status, slim: keyboard,
                             label: tr("call_strip_return", name), action: onOpenCall)
                }
                .transition(stripTransition)
            }
            if group {
                TimelineView(.periodic(from: .now, by: 0.5)) { tl in
                    let now = Int64(tl.date.timeIntervalSince1970 * 1000)
                    let secs = g.phase == .active ? max(0, (now - (g.sinceMs ?? now)) / 1000) : 0
                    let name = calls.groupUi.groupName(g.gid)
                    StripRow(icon: .users, name: name, status: g.phase == .active ? clockOf(secs) : g.status, slim: keyboard,
                             label: tr("call_strip_return_group", name)) { calls.groupUi.restore() }
                }
                .transition(stripTransition)
            }
        }
        .background { if one != nil || group { tin.callAccent.ignoresSafeArea(edges: .top) } }
        .animation(.easeInOut(duration: 0.16), value: one?.info.callId)
        .animation(.easeInOut(duration: 0.16), value: group)
        .animation(.easeInOut(duration: 0.16), value: keyboard)
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { _ in keyboard = true }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in keyboard = false }
    }

    /** Expands in after 80 ms (160 ms), shrinks out in 160 ms, with a fade. */
    private var stripTransition: AnyTransition {
        .asymmetric(insertion: .move(edge: .top).combined(with: .opacity).animation(.easeOut(duration: 0.16).delay(0.08)),
                    removal: .move(edge: .top).combined(with: .opacity).animation(.easeIn(duration: 0.16)))
    }
}

/** 44 pt accent line: icon 18, the name 15/600, the time (or status) 13 tabular at the end. Slim: 26 pt, 13 and 11. */
private struct StripRow: View {
    @Environment(\.tin) var tin
    let icon: CallIcon
    let name: String
    let status: String
    let slim: Bool
    let label: String
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                icon.image.font(.system(size: slim ? 12 : 16)).foregroundStyle(tin.callOnAccent)
                Text(name).font(CallFont.figtree(slim ? 13 : 15, .semibold)).foregroundStyle(tin.callOnAccent)
                    .lineLimit(1).truncationMode(.tail).frame(maxWidth: .infinity, alignment: .leading)
                Text(status).font(CallFont.figtree(slim ? 11 : 13).monospacedDigit()).foregroundStyle(tin.callOnAccent).lineLimit(1)
            }
            .padding(.horizontal, 16)
            .frame(height: slim ? 26 : 44)
            .frame(maxWidth: .infinity)
            .background(tin.callAccent)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(tr("call_strip_label", label, status))
        .accessibilityAddTraits(.isButton)
    }
}

/**
 * MiniVideo.svelte: the call is minimised during video, so a small floating tile of their picture sits over the app,
 * in addition to the strip: 92 pt wide, 3:4, radius 16, a 1.5 pt light ring, 12 pt from the end and 92 pt above the
 * bottom of the content. Tap it to return to the call. It pops in (.9 to 1, with a fade).
 *
 * My own picture is not offered when theirs is off (the playground does): the camera stops with the call screen ("no
 * capture in the background"), so there is no picture of mine to show here.
 */
struct CallMiniVideo: View {
    @ObservedObject var calls: CallsEnv
    let onOpenCall: () -> Void
    @Environment(\.tin) var tin
    @Environment(\.accessibilityReduceMotion) var reduce

    var body: some View {
        let u = calls.controller.ui
        let vi = calls.video.info
        let live = u.flatMap { $0.isActive && $0.info.callId == vi?.callId ? $0 : nil }
        let theirs = vi.map { $0.negotiated && $0.remoteCamera } ?? false
        let name = live.map { callOr($0.info.peerName, tr("call_unknown_name")) } ?? ""
        let frozen = live?.stats?.reconnecting == true
        ZStack {
            if live != nil && theirs {
                Button(action: onOpenCall) {
                    ZStack {
                        RemoteVideoView(calls: calls)
                        if frozen { Color.black.opacity(0.2) }
                    }
                    .frame(width: 92, height: 92 * 4 / 3)
                    .background(Color(hex: 0x15171C))
                    .clipShape(SmoothRoundedRectangle(cornerRadius: 16))
                    .overlay(SmoothRoundedRectangle(cornerRadius: 16).stroke(Color.white.opacity(0.38), lineWidth: 1.5))
                    .callDepth(.pop, dark: tin.dark)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(tr("call_strip_return_video", name))
                .transition(.asymmetric(
                    insertion: .opacity.combined(with: .scale(scale: reduce ? 1 : 0.9)).animation(CallMotion.easeOut(200)),
                    removal: .opacity.combined(with: .scale(scale: reduce ? 1 : 0.9)).animation(CallMotion.cubicIn(140))))
            }
        }
        .animation(CallMotion.easeOut(200), value: live != nil && theirs)
    }
}
