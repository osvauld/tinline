import SwiftUI

/*
 * The group call's views (GroupCallUi.kt): the full call screen while a call is on and not minimised, About this call,
 * the "Call in progress" banner under a group's header, and the announce card when someone starts a call.
 */

/** The full group call screen while a call is on and not minimised (GroupCallScreen.svelte, phone form). */
struct GroupCallLayer: View {
    @ObservedObject var ui: GroupCallUi
    @ObservedObject var audio: GroupCallAudio
    @Environment(\.tin) var tin

    var body: some View {
        let s = ui.state
        if s.active && !s.minimized {
            ZStack {
                tin.bg.ignoresSafeArea()
                VStack(spacing: 0) {
                    // .top: minimise · the mark · the "i" (no line data from the core yet, so no bars).
                    ZStack {
                        CallLiveMark(look: s.connected ? callMarkLook(.active) : callMarkLook(.connecting))
                        HStack {
                            if s.live4 {
                                Button { ui.minimize() } label: {
                                    CallIcon.back.image.font(.system(size: 20)).rotationEffect(.degrees(-90)).foregroundStyle(tin.ink)
                                        .frame(width: 44, height: 44).contentShape(Circle())
                                }
                                .buttonStyle(.plain).accessibilityLabel("Minimize call")
                            }
                            Spacer()
                            if s.live4 && (s.host != nil || s.hosting) {
                                CallTopButton(icon: .info, label: "About this call") { ui.about(true) }
                            }
                        }
                    }
                    .frame(height: 48)
                    // .head: name 26/32, the status or the timer, the count, the (reserved) speaking line.
                    Text(ui.groupName(s.gid)).font(CallFont.figtree(26, .semibold, .title)).foregroundStyle(tin.ink)
                        .multilineTextAlignment(.center).lineLimit(1).truncationMode(.tail)
                        .frame(maxWidth: .infinity).padding(.top, 6)
                    TimelineView(.periodic(from: .now, by: 0.5)) { tl in
                        let now = Int64(tl.date.timeIntervalSince1970 * 1000)
                        let secs = max(0, (now - (s.sinceMs ?? now)) / 1000)
                        Group {
                            if s.phase == .active {
                                Text(clockOf(secs)).font(CallFont.figtree(20, .medium).monospacedDigit())
                            } else {
                                Text(s.status).font(CallFont.figtree(17, .medium))
                            }
                        }
                        .foregroundStyle(tin.ink)
                        .frame(minHeight: 30)
                    }
                    .padding(.top, 4)
                    let n = s.parts.count
                    Text(s.connected && n > 0 ? (n == 1 ? "Only you so far. Waiting for others." : "\(n) people in the call") : " ")
                        .font(CallFont.figtree(13).monospacedDigit()).foregroundStyle(tin.ink)
                        .frame(minHeight: 20).padding(.top, 4)
                    Spacer().frame(height: 20)
                    stage(s)
                    host(s)
                    controls(s)
                }
                .padding(EdgeInsets(top: 8, leading: 20, bottom: 36, trailing: 20))
                if s.about { GcAboutCard(ui: ui, s: s) }
            }
        }
    }

    /** .stage: the roster (phone: up to 2 -> 104, up to 4 -> 2 cols 92, up to 6 -> 3 cols 76, else 3 cols 68). */
    private func stage(_ s: GcState) -> some View {
        let n = s.parts.count
        let tiles = min(n, 9)
        let (cols, av): (Int, CGFloat) = tiles <= 2 ? (max(tiles, 1), 104) : tiles <= 4 ? (2, 92) : tiles <= 6 ? (3, 76) : (3, 68)
        let shown = Array(s.parts.prefix(n > 9 ? 8 : 9))
        let rows = stride(from: 0, to: shown.count, by: cols).map { Array(shown[$0..<min($0 + cols, shown.count)]) }
        return ZStack(alignment: .bottom) {
            VStack(spacing: 20) {
                ForEach(rows.indices, id: \.self) { r in
                    HStack(spacing: 0) {
                        ForEach(rows[r], id: \.self) { did in
                            let isMe = ui.isMe(did)
                            VStack(spacing: 8) {
                                TinAvatar(did: did, size: av)
                                Text(isMe ? "You" : callFirstName(ui.gcName(did))).font(CallFont.figtree(13, .medium))
                                    .foregroundStyle(tin.ink).lineLimit(1).truncationMode(.tail)
                            }
                            .frame(width: av + 32)
                        }
                    }
                }
                if n > 9 { Text("+\(n - 8) more").font(CallFont.figtree(13, .semibold)).foregroundStyle(tin.ink) }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            if let note = s.note {
                HStack(spacing: 10) {
                    Text(note).font(CallFont.figtree(13)).foregroundStyle(tin.onWarn).frame(maxWidth: .infinity, alignment: .leading)
                    Button { ui.noteOk() } label: {
                        Text("OK").font(CallFont.figtree(13, .bold)).foregroundStyle(tin.onWarn).padding(.horizontal, 14)
                            .frame(minHeight: 36).background(Capsule().fill(tin.onWarn.opacity(0.14)))
                    }
                    .buttonStyle(.plain)
                }
                .padding(EdgeInsets(top: 8, leading: 14, bottom: 8, trailing: 8))
                .background(RoundedRectangle(cornerRadius: 18).fill(tin.warn))
                .callDepth(.pop, dark: tin.dark)
                .padding(.bottom, 4)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    /** .host: "Call host: Arjun's device" with the info icon; it opens About this call. */
    private func host(_ s: GcState) -> some View {
        let host = ui.gcHost(s)
        return ZStack {
            if let host, s.connected || s.phase == .connecting {
                Button { ui.about(true) } label: {
                    HStack(spacing: 8) {
                        Text(s.hosting ? "You’re the call host" : "Call host: \(host)").font(CallFont.figtree(13)).foregroundStyle(tin.ink)
                        CallIcon.info.image.font(.system(size: 14)).foregroundStyle(tin.ink).opacity(0.75)
                    }
                    .padding(.horizontal, 14).frame(minHeight: 44).contentShape(Capsule())
                }
                .buttonStyle(.plain)
            }
        }
        .frame(minHeight: 44)
        .padding(.top, 4).padding(.bottom, 12)
    }

    /** .controls (phone): Mute · Leave · Speaker. The speaker slot shows the route the call plays through. */
    private func controls(_ s: GcState) -> some View {
        let fill = tin.callControlFill
        let speaker = audio.routes.speaker
        let slot = CallRoutes.slot(audio.routes)
        return HStack(spacing: 36) {
            GcControl(caption: s.muted ? "Muted" : "Mute", bg: s.muted ? tin.ink : fill, enabled: s.connected, action: { ui.toggleMute() }) {
                (s.muted ? CallIcon.micOff : .mic).image.font(.system(size: 22)).foregroundStyle(s.muted ? tin.bg : tin.ink)
            }
            GcControl(caption: "Leave", bg: tin.callButtonEnd, enabled: s.live4, action: { ui.leave() }) {
                CallHangupGlyph(gone: false, tint: .white).frame(width: 28, height: 28)
            }
            GcControl(caption: slot.caption, bg: speaker ? tin.ink : fill, enabled: true, action: { audio.setSpeaker(!speaker) }) {
                slot.icon.image.font(.system(size: 22)).foregroundStyle(speaker ? tin.bg : tin.ink)
            }
        }
    }
}

private struct GcControl<Glyph: View>: View {
    @Environment(\.tin) var tin
    let caption: String
    let bg: Color
    let enabled: Bool
    let action: () -> Void
    @ViewBuilder let glyph: () -> Glyph
    var body: some View {
        VStack(spacing: 8) {
            Button(action: action) {
                glyph().frame(width: 64, height: 64).background(Circle().fill(bg)).contentShape(Circle())
            }
            .buttonStyle(.plain)
            .disabled(!enabled)
            .callDepth(.lift, dark: tin.dark)
            .accessibilityLabel(caption)
            Text(caption).font(CallFont.figtree(13, .medium)).foregroundStyle(tin.ink).lineLimit(1).accessibilityHidden(true)
        }
        .frame(width: 76)
        .opacity(enabled ? 1 : 0.6)
    }
}

/** About this call (group): who carries it and what that means; honest that the host can hear the call. */
private struct GcAboutCard: View {
    @ObservedObject var ui: GroupCallUi
    let s: GcState
    @Environment(\.tin) var tin
    var body: some View {
        let h = ui.gcHost(s) ?? "the call host"
        let host = h.prefix(1).uppercased() + h.dropFirst()
        let p = CallFont.figtree(13)
        CallScrimCard(onDismiss: { ui.about(false) }) {
            VStack(alignment: .leading, spacing: 8) {
                Text("About this call").font(CallFont.figtree(17, .semibold)).foregroundStyle(tin.ink)
                (Text(host).foregroundStyle(tin.ink).fontWeight(.semibold)
                 + Text(" \(s.hosting ? "is carrying" : "carries") this call. Everyone in the call connects to it, and it passes the audio on and chooses who is heard when several people speak.")
                    .foregroundStyle(tin.ink2))
                    .font(p)
                Text("Your connection to the call host is encrypted, but the call host itself receives the audio, so it can hear this call. You trust it for the call the way you trust a chat host with posts.")
                    .font(p).foregroundStyle(tin.ink2)
                Text("A call host is a device a group admin chose and that agreed to it. It is separate from chat hosting. Admins can change call hosts in Group info.")
                    .font(p).foregroundStyle(tin.ink2)
                CallButton("Got it", height: 44) { ui.about(false) }.padding(.top, 8)
            }
        }
    }
}

/**
 * OngoingCall.svelte: "Call in progress" under a group's header while its call runs and I'm not in it (Join), or
 * "You're in the call" with the timer (Open). The core doesn't say how many joined, so the line names who started it.
 */
struct GroupCallOngoingBanner: View {
    @ObservedObject var ui: GroupCallUi
    let gid: String
    let onError: (String) -> Void
    @Environment(\.tin) var tin

    var body: some View {
        let s = ui.state
        let mine = s.gid == gid && s.active
        let call = s.live[gid]
        if mine || call != nil {
            let fg = tin.dark ? tin.ink : tin.callAccentInk
            let bg = tin.dark ? Color.callLerp(tin.sf, tin.callAccent, 0.24) : tin.callAccentSoft
            HStack(spacing: 12) {
                CallIcon.users.image.font(.system(size: 18)).foregroundStyle(fg)
                VStack(alignment: .leading, spacing: 0) {
                    Text(mine ? "You’re in the call" : "Call in progress").font(CallFont.figtree(15, .semibold)).foregroundStyle(fg).lineLimit(1)
                    TimelineView(.periodic(from: .now, by: 0.5)) { tl in
                        let now = Int64(tl.date.timeIntervalSince1970 * 1000)
                        let secs = max(0, (now - (s.sinceMs ?? now)) / 1000)
                        Text(mine ? (s.phase == .active ? clockOf(secs) : s.status) : "Started by \(callFirstName(ui.gcName(call!.starterDid)))")
                            .font(CallFont.figtree(13).monospacedDigit()).foregroundStyle(fg.opacity(0.85))
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                Button { if mine { ui.restore() } else { ui.call(gid, onError: onError) } } label: {
                    Text(mine ? "Open" : "Join").font(CallFont.figtree(13, .bold)).foregroundStyle(mine ? tin.callAccentInk : tin.callOnAccent)
                        .padding(.horizontal, 18).frame(minWidth: 72, minHeight: 36)
                        .background { if mine { Capsule().strokeBorder(tin.callAccentInk, lineWidth: 1.5) } else { Capsule().fill(tin.callAccent) } }
                        .contentShape(Capsule())
                }
                .buttonStyle(.plain)
            }
            .padding(EdgeInsets(top: 8, leading: 14, bottom: 8, trailing: 12))
            .frame(maxWidth: .infinity, minHeight: 56)
            .background(bg)
        }
    }
}

/**
 * GroupCallAnnounce.svelte (phone): a quiet card under the status bar, 8 pt from the sides: the starter's avatar, the
 * group's name over "<name> started a call", a dismiss X and Join. Slides 20 pt down in (200 ms), 12 pt up out (140 ms).
 * Shown while the call still runs, the banner has not settled or been dismissed, I hold no line, and that group is not
 * the one open. The core has no group mute yet, so a muted group cannot be left quiet (gap).
 */
struct GroupCallAnnounce: View {
    @ObservedObject var ui: GroupCallUi
    /** Whether a 1:1 call is on (no announce over it). */
    let inOneToOne: Bool
    let openGid: String?
    let onOpen: (String) -> Void
    @Environment(\.tin) var tin
    @Environment(\.accessibilityReduceMotion) var reduce

    var body: some View {
        let s = ui.state
        let a = s.announce
        let show = a.map { $0.bannerOn && !$0.dismissed && s.live[$0.gid] != nil && !s.active && !inOneToOne && (openGid != $0.gid || $0.ringing) } ?? false
        VStack {
            if show, let l = a {
                card(l).transition(.asymmetric(
                    insertion: .opacity.combined(with: .offset(y: reduce ? 0 : -20)).animation(CallMotion.easeOut(200)),
                    removal: .opacity.combined(with: .offset(y: reduce ? 0 : -12)).animation(CallMotion.cubicIn(140))))
            }
            Spacer(minLength: 0)
        }
        .padding(EdgeInsets(top: 8, leading: 8, bottom: 0, trailing: 8))
        .animation(CallMotion.easeOut(200), value: show)
    }

    private func card(_ l: GcAnnounce) -> some View {
        let group = ui.groupLabel(l.gid)
        let by = ui.gcName(l.by)
        return HStack(spacing: 4) {
            Button { ui.dismissAnnounce(); onOpen(l.gid) } label: {
                HStack(spacing: 12) {
                    TinAvatar(did: l.by, size: 44)
                    VStack(alignment: .leading, spacing: 0) {
                        Text(group).font(CallFont.figtree(15, .semibold)).foregroundStyle(tin.ink).lineLimit(1)
                        Text(l.ringing ? "\(callFirstName(by)) is calling the group" : "\(by) started a call")
                            .font(CallFont.sub).foregroundStyle(tin.ink2).lineLimit(1)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(.horizontal, 4).frame(minHeight: 48).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Open \(group)")
            if l.ringing {
                Button { ui.notNow() } label: {
                    Text("Not now").font(CallFont.figtree(13, .bold)).foregroundStyle(tin.ink).padding(.horizontal, 14)
                        .frame(height: 44).background(SmoothRoundedRectangle(cornerRadius: 22).fill(tin.sf2))
                }
                .buttonStyle(.plain)
            } else {
                Button { ui.dismissAnnounce() } label: {
                    CallIcon.close.image.font(.system(size: 15)).foregroundStyle(tin.ink2).frame(width: 36, height: 36).contentShape(Circle())
                }
                .buttonStyle(.plain).accessibilityLabel("Dismiss")
            }
            Button { ui.call(l.gid) { _ in } } label: {
                Text("Join").font(CallFont.figtree(13, .bold)).foregroundStyle(.white).padding(.horizontal, 20)
                    .frame(height: 44).background(SmoothRoundedRectangle(cornerRadius: 22).fill(tin.callButtonAnswer))
            }
            .buttonStyle(.plain)
        }
        .padding(EdgeInsets(top: 8, leading: 8, bottom: 8, trailing: 10))
        .background(SmoothRoundedRectangle(cornerRadius: 20).fill(tin.sfRaised))
        .overlay(SmoothRoundedRectangle(cornerRadius: 20).stroke(tin.ln, lineWidth: 1))
        .callDepth(.pop, dark: tin.dark)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(l.ringing ? "\(by) is calling the group \(group)" : "\(by) started a call in \(group)")
    }
}
