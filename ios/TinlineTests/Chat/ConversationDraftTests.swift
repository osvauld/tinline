import XCTest
@testable import Tinline

/// Port of android ScreenConversationTest.kt (drafts and find highlighting).
@MainActor
final class ConversationDraftTests: XCTestCase {
    func testEditCancelAndSaveRestoreDraft() {
        let d = ConversationDraft(); d.text = "unsent"
        d.beginEdit(id: "one", body: "original"); d.text = "changed"; d.cancel()
        XCTAssertEqual(d.text, "unsent"); XCTAssertNil(d.edit)
        d.beginEdit(id: "two", body: "second"); d.sent()
        XCTAssertEqual(d.text, "unsent"); XCTAssertNil(d.edit)
    }

    func testReplySurvivesLeavingButEditDoesNot() {
        let d = ConversationDraft(); d.text = "unsent"; d.beginReply(id: "reply")
        d.cancelEdit(); XCTAssertEqual(d.reply, "reply"); XCTAssertEqual(d.text, "unsent")
        d.sent(); XCTAssertNil(d.reply); XCTAssertEqual(d.text, "")
    }

    func testChangingEditTargetDoesNotOverwriteStash() {
        let d = ConversationDraft(); d.text = "unsent"
        d.beginEdit(id: "one", body: "first"); d.beginEdit(id: "two", body: "second"); d.cancelEdit()
        XCTAssertEqual(d.text, "unsent")
    }

    func testClearedDraftCannotBeRepublishedByDisposedScreen() {
        ConversationDrafts.clear()
        let stale = ConversationDrafts.forPeer("peer")
        stale.text = "private draft"
        ConversationDrafts.publish("peer", stale)
        XCTAssertEqual(ConversationDrafts.previews.value["peer"], "private draft")
        ConversationDrafts.clear()
        ConversationDrafts.publish("peer", stale)
        XCTAssertTrue(ConversationDrafts.previews.value.isEmpty)
        XCTAssertEqual(ConversationDrafts.forPeer("peer").text, "")
        ConversationDrafts.clear()
    }

    /// Ranges are Unicode scalar offsets here (UTF-16 offsets on Android), so the emoji count as one each.
    func testHighlightsRespectUnicodeAndPrefixes() {
        XCTAssertEqual(ChatFind.ranges("😀😀 café", "cafe"), [3..<7])
        XCTAssertTrue(ChatFind.ranges("landlord", "andlord").isEmpty)
        XCTAssertFalse(ChatFind.ranges("ഊണ്", "ഊണ്").isEmpty)
        XCTAssertTrue(ChatFind.ranges("ഊണ്", "ഊണി").isEmpty)
        XCTAssertEqual(ChatFind.ranges("会議は", "議は"), [1..<3])
    }

    func testFindStateStepsAndWraps() {
        let f = FindState()
        f.hits = [FindHit(id: "a", day: ""), FindHit(id: "b", day: ""), FindHit(id: "c", day: "")]
        XCTAssertEqual(f.countLabel, "1/3")
        XCTAssertFalse(f.olderOrNeedsPage()); XCTAssertEqual(f.cur, 1)
        f.next = "cursor"
        XCTAssertFalse(f.olderOrNeedsPage()); XCTAssertEqual(f.cur, 2)
        XCTAssertTrue(f.olderOrNeedsPage())              // at the end with a cursor: ask the core for more
        XCTAssertEqual(f.countLabel, "3/3+")
        f.next = nil
        XCTAssertFalse(f.olderOrNeedsPage()); XCTAssertEqual(f.cur, 0)   // wraps
        f.newer(); XCTAssertEqual(f.cur, 2)
        f.error = "x"; XCTAssertEqual(f.countLabel, "—")
    }
}
