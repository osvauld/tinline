import XCTest
@testable import Tinline

/// Port of android ProfileRecoveryTest.kt.
final class ProfileRecoveryTests: XCTestCase {
    func testBusyCannotSilentlyRefuseCalls() {
        XCTAssertNil(ProfilePolicy.availabilityProblem(available: true))
        XCTAssertEqual(ProfilePolicy.availabilityProblem(available: false), ProfilePolicy.busyUnsupported)
    }

    func testAccountRoundTripRejectsOldTicket() {
        let s = ProfileSession()
        s.select("account-a")
        let old = s.ticket()!
        s.select("account-b")
        s.select("account-a")
        XCTAssertFalse(s.accepts(old))
        XCTAssertTrue(s.accepts(s.ticket()!))
    }

    func testLockInvalidatesAndSameAccountRefreshDoesNot() {
        let s = ProfileSession()
        s.select("account-a")
        let t = s.ticket()!
        XCTAssertFalse(s.select("account-a"))
        XCTAssertTrue(s.accepts(t))
        s.invalidate()
        XCTAssertNil(s.ticket())
        XCTAssertFalse(s.accepts(t))
    }
}
