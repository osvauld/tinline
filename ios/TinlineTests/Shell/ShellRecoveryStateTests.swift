import XCTest
@testable import Tinline

/// The monotonic reveal policy of the recovery backup screen (android RecoveryBackupStateTest).
final class ShellRecoveryStateTests: XCTestCase {
    func test_startsHiddenWithKeySelected() {
        let s = ShellRecoveryState()
        XCTAssertTrue(s.asKey)
        XCTAssertFalse(s.visible(0))
        XCTAssertFalse(s.canCopyKey(0))
    }

    func test_revealHidesAfterThirtySeconds() {
        let s = ShellRecoveryState().reveal(1_000)
        XCTAssertEqual(ShellRecoveryState.revealMs, 30_000)
        XCTAssertTrue(s.visible(1_000))
        XCTAssertTrue(s.visible(30_999))
        XCTAssertFalse(s.visible(31_000))
        XCTAssertFalse(s.canCopyKey(31_000))
    }

    func test_hideClearsDeadlineAndKeepsMode() {
        let s = ShellRecoveryState(asKey: false).reveal(0).hide()
        XCTAssertNil(s.revealUntil)
        XCTAssertFalse(s.asKey)
        XCTAssertFalse(s.visible(1))
    }

    func test_copyOnlyForKeyWhileVisible() {
        let key = ShellRecoveryState().reveal(0)
        XCTAssertTrue(key.canCopyKey(10))
        let words = ShellRecoveryState(asKey: false).reveal(0)
        XCTAssertTrue(words.visible(10))
        XCTAssertFalse(words.canCopyKey(10))
    }

    func test_switchingTabHidesAgain() {
        let s = ShellRecoveryState().reveal(0).select(false)
        XCTAssertFalse(s.visible(1))
        XCTAssertFalse(s.asKey)
        XCTAssertFalse(s.select(true).visible(1))
    }
}
