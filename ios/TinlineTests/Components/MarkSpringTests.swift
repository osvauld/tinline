import XCTest
@testable import Tinline

/// The Done / first-contact pull (LINE_SPRING) and the everyday recoil: parameters of the one model (MarkModel.swift; android MarkSpringTest).
final class MarkSpringTests: XCTestCase {
    private func pull(_ m: MarkModel) -> (tugs: [Double], settled: Double) {
        m.connect()
        var t = 0.0
        var tugs: [Double] = []
        var seen = m.tugCount
        while m.advance(1.0 / 600) {
            t += 1.0 / 600
            if m.tugCount > seen { seen = m.tugCount; tugs.append(t) }
            XCTAssertLessThan(t, 3.0, "still moving after 3 s")
            if t >= 3.0 { break }
        }
        return (tugs, t)
    }

    func test_lineSpringTugsTwiceOnTheCueGap() {
        let r = pull(markModelFor(96, spring: .line))
        XCTAssertEqual(r.tugs.count, 2)
        let gap = r.tugs[1] - r.tugs[0]
        XCTAssertTrue((0.10...0.16).contains(gap), "gap \(gap)")
    }

    func test_defaultPullRecoilsAndSettlesDeadStopDoesNot() {
        let r = pull(markModelFor(96))
        XCTAssertGreaterThanOrEqual(r.tugs.count, 2)
        XCTAssertLessThan(r.settled, 1.2)
        let dead = pull(markModelFor(96, spring: .deadStop))
        XCTAssertEqual(dead.tugs.count, 1)
    }

    func test_mountingTautCountsNoTug() {
        let m = markModelFor(96, spring: .line)
        m.set(.taut)
        XCTAssertEqual(m.tugCount, 0)
        XCTAssertTrue(m.tight)
    }

    func test_statusLookPrecedence() {
        XCTAssertEqual(statusLook(call: .incoming, gcall: .active, conn: .online).label, "Incoming call")
        XCTAssertEqual(statusLook(gcall: .active, conn: .offline).label, "In a group call")
        XCTAssertTrue(statusLook(conn: .offline).dim)
    }
}
