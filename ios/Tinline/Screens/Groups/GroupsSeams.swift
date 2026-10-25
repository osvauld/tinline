import SwiftUI
import Combine

/*
 * What the Groups area needs from the other areas, as small protocols the Shell fills in:
 * the Calls area (group call entry points), the contact/block state, and the camera QR scanner.
 */

// ------------------------------------------------------------------ group calls (Calls area)

/// A group call running in a group, as far as the list knows (android GroupCallUi `live`).
struct GroupLiveCall: Equatable {
    var groupId: String
    /// Whether this device is in it.
    var joined: Bool
    var starterDid: String
}

/// Everything the group screens show about calls, in one value the Calls area publishes.
struct GroupCallSnapshot: Equatable {
    /// Calls running in my groups, by group id. Kept current by the call UI: Announced, Joined, Left and Ended all
    /// re-read or drop the group's call, so a call that ended never leaves a Join pill behind.
    var live: [String: GroupLiveCall] = [:]
    /// The group whose call this device is in, if any (and whether it is connected).
    var activeGroupId: String?
    /// "You're in the call" line: the timer once active, else the status ("Connecting...").
    var activeLine: String = ""
    /// This group's call log, merged into the thread by time (android `groupLog`).
    var history: [GroupCallRecord] = []
}

/// The group call entry points. The Calls area implements this; the Shell supplies it through
/// `.environment(\.groupCallLauncher, ...)`. The group screens never touch call state themselves.
protocol GroupCallLauncher: AnyObject {
    var snapshot: GroupCallSnapshot { get }
    /// Fires after `snapshot` changes.
    var snapshots: AnyPublisher<GroupCallSnapshot, Never> { get }
    /// Re-reads the running call of a group from the core (a core call: background).
    func refreshLive(groupId: String)
    /// Start a call in the group, or join the one that runs. Errors come back in words through `onError`.
    func startOrJoin(groupId: String, onError: @escaping (String) -> Void)
    /// Return to the call this device is already in (the full call screen).
    func restore()
}

/// Used until the Shell supplies the Calls area's launcher: no calls, honest words on tap.
// TODO(ios-device): replaced by the Calls area's implementation in the Shell.
final class NoGroupCallLauncher: GroupCallLauncher {
    let snapshot = GroupCallSnapshot()
    var snapshots: AnyPublisher<GroupCallSnapshot, Never> { Empty().eraseToAnyPublisher() }
    func refreshLive(groupId: String) {}
    func startOrJoin(groupId: String, onError: @escaping (String) -> Void) { onError(tr("group_calls_unavailable")) }
    func restore() {}
}

private struct GroupCallLauncherKey: EnvironmentKey { static let defaultValue: GroupCallLauncher = NoGroupCallLauncher() }
extension EnvironmentValues {
    var groupCallLauncher: GroupCallLauncher { get { self[GroupCallLauncherKey.self] } set { self[GroupCallLauncherKey.self] = newValue } }
}

/// Mirrors a launcher's snapshot for SwiftUI (an existential cannot be an `@ObservedObject`).
final class GroupCallWatcher: ObservableObject {
    @Published private(set) var snapshot: GroupCallSnapshot
    private var bag: AnyCancellable?
    private var launcher: GroupCallLauncher

    init(_ launcher: GroupCallLauncher) {
        self.launcher = launcher
        snapshot = launcher.snapshot
        bind()
    }

    func use(_ l: GroupCallLauncher) {
        guard l !== launcher else { return }
        launcher = l; snapshot = l.snapshot; bind()
    }

    private func bind() {
        bag = launcher.snapshots.receive(on: DispatchQueue.main).sink { [weak self] s in self?.snapshot = s }
    }
}

// ------------------------------------------------------------------ contacts and blocks

struct GroupsContact: Equatable { var did: String; var name: String }

/// The contact list and the block state the group screens read while rendering (android `app.contacts`, `app.blocks`).
/// Cheap reads: the node's in-memory state, never a network call.
protocol GroupsDirectory: AnyObject {
    var contacts: [GroupsContact] { get }
    func blockedByMe(_ did: String) -> Bool
    /// They blocked us (we hold their signed notice).
    func blocksMe(_ did: String) -> Bool
    /// The profile name of this account, for "You" lines that name me.
    var myName: String? { get }
}

final class NodeGroupsDirectory: GroupsDirectory {
    private let node: Node
    init(_ node: Node) { self.node = node }
    var contacts: [GroupsContact] { node.contacts().map { GroupsContact(did: $0.did, name: displayName(alias: $0.alias, name: $0.name)) } }
    func blockedByMe(_ did: String) -> Bool { node.blockState(did: did).blockedByMe }
    func blocksMe(_ did: String) -> Bool { node.blockState(did: did).blockedMe != nil }
    var myName: String? { node.profile()?.name }
}

/// The word for the user in group lines; some code compares against it, so always read it from here.
var groupsYou: String { tr("group_you") }

extension GroupsDirectory {
    /// A DID as the user knows it: their contact name, else what the group calls them (android `nameOf`).
    func name(of did: String, fallback: String, me: String) -> String {
        if did == me { return groupsYou }
        if let c = contacts.first(where: { $0.did == did }) { return c.name }
        return fallback.groupsIfBlank(tr("group_someone"))
    }
}

// ------------------------------------------------------------------ QR scanning (camera)

/// Scans a QR code with the camera (android QrCameraBox). The Shell or Components area supplies it.
protocol GroupsQRScanner {
    func scannerView(prompt: String, onCode: @escaping (String) -> Void) -> AnyView
}

/// A placeholder that says so.
// TODO(ios-device): AVCaptureSession + AVCaptureMetadataOutput (.qr) behind this protocol; needs NSCameraUsageDescription.
struct UnavailableGroupsQRScanner: GroupsQRScanner {
    func scannerView(prompt: String, onCode: @escaping (String) -> Void) -> AnyView {
        AnyView(Text(tr("group_scan_unavailable")).multilineTextAlignment(.center).padding(24))
    }
}

private struct GroupsQRScannerKey: EnvironmentKey { static let defaultValue: GroupsQRScanner = UnavailableGroupsQRScanner() }
extension EnvironmentValues {
    var groupsQRScanner: GroupsQRScanner { get { self[GroupsQRScannerKey.self] } set { self[GroupsQRScannerKey.self] = newValue } }
}
