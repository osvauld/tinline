import XCTest
@testable import Tinline

/// The rules of an open conversation (android Conversation.kt's row building, run grouping, unread divider, send).
@MainActor
final class ConversationModelTests: XCTestCase {
    private typealias F = ChatFixtures

    private func positions(_ rows: [ConvRow]) -> [BubblePos] {
        rows.compactMap { if case .msg(_, let p) = $0 { return p } else { return nil } }
    }

    func testRunsJoinSameSenderWithinFiveMinutes() {
        var cal = Calendar(identifier: .gregorian); cal.timeZone = TimeZone(identifier: "UTC")!
        ChatFmt.calendar = cal
        defer { ChatFmt.calendar = .current }
        let msgs = [
            F.message("1", "a", at: F.at(minutes: 0)), F.message("2", "b", at: F.at(minutes: 1)), F.message("3", "c", at: F.at(minutes: 2)),
            F.message("4", "d", at: F.at(minutes: 3), outgoing: true),
            F.message("5", "e", at: F.at(minutes: 30), outgoing: true),     // same sender but more than five minutes later
        ]
        let rows = ChatRows.build(messages: msgs, calls: [], nowMs: Int64(F.at(minutes: 60)), dividerId: nil, unreadAtOpen: 0)
        XCTAssertEqual(positions(rows), [.first, .mid, .last, .solo, .solo])
        if case .day(let label, _) = rows[0] { XCTAssertEqual(label, "Today") } else { XCTFail("a day pill leads") }
    }

    func testUnreadDividerBreaksTheRunAndCallsSitBetweenMessages() {
        var cal = Calendar(identifier: .gregorian); cal.timeZone = TimeZone(identifier: "UTC")!
        ChatFmt.calendar = cal
        defer { ChatFmt.calendar = .current }
        let msgs = [F.message("1", "a", at: F.at(minutes: 0)), F.message("2", "b", at: F.at(minutes: 1)), F.message("3", "c", at: F.at(minutes: 10))]
        let call = CallRecord(callId: "call1", peerDid: F.ana, peerName: "Ana", incoming: true, startedAt: UInt64(F.at(minutes: 5) / 1000), durationSecs: 60,
                              reason: "hangup", direct: true, missed: false, video: false)
        let rows = ChatRows.build(messages: msgs, calls: [call], nowMs: Int64(F.at(minutes: 60)), dividerId: "2", unreadAtOpen: 2)
        XCTAssertEqual(rows.map(\.id), ["d0Today", "1", "unread", "2", "callcall1", "3"])
        XCTAssertEqual(positions(rows), [.first, .solo, .solo])   // as on Android: the divider does not end the run above it
    }

    func testStartLoadsTheNewestDayAndClearsUnread() async {
        let src = ChatFakeSource(chats: [F.chat(unread: 1)], messages: [F.message("1", "a", at: F.at(minutes: 0)), F.message("2", "b", at: F.at(minutes: 1))])
        let model = ConversationModel(peerDid: F.ana, source: src)
        await model.start()
        XCTAssertTrue(model.loaded)
        XCTAssertEqual(model.messages.map(\.id), ["1", "2"])
        XCTAssertEqual(src.markReadCount, 1)
        XCTAssertTrue(model.rows.contains { if case .unread(1) = $0 { return true } else { return false } })
    }

    func testSendTrimsClearsTheDraftAndKeepsTheReply() async {
        let src = ChatFakeSource(messages: [F.message("1", "a", at: F.at(minutes: 0))])
        let model = ConversationModel(peerDid: F.ana, source: src)
        await model.start()
        let draft = ConversationDraft(); draft.text = "  hello  "; draft.beginReply(id: "1")
        model.send(draft)
        try? await Task.sleep(nanoseconds: 100_000_000)
        XCTAssertEqual(src.sentTexts.first?.text, "hello")
        XCTAssertEqual(src.sentTexts.first?.replyTo, "1")
        XCTAssertEqual(draft.text, ""); XCTAssertNil(draft.reply)
        XCTAssertEqual(model.messages.last?.text, "hello")
    }

    func testAFailedSendKeepsTheDraftAndSaysSo() async {
        let src = ChatFakeSource()
        src.failSend = true
        let model = ConversationModel(peerDid: F.ana, source: src)
        await model.start()
        let draft = ConversationDraft(); draft.text = "hello"
        model.send(draft)
        try? await Task.sleep(nanoseconds: 100_000_000)
        XCTAssertEqual(draft.text, "hello")
        XCTAssertNotNil(model.toast)
    }

    func testEventsForThisChatUpsertAndProgressClearsWhenDone() async {
        let src = ChatFakeSource()
        let model = ConversationModel(peerDid: F.ana, source: src)
        await model.start()
        src.emit(.added(F.message("n", "new", at: F.at(minutes: 5))))
        src.emit(.added(F.message("x", "other chat", at: F.at(minutes: 6), peer: "someone-else")))
        src.emit(.progress(peerDid: F.ana, hash: "h", done: 5, total: 10, outgoing: false))
        try? await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(model.messages.map(\.id), ["n"])
        XCTAssertEqual(model.progress["h"]?.done, 5)
        src.emit(.progress(peerDid: F.ana, hash: "h", done: 10, total: 10, outgoing: false))
        try? await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertNil(model.progress["h"])
    }

    func testOlderDayLoadsWhenAskedAndRunsAreSortedByTimeThenId() async {
        let src = ChatFakeSource(messages: [F.message("b", "b", at: F.at(minutes: 10))], olderDay: "2026-10-13")
        src.pages["2026-10-13"] = DayPage(day: "2026-10-13", messages: [F.message("a", "a", at: F.at(minutes: -1000))], olderDay: nil)
        let model = ConversationModel(peerDid: F.ana, source: src)
        await model.start()
        await model.loadOlder()
        XCTAssertEqual(model.messages.map(\.id), ["a", "b"])
        XCTAssertNil(model.olderDay)
        XCTAssertEqual(model.prependTick, 1)
    }

    func testLocalFindIsNewestFirstAndSkipsDeleted() {
        let msgs = [F.message("1", "lunch friday", at: 1), F.message("2", "Friday notes", at: 2), F.message("3", "friday", at: 3, deleted: true)]
        XCTAssertEqual(ChatFind.local(msgs, "fri").map(\.id), ["2", "1"])
    }

    func testUtcDayIsTheCoreKey() {
        XCTAssertEqual(ConversationModel.utcDay(Int64(F.at(minutes: 0))), "2026-10-14")
    }
}
