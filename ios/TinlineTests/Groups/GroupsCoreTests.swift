import XCTest
@testable import Tinline

/// The group model and controller on a stand-in for the core (rules from docs/group-core.md).
/// Port of android GroupsCoreTest.kt.
final class GroupsCoreTests: XCTestCase {
    private let me = "did:key:me"

    private func msg(_ id: String, _ at: UInt64, author: String = "did:key:sam", name: String = "Sam Rao", kind: GroupItemKind = .post, text: String = "hi",
                     delivery: GroupDelivery = .delivered, gid: String = "g") -> GroupMessage {
        GroupMessage(id: id, groupId: gid, kind: kind, authorDid: author, authorName: name, subjectDid: author, subjectName: name, at: at, text: text,
                     replyTo: nil, outgoing: author == me, delivery: delivery, beforeRemoval: false)
    }

    private func member(_ did: String, owner: Bool = false, admin: Bool? = nil, flags: UInt32 = 0, devs: [String]? = nil) -> GroupMember {
        GroupMember(did: did, name: String(did.dropFirst("did:key:".count)), isMe: did == me, isOwner: owner, isAdmin: admin ?? owner, isHost: false,
                    flags: flags, joinedAt: 1, invitedBy: nil, deviceIds: devs ?? ["dev-\(did)"])
    }

    private func detail(_ role: GroupRole, _ members: [GroupMember], hosts: [GroupHost] = [], amHost: Bool = false, removal: GroupRemoval? = nil,
                        hostRequests: [GroupHost] = []) -> GroupDetail {
        GroupDetail(id: "g", name: "Hike", ownerDid: members.first(where: { $0.isOwner })!.did, myRole: role, amHost: amHost, members: members, hosts: hosts,
                    hostOffers: [], hostRequests: hostRequests, hostRequestedHere: hostRequests.contains(where: { $0.isMe }), removal: removal,
                    amCallHost: false, callHosts: [], callHostRequests: [], callHostRequestedHere: false)
    }

    private func summary(_ id: String, _ role: GroupRole, _ unread: UInt32, _ last: GroupMessage?) -> GroupSummary {
        GroupSummary(id: id, name: "G \(id)", myRole: role, memberCount: 3, hostCount: 1, hostedHere: false, hostReachable: true, last: last, unread: unread)
    }

    private func refused(_ why: String) -> GroupsCoreError { .Rejected(message: "they refused: \(why)") }

    private final class FakeApi: GroupApi {
        let me = "did:key:me"
        var list: [GroupSummary] = []
        var details: [String: GroupDetail] = [:]
        var msgs: [String: [GroupMessage]] = [:]
        private let lock = NSLock()
        private var _marked: [String] = []
        var marked: [String] { lock.lock(); defer { lock.unlock() }; return _marked }
        var postError: Swift.Error?
        var locked = false
        /// Holds markRead (the slow, receipt-signing core call) until signalled; `markEntered` says it is waiting.
        var markGate: DispatchSemaphore?
        let markEntered = DispatchSemaphore(value: 0)
        var reqs: [GroupChatRequest] = []
        var blocked: [String] = []
        var invs: [GroupInvitation] = []

        func groups() throws -> [GroupSummary] {
            if locked { throw GroupsCoreError.Locked(message: "identity is locked") }
            return list
        }
        func detail(_ id: String) throws -> GroupDetail {
            guard let d = details[id] else { throw GroupsCoreError.NotFound(message: "not found") }
            return d
        }
        func messages(_ id: String, before: String?, limit: Int) throws -> GroupPage {
            let all = msgs[id] ?? []
            let end = before.flatMap { b in all.firstIndex(where: { $0.id == b }) } ?? all.count
            let start = max(0, end - limit)
            return GroupPage(messages: Array(all[start..<end]), older: start > 0 ? all[start].id : nil)
        }
        func markRead(_ id: String) throws {
            if let g = markGate { markEntered.signal(); g.wait() }
            lock.lock(); _marked.append(id); lock.unlock()
        }
        func create(_ name: String) throws -> String { "new" }
        func invite(_ id: String) throws -> String { "tinline://g/\(id)" }
        func peek(_ token: String) -> GroupInvitePeek? {
            token.hasPrefix("tinline://g/") ? GroupInvitePeek(groupId: "g", name: "Hike", ownerDid: "did:key:sam", issuerDid: "did:key:sam", expires: 1, hostCount: 1) : nil
        }
        func join(_ token: String) throws -> String { throw GroupsCoreError.Rejected(message: "they refused: invite expired or invalid") }
        func post(_ id: String, text: String, replyTo: String?) throws -> GroupMessage {
            if let e = postError { throw e }
            let m = GroupMessage(id: "p\(msgs[id]!.count)", groupId: id, kind: .post, authorDid: "did:key:me", authorName: "Me", subjectDid: "did:key:me", subjectName: "Me",
                                 at: 9999, text: text, replyTo: replyTo, outgoing: true, delivery: .waiting, beforeRemoval: false)
            msgs[id]!.append(m)
            return m
        }
        func flag(_ id: String, did: String) throws { throw GroupsCoreError.Rejected(message: "they refused: only admins can do that") }
        func setAdmin(_ id: String, did: String, admin: Bool) throws {}
        func leave(_ id: String) throws {}
        func appointHost(_ id: String, did: String, deviceId: String) throws {}
        func revokeHost(_ id: String, did: String, deviceId: String) throws {}
        func acceptHost(_ id: String, accept: Bool) throws {}
        func deviceId() throws -> String { String(repeating: "aa", count: 32) }
        func chatRequest(_ id: String, did: String) throws -> GroupChatRequest {
            if did == "did:key:contact" { throw GroupsCoreError.Rejected(message: "they refused: already a contact") }
            let r = GroupChatRequest(id: "r\(reqs.count)", groupId: id, groupName: "Hike", peerDid: did, peerName: "Leo B", outgoing: true, state: .pending, at: 1, contactAdded: false)
            reqs.append(r)
            return r
        }
        func requestsIncoming() throws -> [GroupChatRequest] { reqs.filter { !$0.outgoing } }
        func requestsOutgoing() throws -> [GroupChatRequest] { reqs.filter { $0.outgoing } }
        private func answer(_ id: String, _ st: ChatRequestState) throws -> GroupChatRequest {
            guard let i = reqs.firstIndex(where: { $0.id == id && !$0.outgoing }) else { throw GroupsCoreError.NotFound(message: "not found") }
            reqs[i].state = st
            return reqs[i]
        }
        func chatAccept(_ requestId: String) throws -> GroupChatRequest { try answer(requestId, .accepted) }
        func chatDecline(_ requestId: String) throws -> GroupChatRequest { try answer(requestId, .declined) }
        func block(_ did: String) throws { blocked.append(did) }
        func inviteContacts(_ id: String, dids: [String]) throws -> [GroupInvitation] {
            if dids.contains(where: { blocked.contains($0) }) { throw GroupsCoreError.Rejected(message: "they refused: you block them") }
            return dids.map { d in
                let name = String(d.dropFirst("did:key:".count)); let cap = name.prefix(1).uppercased() + name.dropFirst()
                let i = GroupInvitation(id: "i\(invs.count)", groupId: id, groupName: "Hike", inviterDid: me, inviterName: "Me", inviteeDid: d, inviteeName: cap + " K",
                                        outgoing: true, state: .pending, at: 1, expires: 9, delivered: false)
                invs.append(i)
                return i
            }
        }
        func invitationsIncoming() throws -> [GroupInvitation] { invs.filter { !$0.outgoing } }
        func invitationsOutgoing(_ id: String) throws -> [GroupInvitation] { invs.filter { $0.outgoing && $0.groupId == id } }
        private func answerInv(_ id: String, _ st: GroupInvitationState) throws -> GroupInvitation {
            let i = invs.firstIndex(where: { $0.id == id })!
            if invs[i].state != .pending { throw GroupsCoreError.Rejected(message: "they refused: already answered") }
            invs[i].state = st
            return invs[i]
        }
        func invitationAccept(_ invitationId: String) throws -> GroupInvitation { try answerInv(invitationId, .joined) }
        func invitationDecline(_ invitationId: String) throws -> GroupInvitation { try answerInv(invitationId, .declined) }
    }

    private func inv(_ id: String, _ state: GroupInvitationState, outgoing: Bool = false, invitee: String? = nil, delivered: Bool = true) -> GroupInvitation {
        GroupInvitation(id: id, groupId: "g", groupName: "Hike", inviterDid: outgoing ? me : "did:key:sam", inviterName: outgoing ? "Me" : "Sam Rao",
                        inviteeDid: invitee ?? me, inviteeName: "Leo B", outgoing: outgoing, state: state, at: 1, expires: 9, delivered: delivered)
    }

    func testInvitationsCountOnTheBadgeUntilAnswered() {
        let a = summary("a", .member, 2, nil)
        let list = [inv("1", .pending), inv("2", .declined), inv("3", .pending, outgoing: true)]
        XCTAssertEqual(3, GroupText.badge([a], invitations: list))
        XCTAssertEqual(["1"], GroupText.pendingInvitations(list).map { $0.id })
    }

    func testThePickerHidesBlockedContactsAndShowsMembersAndPendingInvites() {
        let d = detail(.owner, [member(me, owner: true), member("did:key:sam")])
        let contacts = ["sam", "leo", "priya", "ravi", "zoe"].map { InviteCandidate(did: "did:key:\($0)", name: $0.prefix(1).uppercased() + $0.dropFirst()) }
        let sent = [inv("1", .pending, outgoing: true, invitee: "did:key:leo"), inv("2", .declined, outgoing: true, invitee: "did:key:zoe")]
        let rows = GroupText.pickRows(contacts, d, sent, blockedByMe: { $0 == "did:key:priya" }, blocksMe: { $0 == "did:key:ravi" })
        XCTAssertEqual(["did:key:zoe", "did:key:leo", "did:key:sam"], rows.map { $0.did })
        XCTAssertNil(rows[0].why) // declined before: can be invited again
        XCTAssertEqual("Invited · waiting", rows[1].why)
        XCTAssertEqual("Already a member", rows[2].why)
        XCTAssertTrue(GroupText.sentInvitationLine(sent[1]) == ("Declined", true))
        var undelivered = sent[0]; undelivered.delivered = false
        XCTAssertTrue(GroupText.sentInvitationLine(undelivered) == ("Invited · not delivered yet", false))
        var joined = sent[0]; joined.state = .joined
        XCTAssertTrue(GroupText.sentInvitationLine(joined) == ("Joined", false))
    }

    func testInvitingContactsAndAnsweringAnInvitation() {
        let api = FakeApi()
        let ctl = GroupsController(api: api, me: { self.me })
        XCTAssertEqual("Pick someone first.", failure(ctl.inviteContacts("g", dids: [])))
        XCTAssertEqual("Invitation sent to Leo. They join when they accept.", success(ctl.inviteContacts("g", dids: ["did:key:leo"])))
        XCTAssertEqual("2 invitations sent. Each person joins when they accept.", success(ctl.inviteContacts("g", dids: ["did:key:a", "did:key:b"])))
        api.blocked.append("did:key:x")
        XCTAssertNotNil(failure(ctl.inviteContacts("g", dids: ["did:key:x"])))
        api.invs.append(inv("in1", .pending))
        api.invs.append(inv("in2", .pending))
        ctl.onGroupInvitation(invitation: api.invs.last!)
        XCTAssertEqual(2, GroupText.pendingInvitations(ctl.invitations).count)
        XCTAssertEqual("g", success(ctl.acceptInvitation("in1")))
        XCTAssertTrue(success(ctl.declineInvitation("in2"))!.hasPrefix("Declined. Sam sees"))
        XCTAssertTrue(GroupText.pendingInvitations(ctl.invitations).isEmpty)
        XCTAssertEqual("Already answered.", failure(ctl.declineInvitation("in2")))
    }

    func testGroupLinksAreRecognised() {
        XCTAssertTrue(GroupLinks.isLink("tinline://g/3xK9abc"))
        XCTAssertTrue(GroupLinks.isLink(" TINLINE://G/3xK9abc "))
        XCTAssertFalse(GroupLinks.isLink("tinline://g/"))
        XCTAssertFalse(GroupLinks.isLink("tlgroup1:AAAA"))
        XCTAssertFalse(GroupLinks.isLink(nil))
    }

    func testPreviewsAndBadgeSkipGroupsWeLeftOrWereRemovedFrom() {
        let a = summary("a", .member, 2, msg("1", 1, text: "anyone?"))
        let b = summary("b", .removed, 5, msg("2", 2))
        let c = summary("c", .admin, 1, msg("3", 3, author: me, text: "on my way"))
        XCTAssertEqual("Sam: anyone?", GroupText.preview(a, me: me))
        XCTAssertEqual("You were removed from this group", GroupText.preview(b, me: me))
        XCTAssertEqual("You: on my way", GroupText.preview(c, me: me))
        XCTAssertEqual(3, GroupText.badge([a, b, c]))
        let joined = msg("4", 4, kind: .joined)
        XCTAssertEqual("Sam Rao joined", GroupText.preview(summary("d", .member, 0, joined), me: me))
    }

    func testSystemLinesSayYouAndNeverShowKeys() {
        let m = GroupMessage(id: "x", groupId: "g", kind: .adminGranted, authorDid: "did:key:sam", authorName: "Sam", subjectDid: me, subjectName: "Me", at: 1, text: "",
                             replyTo: nil, outgoing: false, delivery: .delivered, beforeRemoval: false)
        XCTAssertEqual("Sam made you an admin", GroupText.sysText(m, me: me))
        var removed = m; removed.kind = .removed; removed.subjectDid = "did:key:p"; removed.subjectName = "Priya"
        XCTAssertEqual("Priya was removed after 3 admin flags", GroupText.sysText(removed, me: me))
        var left = m; left.kind = .left; left.subjectDid = "did:key:x"; left.subjectName = ""
        XCTAssertEqual("Someone left", GroupText.sysText(left, me: me))
    }

    func testMemberActionsFollowRoles() {
        let owner = member("did:key:sam", owner: true)
        let priya = member("did:key:priya")
        var mine = member(me)
        mine.isOwner = true; mine.isAdmin = true
        var priyaPlain = priya; priyaPlain.isAdmin = false
        var asOwner = detail(.owner, [mine, priyaPlain, member("did:key:x")])
        asOwner.ownerDid = me
        let a = GroupText.actions(asOwner, priya)
        XCTAssertEqual(MemberAction.message, a[0])
        XCTAssertEqual(MemberAction.makeAdmin, a[1])
        XCTAssertTrue(a.contains(.flag))
        XCTAssertTrue(a.contains(where: { if case .appoint = $0 { return true }; return false }))
        XCTAssertTrue(GroupText.actions(asOwner, asOwner.members.first(where: { $0.isMe })!).isEmpty)

        let host = GroupHost(did: "did:key:sam", name: "Sam", deviceId: "dev-did:key:sam", appointedBy: nil, isMe: false, online: true)
        let mineMember = member(me)
        let asAdmin = detail(.admin, [owner, mineMember, priya], hosts: [host])
        XCTAssertFalse(GroupText.actions(asAdmin, priya).contains(.makeAdmin))
        XCTAssertFalse(GroupText.actions(asAdmin, owner).contains(.flag))
        XCTAssertTrue(GroupText.actions(asAdmin, owner).contains(.revoke(deviceId: "dev-did:key:sam")))

        XCTAssertEqual([MemberAction.message], GroupText.actions(detail(.member, [owner, mineMember, priya]), priya))
        XCTAssertTrue(GroupText.actions(detail(.removed, [owner, mineMember, priya]), priya).isEmpty)
    }

    func testTheFlagDialogSaysWhichFlagOursIs() {
        XCTAssertEqual(1, GroupText.flagCopy("Dev Patel", flagsSoFar: 0).0)
        let (n, text) = GroupText.flagCopy("Dev Patel", flagsSoFar: 2)
        XCTAssertEqual(3, n)
        XCTAssertTrue(text.contains("Dev is removed right away"))
    }

    func testStatusAndWaitingForAHost() {
        let ms = [member("did:key:sam", owner: true), member(me)]
        XCTAssertEqual("2 members · no host yet", GroupText.status(detail(.member, ms)))
        let off = GroupHost(did: "did:key:sam", name: "Sam", deviceId: "d", appointedBy: nil, isMe: false, online: false)
        var d = detail(.member, ms, hosts: [off])
        XCTAssertEqual("2 members · waiting for a host", GroupText.status(d))
        XCTAssertTrue(GroupText.waitingForHost(d))
        var hosting = d; hosting.amHost = true
        XCTAssertEqual("2 members · this device hosts the group", GroupText.status(hosting))
        d.myRole = .removed
        d.removal = GroupRemoval(at: 5, flags: [GroupFlag(byDid: "did:key:a", byName: "A", at: 1)])
        XCTAssertEqual("You were removed from this group", GroupText.status(d))
        XCTAssertFalse(GroupText.waitingForHost(d))
        XCTAssertEqual("Waiting for a host", GroupText.deliveryWord(msg("x", 1, author: me, delivery: .waiting)))
        XCTAssertNil(GroupText.deliveryWord(msg("x", 1, author: me)))
    }

    func testErrorsReadPlainly() {
        XCTAssertEqual("You were removed from this group.", GroupText.error(refused("removed")))
        XCTAssertEqual("The owner cannot leave.", GroupText.error(refused("the owner cannot leave")))
        XCTAssertEqual("No host answered in time. Try again when a host is online.", GroupText.error(GroupsCoreError.Timeout(message: "timed out")))
    }

    func testUpsertKeepsTimeOrderAndReplacesById() {
        var l = [msg("b", 2), msg("d", 4)]
        l = upsertGroupMessage(l, msg("c", 3))
        l = upsertGroupMessage(l, msg("a", 1))
        l = upsertGroupMessage(l, msg("e", 5))
        l = upsertGroupMessage(l, msg("c", 3, text: "edited delivery"))
        XCTAssertEqual(["a", "b", "c", "d", "e"], l.map { $0.id })
        XCTAssertEqual("edited delivery", l[2].text)
    }

    func testTheControllerOpensMarksReadAndTakesLiveMessages() {
        let api = FakeApi()
        api.list.append(summary("g", .member, 1, msg("1", 1)))
        api.details["g"] = detail(.member, [member("did:key:sam", owner: true), member(me)])
        api.msgs["g"] = [msg("1", 10)]
        let ctl = GroupsController(api: api, me: { self.me })
        ctl.refresh()
        XCTAssertEqual(1, ctl.list.count)
        XCTAssertEqual(String(repeating: "aa", count: 32), ctl.myDevice)
        ctl.openGroup("g")
        XCTAssertEqual(["g"], api.marked)
        XCTAssertEqual(1, ctl.opened!.messages.count)
        ctl.onGroupMessage(message: msg("0", 5))
        ctl.onGroupMessage(message: msg("2", 20))
        ctl.onGroupMessage(message: msg("x", 30, gid: "other"))
        XCTAssertEqual(["0", "1", "2"], ctl.opened!.messages.map { $0.id })

        // Our post shows at once, waiting for a host; a refusal comes back in plain words.
        let r = ctl.post("g", text: "  on my way ", replyTo: nil)
        XCTAssertEqual("on my way", try? r.get().text)
        XCTAssertEqual("Waiting for a host", GroupText.deliveryWord(ctl.opened!.messages.last!))
        api.postError = refused("removed")
        XCTAssertEqual("You were removed from this group.", failure(ctl.post("g", text: "still here?", replyTo: nil)))
        XCTAssertNotNil(failure(ctl.post("g", text: "   ", replyTo: nil)))

        XCTAssertEqual("Only admins can do that.", failure(ctl.flag("g", did: "did:key:sam")))
        XCTAssertEqual("Invite expired or invalid.", failure(ctl.join("tinline://g/x")))
        XCTAssertEqual("tinline://g/g", success(ctl.invite("g")))
        XCTAssertNil(ctl.peek("garbage"))
        XCTAssertEqual("Hike", ctl.peek(" tinline://g/x ")?.name)

        ctl.close("g")
        XCTAssertNil(ctl.opened)
    }

    func testOlderPagesGoInFrontAndALockedNodeShowsNoGroups() {
        let api = FakeApi()
        api.list.append(summary("g", .member, 0, nil))
        api.details["g"] = detail(.member, [member("did:key:sam", owner: true), member(me)])
        api.msgs["g"] = (0..<(GroupText.page + 3)).map { msg(String(format: "m%03d", $0), UInt64($0)) }
        let ctl = GroupsController(api: api, me: { self.me })
        ctl.openGroup("g")
        XCTAssertEqual(GroupText.page, ctl.opened!.messages.count)
        ctl.older()
        XCTAssertEqual(GroupText.page + 3, ctl.opened!.messages.count)
        XCTAssertEqual("m000", ctl.opened!.messages.first!.id)
        XCTAssertNil(ctl.opened!.older)
        api.locked = true
        ctl.refresh()
        XCTAssertTrue(ctl.list.isEmpty)
    }

    func testChatRequestsFromAGroupMember() {
        let api = FakeApi()
        let ctl = GroupsController(api: api, me: { self.me })
        XCTAssertEqual("Request sent to Leo. It waits in Chats until they answer.", success(ctl.request("g", did: "did:key:leo")))
        XCTAssertEqual(1, ctl.outgoing.count)
        XCTAssertTrue(GroupText.sentLine(ctl.outgoing[0]) == ("Waiting for Leo · via Hike", false))
        var declined = ctl.outgoing[0]; declined.state = .declined
        XCTAssertTrue(GroupText.sentLine(declined) == ("Leo declined", true))
        var blocked = ctl.outgoing[0]; blocked.state = .blocked
        XCTAssertTrue(GroupText.sentLine(blocked) == ("Leo blocked you", true))
        XCTAssertEqual("You already have them as a contact. Message them from Chats.", failure(ctl.request("g", did: "did:key:contact")))

        // Their answer arrives as an event: one notice when the contact is in place.
        var accepted = ctl.outgoing[0]; accepted.state = .accepted; accepted.contactAdded = true
        ctl.onChatRequest(request: accepted)
        XCTAssertTrue(ctl.notice!.hasPrefix("Leo accepted your request"))
        ctl.clearNotice()
        XCTAssertNil(ctl.notice)

        api.reqs.append(GroupChatRequest(id: "in1", groupId: "g", groupName: "Hike", peerDid: "did:key:priya", peerName: "Priya N", outgoing: false, state: .pending, at: 2, contactAdded: false))
        api.reqs.append(GroupChatRequest(id: "in2", groupId: "g", groupName: "Hike", peerDid: "did:key:ravi", peerName: "Ravi J", outgoing: false, state: .pending, at: 3, contactAdded: false))
        ctl.refresh()
        XCTAssertEqual(2, ctl.incoming.count)
        XCTAssertTrue(success(ctl.accept("in1"))!.contains("messages only"))
        XCTAssertNotNil(success(ctl.blockRequester("in2", did: "did:key:ravi")))
        XCTAssertEqual(["did:key:ravi"], api.blocked)
        XCTAssertTrue(ctl.incoming.allSatisfy { $0.state != .pending })
    }

    func testAnAdminAskingThisDeviceToHostShowsWhoAsked() {
        let ms = [member("did:key:sam", owner: true), member(me)]
        XCTAssertNil(GroupText.hostAsker(detail(.member, ms)))
        let ask = GroupHost(did: me, name: "Me", deviceId: "aa", appointedBy: "did:key:sam", isMe: true, online: true)
        XCTAssertEqual("sam", GroupText.hostAsker(detail(.member, ms, hostRequests: [ask])))
        XCTAssertNil(GroupText.hostAsker(detail(.removed, ms, hostRequests: [ask])))
    }

    /// The main thread closes the chat (onDisappear) while openGroup is still inside markRead holding the controller's monitor:
    /// close must not wait for it (it froze the phone ~0.75 s per group open), and the page that finishes afterwards must not
    /// come back for a screen that is gone. Nor is a closed group marked read by a message arriving later.
    func testClosingNeverWaitsForAnOpenInFlight() {
        let api = FakeApi()
        api.details["g"] = detail(.member, [member("did:key:sam", owner: true), member(me)])
        api.msgs["g"] = [msg("1", 10)]
        let gate = DispatchSemaphore(value: 0)
        api.markGate = gate
        let ctl = GroupsController(api: api, me: { self.me })
        let finished = DispatchSemaphore(value: 0)
        let opening = Thread { ctl.openGroup("g"); finished.signal() }
        opening.start()
        XCTAssertEqual(.success, api.markEntered.wait(timeout: .now() + 5))
        let t0 = DispatchTime.now().uptimeNanoseconds
        ctl.close("g")
        XCTAssertLessThan(DispatchTime.now().uptimeNanoseconds - t0, 200_000_000, "close waited for open")
        gate.signal()
        XCTAssertEqual(.success, finished.wait(timeout: .now() + 5))
        XCTAssertNil(ctl.opened)
        api.markGate = nil
        ctl.onGroupMessage(message: msg("2", 20))
        XCTAssertEqual(["g"], api.marked)
    }

    // MARK: helpers

    private func success<T>(_ r: GroupResult<T>) -> T? { if case .success(let v) = r { return v }; return nil }
    private func failure<T>(_ r: GroupResult<T>) -> String? { if case .failure(let e) = r { return e.message }; return nil }
}
