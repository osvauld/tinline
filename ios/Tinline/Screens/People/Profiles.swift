import SwiftUI
import UIKit

/// One person's account profile as the UI sees it (profile-sync.md: `account_profile(did)` + `profile_picture(did)`).
/// `fields` is nil while no profile is accessible (not arrived yet, or none). `image` is the decoded current picture, present only
/// when `hash` is selected, ready, and its bytes decoded; a hash without an image is "awaiting" (Avatar.svelte's breathing ring).
struct ProfileView {
    var fields: PeopleObject? = nil
    var hash: String? = nil
    var ready = false
    var image: UIImage? = nil
    var broken = false

    var awaiting: Bool { hash != nil && !ready }
    func text(_ key: String) -> String? { fields?[key]?.asString }
}

/// The core calls the profile screens make. Each patches one thing (profile.ts mirrors these one-to-one).
enum ProfileOp {
    case setName(String)
    /// `set_profile_field(key, json?)`: `json` is a JSON value; nil removes the key.
    case setField(key: String, json: String?)
    case setAvailable(Bool, untilSecs: Int64?)
    case replacePicture(Data)
    case removePicture
}

/// UI work is bound to an account session, not just a DID (A -> B -> A is a new session).
final class ProfileSession {
    struct Ticket: Equatable { let epoch: Int64; let did: String }
    private var epoch: Int64 = 0
    private var did: String?
    private let lock = NSLock()

    /// True when the selected account changed.
    @discardableResult func select(_ next: String?) -> Bool {
        lock.lock(); defer { lock.unlock() }
        if did == next { return false }
        epoch += 1; did = next
        return true
    }
    func invalidate() { lock.lock(); epoch += 1; did = nil; lock.unlock() }
    func ticket() -> Ticket? { lock.lock(); defer { lock.unlock() }; return did.map { Ticket(epoch: epoch, did: $0) } }
    func accepts(_ t: Ticket) -> Bool { lock.lock(); defer { lock.unlock() }; return t.epoch == epoch && t.did == did }
}

/// What the profile controller needs from the app (Android `P2pApp`): node reads serialized with account swaps, and the two
/// refreshes a profile change implies. The Shell implements this; `PeopleModel` is the default.
@MainActor
protocol ProfileHost: AnyObject {
    var isOnline: Bool { get }
    /// Runs `body` against the node on a worker, only while `ticket` is still the selected account session; throws otherwise.
    func profileWork<T>(_ ticket: ProfileSession.Ticket, _ body: @escaping (Node) throws -> T) async throws -> T
    func refreshAvailability()
    func refreshContacts()
}

/// The profile adapter the screens and every avatar read. The core callback only enqueues; node reads, picture reads and decoding
/// run on a worker through `ProfileHost.profileWork`, serialized with account swaps. Results are keyed by account session + owner +
/// request and dropped when any changed; a picture is re-checked against the selected hash before it is shown, so an old
/// download never overwrites a new one. The Shell registers it once: `node.setProfileEvents(events: controller)`.
@MainActor
final class ProfilesController: ObservableObject, ProfileEvents, @unchecked Sendable {
    let session = ProfileSession()
    weak var host: ProfileHost?
    private var serial: Int64 = 0
    private var reads: [String: Int64] = [:]
    /// Keyed by DID.
    @Published private(set) var views: [String: ProfileView] = [:]
    /// The selected account (nil while locked / none).
    @Published private(set) var selfDid: String?
    /// Whether this device is connected (a save then says it syncs when connected).
    @Published var online = false

    /// profile.ts SAVE_FAILED, verbatim.
    static var saveFailed: String { tr("people_save_failed") }
    /// Native-only state (the playground has one account in front at a time): a write that raced an account switch.
    static var accountChanged: String { tr("people_account_changed") }

    init(host: ProfileHost? = nil) { self.host = host }

    func invalidate() { session.invalidate(); selfDid = nil; reads.removeAll(); views = [:] }

    func select(_ did: String?) {
        if session.select(did) {
            selfDid = did; reads.removeAll(); views = [:]
            if let did { refresh(did) }
        }
    }

    func watch(_ did: String) { if views[did] == nil && reads[did] == nil { refresh(did) } }

    // ProfileEvents: a core callback thread. Enqueue only; re-read, never infer from event order.
    nonisolated func onProfileChanged(did: String) {
        Task { @MainActor in await self.changed(did) }
    }

    private func changed(_ did: String) async {
        refresh(did)
        guard let ticket = session.ticket(), let host else { return }
        _ = try? await host.profileWork(ticket) { _ in () }   // serialize behind any swap, then refresh what the change implies
        if did == ticket.did { host.refreshAvailability() } else { host.refreshContacts() }
    }

    func refresh(_ did: String) {
        guard let ticket = session.ticket(), let host else { return }
        serial += 1
        let request = serial
        reads[did] = request
        Task { [weak self] in
            guard let self else { return }
            defer { if self.reads[did] == request { self.reads[did] = nil } }
            let meta: AccountProfile?
            do { meta = try await host.profileWork(ticket) { try $0.accountProfile(did: did) } } catch { return }
            var fields: PeopleObject?
            if let meta {
                guard let parsed = PeopleJSON.parse(meta.fieldsJson)?.asObject else { return }
                fields = parsed
            }
            let prior = self.views[did]
            // Keep the decoded image only while the same hash stays selected (no flash back to initials on a field edit).
            let keep: UIImage? = (prior?.hash == meta?.pictureHash && meta?.pictureReady == true) ? prior?.image : nil
            self.publish(ticket, did, request, ProfileView(fields: fields, hash: meta?.pictureHash, ready: meta?.pictureReady == true, image: keep))
            guard let hash = meta?.pictureHash, meta?.pictureReady == true, keep == nil else { return }
            let image: UIImage? = (try? await host.profileWork(ticket) { node in
                try node.profilePicture(did: did).flatMap { UIImage(data: $0) }
            }) ?? nil
            let again: AccountProfile?
            do { again = try await host.profileWork(ticket) { try $0.accountProfile(did: did) } } catch { return }
            if again?.pictureHash != hash || again?.pictureReady != true {
                if self.session.accepts(ticket) { self.refresh(did) }
                return
            }
            self.publish(ticket, did, request, ProfileView(fields: fields, hash: hash, ready: true, image: image, broken: image == nil))
        }
    }

    private func publish(_ t: ProfileSession.Ticket, _ did: String, _ request: Int64, _ view: ProfileView) {
        if session.accepts(t) && reads[did] == request { views[did] = view }
    }

    /// Runs one write on a worker. Nil on success; otherwise the copy to show (what was typed stays).
    func apply(_ op: ProfileOp) async -> String? {
        if case .setAvailable(let available, _) = op, let problem = ProfilePolicy.availabilityProblem(available: available) { return problem }
        guard let ticket = session.ticket(), let host else { return Self.saveFailed }
        let result: Result<Void, any Swift.Error>
        do {
            try await host.profileWork(ticket) { node in
                switch op {
                case .setName(let n): try node.setName(name: n)
                case .setField(let k, let json): try node.setProfileField(key: k, jsonValue: json)
                case .setAvailable(let a, let until): try node.setAvailable(available: a, until: until.map { UInt64($0) })
                case .replacePicture(let png): try node.replaceProfilePicture(bytes: png)
                case .removePicture: try node.removeProfilePicture()
                }
            }
            result = .success(())
        } catch { result = .failure(error) }
        if !session.accepts(ticket) { return Self.accountChanged }
        switch result {
        case .success:
            refresh(ticket.did)
            switch op { case .setAvailable, .setName: host.refreshAvailability(); default: break }
            return nil
        case .failure:
            switch op {
            case .replacePicture: return tr("people_save_picture_failed")
            case .removePicture: return tr("people_remove_picture_failed")
            default: return Self.saveFailed
            }
        }
    }
}
