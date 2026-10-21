import XCTest
import SwiftUI
@testable import Tinline

/// The icon data is generated (crates/tin-icons); these check it parses and that the path parser handles the command subset.
final class TinIconTests: XCTestCase {
    func test_parserReadsLinesCurvesAndClose() {
        let p = TinSVGPath.parse("M12 3L22 21L2 21ZM12 10L12 15")
        XCTAssertFalse(p.isEmpty)
        let box = p.boundingRect
        XCTAssertEqual(box.minX, 2, accuracy: 0.001)
        XCTAssertEqual(box.maxX, 22, accuracy: 0.001)
        XCTAssertEqual(box.minY, 3, accuracy: 0.001)
        XCTAssertEqual(box.maxY, 21, accuracy: 0.001)
    }

    func test_parserReadsQuadraticAndNegativeNumbers() {
        let p = TinSVGPath.parse("M5.4 5.4Q15.39 8.61 18.6 18.6M-3 -3L-1.5 0")
        XCTAssertFalse(p.isEmpty)
        XCTAssertEqual(p.boundingRect.minX, -3, accuracy: 0.001)
    }

    func test_parserRelativeAndHV() {
        let p = TinSVGPath.parse("M6 6l12 12M18 6L6 18")
        XCTAssertEqual(p.boundingRect.maxX, 18, accuracy: 0.001)
        let q = TinSVGPath.parse("M1 1H5V5")
        XCTAssertEqual(q.boundingRect.maxY, 5, accuracy: 0.001)
    }

    func test_emptyStringIsEmptyPath() { XCTAssertTrue(TinSVGPath.parse("").isEmpty) }

    func test_tiersPickBySize() {
        let g16 = TinIcons.call.at(16), g20 = TinIcons.call.at(20), g24 = TinIcons.call.at(24)
        XCTAssertFalse(g16.lines.isEmpty)
        XCTAssertFalse(g20.lines.isEmpty)
        XCTAssertFalse(g24.lines.isEmpty)
        XCTAssertNotEqual(g16.fills, g24.fills)   // the 16 pt tier is hand-tuned, not a scaled copy
    }

    func test_hangupIsAllString() {
        let g = TinIcons.hangup.at(20)
        XCTAssertTrue(g.lines.isEmpty)
        XCTAssertTrue(g.hasString)
    }

    func test_tickDataShape() {
        XCTAssertEqual(TinTicks.tick.count, TinTicks.n * 2)
        XCTAssertEqual(TinTicks.slack.count, TinTicks.n * 2)
        XCTAssertFalse(TinTicks.pull.isEmpty)
        XCTAssertFalse(TinTicks.pluck.isEmpty)
        XCTAssertEqual(tickSlotWidth(12), 12 * 26 / 24, accuracy: 0.001)
    }

    func test_cubicBezierEndpointsAndMonotone() {
        let e = TinMotion.out
        XCTAssertEqual(e.transform(0), 0)
        XCTAssertEqual(e.transform(1), 1)
        XCTAssertGreaterThan(e.transform(0.5), 0.5)   // ease-out leads
        XCTAssertEqual(SvelteEase.cubicOut(1), 1, accuracy: 1e-9)
    }

    func test_avatarColourPairsWrap() {
        XCTAssertEqual(tinAvatarColors(.light, hue: -1).fg, tinAvatarColors(.light, hue: 5).fg)
        XCTAssertEqual(tinAvatarColors(.darkTheme, hue: 6).bg, tinAvatarColors(.darkTheme, hue: 0).bg)
    }
}
