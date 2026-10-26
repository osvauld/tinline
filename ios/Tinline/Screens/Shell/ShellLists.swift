import SwiftUI

/*
 * The home tabs' lists, from the playground: screens/home/ChatsList.svelte (72 pt rows), screens/calls/CallsList.svelte,
 * screens/people/ContactsList.svelte + PersonRow.svelte (64 pt, fixed). List padding 0 8; rows radius 14 (--r-ctl).
 * Ports of android HomeLists.kt, Chats.kt (New chat) and Home.kt (History).
 */

// ---------------------------------------------------------------- Chats

/// Chats tab: All / Unread chips, then 1:1 and group conversations, newest first. `top` goes above the chips (banners).
struct ShellChatsTabBody<Top: View, Empty: View>: View {
    let rows: [ShellChatRow]
    let drafts: [String: String]
    let onOpen: (ShellChatRow) -> Void
    @ViewBuilder var top: () -> Top
    /// The first screen's hero, shown while there are no chats at all.
    @ViewBuilder var empty: () -> Empty
    @State private var unreadOnly = false

    var body: some View {
        let shown = rows.filter { !unreadOnly || $0.unread > 0 }
        ScrollView {
            LazyVStack(spacing: 0) {
                top()
                // With no chats at all the filters mean nothing yet: the whole row goes, so the hero sits at the same height as on Calls.
                if !rows.isEmpty {
                    HStack(spacing: 8) {
                        ShellFilterChip(label: tr("shell_filter_all"), on: !unreadOnly) { unreadOnly = false }
                        ShellFilterChip(label: tr("shell_filter_unread"), on: unreadOnly) { unreadOnly = true }
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 16).padding(.top, 4).padding(.bottom, 8)
                }
                if rows.isEmpty { empty() } else if shown.isEmpty { ShellEmptyQuietLine(text: ShellEmpty.unread) }
                ForEach(shown) { r in ShellChatListRow(r: r, draft: drafts[r.did], onClick: { onOpen(r) }).padding(.horizontal, 8) }
            }
            .padding(.bottom, 8)
        }
    }
}

/// One conversation: avatar 48, name 15/600 + time 11 (accent-ink 700 when unread), preview 13 muted with the draft, Waiting, or
/// block state.
struct ShellChatListRow: View {
    let r: ShellChatRow
    let draft: String?
    var selected = false
    let onClick: () -> Void
    @Environment(\.tin) private var tin

    var body: some View {
        let p = ShellPalette(tin), hot = r.unread > 0
        let hasDraft = !(draft?.trimmingCharacters(in: .whitespaces).isEmpty ?? true)
        Button(action: onClick) {
            HStack(spacing: 12) {
                if r.group { TinGroupAvatar(groupID: r.did, size: 48) } else { ShellAvatar(did: r.did, name: r.name, size: 48) }
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 8) {
                        Text(r.name).shellStyle(.body, weight: .semibold).foregroundStyle(tin.ink).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                        Text(r.time).font(ShellFont.figtree(11, hot ? .bold : .regular, relativeTo: .caption2)).monospacedDigit()
                            .foregroundStyle(hot ? p.accentInk : tin.ink2).lineLimit(1).frame(width: 56, alignment: .trailing)
                    }
                    HStack(spacing: 8) {
                        HStack(spacing: 3) {
                            if r.tick == .clock && !hasDraft && !r.alert {
                                ShellIcon(glyph: .clock, size: 13, color: tin.threadText)
                            } else if r.markSlot && !hasDraft && !r.alert {
                                // My last message's tick: the conversation's glyph at 12 pt in a slot kept from the start (no shift).
                                ZStack {
                                    if let v = r.mark {
                                        ReceiptTickGlyph(key: r.markKey, vis: v, meta: tin.ink2, read: TickInk.read(tin), bad: TickInk.bad(tin),
                                                         bg: selected ? p.accentSoft : tin.bg,
                                                         label: r.group ? shellListTickLabel(v) : tickLabel(v, first: ShellText.firstName(r.name)), size: 12)
                                    }
                                }
                                .frame(width: tickSlotWidth(12), height: 12)
                            }
                            preview(p: p, hasDraft: hasDraft).shellStyle(.sub).foregroundStyle(tin.ink2).lineLimit(1)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        ZStack(alignment: .trailing) { if hot { ShellCountBadge(n: r.unread) } }.frame(width: 32, height: 18, alignment: .trailing)
                    }
                }
            }
            .padding(8).frame(minHeight: 72)
        }
        .buttonStyle(ShellPressStyle(rest: selected ? p.accentSoft : .clear))
    }

    private func preview(p: ShellPalette, hasDraft: Bool) -> Text {
        if r.alert { return Text(r.preview).foregroundColor(p.danger).fontWeight(.semibold) }
        if hasDraft, let d = draft { return Text(tr("shell_chats_draft")).foregroundColor(p.accentInk).fontWeight(.semibold) + Text(" " + d.replacingOccurrences(of: "\n", with: " ")) }
        if r.tick == .clock { return Text(tr("shell_chats_waiting")).foregroundColor(tin.threadText).fontWeight(.semibold) + Text(" \u{00B7} " + r.preview) }
        return Text(r.preview)
    }
}

/// A group row's mark in words (screen reader).
func shellListTickLabel(_ v: TickVis) -> String {
    switch v {
    case .slack: return tr("shell_tick_slack")
    case .one: return tr("shell_tick_one")
    case .two: return tr("shell_tick_two")
    case .read: return tr("shell_tick_read")
    case .bad: return tr("shell_tick_bad")
    }
}

// ---------------------------------------------------------------- Calls

private enum ShellCallEntry: Identifiable {
    case one(CallRecord)
    case group(GroupCallRecord, String)
    var at: UInt64 { switch self { case .one(let r): r.startedAt; case .group(let r, _): r.startedAt } }
    var id: String { switch self { case .one(let r): "\(r.callId):\(r.startedAt)"; case .group(let r, _): "g:\(r.groupId):\(r.callId)" } }
}

/// Calls tab: every call record, newest first, with the group calls of groups I am still in (a group I left has no row). The
/// avatar opens their profile (a group's: its info), the row calls them back (decided 2026-10-14: this is the Calls tab; the chat
/// lives in Chats). Someone I can't call: their profile, which says why.
struct ShellCallsTabBody<Top: View>: View {
    let history: [CallRecord]
    let groupLog: [GroupCallRecord]
    let groups: [GroupSummary]
    let contacts: [Contact]
    let canCall: (String) -> Bool
    let onProfile: (String) -> Void
    let onCall: (String) -> Void
    let onGroupInfo: (String) -> Void
    let onGroupCall: (String) -> Void
    @ViewBuilder var top: () -> Top
    /// The first screen's hero while nobody is added yet; otherwise the quiet line.
    var empty: AnyView?
    @Environment(\.tin) private var tin

    var body: some View {
        let names = Dictionary(groups.filter { ShellRows.groupActive($0.myRole) }.map { ($0.id, $0.name) }, uniquingKeysWith: { a, _ in a })
        let entries = (history.map { ShellCallEntry.one($0) } + groupLog.compactMap { r in names[r.groupId].map { ShellCallEntry.group(r, $0) } })
            .sorted { $0.at > $1.at }
        let now = ShellText.nowMs()
        ScrollView {
            LazyVStack(spacing: 0) {
                top()
                if entries.isEmpty { if let empty { empty } else { ShellEmptyQuietLine(text: ShellEmpty.calls) } }
                ForEach(entries) { e in
                    switch e {
                    case .one(let r): oneRow(r, now: now)
                    case .group(let r, let name): groupRow(r, name: name, now: now)
                    }
                }
            }
            .padding(.bottom, 8)
        }
    }

    private func nameOf(_ r: CallRecord) -> String {
        contacts.first { $0.did == r.peerDid }.map(ShellText.display) ?? ShellText.ifBlank(r.peerName, tr("shell_unknown"))
    }

    @ViewBuilder private func oneRow(_ r: CallRecord, now: Int64) -> some View {
        let p = ShellPalette(tin)
        let name = nameOf(r), callable = canCall(r.peerDid), what = ShellText.callWhat(r)
        let stamp = ShellText.listTime(Int64(r.startedAt) * 1000, now)
        let dur = r.durationSecs > 0 ? clock(secs: Int64(r.durationSecs)) : nil
        let fg = r.missed ? p.danger : tin.ink2
        HStack(spacing: 2) {
            Button { onProfile(r.peerDid) } label: { ShellAvatar(did: r.peerDid, name: name, size: 48).padding(.leading, 8).padding(.vertical, 8) }
                .buttonStyle(ShellPressStyle()).accessibilityLabel(tr("shell_calls_profile_a11y", name))
            Button { if callable { onCall(r.peerDid) } else { onProfile(r.peerDid) } } label: {
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(name).shellStyle(.body, weight: .semibold).foregroundStyle(tin.ink).lineLimit(1)
                        HStack(spacing: 5) {
                            ShellIcon(glyph: r.incoming ? .callIn : .callOut, size: 15, color: fg)
                            Text(dur.map { tr("shell_calls_what_duration", what, $0) } ?? what).shellStyle(.sub).monospacedDigit().foregroundStyle(fg).lineLimit(1)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    Text(stamp).shellStyle(.meta).monospacedDigit().foregroundStyle(tin.ink2)
                    ShellIcon(glyph: .phone, size: 22, color: callable ? p.accentInk : tin.ink2).opacity(callable ? 1 : 0.45).frame(width: 44, height: 44)
                }
                .padding(.leading, 12).padding(.vertical, 8).frame(minHeight: 68)
            }
            .buttonStyle(ShellPressStyle())
            .accessibilityLabel(dur.map { tr("shell_calls_row_a11y_duration", name, ShellText.callDesc(r), $0, stamp, callable ? tr("shell_calls_action_call") : tr("shell_calls_action_cant")) } ?? tr("shell_calls_row_a11y", name, ShellText.callDesc(r), stamp, callable ? tr("shell_calls_action_call") : tr("shell_calls_action_cant")))
        }
        .padding(.horizontal, 8)
    }

    /// A group call: group avatar (opens its info), name, people icon + "Group call · 12 min · 3 people" (calm), time, phone.
    @ViewBuilder private func groupRow(_ r: GroupCallRecord, name: String, now: Int64) -> some View {
        let p = ShellPalette(tin)
        let what = groupCallLabel(r: r)
        let stamp = ShellText.listTime(Int64(r.startedAt) * 1000, now)
        HStack(spacing: 2) {
            Button { onGroupInfo(r.groupId) } label: { TinGroupAvatar(groupID: r.groupId, size: 48).padding(.leading, 8).padding(.vertical, 8) }
                .buttonStyle(ShellPressStyle()).accessibilityLabel(tr("shell_calls_group_info_a11y", name))
            Button { onGroupCall(r.groupId) } label: {
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(name).shellStyle(.body, weight: .semibold).foregroundStyle(tin.ink).lineLimit(1)
                        HStack(spacing: 5) {
                            ShellIcon(glyph: .users, size: 15, color: tin.ink2)
                            Text(what).shellStyle(.sub).monospacedDigit().foregroundStyle(tin.ink2).lineLimit(1)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    Text(stamp).shellStyle(.meta).monospacedDigit().foregroundStyle(tin.ink2)
                    ShellIcon(glyph: .phone, size: 22, color: p.accentInk).frame(width: 44, height: 44)
                }
                .padding(.leading, 12).padding(.vertical, 8).frame(minHeight: 68)
            }
            .buttonStyle(ShellPressStyle()).accessibilityLabel(tr("shell_calls_group_row_a11y", name, what, stamp))
        }
        .padding(.horizontal, 8)
    }
}

// ---------------------------------------------------------------- Contacts

/// PersonRow: 64 pt fixed; avatar 44, name 600 (21), sub 13/18 kept even when empty, so a changing subtitle never moves the list.
struct ShellPersonRow: View {
    let name: String
    let did: String
    let sub: String
    var dim = false
    let onClick: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        Button(action: onClick) {
            HStack(spacing: 12) {
                ShellAvatar(did: did, name: name, size: 44, alpha: dim ? 0.6 : 1)
                VStack(alignment: .leading, spacing: 1) {
                    Text(name).shellStyle(.body, weight: .semibold).foregroundStyle(tin.ink).lineLimit(1).opacity(dim ? 0.6 : 1)
                    Text(sub).shellStyle(.sub).foregroundStyle(tin.ink2).lineLimit(1).frame(minHeight: 18)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.horizontal, 8).frame(minHeight: 64)
        }
        .buttonStyle(ShellPressStyle())
    }
}

/// Contacts tab: a find field, "Add contact", requests waiting for me and mine waiting for them, my contacts A to Z under letters,
/// then people I blocked, then the count. No "declined" anywhere: an unanswered request waits.
struct ShellContactsTabBody<Top: View>: View {
    let contacts: [Contact]
    let blocked: (String) -> Bool
    let sub: (String) -> String
    let onOpen: (String) -> Void
    let onAdd: () -> Void
    @ViewBuilder var top: () -> Top
    var waitingIn: [ContactRequest] = []
    var waitingOut: [ContactRequest] = []
    var onAccept: (String) -> Void = { _ in }
    var onNotNow: (String) -> Void = { _ in }
    @State private var q = ""
    @FocusState private var focused: Bool
    @Environment(\.tin) private var tin

    private func match(_ c: Contact) -> Bool {
        let t = q.trimmingCharacters(in: .whitespaces)
        return t.isEmpty || ShellText.display(c).localizedCaseInsensitiveContains(t) || c.name.localizedCaseInsensitiveContains(t)
    }

    var body: some View {
        let p = ShellPalette(tin)
        let sorted = contacts.sorted { ShellText.display($0).lowercased() < ShellText.display($1).lowercased() }
        let mine = sorted.filter { !blocked($0.did) && match($0) }
        let gone = sorted.filter { blocked($0.did) && match($0) }
        let total = contacts.filter { !blocked($0.did) }.count
        let letters = Dictionary(grouping: mine) { String(ShellText.display($0).first.map { String($0).uppercased() } ?? "#") }
        let qBlank = q.trimmingCharacters(in: .whitespaces).isEmpty
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                top()
                HStack(spacing: 8) {
                    ShellIcon(glyph: .search, size: 18, color: tin.ink2)
                    TextField("", text: $q, prompt: Text(tr("shell_contacts_find")).foregroundColor(tin.ink2))
                        .font(ShellFont.of(.body)).foregroundStyle(tin.ink).tint(p.accentInk).focused($focused)
                        .autocorrectionDisabled(true).accessibilityLabel(tr("shell_contacts_find"))
                }
                .padding(.horizontal, 14).frame(height: 44)
                .background(Capsule().fill(tin.sf2)).overlay(Capsule().stroke(focused ? p.accent : .clear, lineWidth: 2))
                .padding(.horizontal, 16).padding(.top, 4).padding(.bottom, 8)
                Button(action: onAdd) {
                    HStack(spacing: 12) {
                        ShellIcon(glyph: .userPlus, size: 22, color: p.accentInk).frame(width: 44, height: 44).background(Circle().fill(p.accentSoft))
                        VStack(alignment: .leading, spacing: 1) {
                            Text(tr("shell_contacts_add")).shellStyle(.body, weight: .semibold).foregroundStyle(tin.ink)
                            Text(tr("shell_contacts_add_sub")).shellStyle(.sub).foregroundStyle(tin.ink2)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .padding(.horizontal, 8).frame(minHeight: 64)
                }
                .buttonStyle(ShellPressStyle()).padding(.horizontal, 8)
                if !waitingIn.isEmpty {
                    section(tr("shell_contacts_waiting_you"))
                    ForEach(waitingIn, id: \.did) { r in
                        VStack(alignment: .leading, spacing: 10) {
                            HStack(spacing: 12) {
                                ShellAvatar(did: r.did, name: ShellText.ifBlank(r.name, "?"), size: 44)
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(r.name).shellStyle(.body, weight: .semibold).foregroundStyle(tin.ink).lineLimit(1)
                                    Text(tr("shell_contacts_wants_add")).shellStyle(.sub).foregroundStyle(tin.ink2)
                                }
                            }
                            HStack(spacing: 8) {
                                ShellPBtn(text: tr("shell_contacts_accept"), variant: .primary) { onAccept(r.did) }
                                ShellPBtn(text: tr("shell_not_now"), variant: .quiet) { onNotNow(r.did) }
                            }
                            Text(tr("shell_contacts_accept_hint")).shellStyle(.sub19).foregroundStyle(tin.ink2)
                        }
                        .padding(14).frame(maxWidth: .infinity, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: 14).fill(tin.sf))
                        .overlay(RoundedRectangle(cornerRadius: 14).stroke(p.hairline, lineWidth: 1))
                        .padding(.horizontal, 8).padding(.vertical, 6)
                    }
                }
                if !waitingOut.isEmpty {
                    section(tr("shell_contacts_waiting_them"))
                    ForEach(waitingOut, id: \.did) { x in
                        HStack(spacing: 12) {
                            ShellAvatar(did: x.did, name: ShellText.ifBlank(x.name, "?"), size: 40)
                            VStack(alignment: .leading, spacing: 0) {
                                Text(x.name).shellStyle(.body, weight: .semibold).foregroundStyle(tin.ink).lineLimit(1)
                                Text(tr("shell_contacts_request_sent", ShellText.firstName(x.name))).shellStyle(.sub).foregroundStyle(tin.ink2)
                            }
                        }
                        .padding(8).frame(minHeight: 64).padding(.horizontal, 8)
                    }
                }
                ForEach(letters.keys.sorted(), id: \.self) { letter in
                    section(letter)
                    ForEach(letters[letter] ?? [], id: \.did) { ct in
                        ShellPersonRow(name: ShellText.display(ct), did: ct.did, sub: sub(ct.did)) { onOpen(ct.did) }.padding(.horizontal, 8)
                    }
                }
                if !gone.isEmpty {
                    section(tr("shell_blocked"))
                    ForEach(gone, id: \.did) { ct in
                        ShellPersonRow(name: ShellText.display(ct), did: ct.did, sub: tr("shell_blocked"), dim: true) { onOpen(ct.did) }.padding(.horizontal, 8)
                    }
                }
                if total == 0 && gone.isEmpty && waitingOut.isEmpty && qBlank {
                    ShellEmptyQuietLine(text: ShellEmpty.contacts)
                } else if !(total == 0 && qBlank) {
                    let t = !qBlank && mine.isEmpty && gone.isEmpty ? tr("shell_contacts_none_match", q.trimmingCharacters(in: .whitespaces))
                        : trn("shell_contacts_count", total)
                    Text(t).shellStyle(.sub19).foregroundStyle(tin.ink2).multilineTextAlignment(.center)
                        .frame(maxWidth: .infinity, minHeight: 19).padding(.horizontal, 16).padding(.top, 14)
                }
            }
            .padding(.bottom, 24)
        }
    }

    private func section(_ t: String) -> some View {
        ShellListSection(t).padding(.horizontal, 16).padding(.top, 18).padding(.bottom, 6)
    }
}

// ---------------------------------------------------------------- New chat (Chats.kt)

/// "New chat": pick a contact, or add one.
struct ShellNewChatScreen: View {
    let onBack: () -> Void
    let onPick: (String) -> Void
    let onAdd: () -> Void
    @EnvironmentObject private var app: AppEnv
    @EnvironmentObject private var people: PeopleModel
    @Environment(\.tin) private var tin
    @State private var query = ""
    @State private var searching = false

    var body: some View {
        let now = ShellText.nowMs()
        let last = Dictionary(app.chats.filter { $0.lastActivity > 0 }.map { ($0.peerDid, tr("shell_new_chat_last_message", ShellText.listTime(Int64($0.lastActivity), now))) },
                              uniquingKeysWith: { a, _ in a })
        let sorted = people.contacts.sorted { ShellText.display($0).lowercased() < ShellText.display($1).lowercased() }
        let t = query.trimmingCharacters(in: .whitespaces)
        let shown = sorted.filter { t.isEmpty || ShellText.display($0).localizedCaseInsensitiveContains(t) || $0.name.localizedCaseInsensitiveContains(t) }
        ShellPage {
            if searching {
                ShellSearchBar(query: $query, hint: tr("shell_new_chat_search_contacts"), onClose: { searching = false; query = "" })
            } else {
                ShellTopBar(tr("shell_home_new_chat"), onBack: onBack)
            }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if !searching {
                        Button { searching = true } label: {
                            HStack(spacing: 12) {
                                ShellIcon(glyph: .search, size: 24, color: tin.ink2)
                                Text(tr("shell_new_chat_search_contacts")).font(TinType.bodyL).foregroundStyle(tin.ink2)
                                Spacer(minLength: 0)
                            }
                            .padding(.horizontal, 16).frame(minHeight: 52).background(Capsule().fill(tin.sf2))
                        }
                        .buttonStyle(ShellPressStyle(wash: .clear, radius: 26)).padding(.horizontal, 16).padding(.vertical, 4)
                        Button(action: onAdd) {
                            HStack(spacing: 14) {
                                ShellIcon(glyph: .userPlus, size: 24, color: tin.onPr).frame(width: 48, height: 48).background(Circle().fill(tin.pr))
                                VStack(alignment: .leading, spacing: 0) {
                                    Text(tr("shell_new_chat_add")).font(TinType.bodyL.weight(.semibold)).foregroundStyle(tin.ink)
                                    Text(tr("shell_new_chat_add_sub")).font(TinType.bodyM).foregroundStyle(tin.ink2)
                                }
                                Spacer(minLength: 0)
                            }
                            .padding(.horizontal, 20).frame(minHeight: 68)
                        }
                        .buttonStyle(ShellPressStyle(radius: 0))
                    }
                    if !people.contacts.isEmpty { ShellSectionLabel(tr("shell_new_chat_contacts_header", people.contacts.count)) }
                    ForEach(shown, id: \.did) { ct in
                        Button { onPick(ct.did) } label: {
                            HStack(spacing: 14) {
                                ShellAvatar(did: ct.did, name: ShellText.display(ct), size: 48)
                                VStack(alignment: .leading, spacing: 0) {
                                    Text(ShellText.display(ct)).font(TinType.bodyL.weight(.semibold)).foregroundStyle(tin.ink).lineLimit(1)
                                    Text(last[ct.did] ?? tr("shell_new_chat_no_messages")).font(TinType.bodyM).foregroundStyle(tin.ink2)
                                }
                                Spacer(minLength: 0)
                            }
                            .padding(.horizontal, 20).frame(minHeight: 68)
                        }
                        .buttonStyle(ShellPressStyle(radius: 0))
                    }
                }
                .padding(.bottom, 24)
            }
        }
    }
}

/// Home.kt SearchBar: back, the 18 pt field (focused on open), clear; a line below.
struct ShellSearchBar: View {
    @Binding var query: String
    var hint = tr("shell_new_chat_search_contacts")
    let onClose: () -> Void
    @FocusState private var focused: Bool
    @Environment(\.tin) private var tin
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                ShellIconButton(glyph: .back, label: tr("shell_close_search"), size: 48, iconSize: 24, action: onClose)
                TextField("", text: $query, prompt: Text(hint).foregroundColor(tin.ink2))
                    .font(ShellFont.figtree(18)).foregroundStyle(tin.ink).tint(tin.pr).focused($focused).submitLabel(.search)
                    .frame(maxWidth: .infinity, minHeight: 48)
                if !query.isEmpty { ShellIconButton(glyph: .x, label: tr("shell_clear"), size: 48, iconSize: 24) { query = "" } }
            }
            .frame(minHeight: 64).padding(.leading, 4).padding(.trailing, 8)
            tin.ln.frame(height: 1)
        }
        .onAppear { focused = true }
    }
}

// ---------------------------------------------------------------- History (Home.kt)

/// The full call log (Home "See all").
struct ShellHistoryScreen: View {
    let onBack: () -> Void
    let onContact: (String) -> Void
    @EnvironmentObject private var app: AppEnv
    @EnvironmentObject private var people: PeopleModel
    @Environment(\.tin) private var tin
    var body: some View {
        let now = ShellText.nowSecs()
        ShellPage {
            ShellTopBar(tr("shell_calls_recent"), onBack: onBack)
            if app.history.isEmpty {
                ShellHint(tr("shell_calls_none")).padding(24)
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(app.history, id: \.callId) { r in
                            let name = people.contacts.first { $0.did == r.peerDid }.map(ShellText.display) ?? ShellText.ifBlank(r.peerName, tr("shell_unknown"))
                            let red = r.missed
                            Button { if people.contacts.contains(where: { $0.did == r.peerDid }) { onContact(r.peerDid) } } label: {
                                HStack(spacing: 14) {
                                    ShellAvatar(did: r.peerDid, name: name, size: 44)
                                    VStack(alignment: .leading, spacing: 0) {
                                        Text(name).font(TinType.bodyL.weight(.semibold)).foregroundStyle(red ? tin.er : tin.ink).lineLimit(1)
                                        HStack(spacing: 6) {
                                            ShellIcon(glyph: r.incoming ? .callIn : .callOut, size: 16, color: red ? tin.er : tin.ink2)
                                            Text(whenLong(then: Int64(r.startedAt), now: now, utcOffsetSecs: Int32(TimeZone.current.secondsFromGMT())))
                                                .font(TinType.bodyM).foregroundStyle(red ? tin.er : tin.ink2)
                                        }
                                    }
                                    Spacer(minLength: 12)
                                }
                                .padding(.leading, 20).padding(.trailing, 8).frame(minHeight: 72)
                            }
                            .buttonStyle(ShellPressStyle(radius: 0))
                        }
                        ShellHint(tr("shell_calls_history_note")).padding(.horizontal, 20).padding(.vertical, 12)
                    }
                    .padding(.bottom, 24)
                }
            }
        }
    }
}
