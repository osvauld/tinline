import Combine
import SwiftUI

/*
 * The shell from the playground (screens/shell/AndroidShell.svelte; android Shell.kt + Home.kt): the 56 pt app bar with the
 * live mark, the tab panel's quiet crossfade, the search dock above the tab bar, and the 4-tab bar whose indicator grows out
 * from the icon. Values are measured from the playground and named after tokens.css / motion.ts.
 */

/// A person's avatar with their profile picture when the People area has one ready (android `Avatar` -> PgAvatar).
struct ShellAvatar: View {
    let did: String
    var name: String = ""
    let size: CGFloat
    var alpha: Double = 1
    @EnvironmentObject private var people: PeopleModel
    var body: some View {
        let v = people.profiles.views[did]
        TinAvatar(did: did, size: size, name: name, image: v?.image.map { Image(uiImage: $0) }, awaiting: v?.awaiting ?? false, alpha: alpha)
            .onAppear { people.profiles.watch(did) }
    }
}

// ---------------------------------------------------------------- Home

struct ShellHomeScreen: View {
    let core: ShellCore
    let missing: [ShellNeed]
    let fix: (ShellNeed) -> Void
    let ring: ShellRing?
    let callError: String?
    let call: (String) -> Void
    @EnvironmentObject private var app: AppEnv
    @EnvironmentObject private var nav: ShellNav
    @EnvironmentObject private var people: PeopleModel
    @EnvironmentObject private var calls: CallsEnv
    @EnvironmentObject private var toast: ShellToast
    @Environment(\.tin) private var tin
    @State private var grace = true
    @State private var nowSecs = ShellText.nowSecs()
    @State private var callsSeen: Int64 = ShellPrefs().callsSeen
    @State private var marks: [ListMark] = []
    @State private var groups: [GroupSummary] = []
    @State private var groupBadge = 0
    @State private var groupChanges = 0
    @State private var drafts: [String: String] = [:]
    /// "Not now" on a Contacts card hides it for now; the request stays in the core (and comes back next time).
    @State private var hiddenRequests: Set<String> = []

    private var online: Bool { app.status?.online == true }
    private var myDid: String? { app.accountDid }

    private var rows: [ShellChatRow] {
        let all = ShellRows.withMarks(
            ShellRows.chatRows(app.chats, people.contacts, links: ShellWiring.chatLinks(app: app), online: online, nowMs: nowSecs * 1000,
                               myDid: myDid, blocks: people.blocks)
            + ShellRows.groupChatRows(groups, me: myDid ?? "", nowMs: nowSecs * 1000), marks, myDid: myDid)
        return ShellRows.sorted(all, you: myDid)
    }

    private var callPhase: MarkCallPhase {
        guard let u = calls.controller.ui else { return .none }
        switch u.state {
        case .dialing: return .outgoing
        case .ringing: return u.info.incoming ? .incoming : .outgoing
        case .active: return u.stats?.reconnecting == true ? .reconnecting : .active
        case .ended: return .none
        }
    }

    var body: some View {
        let rows = self.rows
        let waitingIn = people.requestsIn.filter { !hiddenRequests.contains($0.did) }
        let tab = nav.tab
        let dock = tab == .chats || tab == .groups
        let look = statusLook(call: callPhase, conn: online ? .online : grace ? .connecting : .offline)
        VStack(spacing: 0) {
            ShellHomeAppBar(look: look, showSearch: !dock, ring: ring, onSearch: { nav.push(.search) }, onSettings: { nav.push(.settings) })
            ShellTabPanel(tab: tab) { t in
                VStack(spacing: 0) {
                    ShellTabTitle(text: t.title)
                    switch t {
                    case .chats:
                        let shown = rows.contains { $0.group || ($0.did != myDid && $0.atMs > 0) } ? rows : []
                        ShellChatsTabBody(rows: shown, drafts: drafts, onOpen: { r in
                            if r.group { nav.push(group: ShellWiring.groupRoute(r.did)) } else { nav.push(.chat(r.did, nil)) }
                        }, top: { banner }, empty: { hero })
                    case .groups:
                        ShellWiring.groupsTab(nav: nav)
                    case .calls:
                        ShellCallsTabBody(history: app.history, groupLog: app.groupLog, groups: groups, contacts: people.contacts,
                                          canCall: { did in people.contacts.contains { $0.did == did } && people.blocks.callGate(did, contact: true) == .ok },
                                          onProfile: { did in if people.contacts.contains(where: { $0.did == did }) { nav.push(.contact(did)) } },
                                          onCall: call,
                                          onGroupInfo: { gid in nav.push(group: .info(gid)) },
                                          onGroupCall: { gid in ShellWiring.groupCall(calls, gid: gid, say: toast.say) },
                                          top: {
                                              VStack(spacing: 0) {
                                                  banner
                                                  if let callError { Text(callError).shellStyle(.sub).foregroundStyle(ShellPalette(tin).danger).padding(.horizontal, 16).padding(.vertical, 8) }
                                              }
                                          },
                                          empty: people.contacts.isEmpty ? AnyView(hero) : nil)
                    case .contacts:
                        ShellContactsTabBody(contacts: people.contacts, blocked: { people.blocks.blockedByMe($0) },
                                             sub: { did in people.blocks.blockedByMe(did) ? "Blocked" : (people.blocks.status(did) ?? "") },
                                             onOpen: { nav.push(.contact($0)) }, onAdd: { nav.openAdd(ShellAddArgs()) }, top: { banner },
                                             waitingIn: waitingIn, waitingOut: people.requestsOut,
                                             onAccept: { did in Task { _ = await people.acceptRequest(did) } }, onNotNow: { hiddenRequests.insert($0) })
                    }
                }
            }
            .frame(maxHeight: .infinity)
            if dock {
                ShellSearchDock(addLabel: tab == .groups ? "New group" : "New chat", onSearch: { nav.push(.search) },
                                onAdd: { if tab == .groups { nav.push(group: ShellWiring.newGroupRoute) } else { nav.push(.newChat) } })
            }
            ShellTabBar(tab: tab, onTab: { nav.tab = $0 }, chatBadge: rows.reduce(0) { $0 + $1.unread }, groupBadge: groupBadge,
                        contactBadge: waitingIn.count, callBadge: 0)
        }
        .background(tin.bg.ignoresSafeArea())
        // While this bar is on screen it carries a ringing call, so the call controller keeps the full-screen alert away. Leaving
        // Home mid-ring gives the call its alert, so it is never left without a way to answer it.
        .onAppear { core.callDirectory.ringHeadUp = true }
        .onDisappear { core.callDirectory.ringHeadUp = false; calls.controller.appLeftFront() }
        .task { try? await Task.sleep(nanoseconds: 6_000_000_000); grace = false }
        // Relative times ("4 min ago") go stale: refresh them once a minute.
        .task { while !Task.isCancelled { try? await Task.sleep(nanoseconds: 60_000_000_000); nowSecs = ShellText.nowSecs() } }
        .task(id: app.node.map(ObjectIdentifier.init)) { reloadGroups() }
        .onReceive(ShellWiring.groupsChanged(node: app.node).debounce(for: .milliseconds(100), scheduler: DispatchQueue.main)) { _ in reloadGroups() }
        .onReceive(ShellWiring.draftPreviews.receive(on: DispatchQueue.main)) { drafts = $0 }
        // My last message's tick per conversation (core `chat_list_marks`), re-read when chats, groups or receipts change.
        .task(id: "\(app.chats.count)-\(app.chats.first?.lastActivity ?? 0)-\(groupChanges)") { await reloadMarks() }
        // The red count on Calls is missed calls since that tab was last open.
        .onChange(of: nav.tab, initial: true) { _, t in if t == .calls { markCallsSeen() } }
        .onChange(of: app.history) { _, _ in if nav.tab == .calls { markCallsSeen() } }
        .environment(\.shellCallBadge, app.history.filter { $0.missed && Int64($0.startedAt) > callsSeen }.count)
    }

    private func markCallsSeen() { callsSeen = ShellText.nowSecs(); core.prefs.callsSeen = callsSeen }

    private func reloadGroups() {
        groups = ShellWiring.groupSummaries(node: app.node)
        groupBadge = ShellWiring.pendingGroupInvitations(node: app.node)
        groupChanges += 1
    }

    private func reloadMarks() async {
        guard let node = app.node else { return }
        if let got = try? await shellIO({ try node.chatListMarks() }) { marks = got }
    }

    /// Banner priority: offline, notifications, then availability. Only one shows. (iOS has no battery or full-screen switch.)
    @ViewBuilder private var banner: some View {
        if !online && !grace {
            ShellBanner(kind: .error, glyph: .off, title: "You\u{2019}re offline", text: "Your calls come through again once you\u{2019}re back online.")
        } else if missing.contains(.notifications) {
            ShellBanner(kind: .warn, glyph: .mute, title: "Calls won\u{2019}t ring", text: "Notifications are off for Tinline.", action: "Turn on") { fix(.notifications) }
        } else if !app.availability.available {
            ShellBanner(kind: .neutral, glyph: .callEnd, title: "You\u{2019}re not available", text: "Calls won\u{2019}t ring until you turn it back on.", action: "Turn on") {
                core.setAvailable(true)
            }
        }
    }

    private var hero: some View {
        ShellEmptyHero(contacts: people.contacts, sent: people.requestsOut, onAdd: { nav.openAdd(ShellAddArgs()) },
                       onMessage: { nav.push(.chat($0, nil)) }, onNewChat: { nav.push(.newChat) })
    }
}

/// The missed-call count since the Calls tab was last open; set by Home, read by the tab bar.
private struct ShellCallBadgeKey: EnvironmentKey { static let defaultValue = 0 }
extension EnvironmentValues {
    var shellCallBadge: Int { get { self[ShellCallBadgeKey.self] } set { self[ShellCallBadgeKey.self] = newValue } }
}

// ---------------------------------------------------------------- app bar

/// `.appbar`: min 56, padding 4 8 0 12; brand = live mark 28 + "Tinline" 19 bold, gap 8; 44 icon buttons. A ringing call
/// replaces it in place (RingHead.svelte), crossfading 120 ms; the last ring stays drawn while it fades out.
struct ShellHomeAppBar: View {
    let look: MarkLook
    let showSearch: Bool
    let ring: ShellRing?
    let onSearch: () -> Void
    let onSettings: () -> Void
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    @State private var last: ShellRing?

    var body: some View {
        let ringing = ring != nil
        ZStack {
            HStack(spacing: 0) {
                HStack(spacing: 8) {
                    TinMark(size: 28, state: look.state, live: true, sway: look.sway, dim: look.dim, description: look.label)
                    Text("Tinline").font(ShellFont.figtree(19, .bold, relativeTo: .title3)).foregroundStyle(tin.ink)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                if showSearch { ShellIconButton(glyph: .search, label: "Search", enabled: !ringing, action: onSearch) }
                ShellIconButton(glyph: .gear, label: "Settings", enabled: !ringing, action: onSettings)
            }
            .opacity(ringing ? 0 : 1)
            if let r = ring ?? last {
                ShellRingHeadRow(r: r, avatar: 28).opacity(ringing ? 1 : 0).allowsHitTesting(ringing)
            }
        }
        .padding(.leading, 12).padding(.trailing, 8).padding(.top, 4).frame(minHeight: 56)
        .animation(reduced ? nil : .linear(duration: TinMotion.crossfade), value: ringing)
        .onChange(of: ring?.did) { _, _ in if let ring { last = ring } }
    }
}

/// The caller in the header box: avatar, name over "Incoming call", Decline and Answer in the 44 pt icon slots.
struct ShellRingHeadRow: View {
    let r: ShellRing
    let avatar: CGFloat
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin)
        HStack(spacing: 0) {
            Button(action: r.onOpen) {
                HStack(spacing: 8) {
                    ShellAvatar(did: r.did, name: r.name, size: avatar)
                    VStack(alignment: .leading, spacing: 0) {
                        Text(r.name).font(ShellFont.figtree(15, .semibold)).foregroundStyle(tin.ink).lineLimit(1)
                        Text(r.sub).font(ShellFont.figtree(12)).foregroundStyle(tin.ink2).lineLimit(1)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: .infinity)
            }
            .buttonStyle(ShellPressStyle()).accessibilityLabel(r.waiting ? "\(r.name) is calling during your call" : "\(r.sub) from \(r.name)")
            .accessibilityHint("Open call from \(r.name)")
            ShellRingButton(label: "Decline", bg: p.dangerBtn, fg: .white, hang: true, action: r.onDecline)
            ShellRingButton(label: r.waiting ? "End and answer" : "Answer", bg: p.accent, fg: p.onAccent, hang: false, action: r.onAnswer)
        }
        .background(tin.bg)
    }
}

private struct ShellRingButton: View {
    let label: String
    let bg: Color
    let fg: Color
    let hang: Bool
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            ShellIcon(glyph: .phone, size: 22, color: fg).rotationEffect(.degrees(hang ? 135 : 0))
                .frame(width: 38, height: 38).background(Circle().fill(bg)).frame(width: 44, height: 44)
        }
        .buttonStyle(ShellPressStyle(wash: .clear, radius: 22)).accessibilityLabel(label)
    }
}

// ---------------------------------------------------------------- tab panel

/// Chosen in the main-tab lab: simultaneous 120 ms fades, no delay, scale, travel or size spring. Header, dock and tab bar stay
/// outside the transition; each list keeps its own scroll position (each tab's view stays alive).
struct ShellTabPanel<Content: View>: View {
    let tab: ShellHomeTab
    @ViewBuilder var content: (ShellHomeTab) -> Content
    @Environment(\.accessibilityReduceMotion) private var reduced
    var body: some View {
        ZStack(alignment: .top) {
            ForEach(ShellHomeTab.allCases, id: \.self) { t in
                content(t).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                    .opacity(t == tab ? 1 : 0).allowsHitTesting(t == tab).accessibilityHidden(t != tab)
            }
        }
        .animation(reduced ? nil : .tinCubicOut(TinMotion.crossfade), value: tab)
    }
}

/// `.title`: 28/34 bold, margin 4 16 8.
struct ShellTabTitle: View {
    let text: String
    @Environment(\.tin) private var tin
    var body: some View {
        Text(text).font(TinType.h1).foregroundStyle(tin.ink).frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16).padding(.top, 4).padding(.bottom, 8).accessibilityAddTraits(.isHeader)
    }
}

// ---------------------------------------------------------------- search dock

/// SearchDock: the search pill + the new button in one row, above the tab bar (thumb reach). Pill 48 (radius 24), button 48.
struct ShellSearchDock: View {
    let addLabel: String
    let onSearch: () -> Void
    let onAdd: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin)
        HStack(spacing: 10) {
            Button(action: onSearch) {
                HStack(spacing: 10) {
                    ShellIcon(glyph: .search, size: 20, color: tin.ink2)
                    Text("Search").shellStyle(.body).foregroundStyle(tin.ink2)
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 16).frame(height: 48)
            }
            .buttonStyle(ShellPressStyle(wash: p.hairline, rest: tin.sf2, radius: 24)).accessibilityLabel("Search")
            Button(action: onAdd) { ShellIcon(glyph: .plus, size: 24, color: p.accentInk).frame(width: 48, height: 48) }
                .buttonStyle(ShellPressStyle(wash: p.accentSoft, rest: p.accentSoft, radius: 24)).accessibilityLabel(addLabel)
        }
        .padding(.horizontal, 12).padding(.top, 8).padding(.bottom, 10).background(tin.bg)
    }
}

// ---------------------------------------------------------------- tab bar

/// `.bar`: 4 equal tabs on the surface, a hairline above. Tab: 56 x 30 pill (radius 15) over a 12 pt 600 label. The active
/// indicator grows out from the icon's centre (scaleX .3 -> 1 with opacity, 200 ms), never a hard swap.
struct ShellTabBar: View {
    let tab: ShellHomeTab
    let onTab: (ShellHomeTab) -> Void
    let chatBadge: Int
    var groupBadge = 0
    var contactBadge = 0
    var callBadge = 0
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    @Environment(\.shellCallBadge) private var missed

    private func glyph(_ t: ShellHomeTab) -> ShellGlyph {
        switch t { case .chats: .chat; case .groups: .users; case .calls: .phone; case .contacts: .contact }
    }

    var body: some View {
        let p = ShellPalette(tin)
        VStack(spacing: 0) {
            p.hairline.frame(height: 1)
            // The pill sits 4 pt lower inside each tab, so the badge at -4 stays inside the tab.
            HStack(spacing: 0) {
                ForEach(ShellHomeTab.allCases, id: \.self) { t in
                    let on = t == tab
                    let fg = on ? p.accentInk : tin.ink2
                    let badge: Int = { switch t { case .chats: chatBadge; case .groups: groupBadge; case .contacts: contactBadge; case .calls: max(callBadge, missed) } }()
                    Button { onTab(t) } label: {
                        VStack(spacing: 2) {
                            ZStack(alignment: .topTrailing) {
                                Capsule().fill(p.accentSoft).frame(width: 56, height: 30).scaleEffect(x: on ? 1 : 0.3, y: 1).opacity(on ? 1 : 0)
                                ShellIcon(glyph: glyph(t), size: 22, color: fg).frame(width: 56, height: 30)
                                if badge > 0 { ShellCountBadge(n: badge).offset(x: 4, y: -4) }
                            }
                            .padding(.top, 5)
                            Text(t.title).font(ShellFont.figtree(12, .semibold, relativeTo: .caption)).foregroundStyle(fg)
                        }
                        .frame(maxWidth: .infinity, minHeight: 56).contentShape(Rectangle())
                    }
                    .buttonStyle(ShellPressStyle(wash: .clear, radius: 0))
                    .accessibilityLabel(t.title).accessibilityAddTraits(on ? [.isSelected, .isButton] : .isButton)
                    .accessibilityValue(badge > 0 ? (t == .contacts ? "\(badge) waiting" : "\(badge) unread") : "")
                    .animation(reduced ? nil : TinMotion.easeOut(TinMotion.indicator), value: on)
                }
            }
            .padding(.top, 2).padding(.bottom, 10)
        }
        .background(tin.sf.ignoresSafeArea(edges: .bottom))
    }
}

// ---------------------------------------------------------------- banner, chips, empty states

/// Components.kt Banner: one calm strip at the top of a tab (offline, notifications, availability).
struct ShellBanner: View {
    enum Kind { case warn, error, neutral }
    let kind: Kind
    let glyph: ShellGlyph
    let title: String
    let text: String
    var action: String? = nil
    var onAction: () -> Void = {}
    @Environment(\.tin) private var tin
    var body: some View {
        let (bg, fg): (Color, Color) = { switch kind { case .warn: (tin.warn, tin.onWarn); case .error: (tin.erc, tin.onErc); case .neutral: (tin.sf2, tin.ink) } }()
        HStack(alignment: .top, spacing: 12) {
            ShellIcon(glyph: glyph, size: 24, color: fg)
            VStack(alignment: .leading, spacing: 0) {
                Text(title).font(ShellFont.figtree(15, .bold)).foregroundStyle(fg)
                Text(text).font(TinType.bodyM).foregroundStyle(fg)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if let action {
                Button(action: onAction) {
                    Text(action).font(TinType.label.weight(.bold)).underline().foregroundStyle(kind == .neutral ? tin.pr : fg)
                        .padding(.horizontal, 10).frame(minHeight: 40)
                }
                .buttonStyle(ShellPressStyle(radius: 20)).frame(maxHeight: .infinity)
            }
        }
        .padding(.leading, 14).padding(.vertical, 14).padding(.trailing, 8)
        .background(SmoothRoundedRectangle(cornerRadius: 14).fill(bg))
        .padding(.horizontal, 16)
        .accessibilityElement(children: .combine)
    }
}

/// `.t-chip` as a filter: 32 tall, radius 16, 1 px line, 13 600; pressed = accent-soft with accent-ink (44 touch target).
struct ShellFilterChip: View {
    let label: String
    let on: Bool
    let action: () -> Void
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    var body: some View {
        let p = ShellPalette(tin)
        Button(action: action) {
            Text(label).shellStyle(.sub, weight: .semibold).foregroundStyle(on ? p.accentInk : tin.ink).padding(.horizontal, 14).frame(minHeight: 44)
                .background(RoundedRectangle(cornerRadius: 16).fill(on ? p.accentSoft : .clear).frame(height: 32))
                .overlay(RoundedRectangle(cornerRadius: 16).stroke(on ? .clear : tin.ln, lineWidth: 1).frame(height: 32))
        }
        .buttonStyle(ShellPressStyle(wash: .clear, radius: 16))
        .animation(reduced ? nil : TinMotion.easeOut(TinMotion.switchTime), value: on)
        .accessibilityAddTraits(on ? .isSelected : [])
    }
}

/// The one empty state (lib/ui/EmptyLine.svelte; SIGNUP-HANDOFF.md 6e), hero size: a new account's first screen (Chats and
/// Calls). The line is slack while you have nobody; when your first contact arrives it pulls tight in two tugs (the same pull as
/// sign-up Done) and the words turn from "ready" to "open". The hero plays no cue: adding the contact already did. Fixed slots
/// (one-line title, two reserved lines, a 220 min button): nothing moves.
struct ShellEmptyHero: View {
    let contacts: [Contact]
    var sent: [ContactRequest] = []
    let onAdd: () -> Void
    let onMessage: (String) -> Void
    let onNewChat: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let copy = ShellEmpty.heroCopy(contacts: contacts, sent: sent)
        VStack(spacing: 0) {
            TinMark(size: 88, state: contacts.isEmpty ? .slack : .taut, live: true, description: "Tinline", spring: .line, lineColor: true)
                .padding(.bottom, 18)
            Text(copy.title).font(ShellFont.figtree(17, .semibold)).foregroundStyle(tin.ink).lineLimit(1).padding(.bottom, 6)
            Text(copy.line).shellStyle(.sub19).foregroundStyle(tin.ink2).multilineTextAlignment(.center).frame(maxWidth: 300, minHeight: 38, alignment: .top)
            Spacer().frame(height: 20)
            ShellPBtn(text: copy.act, variant: .primary, minWidth: 220) {
                switch contacts.count { case 0: onAdd(); case 1: onMessage(contacts[0].did); default: onNewChat() }
            }
        }
        .frame(maxWidth: .infinity, minHeight: 360).padding(24)
    }
}

/// Quiet: an empty list in an app you already use: a small dimmed mark and one muted line, no button.
struct ShellEmptyQuietLine: View {
    let text: String
    @Environment(\.tin) private var tin
    var body: some View {
        VStack(spacing: 10) {
            TinMark(size: 40, dim: true)
            Text(text).shellStyle(.sub19).foregroundStyle(tin.ink2).multilineTextAlignment(.center).frame(maxWidth: 300)
        }
        .frame(maxWidth: .infinity).padding(.top, 40).padding(.horizontal, 24)
    }
}
