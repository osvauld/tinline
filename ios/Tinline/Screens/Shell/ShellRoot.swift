import SwiftUI

/*
 * The app's root (android MainActivity.kt `Root`): which gate is in front (unlinked, onboarding / adding, unlock, terms),
 * then the home stack. Home is the tab screen; every other route covers it full screen. The stack is not saved: the
 * recovery phrase may sit in it, and Home is a fine place to restart.
 */

/// Where the Shell pushes (android MainActivity `Route`). Groups push their own `GroupsRoute` on the same stack.
enum ShellRoute: Hashable {
    case contact(String)
    case settings
    case history
    case chat(String, MessageHit?)
    case newChat
    case passphrase
    case phraseGate
    /// The words in memory only while this screen is up.
    case phraseShown(String)
    case about
    case licences
    case diagnostics
    case micNeeded
    case switchConfirm(String)
    case devices
    case deviceDetail(String)
    case linkDevice
    /// Settings > Blocked people (real data).
    case blocked
    /// Full-screen search (Search.swift).
    case search
    /// Playground-ported settings/profile pages.
    case me
    case notifications
    case sounds
    case video
    case storage
    case privacy
    case callsAllowed
}

/// The Add contact sheet's arguments (android `Route.Add`): a sheet over Home, not a pushed screen.
struct ShellAddArgs: Identifiable, Equatable {
    var scan = false
    var ticket: String? = nil
    var paste = false
    var id: String { "\(scan)-\(ticket ?? "")-\(paste)" }
}

/// The home tabs. Chats opens by default; Groups is group chat on the core (Groups area).
enum ShellHomeTab: String, CaseIterable {
    case chats, groups, calls, contacts
    var title: String { switch self { case .chats: tr("shell_tab_chats"); case .groups: tr("shell_tab_groups"); case .calls: tr("shell_tab_calls"); case .contacts: tr("shell_tab_contacts") } }
}

/// The navigation state every Shell screen reads: the stack, the sheet over Home, and Home's tab (which survives opening a
/// chat or group and coming back).
@MainActor
final class ShellNav: ObservableObject {
    @Published var path = NavigationPath()
    @Published var add: ShellAddArgs?
    @Published var tab: ShellHomeTab = .chats
    /// The last Shell route pushed (NavigationPath cannot be read back): "is that chat already on top?".
    private(set) var top: ShellRoute?

    var atHome: Bool { path.isEmpty }

    func push(_ r: ShellRoute) { path.append(r); top = r }
    func push(group r: GroupsRoute) { path.append(r); top = nil }
    func pop() { if !path.isEmpty { path.removeLast() }; top = nil }
    func home() { path = NavigationPath(); add = nil; top = nil }
    func openAdd(_ a: ShellAddArgs) { add = a }
}

/// The app's root view: the gates, then the home stack. Owns node startup (`ShellCore.boot`) and the app lifecycle.
struct RootView: View {
    @EnvironmentObject private var app: AppEnv
    @Environment(\.scenePhase) private var phase
    @AppStorage("appearance.dark") private var appearanceDark: Bool?
    @State private var booted = false
    @State private var core: ShellCore?

    var body: some View {
        Group {
            if let core, booted { ShellGates(core: core) } else { ShellSplash() }
        }
        .preferredColorScheme(appearanceDark.map { $0 ? .dark : .light })
        .task {
            if core == nil {
                let c = app.core ?? ShellCore(app: app)
                app.core = c
                core = c
            }
            await core?.boot()
            booted = true
        }
        .onChange(of: phase) { _, p in
            guard let core else { return }
            switch p {
            case .active: core.resumed = true; core.refresh()
            case .background, .inactive:
                if core.resumed { core.resumed = false; core.calls.appLeftFront() }
            @unknown default: break
            }
        }
    }
}

/// While the node is created: the page colour only (no spinner, no claim).
private struct ShellSplash: View {
    @Environment(\.tin) private var tin
    var body: some View { tin.bg.ignoresSafeArea() }
}

private struct ShellGates: View {
    let core: ShellCore
    @EnvironmentObject private var app: AppEnv
    @StateObject private var nav = ShellNav()
    @StateObject private var toast = ShellToast()
    @State private var forgot = false
    @State private var onboarding: Bool
    @State private var termsOk: Bool
    @State private var missing: [ShellNeed] = []

    init(core: ShellCore) {
        self.core = core
        _onboarding = State(initialValue: !core.app.hasIdentity)
        _termsOk = State(initialValue: core.prefs.termsAccepted)
    }

    private func refreshMissing() { Task { missing = await ShellWiring.permissions.missing() } }

    /// Create / restore / link: leave the current account and run the add-account onboarding.
    private func addAccount(restore: Bool, link: Bool = false) {
        Task {
            do { try await core.beginAdding(restore: restore, link: link); nav.home(); onboarding = true }
            catch { toast.say(ShellText.friendly(error)) }
        }
    }

    private func finishOnboarding(addFirst: Bool) {
        onboarding = false
        nav.home()
        nav.tab = .chats
        termsOk = core.prefs.termsAccepted
        refreshMissing()
        // Done "Add my first contact": Chats (its empty hero) with Add contact open over it, on By code.
        if addFirst { nav.openAdd(ShellAddArgs()) }
    }

    var body: some View {
        ZStack {
            content
            ShellToastView(toast: toast)
        }
        .environmentObject(core.people)
        .environmentObject(core.calls)
        .environmentObject(nav)
        .environmentObject(toast)
        .onAppear(perform: refreshMissing)
        .onChange(of: app.hasIdentity) { _, has in if !has { nav.home() } }
        // Locking or switching accounts takes the secrets and the drafts away (Conversation drafts are cleared in ShellCore).
        .onChange(of: app.lockState) { _, l in if l != .unlocked { nav.home() } }
    }

    @ViewBuilder private var content: some View {
        if let name = app.unlinked {
            ShellUnlinkedScreen(name: name, onOk: { app.unlinked = nil; nav.home() }, onLinkAgain: {
                app.unlinked = nil
                if !app.hasIdentity { onboarding = true } else { addAccount(restore: false, link: true) }
            })
        } else if onboarding || !app.hasIdentity || app.adding != nil {
            ShellOnboardingFlow(core: core, missing: missing, onFix: fix, onAddFirst: { finishOnboarding(addFirst: true) },
                                onDone: { finishOnboarding(addFirst: false) })
                .id(app.adding)
        } else if app.lockState == .locked {
            if forgot {
                ShellOnboardingFlow(core: core, missing: missing, onFix: fix, forgot: true, onCancelForgot: { forgot = false },
                                    onDone: { forgot = false; refreshMissing() })
            } else {
                ShellUnlockScreen(core: core, onForgot: { forgot = true }).id(app.accountDid)
            }
        } else if !termsOk {
            ShellTermsScreen { core.prefs.acceptTerms(); termsOk = true }
        } else {
            ShellMainStack(core: core, missing: missing, fix: fix, addAccount: addAccount)
        }
    }

    /// A permission row's request: the system dialog first (it answers at once when it will not ask again), then Settings.
    private func fix(_ n: ShellNeed) {
        Task {
            _ = await ShellWiring.permissions.request(n)
            refreshMissing()
        }
    }
}

/// The home stack, the call strip above it and the call screen over everything.
private struct ShellMainStack: View {
    let core: ShellCore
    let missing: [ShellNeed]
    let fix: (ShellNeed) -> Void
    let addAccount: (Bool, Bool) -> Void
    @EnvironmentObject private var app: AppEnv
    @EnvironmentObject private var nav: ShellNav
    @EnvironmentObject private var people: PeopleModel
    @EnvironmentObject private var calls: CallsEnv
    @EnvironmentObject private var toast: ShellToast
    @State private var callError: String?
    @State private var callShown = false

    init(core: ShellCore, missing: [ShellNeed], fix: @escaping (ShellNeed) -> Void, addAccount: @escaping (_ restore: Bool, _ link: Bool) -> Void) {
        self.core = core; self.missing = missing; self.fix = fix; self.addAccount = addAccount
    }

    /// Tap on a call button: microphone first (the system dialog; still missing after it = the explainer screen), then dial.
    private func call(_ did: String, video: Bool) {
        Task {
            if await ShellWiring.permissions.missing().contains(.mic) {
                guard await ShellWiring.permissions.request(.mic) else { nav.push(.micNeeded); return }
            }
            place(did, video: video)
        }
    }

    private func place(_ did: String, video: Bool) {
        callError = nil
        // Either side blocking, or someone no longer a contact: no call (the core refuses it too).
        let contact = people.contacts.first { $0.did == did }
        let isContact = contact != nil
        if people.blocks.callGate(did, contact: isContact) != .ok {
            let name = contact.map(ShellText.display) ?? app.chats.first { $0.peerDid == did }?.peerName ?? ""
            toast.say(BlockCopy.cantCall(name: name, state: people.blocks.composer(did, contact: isContact)))
            return
        }
        Task {
            do { try await ShellWiring.placeCall(core: core, did: did, video: video) }
            catch { callError = ShellText.isInCall(error) ? ShellText.friendly(error) : (error.localizedDescription.isEmpty ? tr("shell_err_call_start") : ShellText.friendly(error)) }
        }
    }

    /// RingHead.svelte: a call ringing while the app is open turns the home header into the caller, in place. Over my own answered
    /// call (call minimised to the strip) a second caller does the same, with Decline / End & answer.
    private var ring: ShellRing? {
        let ctl = calls.controller
        if let u = ctl.ui, let w = ctl.waiting, u.state == .active {
            return ShellRing(did: w.peerDid, name: ShellText.ifBlank(w.peerName, tr("shell_unknown")), sub: tr("shell_ring_waiting"), waiting: true,
                             onOpen: { ctl.openCallScreen() }, onDecline: { ctl.declineWaiting() }, onAnswer: { ctl.endAndAnswer() })
        }
        if let u = ctl.ui, u.info.incoming, u.state == .ringing {
            // Another account's call (`account`): "Asha → Work"; answering switches first and opens the screen after (CallController).
            let switches = u.info.account != nil
            return ShellRing(did: u.info.peerDid, name: u.info.ringName(tr("shell_unknown")), sub: u.info.video ? tr("shell_ring_incoming_video") : tr("shell_ring_incoming"),
                             onOpen: { ctl.openCallScreen() }, onDecline: { ctl.decline() }, onAnswer: { ctl.answer(); if !switches { ctl.openCallScreen() } })
        }
        return nil
    }

    var body: some View {
        ZStack {
            VStack(spacing: 0) {
                // A minimised call's strip sits in flow at the top, under the status bar (CallStrip.kt).
                ShellWiring.callStrip(calls: calls, onOpenCall: { callShown = true })
                NavigationStack(path: $nav.path) {
                    ShellHomeScreen(core: core, missing: missing, fix: fix, ring: ring, callError: callError, call: { call($0, video: false) })
                        .toolbar(.hidden, for: .navigationBar)
                        .navigationDestination(for: ShellRoute.self) { r in
                            routeView(r).toolbar(.hidden, for: .navigationBar)
                        }
                        .groupsNavigation()
                }
                // A group screen that opens a 1:1 chat (a member's row) comes back here.
                .environment(\.groupsOpenChat, { did in nav.push(.chat(did, nil)) })
                // Group calls from the group screens go to the Calls area's group call UI.
                .environment(\.groupCallLauncher, core.groupLauncher)
            }
            // MiniVideo: their picture floats over the app while a video call is minimised.
            CallMiniVideo(calls: calls, onOpenCall: { callShown = true })
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
                .padding(.trailing, 12).padding(.bottom, 92)
            // "Sam started a call in Weekend hike": over the app (GroupsOpenChat.current is the group on screen).
            GroupCallAnnounce(ui: calls.groupUi, inOneToOne: calls.controller.ui != nil, openGid: GroupsOpenChat.current,
                              onOpen: { gid in nav.push(group: ShellWiring.groupRoute(gid)) })
            // The full group call screen while a group call is on and not minimised.
            GroupCallLayer(ui: calls.groupUi, audio: calls.groupAudio)
        }
        .fullScreenCover(isPresented: $callShown) {
            ShellWiring.callScreen(calls: calls, onClose: { callShown = false },
                                   onOpenChat: { did in callShown = false; nav.push(.chat(did, nil)) })
        }
        .onAppear { calls.controller.openCallScreen = { callShown = true } }
        // A notification tap asked for a conversation (another account's notice switches to it first, ShellCore.openFromNotice).
        .onReceive(OpenChat.request) { did in
            guard let did else { return }
            OpenChat.request.send(nil)
            callShown = false
            nav.home()
            nav.push(.chat(did, nil))
        }
        .onChange(of: callShown) { _, on in calls.controller.callScreenShown = on }
        // A group call or join that needs the microphone: the system dialog, then the call (or the explainer screen).
        .onChange(of: calls.groupUi.needMic) { _, gid in
            guard let gid else { return }
            calls.groupUi.micAsked()
            Task {
                if await ShellWiring.permissions.request(.mic) { calls.groupUi.call(gid, onError: toast.say) } else { nav.push(.micNeeded) }
            }
        }
        // A call that starts while the app is open gets its screen, except a ringing one the home header is showing (RingHead).
        .onChange(of: calls.controller.ui?.info.callId) { old, new in
            guard old == nil, new != nil, let u = calls.controller.ui else { return }
            if !(u.info.incoming && u.state == .ringing && core.callDirectory.ringHeadShowing) { callShown = true }
        }
        .sheet(item: $nav.add) { a in
            ShellWiring.addContactSheet(people: people, scan: a.scan, ticket: a.ticket, paste: a.paste, nav: nav)
                .presentationDetents([.large]).presentationDragIndicator(.visible)
        }
        // Someone used my code, or accepted my request while the sheet was closed: the sheet opens by itself over Home (my own
        // open Add contact sheet turns into it), never over a call or another screen; else it is dropped and waits in Contacts.
        .onChange(of: people.incoming) { _, inc in
            guard inc != nil, nav.add == nil else { return }
            if nav.atHome && calls.controller.ui == nil { nav.openAdd(ShellAddArgs()) } else { people.incoming = nil }
        }
    }

    @ViewBuilder private func routeView(_ r: ShellRoute) -> some View {
        switch r {
        case .search:
            ShellSearchScreen(onBack: nav.pop, onChat: { did, hit in nav.push(.chat(did, hit)) },
                              onGroup: { gid in nav.push(group: ShellWiring.groupRoute(gid)) }, onContact: { nav.push(.contact($0)) })
        case .chat(let did, let hit):
            ShellWiring.conversation(app: app, people: people, calls: calls, chatId: did, searchHit: hit, nav: nav, call: { call($0, video: $1) })
        case .newChat:
            ShellNewChatScreen(onBack: nav.pop, onPick: { did in nav.pop(); nav.push(.chat(did, nil)) },
                               onAdd: { nav.pop(); nav.openAdd(ShellAddArgs(scan: true)) })
        case .history:
            ShellHistoryScreen(onBack: nav.pop, onContact: { nav.push(.contact($0)) })
        case .contact(let did):
            ShellWiring.contactDetail(app: app, did: did, nav: nav, call: { call($0, video: $1) })
        case .settings:
            ShellSettingsRoute(core: core, missing: missing, onBack: nav.pop,
                               onCreate: { addAccount(false, false) }, onRestore: { addAccount(true, false) }, onLink: { addAccount(false, true) })
        case .me:
            ShellWiring.profile(fallbackName: "", onBack: nav.pop)
        case .notifications: ShellNotificationsPage(missing: missing, onBack: nav.pop, fix: fix)
        case .sounds: ShellSoundsPage(onBack: nav.pop)
        case .video: ShellVideoSettingsPage(core: core, onBack: nav.pop)
        case .storage: ShellStoragePage(core: core, onBack: nav.pop)
        case .privacy: ShellPrivacyPage(core: core, onBack: nav.pop)
        case .callsAllowed: ShellCallsAllowedPage(onBack: nav.pop)
        case .blocked: ShellWiring.blockedPeople(onBack: nav.pop, say: toast.say)
        case .switchConfirm(let did):
            ShellSwitchConfirmScreen(core: core, did: did, onBack: nav.pop, onSwitched: nav.home, onMessage: toast.say)
        case .devices:
            ShellLinkedDevicesScreen(core: core, onBack: nav.pop, onLink: { nav.push(.linkDevice) }, onDetail: { nav.push(.deviceDetail($0)) })
        case .deviceDetail(let id): ShellDeviceDetailScreen(core: core, device: id, onBack: nav.pop, onMessage: toast.say)
        case .linkDevice: ShellLinkSession(links: core.links, newDevice: false, onClose: nav.pop)
        case .passphrase: ShellChangePassphraseScreen(core: core, onBack: nav.pop)
        case .phraseGate: ShellPhraseGateScreen(core: core, onBack: nav.pop, onPhrase: { p in nav.pop(); nav.push(.phraseShown(p)) })
        case .phraseShown(let phrase):
            // Back to the gate (replacing this screen) so the words leave memory and the next look needs the check again.
            ShellPhraseShownScreen(phrase: phrase, onHide: nav.pop, onRelock: { nav.pop(); nav.push(.phraseGate) })
        case .about: ShellAboutScreen(onBack: nav.pop, onLicences: { nav.push(.licences) })
        case .licences: ShellLicencesScreen(onBack: nav.pop)
        case .diagnostics: ShellDiagnosticsScreen(core: core, onBack: nav.pop)
        case .micNeeded: ShellMicNeededScreen(onOpenSettings: { ShellWiring.permissions.openSettings() }, onNotNow: nav.pop)
        }
    }
}

/// CallScreens.kt MicNeededScreen: the microphone was refused; Settings is the way.
struct ShellMicNeededScreen: View {
    let onOpenSettings: () -> Void
    let onNotNow: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        ShellPage {
            VStack(spacing: 12) {
                Spacer(minLength: 0)
                ShellIcon(glyph: .micoff, size: 44, color: tin.er).frame(width: 96, height: 96).background(Circle().fill(tin.erc))
                Text(tr("shell_mic_title")).font(TinType.h1).foregroundStyle(tin.ink).multilineTextAlignment(.center).padding(.top, 12)
                Text(tr("shell_mic_body"))
                    .font(TinType.bodyL).foregroundStyle(tin.ink2).multilineTextAlignment(.center)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 28)
            VStack(spacing: 8) {
                ShellTinButton(text: tr("shell_mic_open_settings"), action: onOpenSettings)
                ShellHint(tr("shell_mic_hint"), alignment: .center).frame(maxWidth: .infinity)
                ShellTinButton(text: tr("shell_not_now"), style: .text, action: onNotNow)
            }
            .padding(.horizontal, 24).padding(.bottom, 48)
        }
    }
}
