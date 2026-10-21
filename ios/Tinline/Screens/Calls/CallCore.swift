import AVFoundation
import Foundation
import UIKit

/*
 * What the call area needs from the core and from the rest of the app, as small protocols so the controllers are plain
 * Swift and unit-testable with fakes (TinlineTests/Calls). `Node` (ios/build/generated/p2pcore.swift) conforms as is:
 * every requirement below is a Node method with the same signature. Core calls are synchronous and may block, so the
 * controllers run them off the main actor (`callOffMain`).
 */

protocol CallCore: AnyObject, Sendable {
    // 1:1 calls
    func call(did: String) throws -> CallInfo
    func videoCall(did: String) throws -> CallInfo
    func answer(callId: String) throws
    func decline(callId: String) throws
    func hangup(callId: String) throws
    func endAndAnswer(callId: String) throws
    func callStats() -> CallStats?
    func profile() -> ProfileInfo?
    func lockState() -> LockState
    // audio
    func pushMicPcm16(pcm: Data)
    func pullSpeakerPcm16() -> Data
    // video
    func setCamera(on: Bool) throws
    func videoInfo() -> VideoInfo?
    func videoStats() -> VideoStats?
    func setVideoHd(on: Bool)
    func setVideoViewOf(view: UInt32, widthPx: UInt32, heightPx: UInt32, visible: Bool)
    func setVideoBudget(budget: VideoBudget)
    func pushVideoFrameYuv(width: UInt32, height: UInt32, rotation: UInt32, y: Data, yRowStride: UInt32, u: Data, v: Data,
                           uvRowStride: UInt32, uvPixelStride: UInt32, fullRange: Bool) -> Bool
    func pullVideoFrame() -> VideoFrame?
    func reportVideoRenderMs(ms: Float)
    // group calls
    func groups() throws -> [GroupSummary]
    func groupCallHere() -> GroupCallHere?
    func groupCallJoin(groupId: String, callId: String) throws
    func groupCallLeave()
    func groupCallRingMs() -> UInt64
    func groupCallRings(groupId: String, callId: String) -> Bool
    func groupCallSetConditions(conditions: GroupCallConditions)
    func groupCallSetMuted(muted: Bool)
    func groupCallStart(groupId: String) throws -> String
    func groupCalls(groupId: String) throws -> [GroupCall]
}

extension Node: CallCore {}

/** No node yet (locked vault, account switch in progress). */
struct CallCoreUnavailable: Error {}

/** Runs a (possibly blocking) core call off the main actor, as Kotlin's `app.scope.launch(Dispatchers.IO)`. */
@discardableResult
func callOffMain<T: Sendable>(_ work: @escaping @Sendable () throws -> T) async throws -> T {
    try await Task.detached(priority: .userInitiated) { try work() }.value
}

/** Fire-and-forget form: errors are logged with `what`, never thrown (Kotlin's `try { } catch { Log.w }`). */
func callBackground(_ what: String, _ work: @escaping @Sendable () throws -> Void) {
    Task.detached(priority: .userInitiated) {
        do { try work() } catch { CallLog.warn("\(what): \(error)") }
    }
}

/** Names and app state the call screens show that other areas own (contacts, groups, blocks, settings). */
@MainActor
protocol CallDirectory: AnyObject {
    /** This account's DID (cached; read per row). */
    var currentDid: String? { get }
    /** Settings > Video "Sharper video". */
    var preferHdCapture: Bool { get }
    /** Settings > Notifications > Calls (AlertPrefs.calls): off means no ringing. */
    var callsAlertOn: Bool { get }
    /** Settings > Sounds > Quiet (AlertPrefs.quiet). */
    var quiet: Bool { get }
    /** The app is in front, unlocked, on Home with its ring header showing (AppVisible.resumed && ringHeadUp). */
    var ringHeadShowing: Bool { get }
    func blockedByMe(_ did: String) -> Bool
    /** A group's name from the groups list, nil when unknown. */
    func groupName(_ gid: String) -> String?
    /** A person's name: the open group's member list, then contacts; nil when unknown. */
    func personName(_ did: String) -> String?
    /** A group call ended or I left: the group's call log is re-read. */
    func refreshGroupLog()
}

/** Defaults for previews and until the Shell supplies the real lookups. */
@MainActor
final class CallDirectoryStub: CallDirectory {
    var currentDid: String? = nil
    var preferHdCapture = false
    var callsAlertOn = true
    var quiet = false
    var ringHeadShowing = false
    func blockedByMe(_ did: String) -> Bool { false }
    func groupName(_ gid: String) -> String? { nil }
    func personName(_ did: String) -> String? { nil }
    func refreshGroupLog() {}
}

/** A sound that plays until stopped (the ring, the waiting beep). */
protocol CallRing: AnyObject { func stop() }

/** Permissions, ringing and the screen-off-at-ear sensor: the platform parts of [CallController]. */
@MainActor
protocol CallPlatform: AnyObject {
    var micGranted: Bool { get }
    var cameraGranted: Bool { get }
    /** Asks for the microphone (if missing), then runs `then` whatever the answer: a call can still be heard without it. */
    func requestMic(then: @escaping @MainActor () -> Void)
    /** The microphone and the camera, then `then`. A refused camera still answers, with it off. */
    func requestMicAndCamera(then: @escaping @MainActor () -> Void)
    func requestCamera(then: @escaping @MainActor (Bool) -> Void)
    /** The looping incoming ring (CallSounds.startRing), or nil when the ringer is silent. */
    func startRing() -> CallRing?
    /** The ringtone vibration (0 / 800 / 800 ms) unless the phone is fully silent. */
    func vibrate(_ on: Bool)
    /** One quiet "call waiting" beep, on the call's own audio. */
    func waitingBeep()
    func setProximity(_ on: Bool)
}

/**
 * The platform pieces that are ordinary documented API are real here (permissions, proximity monitoring); ringing and
 * vibration are stubs. TODO(ios-device): CallKit rings incoming calls itself (its ringtone and vibration follow the
 * ringer switch); an in-app ring while the app is in front needs an AVAudioPlayer of the core's `callCuePcm("ring")`,
 * and the waiting beep an AVAudioPlayerNode on the call's engine. Verify both on a device.
 */
@MainActor
final class CallPlatformIOS: CallPlatform {
    var micGranted: Bool { AVAudioApplication.shared.recordPermission == .granted }
    var cameraGranted: Bool { AVCaptureDevice.authorizationStatus(for: .video) == .authorized }

    func requestMic(then: @escaping @MainActor () -> Void) {
        if micGranted { then(); return }
        AVAudioApplication.requestRecordPermission { _ in Task { @MainActor in then() } }
    }

    func requestMicAndCamera(then: @escaping @MainActor () -> Void) {
        requestMic { [weak self] in self?.requestCamera { _ in then() } }
    }

    func requestCamera(then: @escaping @MainActor (Bool) -> Void) {
        if cameraGranted { then(true); return }
        AVCaptureDevice.requestAccess(for: .video) { ok in Task { @MainActor in then(ok) } }
    }

    func startRing() -> CallRing? { nil }
    func vibrate(_ on: Bool) {}
    func waitingBeep() {}

    func setProximity(_ on: Bool) {
        if UIDevice.current.isProximityMonitoringEnabled != on { UIDevice.current.isProximityMonitoringEnabled = on }
    }
}
