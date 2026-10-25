import SwiftUI

/// `pp.incoming`: whose line, and where the handshake is. `joined`: the line is open (taut), else asking (ringing).
/// `theirs`: they accepted the request I sent (chip "Accepted your request").
struct IncomingSheet: Equatable {
    let did: String
    let name: String
    var joined: Bool
    var theirs: Bool
}

/// The two sounds this area asks for (sound-direction.md: the `knock` and the `line`). The Shell plays them.
enum PeopleCue { case knock, line }

enum PeopleError: Swift.Error { case accountChanged }

/// The people area's slice of Android `P2pApp`: contacts, block state, contact requests, the Add contact sheet's handshake state,
/// and the profile controller. The Shell creates one, injects it with `.environmentObject`, calls `attach(node:)` whenever the
/// node changes (unlock / lock / account swap), registers `profiles` with `node.setProfileEvents(events:)` and forwards the core's
/// `ContactRequestEvents` to `handleContactRequest` / `handleRequestAccepted` (only one object can register with the core).
@MainActor
final class PeopleModel: ObservableObject, ProfileHost {
    @Published private(set) var contacts: [Contact] = []
    @Published private(set) var status: NodeStatus?
    @Published private(set) var blocks = Blocks()
    /// Requests made with my code, waiting for my answer (Contacts > "Waiting for you").
    @Published private(set) var requestsIn: [ContactRequest] = []
    /// Requests I sent, waiting for theirs (Contacts > "Waiting for them").
    @Published private(set) var requestsOut: [ContactRequest] = []
    /// The Add contact sheet as the other side of a handshake (people.svelte.ts `pp.incoming`). The Shell opens the sheet for it
    /// when nothing else is in front (never over a call or another screen), else drops it: the request waits in Contacts.
    @Published var incoming: IncomingSheet?
    /// The last contact who accepted a request of mine; the open Add contact sheet turns taut by itself.
    @Published var accepted: Contact?
    var addSheetOpen = false

    let profiles = ProfilesController()
    private(set) var node: Node?

    /// Conversation peers beyond contacts (Chat area), so their block state is read too.
    var chatPeerDids: () -> [String] = { [] }
    /// Whether the app is in front (cues play live, else the Shell notifies).
    var appVisible: () -> Bool = { true }
    var playCue: (PeopleCue) -> Void = { _ in }
    /// Own availability / name changed (Android `refreshAvailability`): the Shell re-reads what it shows.
    var onAvailabilityChanged: () -> Void = {}
    /// Notifications for a request or acceptance that arrives while the app is not in front.
    var notifyRequest: (_ did: String, _ name: String) -> Void = { _, _ in }
    var notifyAccepted: (_ did: String, _ name: String) -> Void = { _, _ in }

    init() { profiles.host = self }

    func attach(node: Node?) {
        self.node = node
        guard let node else { profiles.invalidate(); contacts = []; blocks = Blocks(); requestsIn = []; requestsOut = []; return }
        refresh()
        let did = node.profile()?.did
        profiles.select(did)
        status = node.status()
        profiles.online = status?.online == true
    }

    /// The Shell calls this when the node's status changes.
    func statusChanged(_ s: NodeStatus?) { status = s; profiles.online = s?.online == true }

    func refresh() {
        guard let node else { return }
        contacts = node.contacts()
        refreshRequests()
        refreshBlocks()
    }

    // MARK: ProfileHost

    var isOnline: Bool { status?.online == true }

    func profileWork<T>(_ ticket: ProfileSession.Ticket, _ body: @escaping (Node) throws -> T) async throws -> T {
        guard profiles.session.accepts(ticket), let node else { throw PeopleError.accountChanged }
        let result = try await Task.detached(priority: .userInitiated) { try body(node) }.value
        guard profiles.session.accepts(ticket) else { throw PeopleError.accountChanged }
        return result
    }

    func refreshAvailability() { onAvailabilityChanged() }
    func refreshContacts() { if let node { contacts = node.contacts() } }

    // MARK: requests

    func refreshRequests() {
        guard let node else { return }
        requestsIn = node.contactRequests()
        requestsOut = node.sentContactRequests()
    }

    /// Accept (the sheet or a Contacts card): they become a contact who may message and call. The `line` cue plays here.
    func acceptRequest(_ did: String) async -> Bool {
        guard let node else { return false }
        let ok = await Task.detached { (try? node.acceptContactRequest(did: did)) != nil }.value
        refreshRequests()
        if ok { playCue(.line) }
        return ok
    }

    func handleContactRequest(_ request: ContactRequest) {
        refreshRequests()
        // Open: the knock, and the sheet opens by itself. Closed or locked away: a system notification.
        if appVisible() { playCue(.knock); incoming = IncomingSheet(did: request.did, name: request.name, joined: false, theirs: false) }
        else { notifyRequest(request.did, request.name) }
    }

    func handleRequestAccepted(_ contact: Contact) {
        if !appVisible() { notifyAccepted(contact.did, contact.name) }
        refresh()
        if appVisible() { playCue(.line) }
        accepted = contact
        if !addSheetOpen { incoming = IncomingSheet(did: contact.did, name: contact.name, joined: true, theirs: true) }
    }

    // MARK: blocking

    /// Re-reads block states for every contact and conversation, and the blocked list: two core calls, however many contacts.
    func refreshBlocks() {
        guard let node else { return }
        let me = node.profile()?.did
        let dids = Set((contacts.map { $0.did } + chatPeerDids()).filter { $0 != me })
        let rows = node.peerBlocks(dids: Array(dids))
        var views: [String: BlockView] = [:]
        for r in rows {
            views[r.did] = BlockView(blockedByMe: r.state.blockedByMe, blockedAtSecs: r.state.blockedAt.map { Int64($0) }, blockedMeAtSecs: r.state.blockedMe.map { Int64($0.signedAt) })
        }
        let list = node.blockedContacts().map { BlockedRow(did: $0.did, name: $0.name, blockedAtSecs: Int64($0.blockedAt)) }
        let allowed = Set(rows.filter { $0.callsAllowed }.map { $0.did })
        let tokens = Set(rows.filter { $0.canCall }.map { $0.did })
        blocks = Blocks.from(views, list, allowed: allowed, tokens: tokens)
    }

    func block(_ did: String) {
        guard let node else { return }
        Task { _ = try? await Task.detached { try node.block(did: did) }.value; refreshBlocks() }
    }

    func unblock(_ did: String) {
        guard let node else { return }
        Task { _ = try? await Task.detached { try node.unblock(did: did) }.value; refreshBlocks() }
    }

    /// "Allow calls" (U13, P6): whether `did` may ring us. They keep messaging either way.
    func allowCalls(_ did: String, _ allowed: Bool) {
        guard let ticket = profiles.session.ticket() else { return }
        Task {
            _ = try? await profileWork(ticket) { try $0.allowCalls(did: did, allowed: allowed) }
            refreshBlocks()
        }
    }

    func rename(_ did: String, alias: String?) {
        guard let node else { return }
        Task { _ = try? await Task.detached { try node.renameContact(did: did, alias: alias) }.value; refreshContacts() }
    }
}
