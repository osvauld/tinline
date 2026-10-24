import XCTest
@testable import Tinline

/// Port of the pure parts of android ScreenConversationTest.kt, ChatUi.kt and FileOpen.kt.
final class ChatFormatTests: XCTestCase {
    func testSpacingUsesRunPosition() {
        XCTAssertEqual(chatGapAbove(.solo), 10)
        XCTAssertEqual(chatGapAbove(.first), 10)
        XCTAssertEqual(chatGapAbove(.mid), 2)
        XCTAssertEqual(chatGapAbove(.last), 2)
    }

    func testTickFollowsTheCoreThenTheLink() {
        let pending = ChatFixtures.message("m", "hi", at: 0, outgoing: true, delivery: .pending)
        XCTAssertEqual(chatTickOf(pending, receipt: .read, link: .offline, online: true, nowMs: 0), .read)
        XCTAssertEqual(chatTickOf(pending, receipt: .delivered, link: .offline, online: true, nowMs: 0), .two)
        XCTAssertEqual(chatTickOf(pending, receipt: nil, link: .offline, online: true, nowMs: 0), .clock)
        XCTAssertEqual(chatTickOf(pending, receipt: nil, link: .unknown, online: false, nowMs: 0), .clock)
        XCTAssertEqual(chatTickOf(pending, receipt: nil, link: .unknown, online: true, nowMs: 29_000), .one)
        XCTAssertEqual(chatTickOf(pending, receipt: nil, link: .unknown, online: true, nowMs: 31_000), .clock)
        let sent = ChatFixtures.message("m", "hi", at: 0, outgoing: true, delivery: .delivered)
        XCTAssertEqual(chatTickOf(sent, receipt: nil, link: .offline, online: false, nowMs: 99_999), .one)
    }

    func testPhotoBoxKeepsTheShapeWithinTheLimits() {
        let wide = ChatFmt.photoBox(width: 4000, height: 1000)
        XCTAssertEqual(wide.width, 248, accuracy: 0.01)
        XCTAssertEqual(wide.height, 62, accuracy: 0.01)
        let tall = ChatFmt.photoBox(width: 1000, height: 4000)
        XCTAssertEqual(tall.height, 330, accuracy: 0.01)
        XCTAssertEqual(tall.width, 120, accuracy: 0.01)   // at least 120 wide
        let unknown = ChatFmt.photoBox(width: 0, height: 0)
        XCTAssertEqual(unknown.width / unknown.height, 4.0 / 3.0, accuracy: 0.001)
    }

    func testCallPillWords() {
        func rec(incoming: Bool, secs: UInt32, reason: String, video: Bool = false) -> CallRecord {
            CallRecord(callId: "c", peerDid: "p", peerName: "P", incoming: incoming, startedAt: 0, durationSecs: secs, reason: reason, direct: true,
                       missed: false, video: video)
        }
        XCTAssertEqual(ChatFmt.callPillText(rec(incoming: true, secs: 125, reason: "hangup")), "Voice call · 2:05")
        XCTAssertEqual(ChatFmt.callPillText(rec(incoming: true, secs: 0, reason: "timeout")), "Missed voice call")
        XCTAssertEqual(ChatFmt.callPillText(rec(incoming: true, secs: 0, reason: "declined_local", video: true)), "Declined video call")
        XCTAssertEqual(ChatFmt.callPillText(rec(incoming: false, secs: 0, reason: "no_answer")), "No answer")
        XCTAssertEqual(ChatFmt.callPillText(rec(incoming: false, secs: 0, reason: "hangup_local")), "Cancelled call")
        XCTAssertEqual(ChatFmt.callPillText(rec(incoming: false, secs: 0, reason: "boom")), "Call failed")
    }

    func testFirstNameAndTypeLabel() {
        XCTAssertEqual(ChatFmt.firstName("  Ana Rao "), "Ana")
        XCTAssertEqual(ChatFmt.firstName("Ana"), "Ana")
        XCTAssertEqual(ChatFmt.typeLabel(ChatFixtures.attachment(name: "menu.pdf", mime: "application/pdf")), "PDF")
        XCTAssertEqual(ChatFmt.typeLabel(ChatFixtures.attachment(name: "noext", mime: "image/png")), "PNG")
    }

    func testWaveformBarsTakeThePeakOfEachSlice() {
        let wave = Data([0, 255, 0, 0, 51, 0, 0, 0])
        XCTAssertEqual(ChatFmt.bars(wave, 4), [1, 0, 0.2, 0])
        XCTAssertEqual(ChatFmt.bars(Data(), 3), [0, 0, 0])
    }

    func testDayLabels() {
        var cal = Calendar(identifier: .gregorian); cal.timeZone = TimeZone(identifier: "UTC")!
        ChatFmt.calendar = cal
        defer { ChatFmt.calendar = .current }
        let now = Int64(ChatFixtures.at(minutes: 0))
        XCTAssertEqual(ChatFmt.dayLabel(now, now: now), "Today")
        XCTAssertEqual(ChatFmt.dayLabel(now - 86_400_000, now: now), "Yesterday")
        XCTAssertEqual(ChatFmt.clock(now), "12:00")
        XCTAssertTrue(ChatFmt.sameDay(now, now + 60_000))
        XCTAssertFalse(ChatFmt.sameDay(now, now + 86_400_000))
    }

    func testFileOpenPlanTrustsNeitherTheMimeNorTheExtensionAlone() {
        XCTAssertEqual(ChatFileOpen.plan(name: "a.pdf", mime: "application/pdf"), ChatOpenPlan(kind: .pdf, mime: "application/pdf"))
        XCTAssertEqual(ChatFileOpen.plan(name: "a.pdf", mime: "application/octet-stream").kind, .pdf)
        XCTAssertEqual(ChatFileOpen.plan(name: "a.pdf", mime: "text/plain").kind, .unsupported)
        XCTAssertEqual(ChatFileOpen.plan(name: "a.txt", mime: "text/plain; charset=utf-8").kind, .text)
        XCTAssertEqual(ChatFileOpen.plan(name: "a.docx", mime: "").kind, .external)
        XCTAssertEqual(ChatFileOpen.plan(name: "a.mp4", mime: "video/mp4").mime, "video/mp4")
        XCTAssertEqual(ChatFileOpen.plan(name: "a.apk", mime: "").kind, .unsupported)
        XCTAssertEqual(ChatFileOpen.plan(name: "a.exe", mime: "application/octet-stream").kind, .unsupported)
        XCTAssertEqual(ChatFileOpen.plan(name: "noext", mime: "application/pdf").kind, .unsupported)
        XCTAssertEqual(ChatFileOpen.extOf("Photo.JPG"), "jpg")
    }

    func testPreviewOfNamesWhatAMessageCarries() {
        XCTAssertEqual(chatPreviewOf(ChatFixtures.message("a", "hello", at: 0)), "hello")
        XCTAssertEqual(chatPreviewOf(ChatFixtures.message("a", "", at: 0, attachment: ChatFixtures.attachment(name: "p.png", mime: "image/png"))), "Photo")
        XCTAssertEqual(chatPreviewOf(ChatFixtures.message("a", "", at: 0, attachment: ChatFixtures.attachment(name: "v.ogg", mime: "audio/ogg", kind: .voice))), "Voice message")
        XCTAssertEqual(chatPreviewOf(ChatFixtures.message("a", "", at: 0, attachment: ChatFixtures.attachment(name: "x.zip", mime: "application/zip"))), "x.zip")
        XCTAssertEqual(chatPreviewOf(ChatFixtures.message("a", "gone", at: 0, deleted: true)), "")
    }
}
