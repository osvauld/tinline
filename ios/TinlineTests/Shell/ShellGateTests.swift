import XCTest
@testable import Tinline

/// The sign-up order, the passphrase hint and the 24-word rules (android Onboarding.kt progressOf / StrengthMeter / RestoreStep;
/// the Kotlin has no JVM test for them, these pin the ported behaviour).
final class ShellGateTests: XCTestCase {
    func test_progressCountsEachPath() {
        // Welcome and Choose are step 0 of the create count: the header is already there.
        XCTAssertEqual(ShellGate.progressOf(.welcome, restoring: false, linking: false, forgot: false, adding: false).map { [$0.0, $0.1] }, [0, 6])
        XCTAssertEqual(ShellGate.progressOf(.name, restoring: false, linking: false, forgot: false, adding: false).map { [$0.0, $0.1] }, [1, 6])
        XCTAssertEqual(ShellGate.progressOf(.perms, restoring: true, linking: false, forgot: false, adding: false).map { [$0.0, $0.1] }, [6, 6])
        XCTAssertEqual(ShellGate.progressOf(.linkPass, restoring: false, linking: true, forgot: false, adding: false).map { [$0.0, $0.1] }, [3, 5])
        // Adding an account skips Terms and Permissions; forgot has no progress at all.
        XCTAssertEqual(ShellGate.progressOf(.device, restoring: false, linking: false, forgot: false, adding: true).map { [$0.0, $0.1] }, [4, 4])
        XCTAssertNil(ShellGate.progressOf(.pass, restoring: true, linking: false, forgot: true, adding: false))
        XCTAssertNil(ShellGate.progressOf(.saveFailed, restoring: false, linking: false, forgot: false, adding: false))
    }

    func test_strength() {
        XCTAssertEqual(ShellGate.strength(""), 0)
        XCTAssertEqual(ShellGate.strength("abc"), 1)
        XCTAssertEqual(ShellGate.strength("abcdefgh"), 2)
        XCTAssertEqual(ShellGate.strength("a b c d"), 3)
        XCTAssertEqual(ShellGate.strength("sixteen-chars-xx"), 3)
    }

    private let list = ["abandon", "ability", "able", "about", "above", "zoo"]

    func test_pasteSpreadsWordsFromTheField() {
        var m = ShellRestoreWords(list: list)
        let next = m.setWord(2, "Able  ABOUT, above", isKey: { _ in false })
        XCTAssertEqual(Array(m.words[2...4]), ["able", "about", "above"])
        XCTAssertEqual(next, 4)
    }

    func test_aKeyReplacesTheBoxes() {
        var m = ShellRestoreWords(list: list)
        _ = m.setWord(0, " tlkey-xyz ", isKey: { $0.contains("tlkey-") })
        XCTAssertEqual(m.recKey, "tlkey-xyz")
        XCTAssertEqual(m.filled, 0)
    }

    func test_aWordTurnsRedWhenLeftOrWhenNothingStartsThatWay() {
        var m = ShellRestoreWords(list: list)
        _ = m.setWord(0, "ab", isKey: { _ in false })
        XCTAssertFalse(m.bad(0), "a prefix with answers is fine while typing")
        m.seen[0] = true
        XCTAssertTrue(m.bad(0))
        _ = m.setWord(1, "qq", isKey: { _ in false })
        XCTAssertTrue(m.bad(1), "no word starts that way")
        XCTAssertEqual(m.message(error: nil), "Word 1 and 1 more aren\u{2019}t in the recovery word list. Check the spelling.")
    }

    func test_completeTakesTheOnlyAnswer() {
        var m = ShellRestoreWords(list: list)
        _ = m.setWord(0, "zo", isKey: { _ in false })
        m.complete(0)
        XCTAssertEqual(m.words[0], "zoo")
        _ = m.setWord(1, "ab", isKey: { _ in false })
        m.complete(1)
        XCTAssertEqual(m.words[1], "ab", "several answers: left as typed")
    }

    func test_submitAsksForTheMissingWords() {
        var m = ShellRestoreWords(list: list)
        _ = m.setWord(0, "zoo", isKey: { _ in false })
        let r = m.submitWords()
        XCTAssertNil(r.phrase)
        XCTAssertEqual(r.focus, 1)
        XCTAssertEqual(m.message(error: nil), "23 words are still missing.")
        for i in 0..<24 { m.words[i] = "zoo" }
        XCTAssertEqual(m.submitWords().phrase, Array(repeating: "zoo", count: 24).joined(separator: " "))
    }
}
