import XCTest
@testable import Tinline

/// The search highlighter (android ScreenSearchTest): core ranges are code points; the lead keeps the match in view.
final class ShellSearchMarkedTests: XCTestCase {
    private func bold(_ segs: [ShellMarked.Segment]) -> [String] { segs.filter(\.bold).map(\.text) }
    private func text(_ segs: [ShellMarked.Segment]) -> String { segs.map(\.text).joined() }

    func test_codePointRangesMarkTheRightText() {
        let t = "\u{1F600}\u{1F600} hello zebra"
        let ranges = ShellMarked.hitRanges([HitRange(start: 9, end: 14)])
        XCTAssertEqual(ranges, [9..<14])
        let shown = ShellMarked.segments(t, ranges)
        XCTAssertEqual(bold(shown), ["zebra"])
        XCTAssertEqual(text(shown), t)
    }

    func test_longSnippetKeepsMatchInView() {
        let t = "a long introductory sentence with many words before needle and after"
        let start = t.unicodeScalars.distance(from: t.unicodeScalars.startIndex, to: t.range(of: "needle")!.lowerBound)
        let shown = ShellMarked.segments(t, [start..<(start + 6)], lead: 18)
        XCTAssertTrue(text(shown).hasPrefix("\u{2026}"))
        XCTAssertEqual(bold(shown), ["needle"])
        // At most the lead (plus the ellipsis) before the match.
        XCTAssertLessThanOrEqual(shown.first { !$0.bold }!.text.unicodeScalars.count, 19)
    }

    func test_malformedRangesAreClampedAndPlainTextSurvives() {
        XCTAssertEqual(text(ShellMarked.segments("hello", [])), "hello")
        XCTAssertEqual(text(ShellMarked.segments("hello", [-2..<99])), "hello")
    }

    func test_nameRangesAreCaseInsensitive() {
        XCTAssertEqual(ShellMarked.nameRanges("Maya Rao", "rao"), [5..<8])
        XCTAssertEqual(ShellMarked.nameRanges("Maya", " "), [])
    }
}
