import XCTest
@testable import Tinline

/// The Chats tab rows and the Calls tab words (android Chats.kt chatRows / withMarks, HomeLists.kt callWhat, EmptyLine.kt).
final class ShellRowsTests: XCTestCase {
    private func chat(_ did: String, name: String = "", at: UInt64, out: Bool = false, pending: Bool = false, unread: UInt32 = 0, last: String? = nil) -> Chat {
        Chat(peerDid: did, peerName: name, preview: "hi", lastOutgoing: out, lastDelivery: pending ? .pending : .delivered,
             lastActivity: at, unread: unread, lastId: last)
    }
    private func contact(_ did: String, _ name: String, alias: String? = nil) -> Contact {
        Contact(did: did, name: name, device: "d", addedAt: 0, alias: alias, verified: false, lostTouch: false)
    }

    func test_youIsPinnedAndEmptyChatsAreLeftOut() {
        let rows = ShellRows.chatRows([chat("b", at: 0), chat("me", at: 5), chat("a", name: "Ann", at: 9)], [], links: [:], online: true, nowMs: 10, myDid: "me")
        XCTAssertEqual(rows.map(\.did), ["me", "a"])
        XCTAssertEqual(rows[0].name, "You")
        let sorted = ShellRows.sorted(rows + [ShellChatRow(did: "g", name: "G", preview: "", time: "", mine: false, tick: .two, unread: 0, group: true, atMs: 99)], you: "me")
        XCTAssertEqual(sorted.map(\.did), ["me", "g", "a"])
    }

    func test_aContactsNameWinsAndOfflinePendingWaits() {
        let rows = ShellRows.chatRows([chat("a", name: "self-named", at: 9, out: true, pending: true)], [contact("a", "Ann Lee", alias: "Annie")],
                                      links: ["a": .offline], online: true, nowMs: 10, myDid: nil)
        XCTAssertEqual(rows[0].name, "Annie")
        XCTAssertEqual(rows[0].tick, .clock)
        XCTAssertEqual(rows[0].preview, "Waiting to send \u{2014} Annie isn\u{2019}t reachable yet")
    }

    func test_marksOnlyForMyLastMessage() {
        let rows = ShellRows.chatRows([chat("a", at: 9, out: true, last: "m1"), chat("b", at: 8, out: false, last: "m2")], [], links: [:], online: true, nowMs: 10, myDid: nil)
        let marked = ShellRows.withMarks(rows, [ListMark(conversationId: "a", group: false, messageId: "m1", mark: .read),
                                                ListMark(conversationId: "b", group: false, messageId: "m2", mark: .read)], myDid: nil)
        XCTAssertEqual(marked[0].mark, .read)
        XCTAssertTrue(marked[0].markSlot)
        XCTAssertNil(marked[1].mark)
        XCTAssertFalse(marked[1].markSlot)
    }

    private func record(incoming: Bool, missed: Bool = false, secs: UInt32 = 0, reason: String = "") -> CallRecord {
        CallRecord(callId: "c", peerDid: "a", peerName: "Ann", incoming: incoming, startedAt: 0, durationSecs: secs, reason: reason,
                   direct: true, missed: missed, video: false)
    }

    func test_callWhat() {
        XCTAssertEqual(ShellText.callWhat(record(incoming: true, secs: 4)), "Incoming")
        XCTAssertEqual(ShellText.callWhat(record(incoming: false, secs: 4)), "Outgoing")
        XCTAssertEqual(ShellText.callWhat(record(incoming: true, reason: "unavailable")), "While not available")
        XCTAssertEqual(ShellText.callWhat(record(incoming: true, missed: true)), "Missed")
        XCTAssertEqual(ShellText.callWhat(record(incoming: false, reason: "busy")), "Declined")
        XCTAssertEqual(ShellText.callWhat(record(incoming: false, reason: "unreachable")), "Couldn\u{2019}t connect")
    }

    func test_heroCopy() {
        XCTAssertEqual(ShellEmpty.heroCopy(contacts: []).act, "Add my first contact")
        XCTAssertEqual(ShellEmpty.heroCopy(contacts: [contact("a", "Maya Rao")]).act, "Message Maya")
        XCTAssertEqual(ShellEmpty.heroCopy(contacts: [contact("a", "A"), contact("b", "B")]).title, "Your line is open")
        let sent = [ContactRequest(did: "x", name: "Leo Park", at: 0, expiresAt: 0)]
        XCTAssertEqual(ShellEmpty.heroCopy(contacts: [], sent: sent).line, "Your request to Leo is waiting. Your line opens when they accept.")
    }
}
