import SwiftUI

/*
 * A group's chat (android GroupsLive.kt GroupChatLive and its pieces): the bar with the group head, call and info buttons,
 * the ongoing-call banner, the host ask card, the thread (posts and the group's call log merged by time), the waiting strip and
 * the composer or the "removed / left" footer. Bubbles are drawn here (GroupsBubble) because group posts add a sender line,
 * the n/N receipt lines and "Waiting for a host"; the 1:1 bubble belongs to the Chat area.
 */

/// Which group chat is on screen, for the Shell's notifications (android `OpenChat.group` + `GroupNotifier.cancel`).
extension GroupsOpenChat {
    /// Called on the main actor when a chat comes in front of the user: cancel what was shown for it.
    static var onShown: ((String) -> Void)?
}

/// Public screen: one group's chat. Push it from the Shell (`GroupsRoute.group`) or from a notification.
struct GroupDetailView: View {
    let groupId: String
    var body: some View {
        GroupsSessionGate { GroupChatScreen(session: $0, id: groupId) }
    }
}

private enum ThreadRow: Identifiable {
    case post(Int, GroupMessage)
    case call(GroupCallRecord)
    var at: UInt64 {
        switch self { case .post(_, let m): return m.at; case .call(let r): return UInt64(r.startedAt) * 1000 }
    }
    var id: String {
        switch self { case .post(_, let m): return m.id; case .call(let r): return "call:" + r.callId }
    }
    var order: (Int, Int) {
        switch self { case .call: return (0, 0); case .post(let i, _): return (1, i) }
    }
}

private struct GroupChatScreen: View {
    @ObservedObject var session: GroupsSession
    let id: String
    @Environment(\.tin) private var tin
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.groupCallLauncher) private var launcher
    @StateObject private var calls = GroupCallWatcher(NoGroupCallLauncher())
    @State private var draft = ""
    @State private var replyTo: String?
    @State private var sending = false
    /// Unread posts when the group opened: where "n new messages" goes (the count is cleared as it opens).
    @State private var unreadAtOpen: Int

    init(session: GroupsSession, id: String) {
        self.session = session
        self.id = id
        _unreadAtOpen = State(initialValue: Int(session.ctl.list.first(where: { $0.id == id })?.unread ?? 0))
    }

    var body: some View {
        let o = session.ctl.opened.flatMap { $0.id == id ? $0 : nil }
        VStack(spacing: 0) {
            if let o { chat(o) } else { loading }
        }
        .background(tin.bg.ignoresSafeArea(edges: .top))
        .toolbar(.hidden, for: .navigationBar)
        .groupsToasts(session)
        .task { calls.use(launcher) }
        .task(id: id) { session.background { session.ctl.openGroup(id) } }
        .onAppear { GroupsOpenChat.current = id; GroupsOpenChat.onShown?(id) }
        .onDisappear { session.ctl.close(id); if GroupsOpenChat.current == id { GroupsOpenChat.current = nil } }
        .onChange(of: scenePhase) { _, phase in
            // In front of the user: its posts don't notify, and what was shown for it goes away.
            if phase == .active { GroupsOpenChat.current = id; GroupsOpenChat.onShown?(id) }
            else if GroupsOpenChat.current == id { GroupsOpenChat.current = nil }
        }
    }

    private var loading: some View {
        VStack(spacing: 0) {
            GroupsBar(title: tr("group_title"), onBack: { dismiss() }, surface: false)
            GroupsHint(tr("group_loading")).padding(24)
            Spacer()
        }
    }

    private func chat(_ o: OpenGroup) -> some View {
        let d = o.detail
        let me = session.me
        let waiting = GroupText.waitingForHost(d)
        let nowMs = UInt64(Date().timeIntervalSince1970 * 1000)
        let log = calls.snapshot.history.filter { $0.groupId == id }.map { ThreadRow.call($0) }
        // The thread: posts and this group's call log merged by time, oldest first. Calls go before a post of the same
        // second; posts keep their own order.
        let rows = (o.messages.enumerated().map { ThreadRow.post($0.offset, $0.element) } + log).sorted { a, b in
            if a.at != b.at { return a.at < b.at }
            return a.order < b.order
        }
        let dividerId: String? = unreadAtOpen > 0
            ? o.messages.filter { !$0.outgoing && $0.kind == .post }.suffix(unreadAtOpen).first?.id : nil
        return VStack(spacing: 0) {
            bar(d)
            GroupsRule()
            if GroupText.active(d.myRole) { GroupsOngoingCallBanner(session: session, gid: id, snapshot: calls.snapshot) }
            HostAskCard(session: session, d: d)
            ZStack(alignment: .top) {
                tin.grpCanvas
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            if o.older != nil {
                                GroupsFlowButton(text: tr("group_older"), style: .text, height: 40, fill: false) { session.background { session.ctl.older() } }
                            }
                            if rows.isEmpty { GroupsSysLine(text: tr("group_no_messages")) }
                            ForEach(Array(rows.enumerated()), id: \.element.id) { k, row in
                                let newDay = k == 0 || !GroupsTime.sameDay(rows[k - 1].at, row.at)
                                VStack(spacing: 0) {
                                    if newDay { GroupsDayPill(label: GroupsTime.dayLabel(row.at, nowMs)).padding(.vertical, 10) }
                                    switch row {
                                    case .call(let r):
                                        GroupCallPill(r: r, me: me, enabled: GroupText.active(d.myRole)) { launcher.startOrJoin(groupId: id, onError: { session.say($0) }) }
                                    case .post(let i, let m):
                                        if m.id == dividerId { GroupsUnreadDivider(n: unreadAtOpen) }
                                        GroupItem(session: session, all: o.messages, i: i, m: m, d: d, onReply: { replyTo = m.id },
                                                  top: newDay ? 0 : nil, receipt: o.receipts[m.id])
                                    }
                                }.id(row.id)
                            }
                            Color.clear.frame(height: 1).id("end")
                        }
                        .padding(.horizontal, 10).padding(.top, 6 + (waiting ? 33 : 0)).padding(.bottom, 19)
                    }
                    .defaultScrollAnchor(.bottom)
                    .onChange(of: rows.last?.id) { _, _ in proxy.scrollTo("end", anchor: .bottom) }
                    .scrollDismissesKeyboard(.interactively)
                }
                // The strip over the top of the list (gold): waiting for a host. It overlays, so nothing moves.
                if waiting { GroupsStrip(text: tr("group_strip_waiting"), icon: GroupsIcons.clock, warn: true) }
            }
            footer(o)
        }
    }

    /// Conversation.svelte `.bar` for a group: back, the head (avatar 36, name 17/22, status 13/18; tap = info), call, info.
    private func bar(_ d: GroupDetail) -> some View {
        let snap = calls.snapshot
        let running = snap.live[d.id] != nil || snap.activeGroupId == d.id
        let noHost = d.callHosts.isEmpty && !running
        return HStack(spacing: 2) {
            GroupsBarIcon(icon: GroupsIcons.back, label: tr("group_back_to_groups")) { dismiss() }
            NavigationLink(value: GroupsRoute.info(id)) {
                HStack(spacing: 10) {
                    GroupsAvatar(size: 36, dim: !GroupText.active(d.myRole))
                    VStack(alignment: .leading, spacing: 0) {
                        Text(d.name).font(.custom("Figtree", size: 17, relativeTo: .headline).weight(.semibold)).foregroundStyle(tin.ink).lineLimit(1)
                        Text(GroupText.status(d)).font(.custom("Figtree", size: 13, relativeTo: .footnote))
                            .foregroundStyle(d.myRole == .removed ? tin.er : tin.ink2).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                .padding(.leading, 6).padding(.trailing, 10).padding(.vertical, 4).frame(minHeight: 44)
                .clipShape(SmoothRoundedRectangle(cornerRadius: 12)).contentShape(Rectangle())
            }
            .buttonStyle(GroupsPressStyle())
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(tr("group_chat_header_desc", d.name, GroupText.status(d)))
            // The call button: start, join the running call, or return to mine (accent while one runs); dimmed while the
            // group has no call host and no call runs (a tap says why).
            if GroupText.active(d.myRole) {
                GroupsBarIcon(icon: GroupsIcons.phone,
                              label: snap.activeGroupId == d.id ? tr("group_call_return") : (running ? tr("group_call_join") : tr("group_call_start"))) {
                    if noHost { session.say(tr("group_call_no_host")) }
                    else { launcher.startOrJoin(groupId: d.id, onError: { session.say($0) }) }
                }
                .background { if running { Circle().fill(tin.grpAccentSoft) } }
                .opacity(noHost ? 0.45 : 1)
            }
            NavigationLink(value: GroupsRoute.info(id)) { GroupsIconView(icon: GroupsIcons.info, size: 22, color: tin.ink).frame(width: 44, height: 44).contentShape(Circle()) }
                .buttonStyle(GroupsPressStyle()).accessibilityLabel(tr("group_info_desc"))
        }
        .padding(.horizontal, 6).padding(.vertical, 4).frame(height: 56).background(tin.sf)
    }

    @ViewBuilder private func footer(_ o: OpenGroup) -> some View {
        let d = o.detail
        switch d.myRole {
        case .removed:
            GroupsBlockedFooter(hint: tr("group_removed_footer")) {
                NavigationLink(value: GroupsRoute.removal(id)) {
                    Text(tr("group_why")).font(TinType.button).foregroundStyle(tin.pr).frame(maxWidth: .infinity, minHeight: 48)
                        .overlay { Capsule().strokeBorder(tin.ln2, lineWidth: 1) }.contentShape(Capsule())
                }.buttonStyle(GroupsPressStyle())
            }
        case .left:
            GroupsBlockedFooter(hint: tr("group_left_footer")) { EmptyView() }
        default:
            let r = replyTo.flatMap { rid in o.messages.first(where: { $0.id == rid }) }
            GroupsComposer(
                draft: $draft, placeholder: tr("group_message_placeholder"),
                ctxTitle: r.map { $0.outgoing ? tr("group_replying_to_self") : tr("group_replying_to", groupsFirstName(session.name($0.authorDid, $0.authorName))) },
                ctxText: r?.text ?? "", onCancelCtx: { replyTo = nil }
            ) {
                if sending || draft.groupsIsBlank { return }
                let text = draft, reply = replyTo
                draft = ""; replyTo = nil; sending = true
                let gid = id
                session.exec({ session.ctl.post(gid, text: text, replyTo: reply) }) { res in
                    sending = false
                    if case .failure(let e) = res { if draft.isEmpty { draft = text }; session.say(e.message.groupsIfBlank(tr("group_cant_send"))) }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ pieces

private struct GroupsBarIcon: View {
    @Environment(\.tin) private var tin
    let icon: GroupsIcon, label: String
    let action: () -> Void
    var body: some View {
        Button(action: action) { GroupsIconView(icon: icon, size: 22, color: tin.ink).frame(width: 44, height: 44).contentShape(Circle()) }
            .buttonStyle(GroupsPressStyle()).accessibilityLabel(label)
    }
}

/// Day separator (`.day`): a centred pill, surface with an inset hairline, 11 semibold muted.
private struct GroupsDayPill: View {
    @Environment(\.tin) private var tin
    let label: String
    var body: some View {
        Text(label).font(.custom("Figtree", size: 11, relativeTo: .caption2).weight(.semibold)).foregroundStyle(tin.ink2).lineLimit(1)
            .padding(.horizontal, 10).padding(.vertical, 3).background(tin.sf, in: Capsule())
            .overlay { Capsule().strokeBorder(tin.grpHairline, lineWidth: 1) }.frame(maxWidth: .infinity)
    }
}

/// "2 new messages" (`.unread`): accent-ink label between two thin lines.
private struct GroupsUnreadDivider: View {
    @Environment(\.tin) private var tin
    let n: Int
    var body: some View {
        let line = tin.grpAccentInk.opacity(0.35)
        let label = trn("group_unread_divider", n)
        HStack(spacing: 10) {
            Rectangle().fill(line).frame(height: 1)
            Text(label).font(.custom("Figtree", size: 11, relativeTo: .caption2).weight(.bold)).foregroundStyle(tin.grpAccentInk)
            Rectangle().fill(line).frame(height: 1)
        }
        .padding(.top, 12).padding(.bottom, 6).accessibilityElement(children: .ignore).accessibilityLabel(label)
    }
}

/// The status strip over the top of the list (`.strip`); `warn` is the gold "waiting for a host" look.
private struct GroupsStrip: View {
    @Environment(\.tin) private var tin
    let text: String, icon: GroupsIcon
    var warn = false
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                GroupsIconView(icon: icon, size: 16, color: warn ? tin.onWarn : tin.ink2)
                Text(text).font(.custom("Figtree", size: 13, relativeTo: .footnote)).foregroundStyle(warn ? tin.onWarn : tin.ink2)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 16).padding(.vertical, 7).background(warn ? tin.warn : tin.sf2)
            Rectangle().fill(tin.grpHairline).frame(height: 1)
        }
    }
}

/// MessageRow `.callrec` for a group call (32 tall pill, radius 16, surface, hairline): "Group call · 19 min · 4 people", or
/// "Missed group call · 5 min" (calm, never red). On my side when I started it; tap = start or join a call in this group.
private struct GroupCallPill: View {
    @Environment(\.tin) private var tin
    let r: GroupCallRecord, me: String, enabled: Bool
    let onCall: () -> Void
    var body: some View {
        let text = groupCallLabel(r: r)
        let time = GroupsTime.clock(UInt64(r.startedAt) * 1000)
        HStack {
            if !r.missed && r.startedBy == me { Spacer(minLength: 0) }
            Button(action: onCall) {
                HStack(spacing: 6) {
                    GroupsIconView(icon: GroupsIcons.phone, size: 16, color: tin.ink2)
                    Text(text).font(.custom("Figtree", size: 13, relativeTo: .footnote).weight(.semibold)).foregroundStyle(tin.ink).lineLimit(1)
                    Text(time).font(.custom("Figtree", size: 11, relativeTo: .caption2).monospacedDigit()).foregroundStyle(tin.ink2)
                }
                .padding(.horizontal, 12).frame(minHeight: 32).background(tin.sf, in: Capsule())
                .overlay { Capsule().strokeBorder(tin.ln, lineWidth: 1) }.contentShape(Capsule())
            }
            .buttonStyle(GroupsPressStyle()).disabled(!enabled)
            .accessibilityLabel(tr("group_call_line_desc", text, time))
            if r.missed || r.startedBy != me { Spacer(minLength: 0) }
        }.padding(.top, 10)
    }
}

/// OngoingCall.svelte: "Call in progress" under a group's header while its call runs and I'm not in it (Join), or
/// "You're in the call" with the timer (Open). The core doesn't say how many joined, so the line names who started it.
private struct GroupsOngoingCallBanner: View {
    @ObservedObject var session: GroupsSession
    let gid: String
    let snapshot: GroupCallSnapshot
    @Environment(\.tin) private var tin
    @Environment(\.groupCallLauncher) private var launcher
    var body: some View {
        let mine = snapshot.activeGroupId == gid
        let call = snapshot.live[gid]
        if mine || call != nil {
            let fg = tin.dark ? tin.ink : tin.grpAccentInk
            let bg = tin.dark ? GroupsBlend.mix(tin.sf, tin.grpAccent, 0.24) : tin.grpAccentSoft
            HStack(spacing: 12) {
                GroupsIconView(icon: GroupsIcons.users, size: 20, color: fg)
                VStack(alignment: .leading, spacing: 0) {
                    Text(mine ? tr("group_call_you_in") : tr("group_call_in_progress")).font(.custom("Figtree", size: 15, relativeTo: .body).weight(.semibold)).foregroundStyle(fg).lineLimit(1)
                    Text(mine ? snapshot.activeLine : tr("group_call_started_by", groupsFirstName(session.name(call?.starterDid ?? "", ""))))
                        .font(.custom("Figtree", size: 13, relativeTo: .footnote).monospacedDigit()).foregroundStyle(fg.opacity(0.85))
                }
                Spacer(minLength: 0)
                Button { if mine { launcher.restore() } else { launcher.startOrJoin(groupId: gid, onError: { session.say($0) }) } } label: {
                    Text(mine ? tr("group_call_open") : tr("group_join")).font(.custom("Figtree", size: 13, relativeTo: .footnote).weight(.bold))
                        .foregroundStyle(mine ? tin.grpAccentInk : tin.grpOnAccent)
                        .padding(.horizontal, 18).frame(minWidth: 72, minHeight: 36)
                        .background { if mine { Capsule().strokeBorder(tin.grpAccentInk, lineWidth: 1.5) } else { Capsule().fill(tin.grpAccent) } }
                        .contentShape(Capsule())
                }.buttonStyle(GroupsPressStyle())
            }
            .padding(.init(top: 8, leading: 14, bottom: 8, trailing: 12)).frame(minHeight: 56).frame(maxWidth: .infinity).background(bg)
        }
    }
}

/// "X asked this phone to host <group>": Host here / Decline (G2 `host_requested_here`).
private struct HostAskCard: View {
    @ObservedObject var session: GroupsSession
    @Environment(\.tin) private var tin
    let d: GroupDetail
    var body: some View {
        if let by = GroupText.hostAsker(d) {
            GroupsNoticeCard(bg: tin.warn, fg: tin.onWarn, title: tr("group_host_asked_title", groupsFirstName(by), d.name),
                text: tr("group_host_asked_text")) {
                HStack(spacing: 8) {
                    GroupsFlowButton(text: tr("group_host_here"), style: .filled, height: 44) { session.exec({ session.ctl.acceptHost(d.id, accept: true) }, session.sayDone) }
                    GroupsFlowButton(text: tr("group_decline"), style: .outlined, height: 44) {
                        session.exec({ session.ctl.acceptHost(d.id, accept: false).map { _ in tr("group_done_host_declined") } }, session.sayDone)
                    }
                }
            }
            .padding(.horizontal, 16)
        }
    }
}

// ------------------------------------------------------------------ one group item


/// One group item (MessageRow.svelte): a system line, my removal card, a folded post from someone I blocked, or a post as a
/// bubble (sender's first name in their colour on the first of their run, the quote, the inline time and tick; "Waiting for a
/// host" under my post while no host has it). Long-press replies.
private struct GroupItem: View {
    @ObservedObject var session: GroupsSession
    @Environment(\.tin) private var tin
    let all: [GroupMessage]
    let i: Int
    let m: GroupMessage
    let d: GroupDetail
    let onReply: () -> Void
    var top: CGFloat?
    var receipt: GroupReceiptStatus?
    @State private var shown = false
    @State private var info = false

    var body: some View {
        let me = session.me
        if m.kind == .removed && m.subjectDid == me { removedItem }
        else if m.kind != .post {
            // .sys: centred 12/17 muted, 6 above and below.
            Text(GroupText.sysText(m, me: me)).font(.custom("Figtree", size: 12, relativeTo: .caption)).foregroundStyle(tin.ink2).multilineTextAlignment(.center)
                .frame(maxWidth: .infinity).padding(.vertical, 6).padding(.horizontal, 24)
        } else { post(me) }
    }

    @ViewBuilder private var removedItem: some View {
        if let r = d.removal {
            let names = r.flags.map { session.name($0.byDid, $0.byName) }.reduce(into: [String]()) { if !$0.contains($1) { $0.append($1) } }
            let who = names.count > 1 ? tr("group_names_and", names.dropLast().joined(separator: ", "), names.last!) : names.joined(separator: ", ")
            GroupsNoticeCard(bg: tin.erc, fg: tin.onErc, title: trn("group_removed_card_title", r.flags.count),
                text: tr("group_removed_card_text", who)) {
                NavigationLink(value: GroupsRoute.removal(d.id)) {
                    Text(tr("group_see_details")).font(TinType.label.weight(.bold)).underline().foregroundStyle(tin.onErc).frame(minHeight: 48, alignment: .leading)
                }.buttonStyle(GroupsPressStyle())
            }
        } else { GroupsSysLine(text: tr("group_preview_removed")) }
    }

    /// The tick and what a screen reader hears: waiting = the slack string; sent = one tick; delivered/read from the member receipts.
    private func tickOf(_ lines: [String]?) -> (ReceiptTick, String)? {
        if !m.outgoing { return nil }
        if m.delivery == .waiting { return (ReceiptTick.waiting, tr("group_waiting_for_host")) }
        var t = ReceiptTick.onlyHere
        if let r = receipt, r.tick != ReceiptTick.none { t = r.tick }
        return (t, lines.map { $0.prefix(2).joined(separator: ", ") } ?? tr("group_receipt_sent"))
    }

    /// Runs: same author, same day, within 5 minutes (Conversation.svelte `join`).
    private func joined(_ a: GroupMessage?, _ b: GroupMessage?) -> Bool {
        guard let a, let b, a.kind == .post, b.kind == .post, a.outgoing == b.outgoing, a.outgoing || a.authorDid == b.authorDid,
              GroupsTime.sameDay(a.at, b.at) else { return false }
        return (a.at > b.at ? a.at - b.at : b.at - a.at) <= 5 * 60_000
    }

    @ViewBuilder private func post(_ me: String) -> some View {
        let name = session.name(m.authorDid, m.authorName)
        let blocked = session.dir.blockedByMe(m.authorDid)
        if blocked && !m.outgoing && !shown {
            // A personal block never changes the group; the message is folded, not dropped.
            HStack {
                Button { shown = true } label: {
                    HStack(spacing: 8) {
                        GroupsIconView(icon: GroupsIcons.off, size: 18, color: tin.ink2)
                        Text(tr("group_blocked_message", groupsFirstName(name))).font(TinType.bodyM).foregroundStyle(tin.ink2).multilineTextAlignment(.leading)
                    }
                    .padding(.horizontal, 12).frame(minHeight: 48).background(tin.sf2, in: SmoothRoundedRectangle(cornerRadius: 18))
                }.buttonStyle(GroupsPressStyle())
                Spacer(minLength: 0)
            }.padding(.top, 10)
        } else {
            let p = joined(i > 0 ? all[i - 1] : nil, m), n = joined(m, i + 1 < all.count ? all[i + 1] : nil)
            let pos: BubblePos = p && n ? .mid : (p ? .last : (n ? .first : .solo))
            let qr = m.replyTo.flatMap { rid in all.first(where: { $0.id == rid }) }
            let qWho = qr.map { $0.outgoing ? groupsYou : groupsFirstName(session.name($0.authorDid, $0.authorName)) }
            let qColor = qr?.outgoing == true ? tin.grpAccentInk : (qr.map { groupsAuthorColor($0.authorDid, tin) } ?? tin.grpAccentInk)
            // Waiting for a host = the slack string; sent = one tick; every eligible member signed delivered = two ticks; every
            // one read = the read ink (core's GroupReceiptStatus). Host storage is never members' delivery/read.
            let lines = receipt.flatMap { $0.eligible > 0 ? groupReceiptLines($0) : nil }
            let tick = tickOf(lines)
            VStack(alignment: m.outgoing ? .trailing : .leading, spacing: 0) {
                GroupsFractionWidth(fraction: 0.78, trailing: m.outgoing) {
                    GroupsBubble(text: m.text, pos: pos, mine: m.outgoing, time: GroupsTime.clock(m.at), tick: tick,
                                 sender: m.outgoing || p ? nil : m.beforeRemoval ? tr("group_sender_removed", groupsFirstName(name)) : groupsFirstName(name),
                                 senderColor: m.beforeRemoval ? tin.ink2 : groupsAuthorColor(m.authorDid, tin),
                                 quote: m.replyTo == nil ? nil : (qWho, qColor, qr?.text ?? tr("group_quote_unavailable")))
                        .onTapGesture { if lines != nil { info.toggle() } }
                        .contextMenu { if GroupText.active(d.myRole) { Button(tr("group_reply"), action: onReply) } }
                }
                if m.outgoing && m.delivery == .waiting {
                    HStack(spacing: 4) {
                        GroupsIconView(icon: GroupsIcons.clock, size: 12, color: tin.grpGoldInk)
                        Text(tr("group_waiting_for_host")).font(.custom("Figtree", size: 12, relativeTo: .caption).weight(.semibold)).foregroundStyle(tin.grpGoldInk)
                    }.padding(.top, 3).padding(.trailing, 2)
                }
                if m.beforeRemoval {
                    Text(tr("group_written_before", groupsFirstName(name))).font(.custom("Figtree", size: 11, relativeTo: .caption2)).foregroundStyle(tin.ink2).padding(.top, 3)
                }
                if info, let lines {
                    Text(lines.joined(separator: "\n")).font(.custom("Figtree", size: 12, relativeTo: .caption)).foregroundStyle(tin.ink2)
                        .multilineTextAlignment(.trailing).padding(.top, 3).padding(.trailing, 2)
                }
            }
            .frame(maxWidth: .infinity, alignment: m.outgoing ? .trailing : .leading)
            .padding(.top, top ?? (pos == .mid || pos == .last ? 2 : 10))
        }
    }
}

/// "Delivered n/N", "Read n/N", then who (eligible members by level; kept statements of people no longer counted). Same words as desktop.
func groupReceiptLines(_ r: GroupReceiptStatus) -> [String] {
    func who(_ m: GroupMemberReceipts) -> String { m.name.isEmpty ? tr("group_someone") : groupsFirstName(m.name) }
    var out = [tr("group_receipt_delivered", Int(r.delivered), String(r.eligible)), tr("group_receipt_read", Int(r.read), String(r.eligible))]
    for (key, want) in [("group_receipt_level_read", ReceiptLevel.read), ("group_receipt_level_delivered", .delivered), ("group_receipt_level_not_yet", .unconfirmed)] {
        let n = r.members.filter { $0.eligible && $0.receipts.level == want }.map(who)
        if !n.isEmpty { out.append(tr(key, n.joined(separator: ", "))) }
    }
    let gone = r.members.filter { !$0.eligible }.map(who)
    if !gone.isEmpty { out.append(tr("group_receipt_also", gone.joined(separator: ", "))) }
    return out
}

// ------------------------------------------------------------------ bubble and composer

/// Gives its one child at most `fraction` of the proposed width (the bubbles' 78 %), at the start or the end of the row.
private struct GroupsFractionWidth: Layout {
    var fraction: CGFloat
    var trailing: Bool
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let w = proposal.width ?? 320
        let s = subviews[0].sizeThatFits(ProposedViewSize(width: w * fraction, height: nil))
        return CGSize(width: w, height: s.height)
    }
    func placeSubviews(in b: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let w = b.width * fraction
        let s = subviews[0].sizeThatFits(ProposedViewSize(width: w, height: nil))
        subviews[0].place(at: CGPoint(x: trailing ? b.maxX - s.width : b.minX, y: b.minY), proposal: ProposedViewSize(width: w, height: nil))
    }
}

/// The bubble surface (MessageRow.svelte `.bub` + `.skin`): outline by run position, sent = --bub-out (prc), received = --bub-in
/// with a 1 pt --bub-in-line stroke. 7/12 padding plus the 1 px border = 8/13. The meta (time, tick) sits at the end of the last
/// line when the text and meta fit on one row, else on a row of its own (android floats it inline in all cases).
private struct GroupsBubble: View {
    @Environment(\.tin) private var tin
    let text: String
    let pos: BubblePos
    let mine: Bool
    let time: String
    let tick: (ReceiptTick, String)?
    var sender: String?
    var senderColor: Color
    var quote: (String?, Color, String)?

    private var shape: UnevenRoundedRectangle {
        let o: CGFloat = 18, t: CGFloat = 6
        let top = pos == .mid || pos == .last ? t : o
        let bot = pos == .mid || pos == .first ? t : o
        return mine
            ? UnevenRoundedRectangle(topLeadingRadius: o, bottomLeadingRadius: o, bottomTrailingRadius: bot, topTrailingRadius: top, style: .continuous)
            : UnevenRoundedRectangle(topLeadingRadius: top, bottomLeadingRadius: bot, bottomTrailingRadius: o, topTrailingRadius: o, style: .continuous)
    }

    private var meta: some View {
        HStack(spacing: 3) {
            Text(time).font(.custom("Figtree", size: 11, relativeTo: .caption2).monospacedDigit()).foregroundStyle(mine ? tin.grpBubMetaOut : tin.grpBubMetaIn)
            if let tick {
                // The tick keeps one slot (two ticks wide) whatever the state, so a status change never narrows the bubble.
                ReceiptTickView(tick: tick.0, size: 12).frame(width: 20, height: 12).accessibilityLabel(tick.1)
            }
        }
    }

    var body: some View {
        let ink = mine ? tin.onPrc : tin.ink
        let body = Text(text).font(.custom("Figtree", size: 15, relativeTo: .body)).foregroundStyle(ink)
        VStack(alignment: .leading, spacing: 0) {
            if let sender {
                Text(sender).font(.custom("Figtree", size: 13, relativeTo: .footnote).weight(.bold)).foregroundStyle(senderColor).lineLimit(1).padding(.bottom, 1)
            }
            if let quote { GroupsQuote(who: quote.0, color: quote.1, text: quote.2).padding(.top, 2).padding(.bottom, 6) }
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .lastTextBaseline, spacing: 10) { body; meta }
                VStack(alignment: .trailing, spacing: 2) { body.frame(maxWidth: .infinity, alignment: .leading); meta }
            }
        }
        .padding(.horizontal, 13).padding(.vertical, 8)
        .background(mine ? tin.prc : tin.bubIn, in: shape)
        .overlay { if !mine { shape.strokeBorder(tin.bubInLine, lineWidth: 1) } }
        .accessibilityElement(children: .combine)
    }
}

/// The quote of the message replied to (`.quote`): surface-2, 3 pt accent bar at the start, smooth 12 corners.
private struct GroupsQuote: View {
    @Environment(\.tin) private var tin
    let who: String?, color: Color, text: String
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let who { Text(who).font(.custom("Figtree", size: 12, relativeTo: .caption).weight(.bold)).foregroundStyle(color).lineLimit(1) }
            Text(text).font(.custom("Figtree", size: 13, relativeTo: .footnote)).foregroundStyle(tin.ink2).lineLimit(2)
        }
        .padding(.init(top: 5, leading: 13, bottom: 6, trailing: 10)).frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
        .background(tin.sf2).overlay(alignment: .leading) { Rectangle().fill(tin.grpAccent).frame(width: 3) }
        .clipShape(SmoothRoundedRectangle(cornerRadius: 12))
    }
}

/// Composer (Composer.svelte): [field] [send], 6 pt apart, bottom-aligned; the field is a smooth 22 pt pill on the surface with
/// a hairline that turns accent with a soft glow on focus; it grows with the text up to 140 pt. The send button is the accent
/// circle, dimmed to .38 while there is nothing to send. Groups have no attachments yet, so there is no attach button.
private struct GroupsComposer: View {
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    @FocusState private var focused: Bool
    @Binding var draft: String
    let placeholder: String
    let ctxTitle: String?
    let ctxText: String
    let onCancelCtx: () -> Void
    let onSend: () -> Void

    var body: some View {
        let empty = draft.groupsIsBlank
        let shape = SmoothRoundedRectangle(cornerRadius: 22)
        VStack(spacing: 0) {
            if let ctxTitle {
                HStack(spacing: 8) {
                    VStack(alignment: .leading, spacing: 0) {
                        Text(ctxTitle).font(.custom("Figtree", size: 12, relativeTo: .caption).weight(.bold)).foregroundStyle(tin.grpAccentInk).lineLimit(1)
                        Text(ctxText).font(.custom("Figtree", size: 13, relativeTo: .footnote)).foregroundStyle(tin.ink2).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                    Button(action: onCancelCtx) { GroupsIconView(icon: GroupsIcons.x, size: 18, color: tin.ink2).frame(width: 44, height: 44) }
                        .accessibilityLabel(tr("group_cancel_reply"))
                }
                .padding(.leading, 13).frame(minHeight: 44).background(tin.sf2, in: SmoothRoundedRectangle(cornerRadius: 12))
                .padding(.bottom, 6).padding(.trailing, 50)
            }
            HStack(alignment: .bottom, spacing: 6) {
                TextField(placeholder, text: $draft, axis: .vertical).lineLimit(1...6)
                    .font(.custom("Figtree", size: 15, relativeTo: .body)).foregroundStyle(tin.ink).tint(tin.grpAccentInk).focused($focused)
                    .padding(.horizontal, 16).padding(.vertical, 11).frame(minHeight: 44)
                    .background(tin.sf, in: shape)
                    .overlay { shape.stroke(focused ? tin.grpAccent : tin.grpHairline, lineWidth: 1) }
                    .background { if focused { shape.stroke(tin.grpAccentInk.opacity(0.14), lineWidth: 3) } }
                    .animation(reduced ? nil : GroupsMotion.easeOut(GroupsMotion.switchT), value: focused)
                // The same 48 pt slot whatever the state, so the composer row keeps its height on the first letter.
                Button(action: onSend) {
                    ZStack {
                        Circle().fill(tin.grpAccent).frame(width: 44, height: 44)
                        GroupsIconView(icon: GroupsIcons.send, size: 20, color: tin.grpOnAccent).padding(.trailing, 2)
                    }
                    .frame(width: 48, height: 48).opacity(empty ? 0.38 : 1).contentShape(Circle())
                }
                .buttonStyle(GroupsPressStyle()).disabled(empty).accessibilityLabel(tr("group_send"))
            }
        }
        .padding(.init(top: 8, leading: 10, bottom: 10, trailing: 10)).background(tin.bg)
    }
}
