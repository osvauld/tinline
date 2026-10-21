import Combine
import Foundation
import SwiftUI

/*
 * The node lifecycle: android P2pApp.kt (the one Node, account selection, unlock with the remembered key, persist, add /
 * switch / remove accounts) plus the parts of CoreService.kt that are not Android plumbing (start the node, tell it about
 * network changes). iOS has no foreground service: the node runs while the app runs, in-app. Contacts, blocks, contact
 * requests and profiles are the People area's `PeopleModel`; this file creates it, attaches it to the node and forwards the
 * core's ContactRequestEvents to it (only one object can register with the core).
 *
 * TODO(ios-device): background behaviour. Android keeps a foreground service so calls ring with the app closed. On iOS that
 * needs PushKit + CallKit (a VoIP push wakes the app, the node starts, the call is reported to CallKit) and a background
 * audio session during calls. Until then the line is open only while Tinline is in front or recently backgrounded.
 */

/// Set while a new account is being created/restored next to the existing ones (android `P2pApp.Adding`).
struct ShellAdding: Hashable {
    let from: String?
    let restore: Bool
    var link = false
}

/// A ringing call shown in the home header (android `RingHead.Ring`).
struct ShellRing {
    let did: String
    let name: String
    /// "Incoming call", "Incoming video call", or "Calling during your call".
    var sub = "Incoming call"
    /// Call waiting: the buttons are Decline and End & answer.
    var waiting = false
    var onOpen: () -> Void = {}
    var onDecline: () -> Void = {}
    var onAnswer: () -> Void = {}
}

/// One serial worker: node start/stop and identity swaps run one at a time here, never on the callers' threads
/// (android `P2pApp.lifecycle`, a single-thread executor).
final class ShellSerial: @unchecked Sendable {
    private let queue: DispatchQueue
    init(_ label: String) { queue = DispatchQueue(label: label, qos: .userInitiated) }

    func run<T>(_ work: @escaping () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { (k: CheckedContinuation<T, Swift.Error>) in
            queue.async { k.resume(with: Result { try work() }) }
        }
    }

    func submit(_ work: @escaping () -> Void) { queue.async(execute: work) }
}

/// Runs a blocking core call off the main thread (android `withContext(Dispatchers.IO)`).
func shellIO<T>(_ work: @escaping () throws -> T) async throws -> T {
    try await Task.detached(priority: .userInitiated) { try work() }.value
}

/// What one refresh reads from the core, off the main thread; published in one go.
private struct ShellSnapshot {
    var lock: LockState = .noIdentity
    var has = false
    var committed = false
    var did: String?
    var name: String?
    var status: NodeStatus?
    var history: [CallRecord] = []
    var groupLog: [GroupCallRecord] = []
    var availability = Availability(available: true, until: nil)
}

@MainActor
final class ShellCore {
    unowned let app: AppEnv
    /// The one node, from boot on (locked or not). `app.node` mirrors it only while it is unlocked.
    private(set) var node: Node?
    let keys: ShellKeyStore
    let prefs: ShellPrefs
    /// Device linking: the state machine behind the Link a device screens, and the core's LinkEvents.
    let links = ShellLinkController()
    /// The People area's model: contacts, blocks, requests and the `ProfilesController` every avatar reads.
    let people: PeopleModel
    /// The Calls area's object graph (1:1 controller, video, group call audio and UI); the Shell forwards the node's call events.
    let calls: CallsEnv
    /// The names and settings the call screens read from the other areas.
    let callDirectory: ShellCallDirectory
    private let lifecycle = ShellSerial("p2p-lifecycle")
    private var events: ShellNodeEvents?
    private var requestEvents: ShellRequestEvents?
    private var availTask: Task<Void, Never>?
    private var network: ShellNetworkWatch?
    private var bag = Set<AnyCancellable>()
    /// DID -> name of the accounts seen, so "this phone was unlinked" can still say whose account it was.
    private var names: [String: String] = [:]
    /// The app is in front (android `AppVisible.resumed`): cues play live, else the system notifies.
    var resumed = true

    init(app: AppEnv, keys: ShellKeyStore = ShellKeychainStore(), prefs: ShellPrefs = ShellPrefs()) {
        self.app = app
        self.keys = keys
        self.prefs = prefs
        let pm = PeopleModel()
        people = pm
        let dir = ShellCallDirectory(app: app, people: pm, prefs: prefs)
        callDirectory = dir
        calls = CallsEnv(directory: dir)
        links.core = self
        people.chatPeerDids = { [weak app] in app?.chats.map(\.peerDid) ?? [] }
        people.appVisible = { [weak self] in self?.resumed ?? false }
        people.playCue = { ShellWiring.cue($0) }
        people.onAvailabilityChanged = { [weak self] in self?.refreshAvailability() }
        // The other areas that only hold an AppEnv read these mirrors.
        people.$contacts.sink { [weak app] in app?.contacts = $0 }.store(in: &bag)
        people.$blocks.sink { [weak app] in app?.blocks = $0 }.store(in: &bag)
        people.$requestsIn.sink { [weak app] in app?.requestsIn = $0 }.store(in: &bag)
        people.$requestsOut.sink { [weak app] in app?.requestsOut = $0 }.store(in: &bag)
    }

    var profiles: ProfilesController { people.profiles }

    // ---------------------------------------------------------------- boot (P2pApp.onCreate)

    /// Creates the node, selects an account, tries the remembered key, then starts it. Safe to call more than once.
    func boot() async {
        guard node == nil else { return }
        let ev = ShellNodeEvents(core: self)
        events = ev
        let dir = Self.dataDir()
        do {
            node = try await lifecycle.run { try Node(dataDir: dir, events: ev) }
        } catch {
            // Nothing works without the node; the screens stay on onboarding. Logged for the Mac build.
            NSLog("Tinline: Node(dataDir:) failed: \(error)")
            return
        }
        guard let node else { return }
        node.setProfileEvents(events: people.profiles)
        node.setVideoEnabled(enabled: prefs.videoEnabled)
        node.setLinkEvents(events: links)
        let rq = ShellRequestEvents(core: self)
        requestEvents = rq
        node.setContactRequestEvents(events: rq)
        network = ShellNetworkWatch { metered in
            Task.detached { node.setConnectionMetered(metered: metered); node.networkChanged() }
        }
        let prefs = self.prefs
        try? await lifecycle.run { Self.selectLastAccountIfNone(node, prefs: prefs) }
        _ = await tryAutoUnlock()
        await refreshNow()
        if app.hasIdentity { startNode() }
    }

    /// Application Support/core: the vault, the store and the endpoint key. Excluded from backup, like Android's
    /// `allowBackup=false` (a restored vault without its Keychain key is the "key lost" screen).
    nonisolated static func dataDir() -> String {
        let fm = FileManager.default
        let base = (try? fm.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)) ?? fm.temporaryDirectory
        var url = base.appendingPathComponent("core", isDirectory: true)
        try? fm.createDirectory(at: url, withIntermediateDirectories: true)
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? url.setResourceValues(values)
        return url.path
    }

    // ---------------------------------------------------------------- refresh

    /// Re-reads the state the screens render (android `refresh()`). Returns at once; publishes when read.
    func refresh() { Task { await refreshNow() } }

    func refreshNow() async {
        guard let node else { return }
        let snap: ShellSnapshot
        do { snap = try await shellIO { Self.read(node) } } catch { return }
        publish(snap)
        // The account to come back to if the app dies while a new one is being added.
        if snap.has && snap.committed, let did = snap.did { prefs.lastAccount = did }
    }

    nonisolated private static func read(_ node: Node) -> ShellSnapshot {
        var s = ShellSnapshot()
        s.lock = node.lockState()
        s.has = node.hasIdentity()
        s.committed = s.has && node.identityCommitted()
        let me = node.profile()
        s.did = me?.did
        s.name = me?.name
        s.status = node.status()
        if s.lock == .unlocked {
            s.history = node.recentCalls(limit: 200)
            s.groupLog = (try? node.groupCallHistory(groupId: nil)) ?? []
            s.availability = node.availability()
        }
        return s
    }

    private func publish(_ s: ShellSnapshot) {
        if let did = s.did, let name = s.name { names[did] = name }
        let accountChanged = app.accountDid != s.did
        app.lockState = s.lock
        app.hasIdentity = s.has
        app.accountDid = s.did
        app.status = s.status
        app.history = s.history
        app.groupLog = s.groupLog
        if s.lock == .unlocked { setAvailability(s.availability) } else { availTask?.cancel(); app.availability = Availability(available: true, until: nil) }
        // Drafts and a pending search jump never outlive the account or the lock (secure gate).
        if accountChanged || s.lock != .unlocked { ShellWiring.clearDrafts(); app.searchJump = nil }
        let usable = s.lock == .unlocked ? node : nil
        if app.node !== usable || accountChanged {
            app.node = usable
            people.attach(node: usable)
            if let usable { calls.attach(usable); ShellWiring.attachAreas(app: app, node: usable) } else { calls.detach() }
        } else {
            people.refresh()
        }
        people.statusChanged(s.status)
        ShellWiring.refreshChats(app: app)
    }

    func refreshHistory() {
        guard let node, app.lockState == .unlocked else { return }
        Task {
            guard let r = try? await shellIO({ (node.recentCalls(limit: 200), (try? node.groupCallHistory(groupId: nil)) ?? []) }) else { return }
            app.history = r.0
            app.groupLog = r.1
        }
    }

    /// Re-reads availability and arms a timer for when a timed break ends (no polling).
    func refreshAvailability() {
        guard let node else { return }
        Task { if let a = try? await shellIO({ node.availability() }) { setAvailability(a) } }
    }

    private func setAvailability(_ a: Availability) {
        app.availability = a
        availTask?.cancel()
        if !a.available, let until = a.until {
            let wait = max(0, Double(until) - Date().timeIntervalSince1970) + 0.5
            availTask = Task { [weak self] in
                try? await Task.sleep(nanoseconds: UInt64(wait * 1_000_000_000))
                if !Task.isCancelled { self?.refreshAvailability() }
            }
        }
    }

    func setAvailable(_ available: Bool, until: Int64? = nil) {
        guard let node else { return }
        Task {
            _ = try? await shellIO { try node.setAvailable(available: available, until: until.map { UInt64($0) }) }
            refreshAvailability()
        }
    }

    // ---------------------------------------------------------------- unlock and persist

    /// If the vault is locked and a remembered key exists, unlock with it (fast, no passphrase). A key that is definitively
    /// unusable (gone, or the core says WrongPassphrase) is deleted and the node stays Locked; on a transient Keychain error
    /// the item is kept and the next startNode retries. Returns true if usable.
    func tryAutoUnlock() async -> Bool {
        guard let node else { return false }
        let keys = self.keys
        return (try? await lifecycle.run { Self.autoUnlock(node, keys: keys) }) ?? false
    }

    /// The blocking body of `tryAutoUnlock`, for callers already on the lifecycle worker.
    nonisolated static func autoUnlock(_ node: Node, keys: ShellKeyStore) -> Bool {
        let state = node.lockState()
        if state != .locked { return state != .noIdentity }
        guard let did = node.profile()?.did, keys.exists(did) else { return false }
        switch keys.load(did) {
        case .gone: keys.clear(did); return false
        case .transient: return false
        case .key(var bytes):
            defer { bytes.resetBytes(in: 0..<bytes.count) }
            do { try node.unlockWithKey(key: bytes); return true }
            catch Error.WrongPassphrase { keys.clear(did); return false }
            catch { NSLog("Tinline: unlockWithKey failed (kept for retry)"); return false }
        }
    }

    /// Wraps the vault data key in the Keychain for the current account. False if it could not be saved.
    nonisolated static func rememberKey(_ node: Node, keys: ShellKeyStore) -> Bool {
        guard let did = node.profile()?.did, var k = node.unlockKey() else { return false }
        defer { k.resetBytes(in: 0..<k.count) }
        return keys.save(did, k)
    }

    /// S1: makes a freshly created/restored identity durable. A no-passphrase identity exists only in memory until its data key
    /// is saved in the Keychain; only then is `commitIdentity` called. Returns nil on success, else a message for the user (the
    /// identity then stays uncommitted: the caller offers Retry or a passphrase). A passphrase identity is already on disk;
    /// failing to remember its key only means the passphrase is asked after a restart.
    func persistIdentity() async -> String? {
        let failed = "Couldn\u{2019}t save your account on this phone."
        guard let node else { return failed }
        let keys = self.keys
        return (try? await lifecycle.run { () -> String? in
            let saved = Self.rememberKey(node, keys: keys)
            if !saved && !node.hasPassphrase() { return "Couldn\u{2019}t save your account safely on this phone." }
            do { try node.commitIdentity(); return nil } catch { return failed }
        }) ?? failed
    }

    /// Called after unlock / set-passphrase. The state refresh is immediate; remembering the key, committing (S1) and starting
    /// the node run off the main thread. A committed account is started whatever the Keychain said; an uncommitted one is not
    /// running yet (the caller that created it handles the failure, see persistIdentity).
    func identityReady() {
        refresh()
        Task {
            let err = await persistIdentity()
            if err != nil, let node, !((try? await shellIO { node.identityCommitted() }) ?? false) { return }
            await startServices()
        }
    }

    /// Starts the node for the current account (android: the foreground service and the node).
    func startServices() async {
        await refreshNow()
        startNode()
    }

    /// Unlocks (if needed) and starts the node. Queued on the lifecycle worker; returns immediately.
    func startNode() {
        guard let node else { return }
        let keys = self.keys
        lifecycle.submit { [weak self] in
            guard node.hasIdentity() else { return }
            if !Self.autoUnlock(node, keys: keys) {
                // TODO(ios-device): Android posts a "Tinline is locked" notification here (calls cannot ring).
                Task { @MainActor in await self?.refreshNow() }
                return
            }
            do { try node.start() } catch { NSLog("Tinline: node.start failed: \(error)") }
            Task { @MainActor in await self?.refreshNow() }
        }
    }

    // ---------------------------------------------------------------- accounts

    /// Adding an account deselects the current one in the core. If the app died in that state the accounts would be invisible,
    /// so select the last used one (or the first) again.
    nonisolated private static func selectLastAccountIfNone(_ node: Node, prefs: ShellPrefs) {
        if node.hasIdentity() { return }
        let all = node.accounts()
        guard let first = all.first else { return }
        let want = prefs.lastAccount
        _ = try? node.switchAccount(did: (all.first { $0.did == want } ?? first).did)
    }

    func accounts() async -> [AccountSummary] {
        guard let node else { return [] }
        return (try? await shellIO { node.accounts() }) ?? []
    }

    /// Leaves the current account (offline until we come back) and lets onboarding add another. Throws InCall.
    func beginAdding(restore: Bool, link: Bool = false) async throws {
        guard let node else { return }
        let from = app.accountDid
        profiles.invalidate()
        try await lifecycle.run { try node.beginNewAccount() }
        app.adding = ShellAdding(from: from, restore: restore, link: link)
        await refreshNow()
    }

    /// Back out of adding: select the account we left and bring it online.
    func cancelAdding() async {
        if let from = app.adding?.from { try? await switchTo(from, passphrase: nil) }
        app.adding = nil
        await refreshNow()
    }

    func finishAdding() { app.adding = nil }

    /// Selects `did` and unlocks it: with `passphrase`, else with that account's remembered key (else it stays locked and the
    /// unlock screen shows). A wrong passphrase puts the previous account back and rethrows. Throws InCall during a call.
    func switchTo(_ did: String, passphrase: String?) async throws {
        guard let node else { return }
        let prev = app.accountDid
        let keys = self.keys
        profiles.invalidate()
        try await lifecycle.run {
            try node.switchAccount(did: did)
            guard let passphrase else { _ = Self.autoUnlock(node, keys: keys); return }
            do { try node.unlock(passphrase: passphrase) } catch {
                if let prev, prev != did { _ = try? node.switchAccount(did: prev); _ = Self.autoUnlock(node, keys: keys) }
                throw error
            }
            _ = Self.rememberKey(node, keys: keys)
        }
        await refreshNow()
        startNode()
    }

    /// Deletes a non-current account and its remembered key.
    func removeAccount(_ did: String) async throws {
        guard let node else { return }
        try await shellIO { try node.removeAccount(did: did) }
        keys.clear(did)
    }

    /// "I forgot it": restore the locked current account from its phrase. The core restores in place (34f): contacts, history
    /// and chats are kept, the passphrase and device key are new. A phrase of a different identity is refused before anything
    /// changes.
    func restoreOverLocked(phrase: String, name: String, passphrase: String) async throws {
        guard let node else { return }
        guard let oldDid = app.accountDid else { throw ShellFlowError.noAccount }
        if (try? didOfPhrase(phrase: phrase)) != oldDid { throw ShellFlowError.differentIdentity }
        profiles.invalidate()
        let keys = self.keys
        try await lifecycle.run {
            try node.beginNewAccount()
            do {
                try node.restoreIdentity(phrase: phrase, name: name, passphrase: passphrase)
                // The old remembered key opens nothing now (new data key); persistIdentity saves the new one.
                keys.clear(oldDid)
            } catch {
                if !node.hasIdentity() && node.accounts().contains(where: { $0.did == oldDid }) { _ = try? node.switchAccount(did: oldDid) }
                throw error
            }
        }
    }

    /// The core removed account `did` from this phone (we unlinked it here, or another device did): forget its remembered key,
    /// select another account if one is left (and bring it online). `notify` shows the "This phone was unlinked" screen.
    func accountGone(_ did: String, notify: Bool) async {
        let name = names.removeValue(forKey: did) ?? ""
        keys.clear(did)
        if prefs.lastAccount == did { prefs.lastAccount = nil }
        if notify { app.unlinked = name }
        let prefs = self.prefs
        if let node { try? await lifecycle.run { Self.selectLastAccountIfNone(node, prefs: prefs) } }
        await refreshNow()
        if app.hasIdentity { startNode() }
    }

    /// Profile reads/writes serialize with identity swaps; refused once the account changed under them.
    func profileWork<T>(_ ticket: ProfileSession.Ticket, _ work: @escaping (Node) throws -> T) async throws -> T {
        guard let node else { throw ShellFlowError.accountChanged }
        let session = profiles.session
        return try await lifecycle.run {
            guard session.accepts(ticket), node.lockState() == .unlocked, node.profile()?.did == ticket.did else { throw ShellFlowError.accountChanged }
            return try work(node)
        }
    }

    /// Runs `work` against the node on the lifecycle worker (serialized with swaps), for screens that need a plain core read.
    func work<T>(_ work: @escaping (Node) throws -> T) async throws -> T {
        guard let node else { throw ShellFlowError.noAccount }
        return try await lifecycle.run { try work(node) }
    }

    /// Offer and take video in calls (Settings); the core's default is on and it isn't stored there.
    func setVideoEnabled(_ on: Bool) {
        prefs.videoEnabled = on
        node?.setVideoEnabled(enabled: on)
    }

    /// "blocked" while they block us and message `messageId` has not reached them.
    nonisolated func notDeliveredReason(node: Node?, peerDid: String, messageId: String) -> String? {
        node?.notDeliveredReason(peerDid: peerDid, messageId: messageId)
    }

    // ---------------------------------------------------------------- callbacks (hopped to the main actor)

    fileprivate func onStatus(_ s: NodeStatus) {
        app.status = s
        people.statusChanged(s)
    }

    fileprivate func onContactsChanged() {
        people.refresh()
        refreshHistory()
    }

    fileprivate func onCallState(_ callId: String, _ state: CallState) {
        if case .ended = state { refreshHistory() }
        calls.onCallState(callId, state)
    }
}

/// What the call screens read from the other areas (Calls area `CallDirectory`), from the app state, People and Settings.
@MainActor
final class ShellCallDirectory: CallDirectory {
    private unowned let app: AppEnv
    private let people: PeopleModel
    private let prefs: ShellPrefs
    /// The home app bar is on screen and the app in front, so a ringing call shows there (android `AppVisible.ringHeadUp`).
    var ringHeadUp = false

    init(app: AppEnv, people: PeopleModel, prefs: ShellPrefs) { self.app = app; self.people = people; self.prefs = prefs }

    var currentDid: String? { app.accountDid }
    var preferHdCapture: Bool { prefs.preferHd }
    var callsAlertOn: Bool { prefs.alert("calls") }
    var quiet: Bool { prefs.alert("quiet") }
    var ringHeadShowing: Bool { ringHeadUp && (app.core?.resumed ?? false) }
    func blockedByMe(_ did: String) -> Bool { people.blocks.blockedByMe(did) }
    func groupName(_ gid: String) -> String? { ShellWiring.groupSummaries(node: app.node).first { $0.id == gid }?.name }
    func personName(_ did: String) -> String? { people.contacts.first { $0.did == did }.map(ShellText.display) }
    func refreshGroupLog() { app.core?.refreshHistory() }
}

/// Errors the flows raise themselves (the core's own are `Error`, the generated enum, which shadows `Swift.Error` here).
enum ShellFlowError: Swift.Error, LocalizedError {
    case noAccount, differentIdentity, accountChanged
    var errorDescription: String? {
        switch self {
        case .noAccount: return "No account"
        case .differentIdentity: return "That recovery phrase belongs to a different identity"
        case .accountChanged: return "Account changed"
        }
    }
}

// ---------------------------------------------------------------- core callback sinks (core threads: never block here)

final class ShellNodeEvents: NodeEvents, @unchecked Sendable {
    private weak var core: ShellCore?
    @MainActor init(core: ShellCore) { self.core = core }

    func onStatus(status: NodeStatus) { Task { @MainActor [weak core] in core?.onStatus(status) } }
    func onContactsChanged() { Task { @MainActor [weak core] in core?.onContactsChanged() } }
    func onIncomingCall(call: CallInfo) { Task { @MainActor [weak core] in core?.calls.onIncomingCall(call) } }
    func onCallState(callId: String, state: CallState) { Task { @MainActor [weak core] in core?.onCallState(callId, state) } }
    func onLog(line: String) {
        #if DEBUG
        NSLog("p2pcore: %@", line)
        #endif
    }
}

/// The core's contact-request callbacks, handed to the People area's model on the main actor.
final class ShellRequestEvents: ContactRequestEvents, @unchecked Sendable {
    private weak var core: ShellCore?
    @MainActor init(core: ShellCore) { self.core = core }
    func onContactRequest(request: ContactRequest) { Task { @MainActor [weak core] in core?.people.handleContactRequest(request) } }
    func onRequestAccepted(contact: Contact) { Task { @MainActor [weak core] in core?.people.handleRequestAccepted(contact) } }
}
