import SwiftUI
import Combine

/// What every group screen reads: the controller, the contact/block directory, the toast line and the background runner
/// (android `app.groups`, `app.contacts`, `app.blocks`, `say`, `rememberRunner`). One per node.
@MainActor
final class GroupsSession: ObservableObject {
    let ctl: GroupsController
    let dir: GroupsDirectory
    /// The line shown at the bottom ("You joined Hike."); nil when none.
    @Published private(set) var toast: String?
    /// The Shell may route messages into its own snackbar instead: set once, then the local toast stays quiet.
    var externalSay: ((String) -> Void)?
    private var bag: AnyCancellable?
    private var mirrors: AnyCancellable?
    private var toastTask: Task<Void, Never>?

    init(ctl: GroupsController, dir: GroupsDirectory) {
        self.ctl = ctl
        self.dir = dir
        // The controller publishes on the main queue after every state change; views watch the session only.
        bag = ctl.objectWillChange.sink { [weak self] _ in self?.objectWillChange.send() }
    }

    private static var byNode: [ObjectIdentifier: GroupsSession] = [:]
    static func shared(for node: Node, app: AppEnv) -> GroupsSession {
        let key = ObjectIdentifier(node)
        if let s = byNode[key] { return s }
        let dir = LoadedGroupsDirectory(node)
        let s = GroupsSession(ctl: GroupsHub.controller(for: node), dir: dir)
        // Rows read the People area's loaded contacts and blocks; a change to either redraws the group screens.
        s.mirrors = app.$contacts.combineLatest(app.$blocks).sink { [weak s, weak dir] contacts, blocks in
            dir?.update(contacts: contacts, blocks: blocks)
            s?.objectWillChange.send()
        }
        byNode[key] = s
        return s
    }

    var me: String { ctl.myDid }

    /// A DID as the user knows it: their contact name, else what the group calls them.
    func name(_ did: String, _ fallback: String) -> String { dir.name(of: did, fallback: fallback, me: me) }

    func say(_ text: String) {
        if let externalSay { externalSay(text); return }
        toast = text
        AccessibilityNotification.Announcement(text).post()
        toastTask?.cancel()
        toastTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 3_500_000_000)
            if !Task.isCancelled { self?.toast = nil }
        }
    }

    /// Runs a group action off the main thread, then hands its outcome back on it.
    func exec<T>(_ work: @escaping () -> GroupResult<T>, _ done: @escaping (GroupResult<T>) -> Void) {
        Task.detached(priority: .userInitiated) {
            let r = work()
            await MainActor.run { done(r) }
        }
    }

    /// Fire and forget: runs `work` off the main thread.
    func background(_ work: @escaping () -> Void) { Task.detached(priority: .userInitiated) { work() } }

    /// The usual outcome: say what the core's words are, nothing when there are none.
    var sayDone: (GroupResult<String>) -> Void {
        { [weak self] r in
            switch r {
            case .success(let s): if !s.isEmpty { self?.say(s) }
            case .failure(let e): self?.say(e.message.groupsIfBlank("Couldn’t do that"))
            }
        }
    }
}

/// The bottom line for `GroupsSession.say`: a calm capsule, 3.5 s. It overlays, so nothing moves.
struct GroupsToastHost: ViewModifier {
    @ObservedObject var session: GroupsSession
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    func body(content: Content) -> some View {
        content.overlay(alignment: .bottom) {
            if let t = session.toast {
                Text(t).font(TinType.bodyM).foregroundStyle(tin.bg).multilineTextAlignment(.leading)
                    .padding(.horizontal, 16).padding(.vertical, 12)
                    .background(tin.ink, in: RoundedRectangle(cornerRadius: 14))
                    .padding(.horizontal, 16).padding(.bottom, 16)
                    .transition(reduced ? .identity : .move(edge: .bottom).combined(with: .opacity))
                    .accessibilityHidden(true)
            }
        }
        .animation(reduced ? nil : GroupsMotion.easeOut(GroupsMotion.enter), value: session.toast)
    }
}
extension View { func groupsToasts(_ s: GroupsSession) -> some View { modifier(GroupsToastHost(session: s)) } }

/// Resolves the session from the app environment; shows `placeholder` until the vault is unlocked and the node exists.
struct GroupsSessionGate<Content: View>: View {
    @EnvironmentObject private var app: AppEnv
    @ViewBuilder var content: (GroupsSession) -> Content
    var body: some View {
        if let node = app.node { content(GroupsSession.shared(for: node, app: app)) } else { Color.clear }
    }
}
