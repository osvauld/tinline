import XCTest
@testable import Tinline

/** Port of android CallNotifTest.kt: the in-call state is derived from state only (it once stayed on after a call ended). */
final class CallNotifTests: XCTestCase {
    private let now: Int64 = 1_000_000
    typealias O = CallNotif.OneToOne

    /** The derivation with defaults for an idle phone. */
    private func d(_ call: O = .none, groupIn: Bool = false, hosting: Bool = false, preparedAt: Int64? = nil) -> CallNotif.Derived {
        CallNotif.derive(call: call, callName: "Ada", groupInCall: groupIn, groupHosting: hosting, groupName: "Book club",
                         preparedAtMs: preparedAt, nowMs: now)
    }

    func testIdle() { XCTAssertNil(d().title); XCTAssertEqual("idle: no call", d().why) }
    func testRingingIncomingIsNotACall() { XCTAssertNil(d(.ringing).title) }
    func testDialingIsNotACallYet() { XCTAssertNil(d(.dialing).title) }
    func testAnsweredIsACall() { XCTAssertEqual("Ada", d(.answered).title) }
    func testActiveIsACall() { XCTAssertEqual("Ada", d(.active).title) }
    /** An ended call leaves [CallController.ui] (or is mapped to none): back to idle, whatever was before. */
    func testEndedIsIdle() { XCTAssertNil(d(.none).title) }
    func testGroupJoined() { XCTAssertEqual("Book club", d(groupIn: true).title) }

    func testGroupHostingOnlyIsNotACall() {
        let r = d(hosting: true)
        XCTAssertNil(r.title)
        XCTAssertEqual("idle: group hosting only", r.why)
    }

    /** A 1:1 call wins the title over a group call; a 1:1 ending keeps the group call. */
    func testOneToOneThenGroup() {
        XCTAssertEqual("Ada", d(.active, groupIn: true).title)
        XCTAssertEqual("Book club", d(.none, groupIn: true).title)
    }

    func testPreparedCountsUntilTimeout() {
        XCTAssertEqual("Book club", d(preparedAt: now).title)
        XCTAssertEqual("Book club", d(preparedAt: now - CallNotif.prepareTimeoutMs + 1).title)
        let r = d(preparedAt: now - CallNotif.prepareTimeoutMs)
        XCTAssertNil(r.title)
        XCTAssertEqual("idle: group prepare expired", r.why)
        // A clock that went backwards does not keep it either.
        XCTAssertNil(d(preparedAt: now + 5_000).title)
    }

    /** Re-deriving whenever asked gives the same answer whatever order the requests arrive in. */
    func testOutOfOrderPokesDoNotMatter() {
        var state = O.none
        var group = false
        var title: String?
        var queued: [String] = []
        func handle() { title = d(state, groupIn: group).title }

        state = .answered; queued.append("IN_CALL")
        state = .active; queued.append("IN_CALL")
        state = .none; handle()
        XCTAssertNil(title)
        while !queued.isEmpty { queued.removeFirst(); handle() }
        XCTAssertNil(title, "late IN_CALL pokes must not bring the call back")

        group = true; state = .active; queued.append("IN_CALL")
        state = .none; handle()
        XCTAssertEqual("Book club", title)
        queued.removeFirst(); handle()
        XCTAssertEqual("Book club", title)
        group = false; handle()
        XCTAssertNil(title)
    }

    func testTraceIsBounded() {
        CallTrace.clear()
        for i in 0..<(CallTrace.max + 50) { CallTrace.push("line \(i)") }
        let lines = CallTrace.dump().split(separator: "\n").map(String.init)
        XCTAssertEqual(CallTrace.max, lines.count)
        XCTAssertEqual("line 50", lines.first)
        XCTAssertEqual("line \(CallTrace.max + 49)", lines.last)
        CallTrace.clear()
    }
}
