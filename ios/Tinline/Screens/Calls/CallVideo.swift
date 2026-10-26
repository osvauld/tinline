import Combine
import Foundation
import QuartzCore
import UIKit

/**
 * Video of the current call: the core's state ([info]), our camera (only while the call screen is visible) and the
 * remote picture ([RemoteRenderer]). Frames cross into the core only through [io]. Port of CallVideo.kt; implements the
 * core's `VideoEvents` (register with `node.setVideoEvents`).
 *
 * Video is off by default: a voice call never turns the camera on by itself; only the video button (placing), "Video"
 * (answering) or the camera button does.
 */
@MainActor
final class CallVideo: ObservableObject, VideoEvents {
    @Published private(set) var info: VideoInfo?
    @Published private(set) var front = true
    /** Shown on the call screen, e.g. when the camera permission was refused. */
    @Published private(set) var problem: String?
    /** The HD chip's choice for this call (nil = follow Settings). Never written to Settings. */
    @Published private(set) var hdCall: Bool?
    /** The core wants the camera at HD (`VideoStats.hdAuto`); followed unless the chip or Settings decide. */
    @Published private(set) var hdAuto = false
    /** The chip's short line; `id` changes with every new line so the screen can time it out after `ms`. */
    struct HdNote: Equatable { var id: Int; var text: String; var ms: Int64 = hdNoteMs }
    @Published private(set) var hdNote: HdNote?

    private let core: @Sendable () -> CallCore?
    private let platform: CallPlatform
    private let directory: CallDirectory
    let camera: CallCamera
    let io: VideoIo
    weak var controller: CallController?
    /** The active network is metered (HostConditions' reading). */
    var metered: () -> Bool = { false }

    private var noteId = 0
    private var hdMeteredSaid = false
    /** The camera was on when the call screen went away; it comes back on with the screen. */
    private var paused = false
    /**
     * The conversation's video button was pressed: [CallController.place] reads it for the call it places (the call's
     * start clears it, so does its end). Then only for a video call placed without the camera permission: the camera
     * is tried once video is negotiated (without the permission: "Camera permission needed").
     */
    var wantCamera = false
    private let rates = VideoRateMeter()
    private var capturePreference = "-"
    private var cameraGen = 0
    private var nextViewId: UInt32 = 1

    init(core: @escaping @Sendable () -> CallCore?, platform: CallPlatform, directory: CallDirectory, camera: CallCamera) {
        self.core = core; self.platform = platform; self.directory = directory; self.camera = camera
        self.io = VideoIo(core: core)
    }

    func hdOn() -> Bool { effectivePreferHd(setting: directory.preferHdCapture || hdAuto, callChoice: hdCall) }

    func say(_ text: String, ms: Int64 = hdNoteMs) { noteId += 1; hdNote = HdNote(id: noteId, text: text, ms: ms) }
    func noteShown(_ id: Int) { if hdNote?.id == id { hdNote = nil } }

    /** Applies at once: the call screen restarts the camera when [hdCall] changes. */
    func setHd(_ on: Bool, paused: Bool) {
        CallLog.test("video_hd_chip on=\(on)")
        hdCall = on
        say(hdLine(on: on, paused: paused && on))
        syncHd(on)
    }

    /** Tells the core whether the camera captures HD for this call. */
    func syncHd(_ on: Bool) {
        let core = core
        callBackground("setVideoHd") { core()?.setVideoHd(on: on) }
    }

    /** Once a second while my camera sends: the core's HD choice (docs/video.md "Sharper video"), then the mobile-data note. */
    func hdTick(_ stats: VideoStats) {
        if hdAuto != stats.hdAuto { CallLog.test("video_hd_auto on=\(stats.hdAuto)") }
        hdAuto = stats.hdAuto
        if hdNote == nil && hdMeteredNow(hdOn: hdOn(), sending: true, metered: metered(), saidThisCall: hdMeteredSaid) {
            hdMeteredSaid = true
            say(hdMeteredLine, ms: hdMeteredMs)
        }
    }

    // MARK: VideoEvents (core threads)

    nonisolated func onVideoState(info: VideoInfo) {
        Task { @MainActor in self.videoState(info) }
    }

    private func videoState(_ info: VideoInfo) {
        CallLog.test("video_state id=\(info.callId) negotiated=\(info.negotiated) local=\(info.localCamera) remote=\(info.remoteCamera)")
        if let cur = controller?.ui, cur.info.callId != info.callId { return }
        self.info = info
        controller?.onVideo(info)
        if wantCamera && info.negotiated && !info.localCamera { wantCamera = false; setCamera(true) }
    }

    func setCamera(_ on: Bool) {
        paused = false
        if on && !platform.cameraGranted { problem = tr("call_camera_permission_needed"); return }
        problem = nil
        let core = core
        callBackground("setCamera(\(on))") { try core()?.setCamera(on: on) }
    }

    func cameraDenied() { problem = tr("call_camera_permission_needed") }

    func flip() { front.toggle() }

    /** Call screen hidden (backgrounded, minimised, screen off): no capture in the background. */
    func screenHidden() {
        if info?.localCamera == true { setCamera(false); paused = true }
    }

    func screenShown() { if paused { setCamera(true) } }

    /** The call ended (or a new one starts). */
    func reset() {
        hdMeteredSaid = false
        info = nil; problem = nil; paused = false; wantCamera = false; front = true; hdCall = nil; hdAuto = false; hdNote = nil
        rates.reset(); capturePreference = "-"
    }

    /** Ids for [setView]: each place that draws the remote picture has its own. */
    func newViewId() -> UInt32 { defer { nextViewId += 1 }; return nextViewId }

    /** One remote picture's size in pixels and whether it is on screen; the core asks the peer for the largest one shown. */
    func setView(_ id: UInt32, width: Int, height: Int, visible: Bool) {
        if width <= 0 || height <= 0 { return }
        let core = core
        callBackground("setVideoViewOf") {
            core()?.setVideoViewOf(view: id, widthPx: UInt32(width), heightPx: UInt32(height), visible: visible)
        }
    }

    /** Thermal state and Low Power Mode, while a call with video is on screen; the core decides what they mean. */
    func watchBudget() -> () -> Void {
        let core = core
        let push = {
            let p = ProcessInfo.processInfo
            let b = VideoBudget(thermal: HostCond.thermalCode(p.thermalState), powerSave: p.isLowPowerModeEnabled)
            callBackground("setVideoBudget") { core()?.setVideoBudget(budget: b) }
        }
        let nc = NotificationCenter.default
        let obs = [ProcessInfo.thermalStateDidChangeNotification, Notification.Name.NSProcessInfoPowerStateDidChange].map {
            nc.addObserver(forName: $0, object: nil, queue: .main) { _ in push() }
        }
        push()
        return { for o in obs { nc.removeObserver(o) } }
    }

    func statsLine(_ r: RemoteRenderer?) -> String {
        guard let core = core() else { return "video_stats none" }
        let i = core.videoInfo()
        let s = core.videoStats()
        var measured: VideoRates?
        if let i, let s {
            measured = rates.sample(nowNanos: Int64(DispatchTime.now().uptimeNanoseconds), counters: VideoRateSnapshot(
                callId: i.callId, admittedFrames: s.framesSent, receivedFrames: s.framesReceived, decodedFrames: s.framesDecoded,
                admittedPayloadBytes: s.admittedPayloadBytes, receivedPayloadBytes: s.receivedPayloadBytes))
        } else { rates.reset() }
        func value(_ v: Double?) -> String { v.map { String(format: "%.1f", $0) } ?? "unknown" }
        func opt<T>(_ v: T?) -> String { v.map { "\($0)" } ?? "null" }
        let ms = { (ns: Int64) in String(format: "%.2f", Double(ns) / 1e6) }
        return "video_stats negotiated=\(opt(i?.negotiated)) local=\(opt(i?.localCamera)) remote=\(opt(i?.remoteCamera)) " +
            "sent=\(opt(s?.framesSent)) keys=\(opt(s?.keyframesSent)) send=\(opt(s?.sendWidth))x\(opt(s?.sendHeight)) " +
            "kbps=\(opt(s.map { $0.bitrateBps / 1000 })) " +
            "admitted_payload_kbps=\(value(measured?.admittedPayloadKbps)) received_payload_kbps=\(value(measured?.receivedPayloadKbps)) " +
            "admitted_fps=\(value(measured?.admittedFps)) received_fps=\(value(measured?.receivedFps)) decoded_fps=\(value(measured?.decodedFps)) " +
            "capture_pref=\(capturePreference) reports=\(opt(s?.rateReports)) overuse=\(opt(s?.rateOveruse)) starved=\(opt(s?.rateStarved)) " +
            "drops=\(opt(s?.backlogDrops)) q_ms=\(opt(s.map { $0.lastQueuingUs / 1000 })) q_base_ms=\(opt(s.map { $0.queuingOffsetUs / 1000 })) " +
            "delivered_kbps=\(opt(s.map { $0.lastDeliveredBps / 1000 })) draining=\(opt(s?.draining)) " +
            "received=\(opt(s?.framesReceived)) decoded=\(opt(s?.framesDecoded)) errors=\(opt(s?.decodeErrors)) " +
            "recv=\(opt(s?.recvWidth))x\(opt(s?.recvHeight)) path=uniffi camera=\(io.inSize) pushed=\(io.pushed) " +
            "push_ms=\(ms(io.pushNs)) rendered=\(r?.frames ?? 0) pull_ms=\(ms(io.pullNs)) convert_ms=\(ms(io.convertNs))"
    }

    // MARK: capture

    /** Starts the camera for the self-view and the encoder: HD first when it is on, one fallback to SD. */
    func startCamera() {
        cameraGen += 1
        let gen = cameraGen
        let io = io
        do {
            let chosen = try bindCapture(capturePreferences(preferHd: hdOn())) { requested in
                CallLog.test("camera attempt preference=\(requested.width)x\(requested.height) front=\(front)")
                // Frames of a stopped generation never arrive: the camera's own stop() ends its output first.
                try camera.start(front: front, preference: requested) { frame in io.push(frame) }
            }
            guard gen == cameraGen else { return }
            capturePreference = "\(chosen.width)x\(chosen.height)"
            problem = nil
            CallLog.test("camera started front=\(front) preference=\(capturePreference)")
        } catch {
            CallLog.warn("start camera: \(error)")
            problem = tr("call_camera_unavailable")
        }
    }

    func stopCamera() {
        cameraGen += 1
        camera.stop()
        capturePreference = "-"
        CallLog.test("camera stopped")
    }
}
