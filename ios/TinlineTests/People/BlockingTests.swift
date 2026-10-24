import XCTest
@testable import Tinline

/// The block model the screens read (filled from Node.blockState / blockedContacts in PeopleModel). Port of android BlockingTest.kt.
final class BlockingTests: XCTestCase {
    private func blocks(_ views: [String: BlockView], _ list: [BlockedRow] = []) -> Blocks { Blocks.from(views, list) }

    func testNoBlockMeansAnOpenComposer() {
        let b = blocks(["ann": BlockView()])
        XCTAssertTrue(b.states.isEmpty)
        XCTAssertEqual(b.composer("ann", contact: true), .open)
        XCTAssertNil(b.status("ann"))
    }

    func testBlockingAContactKeepsItAndOffersUnblock() {
        let b = blocks(["ann": BlockView(blockedByMe: true, blockedAtSecs: 100)], [BlockedRow(did: "ann", name: "Ann Lee", blockedAtSecs: 100)])
        XCTAssertEqual(b.composer("ann", contact: true), .blockedByMe)
        XCTAssertFalse(b.canCall("ann", contact: true))
        XCTAssertEqual(b.status("ann"), "Blocked")
        XCTAssertEqual(b.list.map { $0.did }, ["ann"])
    }

    func testTheBlockedListIsNewestFirstAndCountsBlocksFromOtherDevices() {
        let b = blocks([:], [BlockedRow(did: "ann", name: "Ann", blockedAtSecs: 100), BlockedRow(did: "bob", name: "Bob", blockedAtSecs: 200)])
        XCTAssertEqual(b.list.map { $0.did }, ["bob", "ann"])
        // Not among the DIDs read (a block made on another device): still blocked here.
        XCTAssertTrue(b.blockedByMe("ann"))
        XCTAssertEqual(b.composer("ann", contact: true), .blockedByMe)
    }

    func testBeingBlockedClosesTheComposerOnTheKeptChat() {
        // Their notice deleted our contact entry for them (U20): not a contact, chat kept.
        let b = blocks(["meera": BlockView(blockedMeAtSecs: 1_791_640_200)])
        XCTAssertEqual(b.composer("meera", contact: false), .blockedMe)
        XCTAssertFalse(b.canCall("meera", contact: false))
        XCTAssertEqual(b.status("meera"), "Blocked you")
        XCTAssertTrue(b.list.isEmpty)
    }

    func testAfterTheirUnblockTheKeptChatAsksToAddThemAgain() {
        let b = blocks(["meera": BlockView()])
        XCTAssertEqual(b.composer("meera", contact: false), .notContact)
        XCTAssertFalse(b.canCall("meera", contact: false))
        XCTAssertNil(b.status("meera"))
    }

    func testCallPermissionsGateTheCallButton() {
        // Ann: a ticket contact (both ways). Priya: from a group chat request, message only.
        let b = Blocks.from([:], [], allowed: ["ann"], tokens: ["ann"])
        XCTAssertEqual(b.callGate("ann", contact: true), .ok)
        XCTAssertTrue(b.callsAllowed("ann"))
        XCTAssertEqual(b.callGate("priya", contact: true), .notAllowed)
        XCTAssertFalse(b.canCall("priya", contact: true))
        XCTAssertFalse(b.callsAllowed("priya"))
        XCTAssertEqual(b.composer("priya", contact: true), .open)
        // A block closes calling whatever the tokens say.
        let blocked = Blocks.from(["ann": BlockView(blockedByMe: true)], [], allowed: ["ann"], tokens: ["ann"])
        XCTAssertEqual(blocked.callGate("ann", contact: true), .closed)
    }

    func testBlockDateIsEnglishMonthDayTime() {
        XCTAssertEqual(BlockCopy.date(1_791_640_200, zone: TimeZone(identifier: "UTC")!), "Oct 10, 13:50")
    }
}
