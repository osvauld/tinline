import XCTest
@testable import Tinline

/**
 * Parity with the playground's model.ts through crates/desktop/src/call_field_golden.txt (frames produced by the
 * playground's own model, tools/call-field/golden.mjs). Same scenarios and the same tolerance (1e-7 relative to
 * max(|golden|, 1)) as crates/desktop/src/call_field_tests.rs and android CallFieldTest.kt. The golden file is read
 * from the repo (found by walking up from this source file), not copied.
 */
final class CallFieldTests: XCTestCase {
    private static let golden: [String] = {
        var d = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        while d.path != "/" {
            let f = d.appendingPathComponent("crates/desktop/src/call_field_golden.txt")
            if let s = try? String(contentsOf: f, encoding: .utf8) { return s.split(separator: "\n").map(String.init) }
            d.deleteLastPathComponent()
        }
        return []
    }()

    private struct Step { let at: Double; let state: CallFieldState; let delay: Double }

    private func scenario(_ name: String) -> [Step] {
        switch name {
        case "out": return [Step(at: 0, state: .outgoing, delay: 0), Step(at: 2600, state: .connecting, delay: 0), Step(at: 3300, state: .active, delay: 97),
                            Step(at: 9000, state: .waiting, delay: 0), Step(at: 12000, state: .active, delay: 0), Step(at: 14000, state: .reconnecting, delay: 0),
                            Step(at: 16000, state: .active, delay: 97), Step(at: 18000, state: .ended, delay: 0)]
        case "inc": return [Step(at: 0, state: .incoming, delay: 0), Step(at: 4000, state: .connecting, delay: 0), Step(at: 4700, state: .active, delay: 97),
                            Step(at: 8000, state: .failed, delay: 0)]
        default: fatalError(name)
        }
    }

    private func run(_ name: String, _ theme: FieldTheme, _ reduced: Bool, _ muteAt: Double?) -> [(Int64, [Double])] {
        let f = CallField(theme: theme, reduced: reduced)
        f.setView(w: 390, h: 844, avx: 195, avy: 300)
        let sc = scenario(String(name.split(separator: "-")[0]))
        var si = 0
        var out: [(Int64, [Double])] = []
        var t: Int64 = 0
        while t <= 20000 {
            while si < sc.count && sc[si].at <= Double(t) {
                let s = sc[si]; si += 1
                f.setState(s.state, t: s.at, cueStart: s.at + s.delay)
            }
            if let muteAt, Double(t) >= muteAt { f.setMute(true) }
            let fr = f.update(Double(t))
            if t % 160 < 16 || (t > 3200 && t < 4200) {
                var v = [fr.tau, fr.warp]
                for l in fr.lobes { v += [l.x, l.y, l.sigma, l.alpha, l.lab[0], l.lab[1], l.lab[2]] }
                out.append((t, v))
            }
            t += t % 1000 == 500 ? 7 : 16
        }
        return out
    }

    private func check(_ name: String, _ theme: FieldTheme, _ reduced: Bool, _ muteAt: Double?) {
        let want: [(Int64, [Double])] = Self.golden.filter { $0.hasPrefix("\(name) ") }.map { l in
            let it = l.split(whereSeparator: { $0 == " " || $0 == "\t" }).dropFirst().map(String.init)
            return (Int64(it[0])!, it.dropFirst().map { Double($0)! })
        }
        let got = run(name, theme, reduced, muteAt)
        XCTAssertFalse(want.isEmpty, "\(name): no golden frames")
        XCTAssertEqual(want.count, got.count, "\(name): frame count")
        var worst = 0.0
        for (w, g) in zip(want, got) {
            XCTAssertEqual(w.0, g.0)
            XCTAssertEqual(2 + FieldMath.k * 7, w.1.count, "golden frame shape")
            XCTAssertEqual(w.1.count, g.1.count, "native frame shape")
            for (i, (a, b)) in zip(w.1, g.1).enumerated() {
                let err = abs(a - b) / max(abs(a), 1)
                worst = max(worst, err)
                XCTAssertLessThan(err, 1e-7, "\(name) t=\(w.0) value \(i): playground \(a), swift \(b)")
            }
        }
        print("\(name): \(got.count) frames, worst relative error \(worst)")
    }

    func testMatchesThePlaygroundOutgoingCallLight() { check("out-light", .light, false, nil) }
    func testMatchesThePlaygroundOutgoingCallDarkMuted() { check("out-dark", .dark, false, 10000) }
    func testMatchesThePlaygroundIncomingCallThatFails() { check("inc-light", .light, false, nil) }
    func testMatchesThePlaygroundReducedMotion() { check("out-reduced", .light, true, nil) }

    func testEndedFieldFadesAndStopsAnimating() {
        let f = CallField(theme: .light, reduced: false)
        f.setState(.active, t: 0)
        _ = f.update(1000)
        f.setState(.ended, t: 1000)
        var t = 1000.0
        while f.update(t).animating { t += 16; XCTAssertLessThan(t, 2000, "the end fade takes 300 ms") }
        XCTAssertTrue(f.update(t).lobes.allSatisfy { $0.alpha == 0 })
    }

    func testReducedMotionSleepsUntilTheNextNote() {
        let f = CallField(theme: .light, reduced: true)
        f.setState(.incoming, t: 0, cueStart: 0)
        var t = 0.0
        var fr = f.update(t)
        while fr.animating { t += 16; fr = f.update(t) }
        guard let wake = fr.wakeAt else { return XCTFail("a looping ring always has a next note") }
        XCTAssertTrue(wake > t && wake <= 3200, "wakes at the next ring note, got \(wake)")
    }

    func testRngIsBitExactWithMulberry32() {
        var r = FieldRng(7)
        XCTAssertEqual([0.011704753153026104, 0.06195825757458806, 0.97690763277933], [r.next(), r.next(), r.next()])
    }
}
