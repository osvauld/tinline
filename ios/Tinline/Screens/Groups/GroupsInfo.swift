import SwiftUI

/*
 * Group info (playground screens/groups/GroupInfo.svelte, android GroupsLive.kt GroupInfoLive and its sections): header, flags
 * on me, invites (admins), hosts, call hosts, members, leave. What I can do depends on my role; the owner can't leave. A removed
 * group shows the removal view instead. Also the removal route, the invite-contacts sheet and the member sheet.
 */

private enum InfoSheet: Identifiable {
    case member(String)
    case invite
    case leave
    var id: String { switch self { case .member(let d): return "m:" + d; case .invite: return "invite"; case .leave: return "leave" } }
}

/// "n members" / "1 member".
func groupsPlural(_ n: Int, _ one: String, _ many: String? = nil) -> String { "\(n) \(n == 1 ? one : (many ?? one + "s"))" }

struct GroupInfoView: View {
    let groupId: String
    var body: some View { GroupsSessionGate { GroupInfoScreen(session: $0, id: groupId) } }
}

private struct GroupInfoScreen: View {
    @ObservedObject var session: GroupsSession
    let id: String
    @Environment(\.tin) private var tin
    @Environment(\.dismiss) private var dismiss
    @Environment(\.groupsOpenChat) private var openChat
    @State private var sheet: InfoSheet?

    var body: some View {
        let ctl = session.ctl
        let o = ctl.opened.flatMap { $0.id == id ? $0 : nil }
        Group {
            if let d = o?.detail { content(d) } else { InfoPage(title: "Group info", onBack: { dismiss() }) { EmptyView() } }
        }
        .groupsToasts(session)
        .task(id: id) { session.background { if session.ctl.opened?.id != id { session.ctl.openGroup(id) } } }
    }

    /// The name of a DID, with "You" replaced by my profile name where the screen names me.
    private func name(_ did: String, _ fb: String) -> String {
        let n = session.name(did, fb)
        return n == "You" ? (session.dir.myName.flatMap { $0.groupsIsBlank ? nil : $0 } ?? "You") : n
    }

    @ViewBuilder private func content(_ d: GroupDetail) -> some View {
        let ctl = session.ctl
        let admin = GroupText.isAdmin(d.myRole)
        let sent = ctl.sent, outgoing = ctl.outgoing
        InfoPage(title: d.myRole == .removed ? "Removed" : "Group info", onBack: { dismiss() }) {
            hero(d)
            switch d.myRole {
            case .removed: RemovalBody(session: session, removal: d.removal) { dismiss() }
            case .left:
                GroupsNote("You can still read the posts on this device. To come back, someone in the group has to invite you again.").padding(.horizontal, 20).padding(.vertical, 24)
            default:
                if let mine = d.members.first(where: { $0.isMe }), mine.flags > 0, !mine.isOwner { FlagsOnMe(n: Int(mine.flags)) }
                if admin { invites(d, sent: sent) }
                HostsSection(session: session, d: d, admin: admin, name: name)
                CallHostsSection(session: session, d: d, admin: admin, name: name)
                members(d, admin: admin, outgoing: outgoing)
                VStack(spacing: 0) {
                    GroupsCard {
                        if d.myRole != .owner { ActRow(icon: GroupsIcons.leave, title: "Leave group", danger: true) { sheet = .leave } }
                        else { ActRow(icon: GroupsIcons.leave, title: "Leave group", sub: "The owner can’t leave yet", enabled: false, off: true) {} }
                    }
                }.padding(.init(top: 24, leading: 16, bottom: 0, trailing: 16))
            }
        }
        .sheet(item: $sheet) { s in
            switch s {
            case .member(let did):
                if let m = d.members.first(where: { $0.did == did }) {
                    MemberSheet(session: session, d: d, m: m, n: name(m.did, m.name), req: outgoing.filter { $0.peerDid == m.did }.max(by: { $0.at < $1.at }),
                                blockedByMe: session.dir.blockedByMe(m.did), onChat: openChat).groupsSheetStyle()
                } else { Color.clear.onAppear { sheet = nil } }
            case .invite: InviteSheet(session: session, d: d, sent: sent).groupsSheetStyle()
            case .leave:
                LeaveSheet(session: session, d: d) { sheet = nil; dismiss() }.groupsSheetStyle()
            }
        }
    }

    private func hero(_ d: GroupDetail) -> some View {
        let line: String = {
            switch d.myRole {
            case .owner: return "\(groupsPlural(d.members.count, "member")) · You are the owner"
            case .admin: return "\(groupsPlural(d.members.count, "member")) · You are an admin"
            case .member: return "\(groupsPlural(d.members.count, "member")) · You are a member"
            case .left: return "You left this group"
            case .removed: return "You’re no longer a member"
            }
        }()
        return VStack(spacing: 0) {
            GroupsAvatar(size: 72, dim: !GroupText.active(d.myRole))
            Text(d.name).font(GroupsType.hero).foregroundStyle(tin.ink).multilineTextAlignment(.center).padding(.top, 12).accessibilityAddTraits(.isHeader)
            Text(line).font(GroupsType.note).foregroundStyle(tin.ink2).multilineTextAlignment(.center).padding(.top, 2)
        }
        .frame(maxWidth: .infinity).padding(.init(top: 16, leading: 20, bottom: 8, trailing: 20))
    }

    private func invites(_ d: GroupDetail, sent: [GroupInvitation]) -> some View {
        let shownSent = sent.filter { inv in inv.groupId == id && !d.members.contains(where: { m in m.did == inv.inviteeDid }) }
        let noHost = d.hosts.isEmpty && !d.amHost
        return InfoSec(title: "Invites") {
            GroupsCard {
                ActRow(icon: GroupsIcons.userPlus, title: "Invite contacts", sub: "A private invite to each person", trailing: GroupsIcons.chevR) { sheet = .invite }
                GroupsRule()
                NavigationLink(value: GroupsRoute.link(id)) {
                    ActRowLabel(icon: GroupsIcons.link, title: "Create invite link",
                                sub: noHost ? "Appoint a host first. A link names one host." : "Valid for 7 days", trailing: GroupsIcons.plus)
                }.buttonStyle(GroupsPressStyle()).disabled(noHost).opacity(noHost ? 0.55 : 1)
            }
            if !shownSent.isEmpty {
                VStack(spacing: 0) {
                    ForEach(shownSent, id: \.id) { inv in
                        let n = name(inv.inviteeDid, inv.inviteeName)
                        let (line, bad) = GroupText.sentInvitationLine(inv)
                        MRow {
                            TinAvatar(did: inv.inviteeDid, size: 40)
                            TwoLine(title: n, sub: line, subColor: bad ? tin.er : tin.ink2)
                        }
                    }
                }.padding(.top, 6)
            }
        }
    }

    /// "Since 3 Oct · 2 of 3 flags · Blocked by you": the flag count in danger ink, 600.
    private func memberSub(_ since: String, flags: Int?, blocked: Bool, reqNote: String?) -> AttributedString {
        var sub = AttributedString(since)
        if let flags {
            var f = AttributedString(" · \(flags) of \(GroupText.flagsToRemove) flags")
            f.foregroundColor = tin.er; f.font = GroupsType.subStrong
            sub += f
        }
        if blocked { sub += AttributedString(" · Blocked by you") } else if let reqNote { sub += AttributedString(" · \(reqNote)") }
        return sub
    }

    private func members(_ d: GroupDetail, admin: Bool, outgoing: [GroupChatRequest]) -> some View {
        let order = { (m: GroupMember) in m.isOwner ? 0 : (m.isAdmin ? 1 : 2) }
        let sorted = d.members.enumerated().sorted { a, b in
            let ka = (order(a.element), !a.element.isMe), kb = (order(b.element), !b.element.isMe)
            if ka.0 != kb.0 { return ka.0 < kb.0 }
            if ka.1 != kb.1 { return !ka.1 }
            return a.offset < b.offset
        }.map { $0.element }
        return InfoSec(title: "Members", count: d.members.count) {
            ForEach(sorted, id: \.did) { m in
                let n = name(m.did, m.name)
                let req = outgoing.filter { $0.peerDid == m.did }.max(by: { $0.at < $1.at })
                let reqNote: String? = {
                    switch req?.state {
                    case .pending: return "Chat request sent"
                    case .declined: return "Declined your chat request"
                    case .blocked: return "Blocked"
                    default: return nil
                    }
                }()
                let since = m.invitedBy.map { "Invited by \(groupsFirstName(name($0, ""))) · \(GroupsTime.dayOnly(m.joinedAt))" } ?? "Since \(GroupsTime.dayOnly(m.joinedAt))"
                let showFlags = (admin || m.isMe) && m.flags > 0 && !m.isOwner
                let isCallHost = d.callHosts.contains(where: { $0.did == m.did })
                let desc = "\(n)\(m.isMe ? " (you)" : ""), \(m.isOwner ? "owner" : (m.isAdmin ? "admin" : "member"))" +
                    (m.isHost ? ", host" : "") + (isCallHost ? ", call host" : "") + ". Open actions"
                let sub = memberSub(since, flags: showFlags ? Int(m.flags) : nil, blocked: session.dir.blockedByMe(m.did), reqNote: reqNote)
                MRow(onTap: { sheet = .member(m.did) }, desc: desc) {
                    TinAvatar(did: m.did, size: 40)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(m.isMe ? "\(n) (you)" : n).font(GroupsType.bodyStrong).foregroundStyle(tin.ink).lineLimit(1)
                        Text(sub).font(GroupsType.sub).foregroundStyle(tin.ink2).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                    HStack(spacing: 4) {
                        if m.isOwner { GroupsPill(text: "Owner", tone: .owner) } else if m.isAdmin { GroupsPill(text: "Admin", tone: .admin) }
                        if m.isHost { GroupsMark(icon: GroupsIcons.server, desc: "Host") }
                        if isCallHost { GroupsMark(icon: GroupsIcons.phone, desc: "Call host", call: true) }
                    }
                }
            }
        }
    }
}

/// Info pages: the 60 pt surface bar with a hairline under it, then a scrolling column capped at 640 pt.
private struct InfoPage<Content: View>: View {
    @Environment(\.tin) private var tin
    let title: String
    let onBack: () -> Void
    @ViewBuilder var content: () -> Content
    var body: some View {
        VStack(spacing: 0) {
            GroupsBar(title: title, onBack: onBack, surface: true)
            GroupsRule()
            ScrollView { VStack(spacing: 0, content: content).frame(maxWidth: 640).frame(maxWidth: .infinity).padding(.bottom, 32) }
        }
        .background(tin.bg.ignoresSafeArea()).toolbar(.hidden, for: .navigationBar)
    }
}

/// A section of the info page (`.sec`): 16 pt sides, 24 above, the head inset 4 more with 8 below.
private struct InfoSec<Content: View>: View {
    let title: String
    var count: Int?
    var countText: String?
    var countColor: Color?
    @ViewBuilder var content: () -> Content
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            GroupsSection(title, count: count, countText: countText, countColor: countColor, padding: .init(top: 0, leading: 4, bottom: 8, trailing: 4))
            content()
        }.padding(.init(top: 24, leading: 16, bottom: 0, trailing: 16))
    }
}

/// A row in a card (`.act`): 20 pt glyph, title and sub, an optional trailing glyph; 60 pt, 16 pt sides, 14 gap.
private struct ActRowLabel: View {
    @Environment(\.tin) private var tin
    let icon: GroupsIcon, title: String
    var sub: String?
    var trailing: GroupsIcon?
    var danger = false
    var off = false
    var body: some View {
        HStack(spacing: 14) {
            GroupsIconView(icon: icon, size: 20, color: danger ? tin.er : tin.ink2)
            VStack(alignment: .leading, spacing: 1) {
                Text(title).font(GroupsType.bodyStrong).foregroundStyle(danger ? tin.er : (off ? tin.ink2 : tin.ink)).lineLimit(1)
                if let sub { Text(sub).font(GroupsType.sub).foregroundStyle(tin.ink2).lineLimit(1) }
            }
            Spacer(minLength: 0)
            if let trailing { GroupsIconView(icon: trailing, size: 18, color: tin.ink2) }
        }
        .padding(.horizontal, 16).padding(.vertical, 8).frame(maxWidth: .infinity, minHeight: 60, alignment: .leading).contentShape(Rectangle())
    }
}

private struct ActRow: View {
    let icon: GroupsIcon, title: String
    var sub: String?
    var trailing: GroupsIcon?
    var enabled = true
    var danger = false
    var off = false
    let action: () -> Void
    var body: some View {
        let label = ActRowLabel(icon: icon, title: title, sub: sub, trailing: trailing, danger: danger, off: off)
        if off { label } else { Button(action: action) { label }.buttonStyle(GroupsPressStyle()).disabled(!enabled).opacity(enabled ? 1 : 0.55) }
    }
}

/// A member / host row (`.mrow`): 60 pt, 12 gap; tappable rows get the wash with 8 pt sides.
private struct MRow<Content: View>: View {
    var onTap: (() -> Void)?
    var desc: String?
    @ViewBuilder var content: () -> Content
    var body: some View {
        let row = HStack(spacing: 12, content: content).padding(.horizontal, 4).padding(.vertical, 6).frame(maxWidth: .infinity, minHeight: 60, alignment: .leading)
        if let onTap {
            Button(action: onTap) { row.clipShape(SmoothRoundedRectangle(cornerRadius: 14)).contentShape(Rectangle()) }
                .buttonStyle(GroupsPressStyle()).accessibilityElement(children: .ignore).accessibilityLabel(desc ?? "")
        } else { row }
    }
}

private struct TwoLine: View {
    @Environment(\.tin) private var tin
    let title: String, sub: String
    var subColor: Color?
    var dot: Bool?
    var body: some View {
        VStack(alignment: .leading, spacing: 1) {
            Text(title).font(GroupsType.bodyStrong).foregroundStyle(tin.ink).lineLimit(1)
            HStack(spacing: 6) {
                if let dot { GroupsDot(on: dot) }
                Text(sub).font(GroupsType.sub).foregroundStyle(subColor ?? tin.ink2).lineLimit(1)
            }
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// A status line whose wordings share one cell (`.status` + `.stk`), so it changing never moves what is below.
private struct StatusLine: View {
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    let lines: [String]
    let current: Int
    let on: Bool
    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            GroupsDot(on: on).padding(.top, 6)
            GroupsStack(lines: lines, current: current, font: GroupsType.note, color: on ? tin.ink : tin.onWarn)
                .animation(reduced ? nil : GroupsMotion.easeOut(GroupsMotion.enter), value: on)
        }.padding(.horizontal, 4).padding(.bottom, 6)
    }
}

/// A trailing quiet text button whose label lines up with the pills' right edge (`margin-inline-end: -11px`, weight 500).
private struct TrailBtn: View {
    @Environment(\.tin) private var tin
    let text: String, desc: String
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            Text(text).font(GroupsType.body.weight(.medium)).foregroundStyle(tin.ink).padding(.horizontal, 18).frame(minHeight: 44)
                .contentShape(Capsule())
        }.buttonStyle(GroupsPressStyle()).offset(x: 11).accessibilityLabel(desc)
    }
}

/// Hosts: the status line, the hosts with pending asks and offers, then what a host is.
private struct HostsSection: View {
    @ObservedObject var session: GroupsSession
    let d: GroupDetail
    let admin: Bool
    let name: (String, String) -> String
    var body: some View {
        let noHost = d.hosts.isEmpty
        let up = d.amHost || d.hosts.contains(where: { $0.online })
        let online = d.hosts.filter { $0.online || $0.isMe }.count
        let line = noHost ? 0 : (up ? 1 : 2)
        let dev = { (h: GroupHost) -> String in
            if h.isMe { return "This device" }
            let several = (d.members.first(where: { $0.did == h.did })?.deviceIds.count ?? 1) > 1
            return name(h.did, h.name) + (several ? " · \(GroupText.shortDev(h.deviceId))" : "")
        }
        InfoSec(title: "Hosts") {
            StatusLine(lines: [
                "This group has no host. Posts wait until a host is appointed.",
                "\(groupsPlural(max(online, 1), "host")) online. Posts go through right away.",
                "No host is online. Your posts are saved and send when one is.",
            ], current: line, on: line == 1)
            ForEach(d.hosts, id: \.deviceId) { h in
                let on = h.isMe || h.online
                MRow {
                    TinAvatar(did: h.did, size: 40)
                    TwoLine(title: dev(h), sub: (on ? "Online" : "Offline") + (d.callHosts.contains(where: { $0.did == h.did && $0.deviceId == h.deviceId }) ? " · also hosts calls" : ""), dot: on)
                    if admin { TrailBtn(text: "Remove", desc: "Remove \(dev(h)) as host") { session.exec({ session.ctl.revoke(d.id, did: h.did, dev: h.deviceId) }, session.sayDone) } }
                }
            }
            ForEach(d.hostRequests, id: \.deviceId) { h in
                MRow {
                    TinAvatar(did: h.did, size: 40)
                    TwoLine(title: dev(h), sub: "Waiting to accept")
                    if h.isMe && d.hostRequestedHere {
                        GroupsButton(text: "Decline", kind: .quiet) { session.exec({ session.ctl.acceptHost(d.id, accept: false) }, session.sayDone) }
                        GroupsButton(text: "Accept", kind: .primary) { session.exec({ session.ctl.acceptHost(d.id, accept: true) }, session.sayDone) }
                    } else if admin {
                        TrailBtn(text: "Cancel", desc: "Cancel host request to \(dev(h))") { session.exec({ session.ctl.revoke(d.id, did: h.did, dev: h.deviceId) }, session.sayDone) }
                    }
                }
            }
            ForEach(d.hostOffers, id: \.deviceId) { h in
                MRow {
                    TinAvatar(did: h.did, size: 40)
                    TwoLine(title: dev(h), sub: "Offered to host · needs an admin")
                    if admin { GroupsButton(text: "Appoint", kind: .primary) { session.exec({ session.ctl.appoint(d.id, did: h.did, dev: h.deviceId) }, session.sayDone) } }
                }
            }
            GroupsNote("Hosts store the group and pass posts on when others are offline. They are members you trust: a host can read everything posted in this group." +
                (admin ? " Appoint a host from a member’s menu; they have to accept. Removing a host changes the group’s address within 2 days, so that device can no longer reach it." : ""))
                .padding(.horizontal, 4).padding(.top, 10)
        }
    }
}

/// Call hosts: a separate permit from chat hosting (the core's call_hosts / call_host_requests).
private struct CallHostsSection: View {
    @ObservedObject var session: GroupsSession
    let d: GroupDetail
    let admin: Bool
    let name: (String, String) -> String
    var body: some View {
        let ready = d.callHosts.filter { $0.online || $0.isMe }.count
        // This phone is said in words, not listed as a row: "Stop hosting calls on this phone" below stands for it.
        let onlyMe = d.amCallHost && d.callHosts.count == 1
        let line = d.callHosts.isEmpty ? 0 : (onlyMe ? 3 : (ready > 0 ? 1 : 2))
        let dev = { (h: GroupHost) -> String in h.isMe ? "This device" : name(h.did, h.name) }
        InfoSec(title: "Call hosts") {
            StatusLine(lines: [
                "This group has no call host. Nobody can start a call until an admin appoints one.",
                "\(groupsPlural(max(ready, 1), "call host")) ready. Calls start through \(ready == 1 ? "it" : "them").",
                "No call host is available right now. A call can’t start until one is back.",
                "This phone carries the calls you start.",
            ], current: line, on: line == 1 || line == 3)
            ForEach(d.callHosts.filter { !$0.isMe }, id: \.deviceId) { h in
                let on = h.isMe || h.online
                MRow {
                    TinAvatar(did: h.did, size: 40)
                    TwoLine(title: dev(h), sub: on ? "Online" : "Offline", dot: on)
                    if admin { TrailBtn(text: "Remove", desc: "Remove \(dev(h)) as call host") { session.exec({ session.ctl.revokeCall(d.id, did: h.did, dev: h.deviceId) }, session.sayDone) } }
                }
            }
            ForEach(d.callHostRequests, id: \.deviceId) { h in
                MRow {
                    TinAvatar(did: h.did, size: 40)
                    TwoLine(title: dev(h), sub: "Waiting to accept")
                    if h.isMe && d.callHostRequestedHere {
                        GroupsButton(text: "Decline", kind: .quiet) { session.exec({ session.ctl.acceptCallHost(d.id, accept: false) }, session.sayDone) }
                        GroupsButton(text: "Accept", kind: .primary) { session.exec({ session.ctl.acceptCallHost(d.id, accept: true) }, session.sayDone) }
                    } else if admin {
                        TrailBtn(text: "Cancel", desc: "Cancel call host request to \(dev(h))") { session.exec({ session.ctl.revokeCall(d.id, did: h.did, dev: h.deviceId) }, session.sayDone) }
                    }
                }
            }
            // This phone: carry the calls it starts (admin: appoint + accept in one go; asked: the Accept above; already: stop).
            if d.amCallHost {
                ActRow(icon: GroupsIcons.phone, title: "Stop hosting calls on this phone") { session.exec({ session.ctl.acceptCallHost(d.id, accept: false) }, session.sayDone) }
            } else if admin && !d.callHostRequestedHere && GroupText.active(d.myRole) {
                ActRow(icon: GroupsIcons.phone, title: "Host calls on this phone", sub: "Carries the calls you start here. A computer that stays on can carry everyone’s.") {
                    session.exec({ session.ctl.hostCallsHere(d.id, requested: false) }, session.sayDone)
                }
            }
            GroupsNote("A call host carries the call. Everyone in a call connects to it, and it passes the audio on, so it can hear the call. That is separate from chat hosting: " +
                "a chat host is not a call host unless you also appoint it as one. Call hosts are members you trust, and each one chooses to accept." +
                (admin ? " Appoint one from a member’s menu, or host calls on this phone. A phone only hosts calls that its owner starts." : ""))
                .padding(.horizontal, 4).padding(.top, 10)
        }
    }
}

/// Flags on me while I'm still a member (FlagsOnMe.svelte). The core gives the count; who and when come only with a removal.
private struct FlagsOnMe: View {
    @Environment(\.tin) private var tin
    let n: Int
    var body: some View {
        let total = GroupText.flagsToRemove
        let left = max(total - n, 0)
        InfoSec(title: "Flags on you", countText: "\(n) of \(total)", countColor: tin.er) {
            GroupsFlagMeter(filled: n, total: total, desc: "\(n) of \(total) flags").padding(.init(top: 2, leading: 4, bottom: 4, trailing: 4))
            GroupsNote((left == 1 ? "One more flag" : "\(left) more flags") + " from any admin removes you from the group. Flags can’t be taken back. You can still read and post until then.")
                .padding(.horizontal, 4).padding(.top, 6)
        }
    }
}

/// The removed person's view (Removal.svelte): who flagged, when, and what it means.
private struct RemovalBody: View {
    @ObservedObject var session: GroupsSession
    @Environment(\.tin) private var tin
    let removal: GroupRemoval?
    let onBack: () -> Void
    var body: some View {
        let ord = ["First flag", "Second flag", "Third flag"]
        let now = UInt64(Date().timeIntervalSince1970 * 1000)
        VStack(alignment: .leading, spacing: 0) {
            Text("Removed after \(GroupText.flagsToRemove) admin flags").font(GroupsType.hero).foregroundStyle(tin.ink).padding(.top, 8).padding(.bottom, 2).accessibilityAddTraits(.isHeader)
            if let r = removal, r.at > 0 { Text(GroupsTime.dayAndTime(r.at, now)).font(GroupsType.note).foregroundStyle(tin.ink2) }
            RemSec("Who flagged you")
            if let r = removal {
                ForEach(Array(r.flags.enumerated()), id: \.offset) { i, f in
                    let who = session.name(f.byDid, f.byName)
                    HStack(spacing: 12) {
                        TinAvatar(did: f.byDid, size: 40)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(who).font(GroupsType.bodyStrong).foregroundStyle(tin.ink)
                            Text((i < ord.count ? ord[i] : "Flag") + (f.at > 0 ? " · " + GroupsTime.dayAndTime(f.at, now) : "")).font(GroupsType.sub.monospacedDigit()).foregroundStyle(tin.ink2)
                        }
                        Spacer(minLength: 0)
                    }.frame(minHeight: 60)
                    if i < r.flags.count - 1 { GroupsRule() }
                }
            }
            GroupsNote("Each flag was made by an admin of the group, and this device checked them before showing this.").padding(.top, 6)
            RemSec("What this means")
            Text("You keep the posts already on this device, and you can read them. You can’t post, and the group’s hosts no longer accept anything from you. Removal is final: to come back, an admin has to invite you again.")
                .font(GroupsType.body).lineSpacing(7).foregroundStyle(tin.ink).fixedSize(horizontal: false, vertical: true)
            GroupsButton(text: "Read the posts", kind: .outline, action: onBack).padding(.top, 20)
        }
        .padding(.init(top: 8, leading: 20, bottom: 32, trailing: 20)).frame(maxWidth: .infinity, alignment: .leading)
    }
    private func RemSec(_ text: String) -> some View {
        Text(text.uppercased()).font(GroupsType.sec).tracking(0.72).foregroundStyle(tin.ink2).padding(.top, 24).padding(.bottom, 6).accessibilityAddTraits(.isHeader)
    }
}

/// "Why you were removed" (the route from the conversation): the same view as a removed group's info.
struct GroupRemovalView: View {
    let groupId: String
    var body: some View { GroupsSessionGate { RemovalScreen(session: $0, id: groupId) } }
}

private struct RemovalScreen: View {
    @ObservedObject var session: GroupsSession
    let id: String
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        let r = session.ctl.opened.flatMap { $0.id == id ? $0.detail.removal : nil }
        InfoPage(title: "Removed", onBack: { dismiss() }) { RemovalBody(session: session, removal: r) { dismiss() } }
            .task(id: id) { session.background { if session.ctl.opened?.id != id { session.ctl.openGroup(id) } } }
    }
}

private struct LeaveSheet: View {
    @ObservedObject var session: GroupsSession
    @Environment(\.dismiss) private var dismiss
    let d: GroupDetail
    let onLeft: () -> Void
    var body: some View {
        ScrollView {
            GroupsDialogBody(title: "Leave \(d.name)?") {
                GroupsPara("You’ll stop getting posts. What’s already on this device stays, and you can read it. To come back, someone in the group has to invite you again.", top: 4)
                if d.myRole == .admin { GroupsPara("You’re an admin. The owner and the other admins stay.") }
                GroupsActs {
                    GroupsButton(text: "Cancel", kind: .quiet) { dismiss() }
                    GroupsButton(text: "Leave", kind: .danger) {
                        session.exec({ session.ctl.leave(d.id) }) { r in
                            session.sayDone(r)
                            if case .success = r { onLeft() }
                        }
                    }
                }
            }
        }
    }
}

/// Invite contacts (InviteSheet.svelte): pick contacts who aren't in the group and have no invite waiting.
private struct InviteSheet: View {
    @ObservedObject var session: GroupsSession
    @Environment(\.tin) private var tin
    @Environment(\.dismiss) private var dismiss
    let d: GroupDetail
    let sent: [GroupInvitation]
    @State private var picked: [String] = []
    @State private var busy = false

    var body: some View {
        let dir = session.dir
        let choices = GroupText.pickRows(dir.contacts.map { InviteCandidate(did: $0.did, name: $0.name) }, d, sent,
                                         blockedByMe: { dir.blockedByMe($0) }, blocksMe: { dir.blocksMe($0) }).filter { $0.why == nil }
        ScrollView {
            GroupsDialogBody(title: "Invite to \(d.name)") {
                if !choices.isEmpty {
                    VStack(spacing: 0) {
                        ForEach(choices, id: \.did) { p in
                            let on = picked.contains(p.did)
                            Button { if on { picked.removeAll { $0 == p.did } } else { picked.append(p.did) } } label: {
                                HStack(spacing: 12) {
                                    TinAvatar(did: p.did, size: 40)
                                    Text(p.name).font(GroupsType.bodyStrong).foregroundStyle(tin.ink).lineLimit(1)
                                    Spacer(minLength: 0)
                                    GroupsCheckBox(on: on)
                                }
                                .padding(.horizontal, 10).padding(.vertical, 6).frame(minHeight: 56)
                                .clipShape(SmoothRoundedRectangle(cornerRadius: 14)).contentShape(Rectangle())
                            }
                            .buttonStyle(GroupsPressStyle())
                            .accessibilityElement(children: .combine).accessibilityAddTraits(on ? [.isButton, .isSelected] : .isButton)
                        }
                    }.padding(.top, 8).padding(.horizontal, -8)
                    GroupsPara("Each person gets a private invite that works for 7 days. Anyone you invite can read the group’s posts once they join.", top: 10)
                } else {
                    GroupsPara("Everyone in your contacts is already in this group or has an invite waiting.", top: 10)
                }
                GroupsActs {
                    GroupsButton(text: "Cancel", kind: .quiet) { dismiss() }
                    GroupsButton(text: busy ? "Sending…" : (picked.isEmpty ? "Invite" : "Invite \(groupsPlural(picked.count, "person", "people"))"), kind: .primary, enabled: !picked.isEmpty && !busy) {
                        busy = true
                        let dids = picked, gid = d.id
                        session.exec({ session.ctl.inviteContacts(gid, dids: dids) }) { r in
                            busy = false
                            switch r {
                            case .success(let s): session.say(s); dismiss()
                            case .failure(let e): session.say(e.message.groupsIfBlank("Couldn’t invite"))
                            }
                        }
                    }
                }
            }
        }
    }
}

private enum MView { case actions, flag, ask }

/// A member's sheet (MemberSheet.svelte): per-member actions by my role, then the two confirm steps (flag, chat request),
/// cross-faded in place. Request admission and cooldown remain the core's responsibility.
private struct MemberSheet: View {
    @ObservedObject var session: GroupsSession
    @Environment(\.tin) private var tin
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceMotion) private var reduced
    let d: GroupDetail
    let m: GroupMember
    let n: String
    let req: GroupChatRequest?
    let blockedByMe: Bool
    let onChat: (String) -> Void
    @State private var view = MView.actions

    var body: some View {
        ScrollView {
            ZStack(alignment: .top) {
                switch view {
                case .actions: actions.transition(.opacity)
                case .flag: flag.transition(.opacity)
                case .ask: ask.transition(.opacity)
                }
            }
            .animation(reduced ? nil : GroupsMotion.easeOut(GroupsMotion.crossfade), value: view)
        }
        .accessibilityLabel(m.isMe ? "Your role in this group" : "\(n) in \(d.name)")
    }

    private func run(_ work: @escaping () -> GroupResult<String>) { session.exec(work, session.sayDone); dismiss() }

    private var actions: some View {
        let fn = groupsFirstName(n)
        let admin = GroupText.isAdmin(d.myRole)
        let contact = session.dir.contacts.contains(where: { $0.did == m.did })
        let acts = GroupText.actions(d, m)
        let several = m.deviceIds.count > 1
        return VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 12) {
                TinAvatar(did: m.did, size: 48)
                VStack(alignment: .leading, spacing: 1) {
                    Text(m.isMe ? "\(n) (you)" : n).font(GroupsType.title).foregroundStyle(tin.ink).lineLimit(1)
                    Text(m.invitedBy.map { "Invited by \(groupsFirstName(session.name($0, ""))) · \(GroupsTime.dayOnly(m.joinedAt))" } ?? "Member since \(GroupsTime.dayOnly(m.joinedAt))")
                        .font(GroupsType.sub).foregroundStyle(tin.ink2).lineLimit(1)
                }
                Spacer(minLength: 0)
                HStack(spacing: 4) {
                    GroupsPill(text: m.isOwner ? "Owner" : (m.isAdmin ? "Admin" : "Member"), tone: m.isOwner ? .owner : (m.isAdmin ? .admin : .muted))
                    if m.isHost { GroupsPill(text: "Host", tone: .host) }
                    if d.callHosts.contains(where: { $0.did == m.did }) { GroupsPill(text: "Call host", tone: .call) }
                }
            }.padding(.init(top: 18, leading: 20, bottom: 10, trailing: 20))
            if admin && !m.isMe && !m.isOwner {
                Text("\(m.flags) of \(GroupText.flagsToRemove) flags").font(GroupsType.sub.monospacedDigit()).foregroundStyle(tin.ink2).padding(.horizontal, 20).padding(.bottom, 4)
            }
            VStack(spacing: 0) {
                if !m.isMe {
                    if contact && !blockedByMe { MItem(icon: GroupsIcons.chat, title: "Message \(fn)") { dismiss(); onChat(m.did) } }
                    else if blockedByMe { MItem(icon: GroupsIcons.off, title: "You blocked \(fn)", sub: "They stay in the group. Unblock them in Contacts to chat.", off: true) {} }
                    else if req?.state == .pending { MItem(icon: GroupsIcons.clock, title: "Request sent", sub: "Waiting for \(fn) to answer", off: true) {} }
                    else if req?.state == .blocked { MItem(icon: GroupsIcons.off, title: "Blocked", sub: "\(fn) can’t receive requests from you.", off: true) {} }
                    else if acts.contains(.message) {
                        MItem(icon: GroupsIcons.chat, title: req?.state == .declined ? "Ask \(fn) again" : "Ask \(fn) for a private chat",
                              sub: req?.state == .declined ? "\(fn) declined your last request" : "Sends a chat request through the group’s host") { view = .ask }
                    }
                }
                ForEach(Array(acts.enumerated()), id: \.offset) { _, a in
                    switch a {
                    case .makeAdmin: MItem(icon: GroupsIcons.shield, title: "Make admin") { run { session.ctl.setAdmin(d.id, did: m.did, admin: true) } }
                    case .revokeAdmin: MItem(icon: GroupsIcons.shield, title: "Remove admin") { run { session.ctl.setAdmin(d.id, did: m.did, admin: false) } }
                    case .appoint(let dev):
                        MItem(icon: GroupsIcons.server, title: several ? "Make device \(GroupText.shortDev(dev)) a host" : "Make host", sub: "Asks \(fn) to host. They have to accept.") {
                            run { session.ctl.appoint(d.id, did: m.did, dev: dev) }
                        }
                    case .revoke(let dev):
                        MItem(icon: GroupsIcons.server, title: several ? "Stop device \(GroupText.shortDev(dev)) hosting" : "Remove as host") { run { session.ctl.revoke(d.id, did: m.did, dev: dev) } }
                    default: EmptyView()
                    }
                }
                if admin && !m.isMe {
                    ForEach(m.deviceIds, id: \.self) { dev in
                        let isCh = d.callHosts.contains(where: { $0.did == m.did && $0.deviceId == dev })
                        let pend = d.callHostRequests.contains(where: { $0.did == m.did && $0.deviceId == dev })
                        let suffix = several ? " · \(GroupText.shortDev(dev))" : ""
                        if pend { MItem(icon: GroupsIcons.phone, title: "Cancel call host request\(suffix)", sub: "Waiting for \(fn)’s device to accept") { run { session.ctl.revokeCall(d.id, did: m.did, dev: dev) } } }
                        else if isCh { MItem(icon: GroupsIcons.phone, title: "Remove as call host\(suffix)") { run { session.ctl.revokeCall(d.id, did: m.did, dev: dev) } } }
                        else { MItem(icon: GroupsIcons.phone, title: "Make call host\(suffix)", sub: "Asks \(fn) to carry group calls. They have to accept.") { run { session.ctl.appointCall(d.id, did: m.did, dev: dev) } } }
                    }
                }
                if m.isMe && GroupText.active(d.myRole) {
                    if d.amHost { MItem(icon: GroupsIcons.server, title: "Stop hosting on this device") { run { session.ctl.acceptHost(d.id, accept: false) } } }
                    else if !d.hostOffers.contains(where: { $0.isMe }) {
                        MItem(icon: GroupsIcons.server, title: "Host on this device", sub: "An admin has to appoint it too. A computer that stays on is a better host.") { run { session.ctl.acceptHost(d.id, accept: true) } }
                    }
                    if d.amCallHost { MItem(icon: GroupsIcons.phone, title: "Stop hosting calls on this device") { run { session.ctl.acceptCallHost(d.id, accept: false) } } }
                }
                if acts.contains(.flag) {
                    MItem(icon: GroupsIcons.flag, title: "Flag for removal", sub: "\(GroupText.flagsToRemove) flags remove \(fn) from the group", danger: true) { view = .flag }
                } else if m.isOwner && !m.isMe { MItem(icon: GroupsIcons.flag, title: "The owner can’t be flagged", off: true) {} }
            }.padding(.horizontal, 8).padding(.top, 4)
            HStack { Spacer(); GroupsButton(text: "Close", kind: .quiet) { dismiss() } }.padding(.init(top: 8, leading: 16, bottom: 14, trailing: 16))
        }
    }

    private var flag: some View {
        let fn = groupsFirstName(n)
        let (next, lead) = GroupText.flagCopy(n, flagsSoFar: Int(m.flags))
        return VStack(alignment: .leading, spacing: 0) {
            Text("Flag \(fn) for removal?").font(GroupsType.title).foregroundStyle(tin.ink).padding(.bottom, 12).accessibilityAddTraits(.isHeader)
            GroupsFlagMeter(filled: Int(m.flags), total: GroupText.flagsToRemove, yours: true, desc: "Flag \(next) of \(GroupText.flagsToRemove)")
            GroupsPara(lead, top: 12, color: tin.ink, font: GroupsType.body)
            GroupsPara("Flags can’t be taken back, and removal is final. The same admin can flag more than once; each flag counts.")
            GroupsPara("Flagging is a decision for the group. It is separate from blocking, which only affects you. \(fn) sees each flag: who made it and when.")
            GroupsActs {
                GroupsButton(text: "Back", kind: .quiet) { view = .actions }
                GroupsButton(text: "Flag \(fn)", kind: next >= GroupText.flagsToRemove ? .danger : .outline) { run { session.ctl.flag(d.id, did: m.did) } }
            }
        }.padding(.init(top: 20, leading: 20, bottom: 16, trailing: 20))
    }

    private var ask: some View {
        let fn = groupsFirstName(n)
        return VStack(alignment: .leading, spacing: 0) {
            Text("Ask \(fn) for a private chat").font(GroupsType.title).foregroundStyle(tin.ink).padding(.bottom, 4).accessibilityAddTraits(.isHeader)
            GroupsPara("\(fn) sees your name and the group. The request goes through the group’s host, sealed, so the host can’t read it.")
            GroupsPara("If \(fn) accepts, you can message each other directly. Calls stay off until one of you allows them. A request expires after 7 days.")
            GroupsActs {
                GroupsButton(text: "Back", kind: .quiet) { view = .actions }
                GroupsButton(text: "Send request", kind: .primary) { run { session.ctl.request(d.id, did: m.did) } }
            }
        }.padding(.init(top: 20, leading: 20, bottom: 16, trailing: 20))
    }
}

/// A sheet menu item (`.mi`): 56 pt, 6/12 padding, radius 14, 20 pt glyph, 16 gap; off items are muted and inert.
private struct MItem: View {
    @Environment(\.tin) private var tin
    let icon: GroupsIcon, title: String
    var sub: String?
    var danger = false
    var off = false
    let action: () -> Void
    var body: some View {
        let row = HStack(spacing: 16) {
            GroupsIconView(icon: icon, size: 20, color: danger ? tin.er : tin.ink2)
            VStack(alignment: .leading, spacing: 1) {
                Text(title).font(GroupsType.bodyStrong).foregroundStyle(danger ? tin.er : (off ? tin.ink2 : tin.ink)).multilineTextAlignment(.leading)
                if let sub { Text(sub).font(GroupsType.sub).foregroundStyle(tin.ink2).multilineTextAlignment(.leading) }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 12).padding(.vertical, 6).frame(maxWidth: .infinity, minHeight: 56, alignment: .leading).clipShape(SmoothRoundedRectangle(cornerRadius: 14)).contentShape(Rectangle())
        if off { row.accessibilityElement(children: .combine) } else { Button(action: action) { row }.buttonStyle(GroupsPressStyle()) }
    }
}
