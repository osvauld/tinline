import Foundation

/*
 * CallNotif.kt on iOS. Android keeps an always-on foreground service whose notification turns into the ongoing-call one;
 * iOS has no such service: CallKit shows the call in the system UI (lock screen, Dynamic Island, the green pill) once
 * the call is reported to it ([CallSession], [CallProvider]). What stays is the pure derivation (the same rule decides
 * whether this device is "in a call" for CallKit and the audio session) and the alerts the controllers post, behind
 * [CallAlerts] with a stub until CallKit/PushKit are configured on a device.
 */

enum CallNotif {
    /** Where the 1:1 call stands, as far as the in-call indicator cares. An ended call is `.none`. */
    enum OneToOne { case none, dialing, ringing, answered, active }

    /** `title`: the in-call title, or nil when idle; `why` goes to [CallTrace]. */
    struct Derived: Equatable { var title: String?; var why: String }

    /** A group-call prepare that never settled stops counting after this (the join failed silently). */
    static let prepareTimeoutMs: Int64 = 30_000

    /**
     * Pure. `callName`/`groupName`: the titles to use. `groupInCall`: this device's user is in the group call (joining,
     * joined or reconnecting); hosting alone is not a call on this device. `preparedAtMs`: when [GroupCallAudio.prepare]
     * marked a start/join not yet seen.
     */
    static func derive(call: OneToOne, callName: String, groupInCall: Bool, groupHosting: Bool, groupName: String,
                       preparedAtMs: Int64?, nowMs: Int64) -> Derived {
        // The user answered (or End & answer): in call from the tap on, as before.
        if call == .active { return Derived(title: callName, why: "1:1 active") }
        if call == .answered { return Derived(title: callName, why: "1:1 answered") }
        if groupInCall { return Derived(title: groupName, why: "group in call") }
        if let p = preparedAtMs {
            let age = nowMs - p
            if age >= 0 && age < prepareTimeoutMs { return Derived(title: groupName, why: "group prepared \(age / 1000)s") }
        }
        let why: String
        if call == .ringing { why = "1:1 ringing" }
        else if call == .dialing { why = "1:1 dialing" }
        else if groupHosting { why = "group hosting only" }
        else if preparedAtMs != nil { why = "group prepare expired" }
        else { why = "no call" }
        return Derived(title: nil, why: "idle: \(why)")
    }

    static func of(_ u: CallUi?) -> OneToOne {
        guard let u else { return .none }
        if u.isActive { return .active }
        if u.isEnded { return .none }
        if u.answered { return .answered }
        if u.isRinging { return .ringing }
        return .dialing
    }
}

/**
 * What Android posts as notifications. On iOS the incoming call itself belongs to CallKit (reported by [CallSession]);
 * these cover the rest: the missed-call note, the call-waiting note while the call screen is away, the group ring.
 */
@MainActor
protocol CallAlerts: AnyObject {
    func incoming(_ call: CallInfo)
    func cancelIncoming()
    func waiting(_ call: CallInfo)
    func cancelWaiting()
    func missed(name: String)
    func groupRing(gid: String, group: String, by: String, ringMs: Int64)
    func cancelGroupRing()
}

/**
 * TODO(ios-device): post through UNUserNotificationCenter (missed, waiting, group ring) once notification permission
 * and categories are set up by the Shell; the incoming 1:1 ring is CallKit's (reportNewIncomingCall), which needs
 * PushKit to wake a suspended app. Until then this only traces, so the controller logic runs unchanged.
 */
@MainActor
final class CallAlertsStub: CallAlerts {
    func incoming(_ call: CallInfo) { CallTrace.add("alert incoming \(shortId(call.callId))") }
    func cancelIncoming() {}
    func waiting(_ call: CallInfo) { CallTrace.add("alert waiting \(shortId(call.callId))") }
    func cancelWaiting() {}
    func missed(name: String) { CallTrace.add("alert missed") }
    func groupRing(gid: String, group: String, by: String, ringMs: Int64) { CallTrace.add("alert group ring") }
    func cancelGroupRing() {}
}
