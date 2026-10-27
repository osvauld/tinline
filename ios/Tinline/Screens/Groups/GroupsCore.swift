import Foundation
import os

/*
 * Group chat on the core's real API (G1: crates/core/src/group/api.rs, docs/group-core.md).
 * Port of android GroupsCore.kt. `GroupsController` is what the screens read; every core call goes through
 * `GroupApi` so the flows are tested without a node (GroupsCoreTests) on a stand-in.
 *
 * Decisions (rotation-open-decisions.md section 0): 3 admin flags remove a member (U9/U15) and the removed member
 * sees the signed flags; a post written before a removal still shows, marked (U11); only hosts know member
 * endpoints (U7), so our posts honestly say "Waiting for a host" until a host acknowledges them.
 * G3: admins invite contacts directly (the invitation travels over the 1:1 channel; blocked contacts are
 * never offered); invite links are short (tinline://g/...) and the core fetches the invite from a host.
 * There is no "add my other device" step: own-device sync (G2) brings groups to linked devices.
 */

/// The generated bindings declare their error enum as plain `Error` in this module, which shadows Swift's `Error`
/// protocol here; use `Swift.Error` for the protocol and this alias for the core's cases (`.Rejected(message:)`...).
typealias GroupsCoreError = Error

/// The core's group calls. They are blocking (create, invite, join, accept host reach the network): call off the main thread.
protocol GroupApi {
    func groups() throws -> [GroupSummary]
    func detail(_ id: String) throws -> GroupDetail
    func messages(_ id: String, before: String?, limit: Int) throws -> GroupPage
    func markRead(_ id: String) throws
    /// Signed member receipts of my posts with `at` in [fromMs, toMs] (the group tick and n/N).
    func receipts(_ id: String, fromMs: UInt64, toMs: UInt64) throws -> [GroupReceiptStatus]
    func create(_ name: String) throws -> String
    func invite(_ id: String) throws -> String
    /// Fetches from the host the link names: network, background threads only.
    func peek(_ token: String) -> GroupInvitePeek?
    func join(_ token: String) throws -> String
    func post(_ id: String, text: String, replyTo: String?) throws -> GroupMessage
    func flag(_ id: String, did: String) throws
    func setAdmin(_ id: String, did: String, admin: Bool) throws
    func leave(_ id: String) throws
    func appointHost(_ id: String, did: String, deviceId: String) throws
    func revokeHost(_ id: String, did: String, deviceId: String) throws
    func acceptHost(_ id: String, accept: Bool) throws
    func deviceId() throws -> String
    // Chat requests through the group's hosts (G2, U3).
    func chatRequest(_ id: String, did: String) throws -> GroupChatRequest
    func requestsIncoming() throws -> [GroupChatRequest]
    func requestsOutgoing() throws -> [GroupChatRequest]
    func chatAccept(_ requestId: String) throws -> GroupChatRequest
    func chatDecline(_ requestId: String) throws -> GroupChatRequest
    /// The personal block (P5), for "Block" on an incoming request.
    func block(_ did: String) throws
    // Direct invitations to contacts (G3).
    func inviteContacts(_ id: String, dids: [String]) throws -> [GroupInvitation]
    func invitationsIncoming() throws -> [GroupInvitation]
    func invitationsOutgoing(_ id: String) throws -> [GroupInvitation]
    func invitationAccept(_ invitationId: String) throws -> GroupInvitation
    func invitationDecline(_ invitationId: String) throws -> GroupInvitation
    // Call hosts (a separate permit from chat hosting; the device accepts it itself).
    func appointCallHost(_ id: String, did: String, deviceId: String) throws
    func revokeCallHost(_ id: String, did: String, deviceId: String) throws
    func acceptCallHost(_ id: String, accept: Bool) throws
    // Voice posts (the 1:1 recording, carried as a picture is): load one now; the opened file once it is here
    // (blocking the first time).
    func fetchVoice(_ id: String, messageId: String) throws
    func voicePath(_ id: String, messageId: String) -> String?
}

extension GroupApi {
    // Defaults keep test stand-ins compiling (android: default bodies in the interface).
    func receipts(_ id: String, fromMs: UInt64, toMs: UInt64) throws -> [GroupReceiptStatus] { [] }
    func appointCallHost(_ id: String, did: String, deviceId: String) throws { throw GroupsUnsupported() }
    func revokeCallHost(_ id: String, did: String, deviceId: String) throws { throw GroupsUnsupported() }
    func acceptCallHost(_ id: String, accept: Bool) throws { throw GroupsUnsupported() }
    func fetchVoice(_ id: String, messageId: String) throws {}
    func voicePath(_ id: String, messageId: String) -> String? { nil }
}

struct GroupsUnsupported: Swift.Error {}

/// `GroupApi` on the real node. Method names are the bindings' (ios/build/generated/p2pcore.swift).
struct NodeGroupApi: GroupApi {
    let node: () -> Node
    func groups() throws -> [GroupSummary] { try node().groups() }
    func detail(_ id: String) throws -> GroupDetail { try node().groupDetail(groupId: id) }
    func messages(_ id: String, before: String?, limit: Int) throws -> GroupPage { try node().groupMessages(groupId: id, before: before, limit: UInt32(limit)) }
    func markRead(_ id: String) throws { try node().groupMarkRead(groupId: id) }
    func receipts(_ id: String, fromMs: UInt64, toMs: UInt64) throws -> [GroupReceiptStatus] { try node().groupReceipts(groupId: id, fromMs: fromMs, toMs: toMs) }
    func create(_ name: String) throws -> String { try node().groupCreate(name: name) }
    func invite(_ id: String) throws -> String { try node().groupInvite(groupId: id) }
    func peek(_ token: String) -> GroupInvitePeek? { node().groupPeekInvite(token: token) }
    func join(_ token: String) throws -> String { try node().groupJoin(token: token) }
    func post(_ id: String, text: String, replyTo: String?) throws -> GroupMessage { try node().groupPost(groupId: id, text: text, replyTo: replyTo) }
    func flag(_ id: String, did: String) throws { try node().groupFlag(groupId: id, memberDid: did) }
    func setAdmin(_ id: String, did: String, admin: Bool) throws { try node().groupSetAdmin(groupId: id, memberDid: did, admin: admin) }
    func leave(_ id: String) throws { try node().groupLeave(groupId: id) }
    func appointHost(_ id: String, did: String, deviceId: String) throws { try node().groupAppointHost(groupId: id, did: did, deviceId: deviceId) }
    func revokeHost(_ id: String, did: String, deviceId: String) throws { try node().groupRevokeHost(groupId: id, did: did, deviceId: deviceId) }
    func acceptHost(_ id: String, accept: Bool) throws { try node().groupAcceptHost(groupId: id, accept: accept) }
    func deviceId() throws -> String { try node().groupDeviceId() }
    func chatRequest(_ id: String, did: String) throws -> GroupChatRequest { try node().groupChatRequest(groupId: id, did: did) }
    func requestsIncoming() throws -> [GroupChatRequest] { try node().groupChatRequestsIncoming() }
    func requestsOutgoing() throws -> [GroupChatRequest] { try node().groupChatRequestsOutgoing() }
    func chatAccept(_ requestId: String) throws -> GroupChatRequest { try node().groupChatAccept(requestId: requestId) }
    func chatDecline(_ requestId: String) throws -> GroupChatRequest { try node().groupChatDecline(requestId: requestId) }
    func block(_ did: String) throws { try node().block(did: did) }
    func inviteContacts(_ id: String, dids: [String]) throws -> [GroupInvitation] { try node().groupInviteContacts(groupId: id, dids: dids) }
    func invitationsIncoming() throws -> [GroupInvitation] { try node().groupInvitationsIncoming() }
    func invitationsOutgoing(_ id: String) throws -> [GroupInvitation] { try node().groupInvitationsOutgoing(groupId: id) }
    func invitationAccept(_ invitationId: String) throws -> GroupInvitation { try node().groupInvitationAccept(id: invitationId) }
    func invitationDecline(_ invitationId: String) throws -> GroupInvitation { try node().groupInvitationDecline(id: invitationId) }
    func appointCallHost(_ id: String, did: String, deviceId: String) throws { try node().groupAppointCallHost(groupId: id, did: did, deviceId: deviceId) }
    func revokeCallHost(_ id: String, did: String, deviceId: String) throws { try node().groupRevokeCallHost(groupId: id, did: did, deviceId: deviceId) }
    func acceptCallHost(_ id: String, accept: Bool) throws { try node().groupAcceptCallHost(groupId: id, accept: accept) }
    func fetchVoice(_ id: String, messageId: String) throws { try node().groupFetchVoice(groupId: id, messageId: messageId) }
    func voicePath(_ id: String, messageId: String) -> String? { node().groupVoicePath(groupId: id, messageId: messageId) }
}

/// A contact as the "Add members" picker sees it.
struct InviteCandidate: Equatable { var did: String; var name: String }

/// One row of the picker: who, and why they can't be picked (nil: pickable).
struct PickRow: Equatable { var did: String; var name: String; var why: String?; var invited: GroupInvitation? }

/// The group on screen: its detail and the loaded window of messages (oldest first).
struct OpenGroup: Equatable {
    var id: String
    var detail: GroupDetail
    var messages: [GroupMessage]
    var older: String?
    /// Signed member receipts of my loaded posts, by message id.
    var receipts: [String: GroupReceiptStatus] = [:]
}

/// What a member's sheet offers us, in order.
enum MemberAction: Equatable {
    /// Message privately: their chat if a contact, else a chat request (U3).
    case message
    case makeAdmin
    case revokeAdmin
    case appoint(deviceId: String)
    case revoke(deviceId: String)
    case flag
}

/// A failed group action: `message` is already in the user's words.
struct GroupFailure: Swift.Error, LocalizedError, Equatable {
    var message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

typealias GroupResult<T> = Result<T, GroupFailure>

extension String {
    /// Kotlin `isBlank()`.
    var groupsIsBlank: Bool { trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    /// Kotlin `ifBlank`.
    func groupsIfBlank(_ other: String) -> String { groupsIsBlank ? other : self }
}

/// First word of a name (android `firstName`).
func groupsFirstName(_ name: String) -> String {
    let t = name.trimmingCharacters(in: .whitespacesAndNewlines)
    let first = t.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init) ?? ""
    return first.groupsIsBlank ? name : first
}

enum GroupText {
    static let flagsToRemove = 3
    static let page = 100

    static func active(_ r: GroupRole) -> Bool { r == .owner || r == .admin || r == .member }
    static func isAdmin(_ r: GroupRole) -> Bool { r == .owner || r == .admin }

    private static func someone(_ name: String) -> String { name.groupsIfBlank(tr("group_someone")) }
    /// A sentence about a person whose name may be unknown: the named sentence, or its whole-sentence twin.
    static func named(_ name: String, _ named: String, _ unnamed: String) -> String {
        name.groupsIsBlank ? tr(unnamed) : tr(named, groupsFirstName(name))
    }
    static func shortDev(_ dev: String) -> String { String(dev.prefix(8)) }

    /// Who a system line is about: the user, a named member, or a member whose name is unknown. Each combination has its
    /// own whole sentence, so no stand-in word is ever slotted into one.
    private enum Who {
        case you, named(String), unknown
        init(_ did: String, _ name: String, me: String) {
            self = did == me ? .you : (name.groupsIsBlank ? .unknown : .named(name))
        }
    }

    /// A system line in plain words. `me` is our DID.
    static func sysText(_ m: GroupMessage, me: String) -> String {
        let by = Who(m.authorDid, m.authorName, me: me)
        let sub = Who(m.subjectDid, m.subjectName, me: me)
        switch m.kind {
        case .post: return m.text
        case .joined:
            switch sub {
            case .you: return tr("group_sys_joined_you")
            case .named(let n): return tr("group_sys_joined_named", n)
            case .unknown: return tr("group_sys_joined_someone")
            }
        case .left:
            switch sub {
            case .you: return tr("group_sys_left_you")
            case .named(let n): return tr("group_sys_left_named", n)
            case .unknown: return tr("group_sys_left_someone")
            }
        case .removed:
            switch sub {
            case .you: return trn("group_sys_removed_you", flagsToRemove)
            case .named(let n): return trn("group_sys_removed", flagsToRemove, n)
            case .unknown: return trn("group_sys_removed_someone", flagsToRemove)
            }
        case .adminGranted:
            switch (by, sub) {
            case (.you, .you): return tr("group_sys_admin_you_you")
            case (.you, .named(let n)): return tr("group_sys_admin_you_named", n)
            case (.you, .unknown): return tr("group_sys_admin_you_someone")
            case (.named(let b), .you): return tr("group_sys_admin_named_you", b)
            case (.named(let b), .named(let n)): return tr("group_sys_admin_named_named", b, n)
            case (.named(let b), .unknown): return tr("group_sys_admin_named_someone", b)
            case (.unknown, .you): return tr("group_sys_admin_someone_you")
            case (.unknown, .named(let n)): return tr("group_sys_admin_someone_named", n)
            case (.unknown, .unknown): return tr("group_sys_admin_someone_someone")
            }
        case .adminRevoked:
            switch (by, sub) {
            case (.you, .you): return tr("group_sys_unadmin_you_you")
            case (.you, .named(let n)): return tr("group_sys_unadmin_you_named", n)
            case (.you, .unknown): return tr("group_sys_unadmin_you_someone")
            case (.named(let b), .you): return tr("group_sys_unadmin_named_you", b)
            case (.named(let b), .named(let n)): return tr("group_sys_unadmin_named_named", b, n)
            case (.named(let b), .unknown): return tr("group_sys_unadmin_named_someone", b)
            case (.unknown, .you): return tr("group_sys_unadmin_someone_you")
            case (.unknown, .named(let n)): return tr("group_sys_unadmin_someone_named", n)
            case (.unknown, .unknown): return tr("group_sys_unadmin_someone_someone")
            }
        case .hostAdded:
            switch sub {
            case .you: return tr("group_sys_host_added_you")
            case .named(let n): return tr("group_sys_host_added", n)
            case .unknown: return tr("group_sys_host_added_someone")
            }
        case .hostRemoved:
            switch sub {
            case .you: return tr("group_sys_host_removed_you")
            case .named(let n): return tr("group_sys_host_removed", n)
            case .unknown: return tr("group_sys_host_removed_someone")
            }
        }
    }

    /// The spoken description of a member row: one whole sentence per role and hosting combination.
    static func memberDesc(_ name: String, owner: Bool, admin: Bool, host: Bool, callHost: Bool) -> String {
        switch (owner ? 0 : (admin ? 1 : 2), host, callHost) {
        case (0, false, false): return tr("group_member_desc_owner", name)
        case (0, false, true): return tr("group_member_desc_owner_callhost", name)
        case (0, true, false): return tr("group_member_desc_owner_host", name)
        case (0, true, true): return tr("group_member_desc_owner_host_callhost", name)
        case (1, false, false): return tr("group_member_desc_admin", name)
        case (1, false, true): return tr("group_member_desc_admin_callhost", name)
        case (1, true, false): return tr("group_member_desc_admin_host", name)
        case (1, true, true): return tr("group_member_desc_admin_host_callhost", name)
        case (2, false, false): return tr("group_member_desc_member", name)
        case (2, false, true): return tr("group_member_desc_member_callhost", name)
        case (2, true, false): return tr("group_member_desc_member_host", name)
        case (2, true, true): return tr("group_member_desc_member_host_callhost", name)
        default: return tr("group_member_desc_member", name)
        }
    }

    /// The list row's second line.
    static func preview(_ s: GroupSummary, me: String) -> String {
        switch s.myRole {
        case .removed: return tr("group_preview_removed")
        case .left: return tr("group_info_hero_left")
        default:
            guard let m = s.last else { return "" }
            if m.kind != .post { return sysText(m, me: me) }
            if m.outgoing { return tr("group_preview_you", postLine(m)) }
            if m.authorName.groupsIsBlank { return tr("group_preview_someone", postLine(m)) }
            return tr("group_preview_author", groupsFirstName(m.authorName), postLine(m))
        }
    }

    /// A post in one line: its text, or "Voice message" for a voice post (android `postLine`).
    static func postLine(_ m: GroupMessage) -> String { m.voice != nil ? tr("chat_preview_voice") : m.text }

    /// The header's sub-line.
    static func status(_ d: GroupDetail) -> String {
        switch d.myRole {
        case .removed: return tr("group_preview_removed")
        case .left: return tr("group_info_hero_left")
        default: break
        }
        let n = d.members.count
        let members = trn("group_members_count", n)
        if d.amHost { return tr("group_status_this_device", members) }
        if d.hosts.isEmpty { return tr("group_status_no_host", members) }
        if !d.hosts.contains(where: { $0.online }) { return tr("group_status_waiting", members) }
        return tr("group_status_hosts", members, trn("group_hosts_count", d.hosts.count))
    }

    static func waitingForHost(_ d: GroupDetail) -> Bool { active(d.myRole) && !d.amHost && !d.hosts.contains(where: { $0.online }) }

    /// The small line under one of our posts, or nil when delivered.
    static func deliveryWord(_ m: GroupMessage) -> String? {
        m.outgoing && m.kind == .post && m.delivery == .waiting ? tr("group_waiting_for_host") : nil
    }

    /// A sent request's state line and whether it is bad news. A block is visible (U5 revised, P5).
    static func sentLine(_ r: GroupChatRequest) -> (String, Bool) {
        let f = groupsFirstName(r.peerName)
        let named = !r.peerName.groupsIsBlank
        switch r.state {
        case .pending: return (named ? tr("group_req_pending", f, r.groupName) : tr("group_req_pending_unnamed", r.groupName), false)
        case .accepted:
            if r.contactAdded { return (named ? tr("group_req_accepted_msgs", f) : tr("group_req_accepted_msgs_unnamed"), false) }
            return (named ? tr("group_req_accepted_connecting", f) : tr("group_req_accepted_connecting_unnamed"), false)
        case .declined: return (named ? tr("group_req_declined", f) : tr("group_req_declined_unnamed"), true)
        case .blocked: return (named ? tr("group_req_blocked", f) : tr("group_req_blocked_unnamed"), true)
        }
    }

    /// Who asked this device to host `d`: nil when nobody did, an empty string when an admin asked whose name is unknown.
    static func hostAsker(_ d: GroupDetail) -> String? {
        if !d.hostRequestedHere || !active(d.myRole) { return nil }
        let by = d.hostRequests.first(where: { $0.isMe })?.appointedBy
        if let b = by, let name = d.members.first(where: { $0.did == b })?.name, !name.groupsIsBlank { return name }
        return ""
    }

    /// Unread posts in groups we are still in, plus invitations waiting for an answer: the Groups tab's badge.
    static func badge(_ list: [GroupSummary], invitations: [GroupInvitation] = []) -> Int {
        list.filter { active($0.myRole) }.reduce(0) { $0 + Int($1.unread) } + pendingInvitations(invitations).count
    }

    /// Incoming invitations still to answer (Join / Decline).
    static func pendingInvitations(_ invitations: [GroupInvitation]) -> [GroupInvitation] {
        invitations.filter { !$0.outgoing && $0.state == .pending }
    }

    /// The "Add members" picker: blocked contacts (either direction, P5) are not offered at all; members show
    /// as already in; someone with a live invitation shows its state and can't be picked again.
    static func pickRows(_ contacts: [InviteCandidate], _ d: GroupDetail, _ sent: [GroupInvitation],
                         blockedByMe: (String) -> Bool, blocksMe: (String) -> Bool) -> [PickRow] {
        let rows: [PickRow] = contacts.filter { !blockedByMe($0.did) && !blocksMe($0.did) }.map { c in
            let inv = sent.filter { $0.inviteeDid == c.did }.max(by: { $0.at < $1.at })
            let why: String?
            if d.members.contains(where: { $0.did == c.did }) { why = tr("group_pick_already_member") }
            else if let i = inv, i.state == .pending { why = tr("group_pick_invited_waiting") }
            else { why = nil }
            return PickRow(did: c.did, name: c.name, why: why, invited: inv)
        }
        // Pickable first, then by name (stable: ties keep the contacts' order).
        return rows.enumerated().sorted { a, b in
            let ka = (a.element.why != nil, a.element.name.lowercased()), kb = (b.element.why != nil, b.element.name.lowercased())
            if ka.0 != kb.0 { return !ka.0 }
            if ka.1 != kb.1 { return ka.1 < kb.1 }
            return a.offset < b.offset
        }.map { $0.element }
    }

    /// An invitation we sent, in words, and whether it is bad news.
    static func sentInvitationLine(_ i: GroupInvitation) -> (String, Bool) {
        switch i.state {
        case .pending: return (i.delivered ? tr("group_inv_waiting") : tr("group_inv_undelivered"), false)
        case .joined: return (tr("group_inv_joined"), false)
        case .declined: return (tr("group_inv_declined"), true)
        case .expired: return (tr("group_inv_expired"), true)
        case .blocked: return (tr("group_inv_blocked"), true)
        }
    }

    static func actions(_ d: GroupDetail, _ m: GroupMember) -> [MemberAction] {
        if m.isMe || !active(d.myRole) { return [] }
        var out: [MemberAction] = [.message]
        if d.myRole == .owner && !m.isOwner { out.append(m.isAdmin ? .revokeAdmin : .makeAdmin) }
        if isAdmin(d.myRole) {
            for dev in m.deviceIds {
                out.append(d.hosts.contains(where: { $0.did == m.did && $0.deviceId == dev }) ? .revoke(deviceId: dev) : .appoint(deviceId: dev))
            }
            if !m.isOwner { out.append(.flag) }
        }
        return out
    }

    /// Which flag ours would be, and the dialog's sentence.
    static func flagCopy(_ name: String, flagsSoFar: Int) -> (Int, String) {
        let f = groupsFirstName(name)
        let yours = flagsSoFar + 1
        if yours >= flagsToRemove {
            return (yours, tr("group_flag_copy_final", String(yours), String(flagsToRemove), f))
        }
        return (yours, tr("group_flag_copy", String(yours), String(flagsToRemove), f))
    }

    /// Words for a failed group call (the core's flat errors carry "they refused: <reason>").
    static func error(_ e: Swift.Error) -> String {
        if let g = e as? GroupFailure { return g.message }
        guard let core = e as? GroupsCoreError else {
            return e.localizedDescription.groupsIfBlank(tr("group_error_generic"))
        }
        switch core {
        case .Rejected(let message):
            var r = message
            if r.hasPrefix("they refused: ") { r.removeFirst("they refused: ".count) }
            r = r.trimmingCharacters(in: .whitespacesAndNewlines)
            switch r {
            case "removed": return tr("group_error_removed")
            case "not a member": return tr("group_error_not_member")
            case "already a contact": return tr("group_error_already_contact")
            case "you block them": return tr("group_error_you_block")
            case "blocked": return tr("group_error_they_block")
            case "unknown or expired invite code": return tr("group_error_invite_invalid")
            case "": return tr("group_error_refused")
            default:
                var s = r.prefix(1).uppercased() + r.dropFirst()
                while s.hasSuffix(".") { s.removeLast() }
                return s + "."
            }
        case .Timeout: return tr("group_error_timeout")
        case .Locked: return tr("group_error_locked")
        case .NotFound: return tr("group_error_not_found")
        case .NoIdentity(let m), .HaveIdentity(let m), .AccountExists(let m), .InCall(let m), .BadPhrase(let m), .WrongPassphrase(let m),
             .WeakPassphrase(let m), .NotStarted(let m), .Busy(let m), .Protocol(let m), .Net(let m), .Io(let m), .Audio(let m):
            return m.groupsIfBlank(tr("group_error_generic"))
        }
    }
}

/// Inserts or replaces by id, keeping the core's (at, id) order.
func upsertGroupMessage(_ list: [GroupMessage], _ m: GroupMessage) -> [GroupMessage] {
    var out = list
    if let i = out.firstIndex(where: { $0.id == m.id }) { out[i] = m; return out }
    let at = out.firstIndex(where: { $0.at > m.at || ($0.at == m.at && $0.id > m.id) }) ?? out.count
    out.insert(m, at: at)
    return out
}

/// Group invite links (G3): `tinline://g/<code>`; the bare code is accepted too.
enum GroupLinks {
    static let prefix = "tinline://g/"
    /// Whether `text` looks like a group invite link (any case of the scheme).
    static func isLink(_ text: String?) -> Bool {
        guard let t = text?.trimmingCharacters(in: .whitespacesAndNewlines) else { return false }
        return t.count > prefix.count && t.lowercased().hasPrefix(prefix)
    }
}

/// A tapped tinline://g/ link waits here until the app is unlocked, then opens Join (the Shell calls `take(url:)` from
/// `onOpenURL` and reads `link` once unlocked).
final class GroupLinkInbox: ObservableObject {
    static let shared = GroupLinkInbox()
    @Published var link: String?
    func take(url: URL) {
        let s = url.absoluteString
        if GroupLinks.isLink(s) { link = s }
    }
}

/// Which group chat is on screen right now (android `OpenChat.group`), so the Shell can keep its posts from notifying.
@MainActor
enum GroupsOpenChat { static var current: String? }

private let groupsLog = Logger(subsystem: "com.osvauld.tinline", category: "groups")

/// Guard for the blocking group calls: logs when one runs on the main thread, where it froze the group screen for most of
/// a second. Silent under XCTest (its tests run on the main thread).
func groupsOffMain(_ what: String) {
    if Thread.isMainThread && ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] == nil {
        groupsLog.error("\(what, privacy: .public) on the main thread: move it to a background task")
    }
}

/// The groups state the screens read, refreshed from the core on its events (`GroupEvents`, `GroupRequestEvents`,
/// `GroupInvitationEvents`). Reads are in-memory in the core; actions return a `Result` with what to tell the user and
/// must be called off the main thread.
///
/// Android holds this state in StateFlows and a monitor. Here the fields sit behind a short lock (`state`) and the SwiftUI
/// side is told with `objectWillChange` on the main queue; the long-running calls hold `monitor` (recursive) the way
/// `@Synchronized` did. `close` only ever takes `state`, never `monitor` (see its comment).
final class GroupsController: ObservableObject, GroupEvents, GroupRequestEvents, GroupInvitationEvents, @unchecked Sendable {
    private let api: GroupApi
    private let me: () -> String?
    private let monitor = NSRecursiveLock()
    private let state = NSLock()

    private var _incoming: [GroupChatRequest] = []
    private var _outgoing: [GroupChatRequest] = []
    private var _notice: String?
    private var _list: [GroupSummary] = []
    private var _open: OpenGroup?
    private var _changes: Int64 = 0
    private var _myDevice = ""
    private var _invitations: [GroupInvitation] = []
    private var _sent: [GroupInvitation] = []
    /// The group a screen shows (set by `openGroup`, cleared by `close`).
    private var shown: String?

    /// Set by the app: a post arrived (notifications; called on a core thread).
    var onPost: ((GroupMessage) -> Void)?
    /// Set by the app: an invitation arrived or changed (notifications; called on a core thread).
    var onInvitation: ((GroupInvitation) -> Void)?

    init(api: GroupApi, me: @escaping () -> String?) { self.api = api; self.me = me }

    private func read<T>(_ f: () -> T) -> T { state.lock(); defer { state.unlock() }; return f() }
    private func write(_ f: () -> Void) {
        state.lock(); f(); state.unlock()
        DispatchQueue.main.async { [weak self] in self?.objectWillChange.send() }
    }

    /// Chat requests to us (blocked senders are never listed), newest first.
    var incoming: [GroupChatRequest] { read { _incoming } }
    var outgoing: [GroupChatRequest] { read { _outgoing } }
    /// Something to tell the user once ("Leo accepted your request"); the UI clears it.
    var notice: String? { read { _notice } }
    func clearNotice() { write { _notice = nil } }
    var list: [GroupSummary] { read { _list } }
    /// The group a screen has open (not Swift's `open` keyword: android `open`).
    var opened: OpenGroup? { read { _open } }
    /// Bumped on every group change (receipts included): what the chat list's marks re-read on.
    var changes: Int64 { read { _changes } }
    var myDevice: String { read { _myDevice } }
    /// Invitations to us, newest first (blocked inviters are never listed).
    var invitations: [GroupInvitation] { read { _invitations } }
    /// Invitations we sent for the open group.
    var sent: [GroupInvitation] { read { _sent } }

    /// Read during rendering, so it is cached after the first non-empty answer (`me` may be a core call).
    var myDid: String {
        if let d = read({ _myDid }) { return d }
        let d = me() ?? ""
        if !d.isEmpty { state.lock(); _myDid = d; state.unlock() }
        return d
    }
    private var _myDid: String?

    private func groupsOrKeep() { let l = (try? api.groups()) ?? read { _list }; write { _list = l } }

    func refresh() {
        groupsOffMain("GroupsController.refresh")
        monitor.lock(); defer { monitor.unlock() }
        // One Node serves every account on iOS, so who "You" is can change between refreshes (unlock, account switch).
        write { _myDid = nil; _myDevice = "" }
        // A node without groups (or locked) answers with an error: an empty list.
        let l = (try? api.groups()) ?? []
        write { _list = l }
        if myDevice.isEmpty { let d = (try? api.deviceId()) ?? ""; write { _myDevice = d } }
        reloadRequests()
        reloadOpen()
        reloadInvitations()
    }

    /// Blocking, off the main thread only: marks read (the core signs read receipts) and loads the page with
    /// its receipt statuses (verifies every receipt signature of the days in range).
    func openGroup(_ id: String) {
        groupsOffMain("GroupsController.openGroup")
        monitor.lock(); defer { monitor.unlock() }
        write { shown = id; _open = nil }
        try? api.markRead(id)
        load(id, want: GroupText.page)
        groupsOrKeep()
        reloadInvitations()
    }

    /// The chat screen went away. Called on the main thread (onDisappear), so it must never take `monitor`:
    /// `openGroup`, `refresh` and the core's group events hold it across markRead and receipt verification
    /// (most of a second on a phone), and the UI thread would freeze waiting. Lock-free of it instead: `load`
    /// re-checks `shown` after publishing, so a page that finishes after this is dropped.
    func close(_ id: String) {
        write {
            if shown == id { shown = nil }
            if _open?.id == id { _open = nil }
        }
    }

    private func load(_ id: String, want: Int) {
        var loaded: OpenGroup?
        do {
            let d = try api.detail(id)
            let p = try api.messages(id, before: nil, limit: want)
            loaded = OpenGroup(id: id, detail: d, messages: p.messages, older: p.older, receipts: receiptsOf(id, p.messages))
        } catch { loaded = nil }
        write {
            _open = loaded
            // A close() that ran meanwhile wins (it clears `shown` before it clears `_open`).
            if shown != id, _open?.id == id { _open = nil }
        }
    }

    /// One core read per day doc in the range of my loaded posts; none if it fails (ticks stay one).
    private func receiptsOf(_ id: String, _ messages: [GroupMessage]) -> [String: GroupReceiptStatus] {
        let mine = messages.filter { $0.outgoing }
        guard let lo = mine.map({ $0.at }).min(), let hi = mine.map({ $0.at }).max() else { return [:] }
        guard let r = try? api.receipts(id, fromMs: lo, toMs: hi) else { return [:] }
        return Dictionary(r.map { ($0.messageId, $0) }, uniquingKeysWith: { _, b in b })
    }

    private func reloadOpen() {
        guard let o = opened else { return }
        load(o.id, want: max(o.messages.count, GroupText.page))
    }

    func older() {
        monitor.lock(); defer { monitor.unlock() }
        guard let o = opened, let before = o.older else { return }
        guard let p = try? api.messages(o.id, before: before, limit: GroupText.page) else { return }
        write {
            guard var cur = _open, cur.id == o.id else { return }
            let have = Set(cur.messages.map { $0.id })
            cur.messages = p.messages.filter { !have.contains($0.id) } + cur.messages
            cur.older = p.older
            _open = cur
        }
    }

    func peek(_ token: String) -> GroupInvitePeek? {
        token.groupsIsBlank ? nil : api.peek(token.trimmingCharacters(in: .whitespacesAndNewlines))
    }

    private func failed<T>(_ e: Swift.Error) -> GroupResult<T> { .failure(GroupFailure(GroupText.error(e))) }

    func post(_ id: String, text: String, replyTo: String?) -> GroupResult<GroupMessage> {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if t.isEmpty { return .failure(GroupFailure(tr("group_err_type_message"))) }
        do {
            let m = try api.post(id, text: t, replyTo: replyTo)
            monitor.lock(); defer { monitor.unlock() }
            write { if let o = _open, o.id == id { var n = o; n.messages = upsertGroupMessage(o.messages, m); _open = n } }
            groupsOrKeep()
            return .success(m)
        } catch { return failed(error) }
    }

    /// Loads a voice post's recording now (tap, or try again); `onGroupImage` reports it. Off the main thread.
    func fetchVoice(_ id: String, messageId: String) -> GroupResult<Void> {
        do { try api.fetchVoice(id, messageId: messageId); return .success(()) } catch { return failed(error) }
    }

    /// The voice post's recording as a file once it is here (nil before). Off the main thread.
    func voicePath(_ id: String, messageId: String) -> String? { api.voicePath(id, messageId: messageId) }

    func create(_ name: String) -> GroupResult<String> {
        if name.groupsIsBlank { return .failure(GroupFailure(tr("group_err_name_group"))) }
        let r = Result { try api.create(name.trimmingCharacters(in: .whitespacesAndNewlines)) }
        refresh()
        return r.mapError { GroupFailure(GroupText.error($0)) }
    }

    /// Blocks until joined (about a minute at most).
    func join(_ token: String) -> GroupResult<String> {
        let r = Result { try api.join(token.trimmingCharacters(in: .whitespacesAndNewlines)) }
        refresh()
        return r.mapError { GroupFailure(GroupText.error($0)) }
    }

    func invite(_ id: String) -> GroupResult<String> {
        Result { try api.invite(id) }.mapError { GroupFailure(GroupText.error($0)) }
    }

    // ---- direct invitations (G3) ----

    private func reloadInvitations() {
        let inc = (try? api.invitationsIncoming()) ?? []
        let out = opened.map { o in (try? api.invitationsOutgoing(o.id)) ?? [] } ?? []
        write { _invitations = inc; _sent = out }
    }

    func inviteContacts(_ id: String, dids: [String]) -> GroupResult<String> {
        if dids.isEmpty { return .failure(GroupFailure(tr("group_err_pick_someone"))) }
        let r = Result { try api.inviteContacts(id, dids: dids) }.map { (made: [GroupInvitation]) -> String in
            if made.count == 1 { return GroupText.named(made[0].inviteeName, "group_done_invite_one", "group_done_invite_one_unnamed") }
            return trn("group_done_invite_many", made.count)
        }
        locked { reloadInvitations() }
        return r.mapError { GroupFailure(GroupText.error($0)) }
    }

    /// Joins (blocks until joined, about a minute at most); the group id.
    func acceptInvitation(_ invitationId: String) -> GroupResult<String> {
        let r = Result { try api.invitationAccept(invitationId).groupId }
        refresh()
        return r.mapError { GroupFailure(GroupText.error($0)) }
    }

    func declineInvitation(_ invitationId: String) -> GroupResult<String> {
        let r = Result { try api.invitationDecline(invitationId) }
            .map { GroupText.named($0.inviterName, "group_done_decline_invite", "group_done_decline_invite_unnamed") }
        locked { reloadInvitations() }
        return r.mapError { GroupFailure(GroupText.error($0)) }
    }

    private func locked(_ f: () -> Void) { monitor.lock(); f(); monitor.unlock() }

    func onGroupInvitation(invitation: GroupInvitation) {
        locked { reloadInvitations() }
        onInvitation?(invitation)
    }

    func flag(_ id: String, did: String) -> GroupResult<String> { act(tr("group_done_flag")) { try api.flag(id, did: did) } }
    func setAdmin(_ id: String, did: String, admin: Bool) -> GroupResult<String> { act(admin ? tr("group_done_admin_made") : tr("group_done_admin_removed")) { try api.setAdmin(id, did: did, admin: admin) } }
    func leave(_ id: String) -> GroupResult<String> { act(tr("group_done_left")) { try api.leave(id) } }
    func appoint(_ id: String, did: String, dev: String) -> GroupResult<String> { act(tr("group_done_appointed")) { try api.appointHost(id, did: did, deviceId: dev) } }
    func revoke(_ id: String, did: String, dev: String) -> GroupResult<String> { act(tr("group_done_host_revoked")) { try api.revokeHost(id, did: did, deviceId: dev) } }
    func acceptHost(_ id: String, accept: Bool) -> GroupResult<String> {
        act(accept ? tr("group_done_host_accepted") : tr("group_done_host_stopped")) { try api.acceptHost(id, accept: accept) }
    }

    func appointCall(_ id: String, did: String, dev: String) -> GroupResult<String> { act(tr("group_done_callhost_asked")) { try api.appointCallHost(id, did: did, deviceId: dev) } }
    func revokeCall(_ id: String, did: String, dev: String) -> GroupResult<String> { act(tr("group_done_callhost_revoked")) { try api.revokeCallHost(id, did: did, deviceId: dev) } }
    func acceptCallHost(_ id: String, accept: Bool) -> GroupResult<String> {
        act(accept ? tr("group_done_callhost_accepted") : tr("group_done_callhost_stopped")) { try api.acceptCallHost(id, accept: accept) }
    }

    /// This phone carries the group's calls: accept when an admin asked; otherwise an admin appoints this device and accepts, in one go.
    func hostCallsHere(_ id: String, requested: Bool) -> GroupResult<String> {
        act(tr("group_done_callhost_here")) {
            if !requested { try api.appointCallHost(id, did: myDid, deviceId: try api.deviceId()) }
            try api.acceptCallHost(id, accept: true)
        }
    }

    private func reloadRequests() {
        let i = (try? api.requestsIncoming()) ?? [], o = (try? api.requestsOutgoing()) ?? []
        write { _incoming = i; _outgoing = o }
    }

    /// "Message privately" for a member who is not a contact.
    func request(_ groupId: String, did: String) -> GroupResult<String> {
        let r = Result { try api.chatRequest(groupId, did: did) }
            .map { GroupText.named($0.peerName, "group_done_request_sent", "group_done_request_sent_unnamed") }
        locked { reloadRequests() }
        return r.mapError { GroupFailure(GroupText.error($0)) }
    }

    func accept(_ requestId: String) -> GroupResult<String> {
        let r = Result { try api.chatAccept(requestId) }
            .map { GroupText.named($0.peerName, "group_done_request_accepted", "group_done_request_accepted_unnamed") }
        locked { reloadRequests() }
        return r.mapError { GroupFailure(GroupText.error($0)) }
    }

    func decline(_ requestId: String) -> GroupResult<String> {
        let r = Result { try api.chatDecline(requestId) }
            .map { GroupText.named($0.peerName, "group_done_request_declined", "group_done_request_declined_unnamed") }
        locked { reloadRequests() }
        return r.mapError { GroupFailure(GroupText.error($0)) }
    }

    /// Block (P5: they are told) and decline.
    func blockRequester(_ requestId: String, did: String) -> GroupResult<String> {
        let r = Result { () -> GroupChatRequest in try api.block(did); return try api.chatDecline(requestId) }
            .map { GroupText.named($0.peerName, "group_done_request_blocked", "group_done_request_blocked_unnamed") }
        locked { reloadRequests() }
        return r.mapError { GroupFailure(GroupText.error($0)) }
    }

    func onChatRequest(request: GroupChatRequest) {
        locked { reloadRequests() }
        if request.outgoing && request.state == .accepted && request.contactAdded {
            write { _notice = GroupText.named(request.peerName, "group_notice_request_accepted", "group_notice_request_accepted_unnamed") }
        }
    }

    private func act(_ ok: String, _ f: () throws -> Void) -> GroupResult<String> {
        let r = Result { try f(); return ok }
        refresh()
        return r.mapError { GroupFailure(GroupText.error($0)) }
    }

    // ---- core events (core threads: never block) ----

    func onGroupMessage(message: GroupMessage) {
        onPost?(message)
        let gid = message.groupId
        write { if let o = _open, o.id == gid { var n = o; n.messages = upsertGroupMessage(o.messages, message); _open = n } }
        // On screen: read now. Outside the monitor: markRead is a blocking core call (it signs read receipts).
        if read({ shown == gid && _open?.id == gid }) { try? api.markRead(gid) }
        locked { groupsOrKeep() }
    }

    func onGroupChanged(groupId: String) {
        write { _changes += 1 }
        locked {
            groupsOrKeep()
            if opened?.id == groupId { reloadOpen(); reloadInvitations() }
        }
    }

    /// A post's picture arrived: reload the open chat if it shows that group (GroupsCore.kt does the same).
    func onGroupImage(groupId: String, messageId: String) {
        guard read({ shown == groupId }) else { return }
        locked { if opened?.id == groupId { reloadOpen() } }
    }

    /// Who is online changed. The port has no separate presence field yet, so the open group's detail (hosts'
    /// `online`) is reloaded instead of patched.
    func onGroupPresence(groupId: String) {
        locked { if opened?.id == groupId { reloadOpen() } }
    }

    func attach(_ node: Node) {
        node.setGroupEvents(events: self)
        node.setGroupRequestEvents(events: self)
        node.setGroupInvitationEvents(events: self)
    }
}

/// One controller per node, with its core events attached once. Views reach it through `GroupsHub.controller(for:)`;
/// the Shell reads it the same way to set `onPost` / `onInvitation` for notifications.
enum GroupsHub {
    private static let lock = NSLock()
    private static var byNode: [ObjectIdentifier: GroupsController] = [:]

    static func controller(for node: Node) -> GroupsController {
        lock.lock(); defer { lock.unlock() }
        let key = ObjectIdentifier(node)
        if let c = byNode[key] { return c }
        let c = GroupsController(api: NodeGroupApi(node: { node }), me: { node.profile()?.did })
        c.attach(node)
        byNode[key] = c
        return c
    }
}
