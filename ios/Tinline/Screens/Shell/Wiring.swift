import Combine
import SwiftUI

/*
 * Where the Shell meets the other areas: every call into another area's public root or model goes through this file, so the
 * lead can fix a signature in one place. Each function names what it guessed.
 */

@MainActor
enum ShellWiring {
    static var qrScanner: ShellQrScanning = ShellQrScannerStub()
    static var deviceAuth: ShellDeviceAuth = ShellLocalAuth()
    static var permissions: ShellPermissions = ShellSystemPermissions()
    private static var chatBag: AnyCancellable?
    private static var chatNode: ObjectIdentifier?

    // ---------------------------------------------------------------- after unlock

    /// Hooks the other areas to the unlocked node. Chat: `ChatBackend.current(node:)` attaches its source (ChatSource.swift);
    /// its chat list feeds `app.chats` (the Chats tab, and block reads for kept chats). Groups attach themselves on first use
    /// (`GroupsHub.controller(for:)`).
    static func attachAreas(app: AppEnv, node: Node) {
        let key = ObjectIdentifier(node)
        guard chatNode != key, let src = ChatBackend.current(node: node) else { return }
        chatNode = key
        chatBag = src.chatsPublisher.receive(on: DispatchQueue.main).sink { [weak app] list in
            app?.chats = list
            app?.core?.people.refreshBlocks()
        }
        src.refresh()
        _ = GroupsHub.controller(for: node)
    }

    static func refreshChats(app: AppEnv) { ChatBackend.current(node: app.node)?.refresh() }

    static func chatLinks(app: AppEnv) -> [String: ChatLink] { ChatBackend.current(node: app.node)?.links ?? [:] }

    /// Composer drafts never outlive the account or the lock (Chat/ChatDrafts.swift).
    static func clearDrafts() { ConversationDrafts.clear() }

    static var draftPreviews: AnyPublisher<[String: String], Never> { ConversationDrafts.previews.eraseToAnyPublisher() }

    // ---------------------------------------------------------------- sounds

    /// The `knock` and the `line` (sound-direction.md). GUESS: the Calls area's cue player is not public yet, so this is silent.
    /// TODO(ios-device): route to the Calls area's cue engine (android Cues.knock / Cues.line), honouring Settings > Sounds > Quiet.
    static func cue(_ c: PeopleCue) {
        if ShellPrefs().alert("quiet") { return }
        _ = c
    }

    /// Settings > Sounds previews (android CallSounds.preview). GUESS: silent until the Calls area exposes its palette player.
    static func previewSound(_ cue: String) { _ = cue }

    // ---------------------------------------------------------------- calls

    /// Places a 1:1 call (android MainActivity `place()` -> `CallController.place`). Video: the camera turns on once the call
    /// runs (`wantCamera`, this call only); a plain call keeps it off (video off by default).
    static func placeCall(core: ShellCore, did: String, video: Bool) async throws {
        core.calls.video.wantCamera = video
        do { _ = try await core.calls.controller.place(did: did) } catch { core.calls.video.wantCamera = false; throw error }
    }

    /// The 1:1 call screen (Calls area `CallScreenRoot`), presented full screen by the root while a call is up.
    @ViewBuilder static func callScreen(calls: CallsEnv, onClose: @escaping () -> Void, onOpenChat: @escaping (String) -> Void) -> some View {
        CallScreenRoot(calls: calls, onClose: onClose, onOpenChat: onOpenChat)
    }

    /// A minimised call's strip at the top (Calls area `CallStripView`); zero height without a call.
    @ViewBuilder static func callStrip(calls: CallsEnv, onOpenCall: @escaping () -> Void) -> some View {
        CallStripView(calls: calls, onOpenCall: onOpenCall)
    }

    // ---------------------------------------------------------------- other areas' roots

    @ViewBuilder static func conversation(app: AppEnv, people: PeopleModel, chatId: String, searchHit: MessageHit?, nav: ShellNav,
                                          call: @escaping (String, Bool) -> Void) -> some View {
        let c = people.contacts.first { $0.did == chatId }
        let me = app.accountDid
        let isSelf = chatId == me
        let name = isSelf ? "You" : c.map(ShellText.display) ?? app.chats.first { $0.peerDid == chatId }.map { ShellText.ifBlank($0.peerName, "Unknown") } ?? "Unknown"
        let node = app.node
        ConversationView(
            chatId: chatId, name: name, online: app.status?.online == true, isSelf: isSelf,
            block: isSelf ? nil : ConvBlock(view: people.blocks.of(chatId), composer: people.blocks.composer(chatId, contact: c != nil),
                                            callGate: people.blocks.callGate(chatId, contact: c != nil)),
            notDelivered: { id in node?.notDeliveredReason(peerDid: chatId, messageId: id) },
            calls: app.history.filter { $0.peerDid == chatId }, addedAtSecs: c.map { Int64($0.addedAt) }, searchHit: searchHit,
            onBack: { nav.pop() }, onCall: { call(chatId, false) }, onVideo: { call(chatId, true) },
            onInfo: c != nil ? ({ nav.push(.contact(chatId)) } as () -> Void) : nil,
            onUnblock: { people.unblock(chatId) }, onAddContact: { nav.push(.add(scan: true)) })
    }

    @ViewBuilder static func contactDetail(app: AppEnv, did: String, nav: ShellNav, call: @escaping (String, Bool) -> Void) -> some View {
        let now = ShellText.nowSecs()
        ContactDetailView(did: did, history: app.history.filter { $0.peerDid == did }.map { r in
            PeopleCallRecord(id: r.callId + ":\(r.startedAt)", kind: r.missed ? .missed : r.incoming ? .incoming : (r.reason == "unreachable" ? .unreached : .outgoing),
                             title: ShellText.callWhat(r), whenLong: whenLong(then: Int64(r.startedAt), now: now, utcOffsetSecs: Int32(TimeZone.current.secondsFromGMT())),
                             direct: r.durationSecs > 0 ? r.direct : nil)
        }, onBack: { nav.pop() }, onCall: { call(did, false) }, onMessage: { nav.push(.chat(did, nil)) })
    }

    /// People's Add contact sheet. Its open flag keeps an acceptance from opening a second sheet (android `addSheetOpen`).
    @ViewBuilder static func addContactSheet(people: PeopleModel, scan: Bool, ticket: String?, paste: Bool, nav: ShellNav) -> some View {
        AddContactSheet(startOnScan: scan, ticket: ticket, pasteOnOpen: paste,
                        onClose: { people.incoming = nil; nav.pop() },
                        onMessage: { did in people.incoming = nil; nav.pop(); nav.push(.chat(did, nil)) },
                        onProfile: { did in nav.pop(); nav.push(.contact(did)) })
            .onAppear { people.addSheetOpen = true }
            .onDisappear { people.addSheetOpen = false }
    }

    @ViewBuilder static func profile(fallbackName: String, title: String = "My profile", onBack: @escaping () -> Void) -> some View {
        ProfileScreen(fallbackName: fallbackName, title: title, onBack: onBack)
    }

    @ViewBuilder static func blockedPeople(onBack: @escaping () -> Void, say: @escaping (String) -> Void) -> some View {
        BlockedPeopleScreen(onBack: onBack, say: say)
    }

    /// The Groups tab body. It registers its own routes on the NavigationStack (`groupsNavigation()`); a 1:1 chat it opens comes
    /// back through `groupsOpenChat`.
    @ViewBuilder static func groupsTab(nav: ShellNav) -> some View {
        GroupsRootView().environment(\.groupsOpenChat, { did in nav.push(.chat(did, nil)) })
    }

    static func groupRoute(_ id: String) -> GroupsRoute { .group(id) }
    static let newGroupRoute = GroupsRoute.newGroup
    static func joinGroupRoute(_ link: String?) -> GroupsRoute { .join(link) }

    /// Group invitations waiting for me (the Groups tab badge). GroupsCore `pendingInvitations`.
    static func pendingGroupInvitations(node: Node?) -> Int {
        guard let node else { return 0 }
        return GroupsHub.controller(for: node).invitations.filter { !$0.outgoing && $0.state == .pending }.count
    }

    static func groupSummaries(node: Node?) -> [GroupSummary] {
        guard let node else { return [] }
        return GroupsHub.controller(for: node).list
    }

    static func groupsChanged(node: Node?) -> AnyPublisher<Void, Never> {
        guard let node else { return Empty().eraseToAnyPublisher() }
        return GroupsHub.controller(for: node).objectWillChange.map { _ in () }.eraseToAnyPublisher()
    }

    /// Start or join a group call from the Calls tab (android `app.groupCallUi.call(gid, say)`).
    static func groupCall(_ calls: CallsEnv, gid: String, say: @escaping (String) -> Void) { calls.groupUi.call(gid, onError: say) }

    // ---------------------------------------------------------------- profile + settings reads

    static func selfView(people: PeopleModel) -> ProfileView? { people.profiles.selfDid.flatMap { people.profiles.views[$0] } }

    /// "Busy until 18:30" (People/ProfileModel.swift). NOTE: People's `enum ProfileInfo` has the same name as the core's
    /// `struct ProfileInfo` (`Node.profile()`); one of them must be renamed before this compiles.
    static func busyUntilLabel(_ ms: Int64) -> String { ProfileInfo.untilLabel(atMs: ms) }
}
