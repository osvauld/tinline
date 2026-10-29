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
            if let d = o?.detail { content(d) } else { InfoPage(title: tr("group_info_title"), onBack: { dismiss() }) { EmptyView() } }
        }
        .groupsToasts(session)
        .keepsGroupOpen(session, id)
    }

    /// The name of a DID, with "You" replaced by my profile name where the screen names me.
    private func name(_ did: String, _ fb: String) -> String {
        let n = session.name(did, fb)
        return n == groupsYou ? (session.dir.myName.flatMap { $0.groupsIsBlank ? nil : $0 } ?? groupsYou) : n
    }

    @ViewBuilder private func content(_ d: GroupDetail) -> some View {
        let ctl = session.ctl
        let admin = GroupText.isAdmin(d.myRole)
        let sent = ctl.sent, outgoing = ctl.outgoing
        InfoPage(title: d.myRole == .removed ? tr("group_info_removed_title") : tr("group_info_title"), onBack: { dismiss() }) {
            hero(d)
            switch d.myRole {
            case .removed: RemovalBody(session: session, removal: d.removal) { dismiss() }
            case .left:
                GroupsNote(tr("group_info_left_note")).padding(.horizontal, 20).padding(.vertical, 24)
            default:
                if let mine = d.members.first(where: { $0.isMe }), mine.flags > 0, !mine.isOwner { FlagsOnMe(n: Int(mine.flags)) }
                if admin { invites(d, sent: sent) }
                HostsSection(session: session, d: d, admin: admin, name: name)
                CallHostsSection(session: session, d: d, admin: admin, name: name)
                members(d, admin: admin, outgoing: outgoing)
                VStack(spacing: 0) {
                    GroupsCard {
                        if d.myRole != .owner { ActRow(icon: GroupsIcons.leave, title: tr("group_info_leave"), danger: true) { sheet = .leave } }
                        else { ActRow(icon: GroupsIcons.leave, title: tr("group_info_leave"), sub: tr("group_info_owner_cant_leave"), enabled: false, off: true) {} }
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
            case .owner: return trn("group_info_hero_owner", d.members.count)
            case .admin: return trn("group_info_hero_admin", d.members.count)
            case .member: return trn("group_info_hero_member", d.members.count)
            case .left: return tr("group_info_hero_left")
            case .removed: return tr("group_info_hero_removed")
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
        return InfoSec(title: tr("group_invites_title")) {
            GroupsCard {
                ActRow(icon: GroupsIcons.userPlus, title: tr("group_invites_contacts"), sub: tr("group_invites_contacts_sub"), trailing: GroupsIcons.chevR) { sheet = .invite }
                GroupsRule()
                NavigationLink(value: GroupsRoute.link(id)) {
                    ActRowLabel(icon: GroupsIcons.link, title: tr("group_invites_link"),
                                sub: noHost ? tr("group_invites_link_need_host") : tr("group_invites_link_valid"), trailing: GroupsIcons.plus)
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
            var f = AttributedString(" · " + trn("group_member_flags", flags, String(GroupText.flagsToRemove)))
            f.foregroundColor = tin.er; f.font = GroupsType.subStrong
            sub += f
        }
        if blocked { sub += AttributedString(" · " + tr("group_member_blocked_by_you")) } else if let reqNote { sub += AttributedString(" · \(reqNote)") }
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
        return InfoSec(title: tr("group_members_title"), count: d.members.count) {
            ForEach(sorted, id: \.did) { m in
                let n = name(m.did, m.name)
                let req = outgoing.filter { $0.peerDid == m.did }.max(by: { $0.at < $1.at })
                let reqNote: String? = {
                    switch req?.state {
                    case .pending: return tr("group_member_req_pending")
                    case .declined: return tr("group_member_req_declined")
                    case .blocked: return tr("group_member_req_blocked")
                    default: return nil
                    }
                }()
                let since = m.invitedBy.map { tr("group_member_invited_by", groupsFirstName(name($0, "")), GroupsTime.dayOnly(m.joinedAt)) } ?? tr("group_member_since", GroupsTime.dayOnly(m.joinedAt))
                let showFlags = (admin || m.isMe) && m.flags > 0 && !m.isOwner
                let isCallHost = d.callHosts.contains(where: { $0.did == m.did })
                let desc = GroupText.memberDesc(m.isMe ? tr("group_member_name_you", n) : n, owner: m.isOwner, admin: m.isAdmin, host: m.isHost, callHost: isCallHost)
                let sub = memberSub(since, flags: showFlags ? Int(m.flags) : nil, blocked: session.dir.blockedByMe(m.did), reqNote: reqNote)
                MRow(onTap: { sheet = .member(m.did) }, desc: desc) {
                    TinAvatar(did: m.did, size: 40)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(m.isMe ? tr("group_member_name_you", n) : n).font(GroupsType.bodyStrong).foregroundStyle(tin.ink).lineLimit(1)
                        Text(sub).font(GroupsType.sub).foregroundStyle(tin.ink2).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                    HStack(spacing: 4) {
                        if m.isOwner { GroupsPill(text: tr("group_role_owner"), tone: .owner) } else if m.isAdmin { GroupsPill(text: tr("group_role_admin"), tone: .admin) }
                        if m.isHost { GroupsMark(icon: GroupsIcons.server, desc: tr("group_role_host")) }
                        if isCallHost { GroupsMark(icon: GroupsIcons.phone, desc: tr("group_role_call_host"), call: true) }
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
            if h.isMe { return tr("group_this_device") }
            let several = (d.members.first(where: { $0.did == h.did })?.deviceIds.count ?? 1) > 1
            return name(h.did, h.name) + (several ? " · \(GroupText.shortDev(h.deviceId))" : "")
        }
        InfoSec(title: tr("group_hosts_title")) {
            StatusLine(lines: [
                tr("group_hosts_none"),
                trn("group_hosts_online", max(online, 1)),
                tr("group_hosts_offline"),
            ], current: line, on: line == 1)
            ForEach(d.hosts, id: \.deviceId) { h in
                let on = h.isMe || h.online
                MRow {
                    TinAvatar(did: h.did, size: 40)
                    TwoLine(title: dev(h), sub: d.callHosts.contains(where: { $0.did == h.did && $0.deviceId == h.deviceId }) ? (on ? tr("group_host_online_calls") : tr("group_host_offline_calls")) : (on ? tr("group_host_online") : tr("group_host_offline")), dot: on)
                    if admin { TrailBtn(text: tr("group_remove"), desc: tr("group_remove_host_desc", dev(h))) { session.exec({ session.ctl.revoke(d.id, did: h.did, dev: h.deviceId) }, session.sayDone) } }
                }
            }
            ForEach(d.hostRequests, id: \.deviceId) { h in
                MRow {
                    TinAvatar(did: h.did, size: 40)
                    TwoLine(title: dev(h), sub: tr("group_waiting_accept"))
                    if h.isMe && d.hostRequestedHere {
                        GroupsButton(text: tr("group_decline"), kind: .quiet) { session.exec({ session.ctl.acceptHost(d.id, accept: false) }, session.sayDone) }
                        GroupsButton(text: tr("group_accept"), kind: .primary) { session.exec({ session.ctl.acceptHost(d.id, accept: true) }, session.sayDone) }
                    } else if admin {
                        TrailBtn(text: tr("group_cancel"), desc: tr("group_cancel_host_request_desc", dev(h))) { session.exec({ session.ctl.revoke(d.id, did: h.did, dev: h.deviceId) }, session.sayDone) }
                    }
                }
            }
            ForEach(d.hostOffers, id: \.deviceId) { h in
                MRow {
                    TinAvatar(did: h.did, size: 40)
                    TwoLine(title: dev(h), sub: tr("group_host_offered"))
                    if admin { GroupsButton(text: tr("group_appoint"), kind: .primary) { session.exec({ session.ctl.appoint(d.id, did: h.did, dev: h.deviceId) }, session.sayDone) } }
                }
            }
            GroupsNote(admin ? tr("group_hosts_note_admin") : tr("group_hosts_note_base"))
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
        let dev = { (h: GroupHost) -> String in h.isMe ? tr("group_this_device") : name(h.did, h.name) }
        InfoSec(title: tr("group_callhosts_title")) {
            StatusLine(lines: [
                tr("group_callhosts_none"),
                trn("group_callhosts_ready", max(ready, 1)),
                tr("group_callhosts_unavailable"),
                tr("group_callhosts_this_phone"),
            ], current: line, on: line == 1 || line == 3)
            ForEach(d.callHosts.filter { !$0.isMe }, id: \.deviceId) { h in
                let on = h.isMe || h.online
                MRow {
                    TinAvatar(did: h.did, size: 40)
                    TwoLine(title: dev(h), sub: on ? tr("group_host_online") : tr("group_host_offline"), dot: on)
                    if admin { TrailBtn(text: tr("group_remove"), desc: tr("group_remove_callhost_desc", dev(h))) { session.exec({ session.ctl.revokeCall(d.id, did: h.did, dev: h.deviceId) }, session.sayDone) } }
                }
            }
            ForEach(d.callHostRequests, id: \.deviceId) { h in
                MRow {
                    TinAvatar(did: h.did, size: 40)
                    TwoLine(title: dev(h), sub: tr("group_waiting_accept"))
                    if h.isMe && d.callHostRequestedHere {
                        GroupsButton(text: tr("group_decline"), kind: .quiet) { session.exec({ session.ctl.acceptCallHost(d.id, accept: false) }, session.sayDone) }
                        GroupsButton(text: tr("group_accept"), kind: .primary) { session.exec({ session.ctl.acceptCallHost(d.id, accept: true) }, session.sayDone) }
                    } else if admin {
                        TrailBtn(text: tr("group_cancel"), desc: tr("group_cancel_callhost_request_desc", dev(h))) { session.exec({ session.ctl.revokeCall(d.id, did: h.did, dev: h.deviceId) }, session.sayDone) }
                    }
                }
            }
            // This phone: carry the calls it starts (admin: appoint + accept in one go; asked: the Accept above; already: stop).
            if d.amCallHost {
                ActRow(icon: GroupsIcons.phone, title: tr("group_callhost_stop_here")) { session.exec({ session.ctl.acceptCallHost(d.id, accept: false) }, session.sayDone) }
            } else if admin && !d.callHostRequestedHere && GroupText.active(d.myRole) {
                ActRow(icon: GroupsIcons.phone, title: tr("group_callhost_here"), sub: tr("group_callhost_here_sub")) {
                    session.exec({ session.ctl.hostCallsHere(d.id, requested: false) }, session.sayDone)
                }
            }
            GroupsNote(admin ? tr("group_callhosts_note_admin") : tr("group_callhosts_note_base"))
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
        InfoSec(title: tr("group_flags_on_you"), countText: tr("group_flags_count", n, String(total)), countColor: tin.er) {
            GroupsFlagMeter(filled: n, total: total, desc: trn("group_flags_count_desc", n, String(total))).padding(.init(top: 2, leading: 4, bottom: 4, trailing: 4))
            GroupsNote(trn("group_flags_left", left))
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
        let ord = [tr("group_removal_flag_first"), tr("group_removal_flag_second"), tr("group_removal_flag_third")]
        let now = UInt64(Date().timeIntervalSince1970 * 1000)
        VStack(alignment: .leading, spacing: 0) {
            Text(tr("group_removal_title", String(GroupText.flagsToRemove))).font(GroupsType.hero).foregroundStyle(tin.ink).padding(.top, 8).padding(.bottom, 2).accessibilityAddTraits(.isHeader)
            if let r = removal, r.at > 0 { Text(GroupsTime.dayAndTime(r.at, now)).font(GroupsType.note).foregroundStyle(tin.ink2) }
            RemSec(tr("group_removal_who"))
            if let r = removal {
                ForEach(Array(r.flags.enumerated()), id: \.offset) { i, f in
                    let who = session.name(f.byDid, f.byName)
                    HStack(spacing: 12) {
                        TinAvatar(did: f.byDid, size: 40)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(who).font(GroupsType.bodyStrong).foregroundStyle(tin.ink)
                            Text((i < ord.count ? ord[i] : tr("group_removal_flag")) + (f.at > 0 ? " · " + GroupsTime.dayAndTime(f.at, now) : "")).font(GroupsType.sub.monospacedDigit()).foregroundStyle(tin.ink2)
                        }
                        Spacer(minLength: 0)
                    }.frame(minHeight: 60)
                    if i < r.flags.count - 1 { GroupsRule() }
                }
            }
            GroupsNote(tr("group_removal_checked")).padding(.top, 6)
            RemSec(tr("group_removal_means"))
            Text(tr("group_removal_text"))
                .font(GroupsType.body).lineSpacing(7).foregroundStyle(tin.ink).fixedSize(horizontal: false, vertical: true)
            GroupsButton(text: tr("group_removal_read"), kind: .outline, action: onBack).padding(.top, 20)
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
        InfoPage(title: tr("group_info_removed_title"), onBack: { dismiss() }) { RemovalBody(session: session, removal: r) { dismiss() } }
            .keepsGroupOpen(session, id)
    }
}

private struct LeaveSheet: View {
    @ObservedObject var session: GroupsSession
    @Environment(\.dismiss) private var dismiss
    let d: GroupDetail
    let onLeft: () -> Void
    var body: some View {
        ScrollView {
            GroupsDialogBody(title: tr("group_leave_title", d.name)) {
                GroupsPara(tr("group_leave_text"), top: 4)
                if d.myRole == .admin { GroupsPara(tr("group_leave_admin")) }
                GroupsActs {
                    GroupsButton(text: tr("group_cancel"), kind: .quiet) { dismiss() }
                    GroupsButton(text: tr("group_leave"), kind: .danger) {
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
            GroupsDialogBody(title: tr("group_invite_title", d.name)) {
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
                    GroupsPara(tr("group_invite_text"), top: 10)
                } else {
                    GroupsPara(tr("group_invite_none"), top: 10)
                }
                GroupsActs {
                    GroupsButton(text: tr("group_cancel"), kind: .quiet) { dismiss() }
                    GroupsButton(text: busy ? tr("group_invite_sending") : (picked.isEmpty ? tr("group_invite_button") : trn("group_invite_people", picked.count)), kind: .primary, enabled: !picked.isEmpty && !busy) {
                        busy = true
                        let dids = picked, gid = d.id
                        session.exec({ session.ctl.inviteContacts(gid, dids: dids) }) { r in
                            busy = false
                            switch r {
                            case .success(let s): session.say(s); dismiss()
                            case .failure(let e): session.say(e.message.groupsIfBlank(tr("group_invite_failed")))
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
        .accessibilityLabel(m.isMe ? tr("group_member_role_desc_you") : tr("group_member_in_group_desc", n, d.name))
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
                    Text(m.isMe ? tr("group_member_name_you", n) : n).font(GroupsType.title).foregroundStyle(tin.ink).lineLimit(1)
                    Text(m.invitedBy.map { tr("group_member_invited_by", groupsFirstName(session.name($0, "")), GroupsTime.dayOnly(m.joinedAt)) } ?? tr("group_member_since_long", GroupsTime.dayOnly(m.joinedAt)))
                        .font(GroupsType.sub).foregroundStyle(tin.ink2).lineLimit(1)
                }
                Spacer(minLength: 0)
                HStack(spacing: 4) {
                    GroupsPill(text: m.isOwner ? tr("group_role_owner") : (m.isAdmin ? tr("group_role_admin") : tr("group_role_member")), tone: m.isOwner ? .owner : (m.isAdmin ? .admin : .muted))
                    if m.isHost { GroupsPill(text: tr("group_role_host"), tone: .host) }
                    if d.callHosts.contains(where: { $0.did == m.did }) { GroupsPill(text: tr("group_role_call_host"), tone: .call) }
                }
            }.padding(.init(top: 18, leading: 20, bottom: 10, trailing: 20))
            if admin && !m.isMe && !m.isOwner {
                Text(trn("group_member_flags", Int(m.flags), String(GroupText.flagsToRemove))).font(GroupsType.sub.monospacedDigit()).foregroundStyle(tin.ink2).padding(.horizontal, 20).padding(.bottom, 4)
            }
            VStack(spacing: 0) {
                if !m.isMe {
                    if contact && !blockedByMe { MItem(icon: GroupsIcons.chat, title: tr("group_member_message", fn)) { dismiss(); onChat(m.did) } }
                    else if blockedByMe { MItem(icon: GroupsIcons.off, title: tr("group_member_you_blocked", fn), sub: tr("group_member_blocked_sub"), off: true) {} }
                    else if req?.state == .pending { MItem(icon: GroupsIcons.clock, title: tr("group_member_request_sent"), sub: tr("group_member_request_waiting", fn), off: true) {} }
                    else if req?.state == .blocked { MItem(icon: GroupsIcons.off, title: tr("group_member_blocked_title"), sub: tr("group_member_cant_receive", fn), off: true) {} }
                    else if acts.contains(.message) {
                        MItem(icon: GroupsIcons.chat, title: req?.state == .declined ? tr("group_member_ask_again", fn) : tr("group_member_ask", fn),
                              sub: req?.state == .declined ? tr("group_member_declined_last", fn) : tr("group_member_ask_sub")) { view = .ask }
                    }
                }
                ForEach(Array(acts.enumerated()), id: \.offset) { _, a in
                    switch a {
                    case .makeAdmin: MItem(icon: GroupsIcons.shield, title: tr("group_member_make_admin")) { run { session.ctl.setAdmin(d.id, did: m.did, admin: true) } }
                    case .revokeAdmin: MItem(icon: GroupsIcons.shield, title: tr("group_member_remove_admin")) { run { session.ctl.setAdmin(d.id, did: m.did, admin: false) } }
                    case .appoint(let dev):
                        MItem(icon: GroupsIcons.server, title: several ? tr("group_member_make_device_host", GroupText.shortDev(dev)) : tr("group_member_make_host"), sub: tr("group_member_make_host_sub", fn)) {
                            run { session.ctl.appoint(d.id, did: m.did, dev: dev) }
                        }
                    case .revoke(let dev):
                        MItem(icon: GroupsIcons.server, title: several ? tr("group_member_stop_device_host", GroupText.shortDev(dev)) : tr("group_member_remove_host")) { run { session.ctl.revoke(d.id, did: m.did, dev: dev) } }
                    default: EmptyView()
                    }
                }
                if admin && !m.isMe {
                    ForEach(m.deviceIds, id: \.self) { dev in
                        let isCh = d.callHosts.contains(where: { $0.did == m.did && $0.deviceId == dev })
                        let pend = d.callHostRequests.contains(where: { $0.did == m.did && $0.deviceId == dev })
                        if pend { MItem(icon: GroupsIcons.phone, title: several ? tr("group_member_cancel_callhost_device", GroupText.shortDev(dev)) : tr("group_member_cancel_callhost"), sub: tr("group_member_cancel_callhost_sub", fn)) { run { session.ctl.revokeCall(d.id, did: m.did, dev: dev) } } }
                        else if isCh { MItem(icon: GroupsIcons.phone, title: several ? tr("group_member_remove_callhost_device", GroupText.shortDev(dev)) : tr("group_member_remove_callhost")) { run { session.ctl.revokeCall(d.id, did: m.did, dev: dev) } } }
                        else { MItem(icon: GroupsIcons.phone, title: several ? tr("group_member_make_callhost_device", GroupText.shortDev(dev)) : tr("group_member_make_callhost"), sub: tr("group_member_make_callhost_sub", fn)) { run { session.ctl.appointCall(d.id, did: m.did, dev: dev) } } }
                    }
                }
                if m.isMe && GroupText.active(d.myRole) {
                    if d.amHost { MItem(icon: GroupsIcons.server, title: tr("group_host_stop_device")) { run { session.ctl.acceptHost(d.id, accept: false) } } }
                    else if !d.hostOffers.contains(where: { $0.isMe }) {
                        MItem(icon: GroupsIcons.server, title: tr("group_host_device"), sub: tr("group_host_device_sub")) { run { session.ctl.acceptHost(d.id, accept: true) } }
                    }
                    if d.amCallHost { MItem(icon: GroupsIcons.phone, title: tr("group_callhost_stop_device")) { run { session.ctl.acceptCallHost(d.id, accept: false) } } }
                }
                if acts.contains(.flag) {
                    MItem(icon: GroupsIcons.flag, title: tr("group_member_flag"), sub: tr("group_member_flag_sub", String(GroupText.flagsToRemove), fn), danger: true) { view = .flag }
                } else if m.isOwner && !m.isMe { MItem(icon: GroupsIcons.flag, title: tr("group_member_owner_no_flag"), off: true) {} }
            }.padding(.horizontal, 8).padding(.top, 4)
            HStack { Spacer(); GroupsButton(text: tr("group_close"), kind: .quiet) { dismiss() } }.padding(.init(top: 8, leading: 16, bottom: 14, trailing: 16))
        }
    }

    private var flag: some View {
        let fn = groupsFirstName(n)
        let (next, lead) = GroupText.flagCopy(n, flagsSoFar: Int(m.flags))
        return VStack(alignment: .leading, spacing: 0) {
            Text(tr("group_flag_title", fn)).font(GroupsType.title).foregroundStyle(tin.ink).padding(.bottom, 12).accessibilityAddTraits(.isHeader)
            GroupsFlagMeter(filled: Int(m.flags), total: GroupText.flagsToRemove, yours: true, desc: tr("group_flag_meter_desc", String(next), String(GroupText.flagsToRemove)))
            GroupsPara(lead, top: 12, color: tin.ink, font: GroupsType.body)
            GroupsPara(tr("group_flag_text1"))
            GroupsPara(tr("group_flag_text2", fn))
            GroupsActs {
                GroupsButton(text: tr("group_back"), kind: .quiet) { view = .actions }
                GroupsButton(text: tr("group_flag_button", fn), kind: next >= GroupText.flagsToRemove ? .danger : .outline) { run { session.ctl.flag(d.id, did: m.did) } }
            }
        }.padding(.init(top: 20, leading: 20, bottom: 16, trailing: 20))
    }

    private var ask: some View {
        let fn = groupsFirstName(n)
        return VStack(alignment: .leading, spacing: 0) {
            Text(tr("group_ask_title", fn)).font(GroupsType.title).foregroundStyle(tin.ink).padding(.bottom, 4).accessibilityAddTraits(.isHeader)
            GroupsPara(tr("group_ask_text1", fn))
            GroupsPara(tr("group_ask_text2", fn))
            GroupsActs {
                GroupsButton(text: tr("group_back"), kind: .quiet) { view = .actions }
                GroupsButton(text: tr("group_ask_send"), kind: .primary) { run { session.ctl.request(d.id, did: m.did) } }
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
