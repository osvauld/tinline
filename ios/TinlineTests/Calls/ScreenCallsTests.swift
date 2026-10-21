import XCTest
@testable import Tinline

/** Port of android ScreenCallsTest.kt. */
final class ScreenCallsTests: XCTestCase {
    func testRealPhaseFlagsChooseStatusWithoutInventingTiming() {
        XCTAssertEqual("Calling…", callStatus(active: false, ringing: false, reconnecting: false, secs: 0))
        XCTAssertEqual("Ringing…", callStatus(active: false, ringing: true, reconnecting: false, secs: 0))
        XCTAssertEqual("Reconnecting…", callStatus(active: true, ringing: false, reconnecting: true, secs: 65))
    }

    func testEncryptionClaimIsConditionalOnConnection() {
        let connecting = callPrivacy(active: false, name: "Arjun")
        XCTAssertTrue(connecting.hasPrefix("Connecting to Arjun"))
        XCTAssertFalse(connecting.hasPrefix("End-to-end encrypted."))
        let connected = callPrivacy(active: true, name: "Arjun")
        XCTAssertTrue(connected.hasPrefix("End-to-end encrypted."))
        // The relay is explained in the card's connection row, only when the call is relayed.
        XCTAssertFalse(connected.contains("relay"))
    }

    func testSignalUsesRecentLossAndSharedThresholds() {
        let l = RecentLoss()
        _ = l.push(received: 0, lost: 0); _ = l.push(received: 1000, lost: 200)
        var last = 0.0
        for i in 2...8 { last = l.push(received: 1000 * Int64(i), lost: 200) }
        XCTAssertEqual(0.0, last)
        XCTAssertEqual(["0", "0.2", "14"], [lossText(0), lossText(0.2), lossText(14.04)])
    }

    func testLiveMarkUsesFoundationCallPriority() {
        XCTAssertEqual(.ringing, callMarkLook(.outgoing).state)
        XCTAssertEqual(.taut, callMarkLook(.active).state)
        XCTAssertEqual(.ringing, callMarkLook(.reconnecting).state)
    }

    func testFirstNameAndBlankFallback() {
        XCTAssertEqual("Arjun", callFirstName("  Arjun Mehta "))
        XCTAssertEqual("Unknown", callOr("  ", "Unknown"))
        XCTAssertEqual("Sam", callOr("Sam", "Unknown"))
    }

    func testGroupClockAndStatus() {
        XCTAssertEqual("01:05", clockOf(65))
        XCTAssertEqual("Finding a call host…", GcState(phase: .starting).status)
        XCTAssertEqual("No call host available", GcState(phase: .failed, reason: "No call host available").status)
        XCTAssertFalse(GcState(phase: .ended).live4)
        XCTAssertTrue(GcState(phase: .reconnecting).connected)
    }
}
