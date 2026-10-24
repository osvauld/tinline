import Foundation
import Network
import UIKit

/*
 * The phone's call-hosting conditions (GA-A1, group-call-spec §2.4): battery level and charging, thermal state and
 * metered data. Pushed to the core on start and on every change; the core advertises no call hosting before the first
 * push. Port of HostConditions.kt; the readings come from UIDevice (battery), ProcessInfo (thermal state) and
 * NWPathMonitor (expensive/constrained paths) behind [HostReadingSource].
 */

/** One snapshot of the platform readings. */
struct HostReading: Equatable {
    var batteryPct: Int
    var plugged: Bool
    var thermal: GroupCallThermal
    var metered: Bool
}

/** Delivers readings on start and whenever one changes (main actor). */
@MainActor
protocol HostReadingSource: AnyObject {
    func start(onChange: @escaping (HostReading) -> Void)
    func stop()
    /** The latest reading (metered is also read by the HD note). */
    var current: HostReading { get }
}

/**
 * UIDevice battery monitoring, ProcessInfo thermal state and an NWPathMonitor on the default path, all documented
 * APIs. TODO(ios-device): check how often batteryLevelDidChange fires (iOS reports in 5 % steps) and that a
 * Personal-Hotspot Wi-Fi reports `isExpensive`.
 */
@MainActor
final class DeviceHostReadings: HostReadingSource {
    private var observers: [NSObjectProtocol] = []
    private var monitor: NWPathMonitor?
    private var metered = false
    private var onChange: ((HostReading) -> Void)?

    var current: HostReading {
        let d = UIDevice.current
        return HostReading(batteryPct: HostCond.batteryPct(level: d.batteryLevel), plugged: HostCond.plugged(d.batteryState),
                           thermal: HostCond.thermal(ProcessInfo.processInfo.thermalState), metered: metered)
    }

    func start(onChange: @escaping (HostReading) -> Void) {
        // CallsEnv.attach runs on every unlock and account switch: never stack a second set of observers and monitors.
        stop()
        self.onChange = onChange
        UIDevice.current.isBatteryMonitoringEnabled = true
        let nc = NotificationCenter.default
        let names: [Notification.Name] = [UIDevice.batteryLevelDidChangeNotification, UIDevice.batteryStateDidChangeNotification,
                                          ProcessInfo.thermalStateDidChangeNotification]
        for n in names {
            observers.append(nc.addObserver(forName: n, object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.fire() }
            })
        }
        let m = NWPathMonitor()
        m.pathUpdateHandler = { [weak self] path in
            let metered = HostCond.metered(expensive: path.isExpensive, constrained: path.isConstrained)
            Task { @MainActor in self?.metered = metered; self?.fire() }
        }
        m.start(queue: DispatchQueue(label: "tl-host-path"))
        monitor = m
        fire()
    }

    func stop() {
        for o in observers { NotificationCenter.default.removeObserver(o) }
        observers.removeAll()
        monitor?.cancel(); monitor = nil
        onChange = nil
    }

    private func fire() { onChange?(current) }
}

@MainActor
final class HostConditions {
    private let core: @Sendable () -> CallCore?
    private let source: HostReadingSource
    private var last: GroupCallConditions?

    init(core: @escaping @Sendable () -> CallCore?, source: HostReadingSource) { self.core = core; self.source = source }

    var metered: Bool { source.current.metered }

    func start() { source.start { [weak self] r in self?.push(r) } }
    func stop() { source.stop() }

    /** Pushes when something changed (battery notifications also come for unrelated state). */
    private func push(_ r: HostReading) {
        let c = HostCond.conditions(batteryPct: r.batteryPct, plugged: r.plugged, metered: r.metered, thermal: r.thermal)
        if c == last { return }
        last = c
        CallLog.test("host-conditions battery=\(c.batteryPct) plugged=\(c.plugged) metered=\(c.metered) thermal=\(c.thermal)")
        let core = core
        callBackground("set conditions") { core()?.groupCallSetConditions(conditions: c) }
    }
}

/** Platform readings -> the core's [GroupCallConditions]; pure, so it runs in unit tests. */
enum HostCond {
    /** Android BatteryManager status codes, kept for the shared tests (STATUS_CHARGING, STATUS_FULL). */
    private static let statusCharging = 2
    private static let statusFull = 5

    /** EXTRA_LEVEL / EXTRA_SCALE -> 0..100. Unknown counts as empty: an unknown phone does not host. */
    static func batteryPct(level: Int, scale: Int) -> Int {
        level < 0 || scale <= 0 ? 0 : min(max(level * 100 / scale, 0), 100)
    }

    /** UIDevice.batteryLevel (0...1, -1 unknown) -> 0..100; unknown counts as empty, as above. */
    static func batteryPct(level: Float) -> Int {
        level < 0 ? 0 : min(max(Int((level * 100).rounded()), 0), 100)
    }

    /** EXTRA_PLUGGED (any source) or a charging/full status. */
    static func plugged(pluggedExtra: Int, status: Int) -> Bool {
        pluggedExtra != 0 || status == statusCharging || status == statusFull
    }

    static func plugged(_ state: UIDevice.BatteryState) -> Bool { state == .charging || state == .full }

    /** PowerManager.THERMAL_STATUS_*: 0 none, 1 light, 2 moderate, 3 severe, 4 critical, 5 emergency, 6 shutdown. */
    static func thermal(_ status: Int) -> GroupCallThermal {
        if status <= 0 { return .none }
        if status == 1 { return .light }
        if status == 2 { return .moderate }
        if status == 3 { return .severe }
        return .critical
    }

    /**
     * ProcessInfo's four steps onto the core's five. `fair` is Apple's "slightly elevated" (light); `serious` already
     * throttles (severe); `critical` is critical. Nothing maps to moderate.
     */
    static func thermal(_ s: ProcessInfo.ThermalState) -> GroupCallThermal {
        switch s {
        case .nominal: .none
        case .fair: .light
        case .serious: .severe
        case .critical: .critical
        @unknown default: .critical
        }
    }

    /** The same steps as Android's thermal status code, for the video budget the core reads (`VideoBudget.thermal`). */
    static func thermalCode(_ s: ProcessInfo.ThermalState) -> UInt8 {
        switch s {
        case .nominal: 0
        case .fair: 1
        case .serious: 3
        case .critical: 4
        @unknown default: 4
        }
    }

    /** The default network is metered unless it says it is not (also temporarily, API 30+). */
    static func metered(notMetered: Bool, temporarilyNotMetered: Bool) -> Bool { !(notMetered || temporarilyNotMetered) }

    /** iOS: an expensive path (cellular, Personal Hotspot) or Low Data Mode counts as metered. */
    static func metered(expensive: Bool, constrained: Bool) -> Bool { expensive || constrained }

    static func conditions(batteryPct: Int, plugged: Bool, metered: Bool, thermal: GroupCallThermal) -> GroupCallConditions {
        GroupCallConditions(deviceClass: .phone, plugged: plugged, batteryPct: UInt8(min(max(batteryPct, 0), 100)),
                            metered: metered, meteredAllowed: false, thermal: thermal)
    }
}
