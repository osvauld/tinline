import XCTest
@testable import Tinline

/** Port of android HostCondTest.kt, plus the iOS readings. */
final class HostCondTests: XCTestCase {
    func testBatteryPercent() {
        XCTAssertEqual(57, HostCond.batteryPct(level: 57, scale: 100))
        XCTAssertEqual(50, HostCond.batteryPct(level: 128, scale: 256))
        XCTAssertEqual(100, HostCond.batteryPct(level: 300, scale: 100))
        // Unknown level or scale: treated as empty, so an unknown phone never advertises hosting.
        XCTAssertEqual(0, HostCond.batteryPct(level: -1, scale: 100))
        XCTAssertEqual(0, HostCond.batteryPct(level: 50, scale: 0))
    }

    func testBatteryPercentFromUIDevice() {
        XCTAssertEqual(0, HostCond.batteryPct(level: Float(-1)))
        XCTAssertEqual(57, HostCond.batteryPct(level: Float(0.57)))
        XCTAssertEqual(100, HostCond.batteryPct(level: Float(1)))
    }

    func testPluggedFromSourceOrStatus() {
        XCTAssertFalse(HostCond.plugged(pluggedExtra: 0, status: 3)) // discharging
        XCTAssertFalse(HostCond.plugged(pluggedExtra: 0, status: 4)) // not charging, unplugged
        XCTAssertTrue(HostCond.plugged(pluggedExtra: 1, status: 3)) // AC
        XCTAssertTrue(HostCond.plugged(pluggedExtra: 2, status: -1)) // USB
        XCTAssertTrue(HostCond.plugged(pluggedExtra: 4, status: -1)) // wireless
        XCTAssertTrue(HostCond.plugged(pluggedExtra: 0, status: 2)) // charging
        XCTAssertTrue(HostCond.plugged(pluggedExtra: 0, status: 5)) // full
        XCTAssertTrue(HostCond.plugged(.charging))
        XCTAssertTrue(HostCond.plugged(.full))
        XCTAssertFalse(HostCond.plugged(.unplugged))
        XCTAssertFalse(HostCond.plugged(.unknown))
    }

    func testThermalStatus() {
        XCTAssertEqual(.none, HostCond.thermal(-1))
        XCTAssertEqual(.none, HostCond.thermal(0))
        XCTAssertEqual(.light, HostCond.thermal(1))
        XCTAssertEqual(.moderate, HostCond.thermal(2))
        XCTAssertEqual(.severe, HostCond.thermal(3))
        XCTAssertEqual(.critical, HostCond.thermal(4))
        XCTAssertEqual(.critical, HostCond.thermal(5)) // emergency
        XCTAssertEqual(.critical, HostCond.thermal(6)) // shutdown
        XCTAssertEqual(.none, HostCond.thermal(ProcessInfo.ThermalState.nominal))
        XCTAssertEqual(.severe, HostCond.thermal(ProcessInfo.ThermalState.serious))
        // The video budget code and the hosting step agree.
        for s in [ProcessInfo.ThermalState.nominal, .fair, .serious, .critical] {
            XCTAssertEqual(HostCond.thermal(s), HostCond.thermal(Int(HostCond.thermalCode(s))))
        }
    }

    func testMeteredFromCapabilities() {
        XCTAssertTrue(HostCond.metered(notMetered: false, temporarilyNotMetered: false))
        XCTAssertFalse(HostCond.metered(notMetered: true, temporarilyNotMetered: false))
        XCTAssertFalse(HostCond.metered(notMetered: false, temporarilyNotMetered: true))
        XCTAssertTrue(HostCond.metered(expensive: true, constrained: false))
        XCTAssertTrue(HostCond.metered(expensive: false, constrained: true))
        XCTAssertFalse(HostCond.metered(expensive: false, constrained: false))
    }

    func testConditionsAreAPhone() {
        let c = HostCond.conditions(batteryPct: 250, plugged: true, metered: true, thermal: .severe)
        XCTAssertEqual(.phone, c.deviceClass)
        XCTAssertEqual(100, c.batteryPct)
        XCTAssertTrue(c.plugged)
        XCTAssertTrue(c.metered)
        XCTAssertFalse(c.meteredAllowed)
        XCTAssertEqual(.severe, c.thermal)
        XCTAssertEqual(0, HostCond.conditions(batteryPct: -5, plugged: false, metered: false, thermal: .none).batteryPct)
    }
}
