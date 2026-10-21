import SwiftUI

/*
 * Group chat screens on the core (state: GroupsCore.swift), after the boards in docs/design/groups (Main, AndroidGroupInfo,
 * AndroidRemoval): the Groups tab (with invitations to us on top), a group's chat, group info with member actions and the
 * invitations we sent, add members (contact picker, G3), new group, join (paste, scan or a tapped tinline://g/ link), the
 * invite link with a large QR, and why-removed. No "add my other device": linked devices get the group by own-device sync (G2).
 * Port of android GroupsLive.kt; this file is the Groups tab.
 */

// ------------------------------------------------------------------ routes

/// Where the group screens push. The Shell's `NavigationStack` resolves them with `.groupsNavigation()` (GroupsRootView already does).
enum GroupsRoute: Hashable {
    case group(String)
    case info(String)
    case removal(String)
    case link(String)
    case addMembers(String)
    /// Join, with a link already in hand (a tapped tinline://g/ link) or nil.
    case join(String?)
    case newGroup
}

/// Opens a 1:1 chat with a DID (the Chat area); the Shell supplies it. Default: nothing.
private struct GroupsOpenChatKey: EnvironmentKey { static let defaultValue: (String) -> Void = { _ in } }
/// Pushes a route from inside a group screen (e.g. the group a join just opened); GroupsRootView supplies it.
private struct GroupsNavigateKey: EnvironmentKey { static let defaultValue: (GroupsRoute) -> Void = { _ in } }
extension EnvironmentValues {
    var groupsOpenChat: (String) -> Void { get { self[GroupsOpenChatKey.self] } set { self[GroupsOpenChatKey.self] = newValue } }
    var groupsNavigate: (GroupsRoute) -> Void { get { self[GroupsNavigateKey.self] } set { self[GroupsNavigateKey.self] = newValue } }
}

/// The screen for a route.
struct GroupsRouteView: View {
    let route: GroupsRoute
    var body: some View {
        switch route {
        case .group(let id): GroupDetailView(groupId: id)
        case .info(let id): GroupInfoView(groupId: id)
        case .removal(let id): GroupRemovalView(groupId: id)
        case .link(let id): GroupTokenView(groupId: id)
        case .addMembers(let id): AddMembersView(groupId: id)
        case .join(let link): JoinGroupView(initial: link)
        case .newGroup: NewGroupView()
        }
    }
}

extension View {
    /// Registers every Groups route on the enclosing `NavigationStack`. Apply once near its root.
    func groupsNavigation() -> some View { navigationDestination(for: GroupsRoute.self) { GroupsRouteView(route: $0) } }
}

// ------------------------------------------------------------------ the Groups tab (playground screens/groups/GroupsList.svelte)

/// The Groups tab: New group (and Join with a link), invitations to me, chat requests (incoming cards, then the sent ones),
/// my groups newest first, and the folded "Left or removed". Sections fold in and out in place; nothing else moves.
/// Public root of the area: put it in the Shell's `NavigationStack`; it registers the Groups routes itself.
struct GroupsRootView: View {
    var body: some View {
        GroupsSessionGate { GroupsTab(session: $0) }
    }
}

private struct GroupsTab: View {
    @ObservedObject var session: GroupsSession
    @Environment(\.tin) private var tin
    @Environment(\.groupCallLauncher) private var launcher
    @Environment(\.groupsOpenChat) private var openChat
    @StateObject private var calls: GroupCallWatcher
    @State private var joining: String?
    @State private var creating = false
    @State private var goneOpen = false
    @State private var pushed: GroupsRoute?
    @ObservedObject private var inbox = GroupLinkInbox.shared

    init(session: GroupsSession) {
        self.session = session
        _calls = StateObject(wrappedValue: GroupCallWatcher(NoGroupCallLauncher()))
    }

    var body: some View {
        let ctl = session.ctl
        let list = ctl.list
        let pendingInv = GroupText.pendingInvitations(ctl.invitations)
        let requestsIn = ctl.incoming.filter { $0.state == .pending }
        let sent = ctl.outgoing
        let active = list.filter { GroupText.active($0.myRole) }
        let gone = list.filter { !GroupText.active($0.myRole) }
        let me = session.me
        let now = UInt64(Date().timeIntervalSince1970 * 1000)
        let live = calls.snapshot.live.filter { k, _ in active.contains(where: { $0.id == k }) }
        let contacts = session.dir.contacts
        ScrollView {
            LazyVStack(spacing: 0) {
                topRow(GroupsIcons.plus, "New group", "You’ll be the owner") { creating = true }
                NavigationLink(value: GroupsRoute.join(nil)) { TopRowLabel(icon: GroupsIcons.link, title: "Join with a link", sub: "Paste or scan a group invite") }
                    .buttonStyle(GroupsPressStyle())
                // ---- invitations to me
                GroupsFold(visible: !pendingInv.isEmpty) {
                    GroupsSection("Invitations to you", count: pendingInv.count)
                    VStack(spacing: 0) {
                        ForEach(pendingInv, id: \.id) { inv in invitationCard(inv) }
                    }.padding(.horizontal, 8)
                }
                // ---- chat requests, incoming then sent
                GroupsFold(visible: !requestsIn.isEmpty || !sent.isEmpty) {
                    GroupsSection("Chat requests", count: requestsIn.isEmpty ? nil : requestsIn.count)
                    VStack(spacing: 0) { ForEach(requestsIn, id: \.id) { r in requestCard(r) } }.padding(.horizontal, 8)
                    if !sent.isEmpty {
                        GroupsSub2("Sent")
                        VStack(spacing: 0) {
                            ForEach(sent, id: \.id) { r in
                                let open = r.contactAdded && contacts.contains(where: { $0.did == r.peerDid })
                                GroupsListRow(onTap: open ? { openChat(r.peerDid) } : nil) { sentRow(r, now: now) }
                            }
                        }.padding(.horizontal, 8)
                    }
                }
                // ---- my groups
                GroupsSection("Your groups", count: active.count)
                GroupsFold(visible: active.isEmpty) {
                    Text("No groups yet. Create one, or accept an invitation.").font(GroupsType.note).foregroundStyle(tin.ink2)
                        .padding(.horizontal, 20).padding(.vertical, 8).frame(maxWidth: .infinity, alignment: .leading)
                }
                VStack(spacing: 0) {
                    ForEach(active, id: \.id) { s in
                        NavigationLink(value: GroupsRoute.group(s.id)) { GroupRow(s: s, me: me, now: now, call: live[s.id]) }.buttonStyle(GroupsPressStyle())
                    }
                }.padding(.horizontal, 8).animation(GroupsMotion.easeOut(GroupsMotion.enter), value: active.map { $0.id })
                // ---- left or removed, folded
                if !gone.isEmpty { goneSection(gone) }
            }
            .padding(.bottom, 96)
        }
        .background(tin.bg)
        .groupsToasts(session)
        .environment(\.groupsNavigate, { pushed = $0 })
        .groupsNavigation()
        .navigationDestination(item: $pushed) { GroupsRouteView(route: $0) }
        .sheet(isPresented: $creating) {
            NewGroupSheet(session: session) { id in creating = false; DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { pushed = .group(id) } }
                .groupsSheetStyle()
        }
        .task { calls.use(launcher); session.background { session.ctl.refresh() } }
        .task(id: active.map { $0.id }) {
            let ids = active.map { $0.id }
            let l = launcher
            session.background { ids.forEach { l.refreshLive(groupId: $0) } }
        }
        .onChange(of: inbox.link) { _, link in
            if let link { pushed = .join(link); inbox.link = nil }
        }
    }

    private func topRow(_ icon: GroupsIcon, _ title: String, _ sub: String, action: @escaping () -> Void) -> some View {
        Button(action: action) { TopRowLabel(icon: icon, title: title, sub: sub) }.buttonStyle(GroupsPressStyle())
    }

    private func invitationCard(_ inv: GroupInvitation) -> some View {
        let who = session.name(inv.inviterDid, inv.inviterName)
        let busy = joining == inv.id
        return GroupsReqCard {
            HStack(spacing: 12) {
                TinAvatar(did: inv.inviterDid, size: 40)
                VStack(alignment: .leading, spacing: 1) {
                    Text(groupsBold(groupsFirstName(who), " invited you to ", inv.groupName)).font(GroupsType.body).foregroundStyle(tin.ink)
                    Text("Valid until \(GroupsTime.dayOnly(inv.expires))").font(GroupsType.sub).foregroundStyle(tin.ink2)
                }
                Spacer(minLength: 0)
            }
            GroupsActs(top: 0) {
                GroupsButton(text: "Decline", kind: .quiet, enabled: !busy) {
                    session.exec({ session.ctl.declineInvitation(inv.id) }, session.sayDone)
                }
                GroupsButton(text: busy ? "Joining…" : "Accept", kind: .primary, enabled: !busy) {
                    joining = inv.id
                    session.exec({ session.ctl.acceptInvitation(inv.id) }) { r in
                        joining = nil
                        switch r {
                        case .success(let gid): pushed = .group(gid); session.say("You joined \(inv.groupName).")
                        case .failure(let e): session.say(e.message.groupsIfBlank("Couldn’t join"))
                        }
                    }
                }
            }
        }
    }

    private func requestCard(_ r: GroupChatRequest) -> some View {
        let name = session.name(r.peerDid, r.peerName)
        return GroupsReqCard {
            HStack(spacing: 12) {
                TinAvatar(did: r.peerDid, size: 40)
                VStack(alignment: .leading, spacing: 1) {
                    Text(groupsBold(name, " wants to chat")).font(GroupsType.body).foregroundStyle(tin.ink)
                    Text("Member of \(r.groupName)").font(GroupsType.sub).foregroundStyle(tin.ink2)
                }
                Spacer(minLength: 0)
            }
            Text("Accepting adds \(groupsFirstName(name)) as a contact for messages only. You can allow calls later.").font(GroupsType.sub).foregroundStyle(tin.ink2)
            GroupsActs(top: 0) {
                GroupsButton(text: "Block", kind: .dangerQuiet) { session.exec({ session.ctl.blockRequester(r.id, did: r.peerDid) }, session.sayDone) }
                GroupsButton(text: "Decline", kind: .quiet) { session.exec({ session.ctl.decline(r.id) }, session.sayDone) }
                GroupsButton(text: "Accept", kind: .primary) { session.exec({ session.ctl.accept(r.id) }, session.sayDone) }
            }
        }
    }

    private func sentRow(_ r: GroupChatRequest, now: UInt64) -> some View {
        let name = session.name(r.peerDid, r.peerName)
        let (line, bad) = GroupText.sentLine(r)
        return HStack(spacing: 12) {
            TinAvatar(did: r.peerDid, size: 48)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 8) {
                    Text(name).font(GroupsType.bodyStrong).foregroundStyle(tin.ink).lineLimit(1)
                    Spacer(minLength: 0)
                    Text(GroupsTime.list(r.at, now)).font(GroupsType.meta).foregroundStyle(tin.ink2)
                }
                Text(line).font(GroupsType.sub).foregroundStyle(bad ? tin.er : tin.ink2).lineLimit(1)
            }
        }
    }

    private func goneSection(_ gone: [GroupSummary]) -> some View {
        VStack(spacing: 0) {
            Button { goneOpen.toggle() } label: {
                GroupsSection("Left or removed", count: gone.count) {
                    GroupsIconView(icon: GroupsIcons.chev, size: 18, color: tin.ink2).rotationEffect(.degrees(goneOpen ? 180 : 0))
                        .animation(GroupsMotion.easeOut(GroupsMotion.enter), value: goneOpen)
                }
                .frame(minHeight: 44)
            }
            .buttonStyle(GroupsPressStyle())
            .accessibilityLabel("Left or removed").accessibilityValue(goneOpen ? "Expanded" : "Collapsed")
            GroupsCollapse(open: goneOpen) {
                VStack(spacing: 0) {
                    ForEach(gone, id: \.id) { s in
                        NavigationLink(value: GroupsRoute.group(s.id)) {
                            GroupsListRow(onTap: nil) {
                                HStack(spacing: 12) {
                                    GroupsAvatar(size: 48, dim: true)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(s.name).font(GroupsType.bodyStrong).foregroundStyle(tin.ink).opacity(0.72).lineLimit(1)
                                        let at = s.last?.at
                                        Text((s.myRole == .removed ? "Removed" : "You left") + (at.map { " · " + GroupsTime.dayOnly($0) } ?? ""))
                                            .font(GroupsType.sub).foregroundStyle(tin.ink2).lineLimit(1)
                                    }
                                    Spacer(minLength: 0)
                                }
                            }
                        }.buttonStyle(GroupsPressStyle())
                    }
                }.padding(.horizontal, 8)
            }
        }.padding(.top, 6)
    }
}

/// "**a** b **c**": the playground's `<b>` runs inside a line (650 there, 600 here). Odd parts are plain.
func groupsBold(_ parts: String...) -> AttributedString {
    var out = AttributedString()
    for (i, p) in parts.enumerated() {
        var a = AttributedString(p)
        if i % 2 == 0 { a.font = GroupsType.bodyStrong }
        out += a
    }
    return out
}

/// The tab's top rows (`.wrap > .row`): a 48 pt accent-soft disc with a 22 pt glyph, a title and a sub-line, 64 pt.
private struct TopRowLabel: View {
    @Environment(\.tin) private var tin
    let icon: GroupsIcon, title: String, sub: String
    var body: some View {
        HStack(spacing: 12) {
            ZStack { Circle().fill(tin.grpAccentSoft); GroupsIconView(icon: icon, size: 22, color: tin.grpAccentInk) }.frame(width: 48, height: 48)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(GroupsType.bodyStrong).foregroundStyle(tin.ink)
                Text(sub).font(GroupsType.sub).foregroundStyle(tin.ink2)
            }
            Spacer(minLength: 0)
        }
        .padding(8).frame(minHeight: 64).padding(.horizontal, 8).contentShape(Rectangle())
    }
}

/// A list row (`.row`): 72 pt, 8 pt padding, radius 14, gap 12; tappable when `onTap` is set (rows inside a NavigationLink pass nil).
private struct GroupsListRow<Content: View>: View {
    let onTap: (() -> Void)?
    @ViewBuilder var content: () -> Content
    var body: some View {
        let row = content().padding(8).frame(maxWidth: .infinity, minHeight: 72, alignment: .leading)
            .clipShape(SmoothRoundedRectangle(cornerRadius: 14)).contentShape(Rectangle())
        if let onTap { Button(action: onTap) { row }.buttonStyle(GroupsPressStyle()) } else { row }
    }
}

/// An invitation or request card (`.card`): surface, hairline, radius 14, padding 14, 10 between parts, 6 above and below.
private struct GroupsReqCard<Content: View>: View {
    @ViewBuilder var content: () -> Content
    var body: some View {
        GroupsCard { VStack(alignment: .leading, spacing: 10, content: content).padding(14) }.padding(.vertical, 6)
    }
}

/// One of my groups: avatar, name and time on top; preview (or the call line, or "Waiting for a host"), Join and the badge below.
private struct GroupRow: View {
    @Environment(\.tin) private var tin
    let s: GroupSummary, me: String, now: UInt64
    let call: GroupLiveCall?
    var body: some View {
        let at = s.last?.at ?? 0
        let waiting = !s.hostedHere && !s.hostReachable
        let hot = s.unread > 0
        GroupsListRow(onTap: nil) {
            HStack(spacing: 12) {
                GroupsAvatar(size: 48)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 8) {
                        Text(s.name).font(GroupsType.bodyStrong).foregroundStyle(tin.ink).lineLimit(1)
                        Spacer(minLength: 0)
                        Text(at > 0 ? GroupsTime.list(at, now) : "").font(GroupsType.meta.weight(hot ? .bold : .regular)).foregroundStyle(hot ? tin.grpAccentInk : tin.ink2)
                    }
                    HStack(spacing: 8) {
                        Group {
                            if let call { GroupsCallLine(text: call.joined ? "You’re in the call" : "Call in progress") }
                            else if waiting {
                                HStack(spacing: 3) {
                                    GroupsIconView(icon: GroupsIcons.clock, size: 13, color: tin.grpGoldInk)
                                    Text("Waiting for a host").font(GroupsType.subStrong).foregroundStyle(tin.grpGoldInk).lineLimit(1)
                                }
                            } else {
                                Text(GroupText.preview(s, me: me).groupsIfBlank("No posts yet")).font(GroupsType.sub).foregroundStyle(tin.ink2).lineLimit(1)
                            }
                        }.frame(maxWidth: .infinity, alignment: .leading)
                        if let call, !call.joined { GroupsJoinChip() }
                        GroupsBadge(count: Int(s.unread))
                    }.frame(minHeight: 18)
                }
            }
        }
    }
}

/// New group (the playground's dialog; a sheet): a name, then Create. This device is the group's first host.
private struct NewGroupSheet: View {
    @ObservedObject var session: GroupsSession
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var busy = false
    @State private var err: String?
    let onCreated: (String) -> Void

    var body: some View {
        ScrollView {
            GroupsDialogBody(title: "New group") {
                Spacer().frame(height: 10)
                GroupsField(label: "Group name", text: $name, placeholder: "Name", error: err, limit: 60)
                    .onChange(of: name) { _, _ in err = nil }
                GroupsPara("You’ll be the owner. This device is the group’s first host, so it stores the group and passes posts on.", top: 10)
                GroupsActs {
                    GroupsButton(text: "Cancel", kind: .quiet) { dismiss() }
                    GroupsButton(text: busy ? "Creating…" : "Create", kind: .primary, enabled: !name.groupsIsBlank && !busy) {
                        busy = true
                        let n = name
                        session.exec({ session.ctl.create(n) }) { r in
                            busy = false
                            switch r { case .success(let id): onCreated(id); case .failure(let e): err = e.message }
                        }
                    }
                }
            }
        }
    }
}
