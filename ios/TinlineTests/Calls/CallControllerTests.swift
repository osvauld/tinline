import XCTest
@testable import Tinline

/*
 * The 1:1 call state machine (CallController.swift) and the system-call session (CallSession.swift) with a fake core,
 * platform and provider. Android has no unit tests for these (they run on devices); these pin the rules that matter:
 * early events replay, call waiting, End & answer, superseded calls and video off by default.
 */

final class FakeCallCore: CallCore, @unchecked Sendable {
    private let lock = NSLock()
    private var _log: [String] = []
    var log: [String] { lock.lock(); defer { lock.unlock() }; return _log }
    private func note(_ s: String) { lock.lock(); _log.append(s); lock.unlock() }
    /** Run inside `call`, before it returns: core events that beat the call id back to the controller. */
    var duringCall: (String) -> Void = { _ in }
    var nextId = "call-1"

    func call(did: String) throws -> CallInfo {
        note("call \(did)")
        duringCall(nextId)
        return CallInfo(callId: nextId, peerDid: did, peerName: "Ada", incoming: false, video: false)
    }
    func videoCall(did: String) throws -> CallInfo {
        note("videoCall \(did)")
        return CallInfo(callId: nextId, peerDid: did, peerName: "Ada", incoming: false, video: true)
    }
    func answer(callId: String) throws { note("answer \(callId)") }
    func decline(callId: String) throws { note("decline \(callId)") }
    func hangup(callId: String) throws { note("hangup \(callId)") }
    func endAndAnswer(callId: String) throws { note("endAndAnswer \(callId)") }
    func callStats() -> CallStats? { nil }
    func profile() -> ProfileInfo? { nil }
    func lockState() -> LockState { .unlocked }
    func pushMicPcm16(pcm: Data) {}
    func pullSpeakerPcm16() -> Data { Data(count: 1920) }
    func setCamera(on: Bool) throws { note("camera \(on)") }
    func videoInfo() -> VideoInfo? { nil }
    func videoStats() -> VideoStats? { nil }
    func setVideoHd(on: Bool) {}
    func setVideoViewOf(view: UInt32, widthPx: UInt32, heightPx: UInt32, visible: Bool) {}
    func setVideoBudget(budget: VideoBudget) {}
    func pushVideoFrameYuv(width: UInt32, height: UInt32, rotation: UInt32, y: Data, yRowStride: UInt32, u: Data, v: Data,
                           uvRowStride: UInt32, uvPixelStride: UInt32, fullRange: Bool) -> Bool { false }
    func pullVideoFrame() -> VideoFrame? { nil }
    func reportVideoRenderMs(ms: Float) {}
    func groups() throws -> [GroupSummary] { [] }
    func groupCallHere() -> GroupCallHere? { nil }
    func groupCallJoin(groupId: String, callId: String) throws {}
    func groupCallLeave() {}
    func groupCallRingMs() -> UInt64 { 30_000 }
    func groupCallRings(groupId: String, callId: String) -> Bool { false }
    func groupCallSetConditions(conditions: GroupCallConditions) {}
    func groupCallSetMuted(muted: Bool) {}
    func groupCallStart(groupId: String) throws -> String { "g-call" }
    func groupCalls(groupId: String) throws -> [GroupCall] { [] }
}

@MainActor
final class FakeCallPlatform: CallPlatform {
    var micGranted = true
    var cameraGranted = true
    var proximity: [Bool] = []
    func requestMic(then: @escaping @MainActor () -> Void) { then() }
    func requestMicAndCamera(then: @escaping @MainActor () -> Void) { then() }
    func requestCamera(then: @escaping @MainActor (Bool) -> Void) { then(cameraGranted) }
    func startRing() -> CallRing? { nil }
    func vibrate(_ on: Bool) {}
    func waitingBeep() {}
    func setProximity(_ on: Bool) { proximity.append(on) }
}

@MainActor
final class CallControllerTests: XCTestCase {
    private var core: FakeCallCore!
    private var platform: FakeCallPlatform!
    private var env: CallsEnv!

    override func setUp() async throws {
        core = FakeCallCore()
        platform = FakeCallPlatform()
        env = CallsEnv(platform: platform, provider: nil, readings: FakeReadings())
        env.useCore(core)
    }

    private var calls: CallController { env.controller }

    private func incoming(_ id: String, video: Bool = false) -> CallInfo {
        CallInfo(callId: id, peerDid: "did:key:z6MkTestBea", peerName: "Bea", incoming: true, video: video)
    }

    /** Lets the detached core calls run. */
    private func settle() async { try? await Task.sleep(nanoseconds: 50_000_000) }

    func testEventsBeforeTheCallIdReturnsAreReplayed() async throws {
        // The core says Ringing before `call` has returned the id: held, then applied once the call is known.
        let controller = calls
        core.duringCall = { id in
            DispatchQueue.main.sync { MainActor.assumeIsolated { controller.onState(id, .ringing) } }
        }
        let info = try await calls.place(did: "did:key:z6MkTestAda")
        XCTAssertEqual("call-1", info.callId)
        XCTAssertTrue(calls.ui?.isRinging == true)
    }

    func testPlacingWithoutTheMicrophoneThrows() async {
        platform.micGranted = false
        do { _ = try await calls.place(did: "did:key:z6MkTestAda"); XCTFail("must ask for the microphone first") }
        catch { XCTAssertTrue(error is MicPermissionNeeded) }
        XCTAssertNil(calls.ui)
    }

    func testAVoiceCallKeepsTheCameraOff() async throws {
        _ = try await calls.place(did: "did:key:z6MkTestAda")
        await settle()
        XCTAssertFalse(core.log.contains { $0.hasPrefix("camera") }, "video off by default: \(core.log)")
        XCTAssertFalse(env.video.wantCamera)
    }

    func testVideoButtonIsForOneCallOnly() async throws {
        env.video.wantCamera = true
        platform.cameraGranted = false
        _ = try await calls.place(did: "did:key:z6MkTestAda")
        await settle()
        XCTAssertTrue(core.log.contains("videoCall did:key:z6MkTestAda"))
        // Rung as video without the camera permission: camera off now, asked for once video runs.
        XCTAssertTrue(core.log.contains("camera false"))
        XCTAssertTrue(env.video.wantCamera)
        calls.onState("call-1", .ended(reason: "hangup"))
        XCTAssertFalse(env.video.wantCamera, "the call's end clears it")
    }

    func testASecondCallDuringAnActiveOneWaits() {
        calls.onIncoming(incoming("a"))
        calls.onState("a", .active)
        calls.onIncoming(incoming("b"))
        XCTAssertEqual("a", calls.ui?.info.callId)
        XCTAssertEqual("b", calls.waiting?.callId)
        // The waiting caller gives up: a missed call, the current call goes on.
        calls.onState("b", .ended(reason: "cancelled"))
        XCTAssertNil(calls.waiting)
        XCTAssertEqual("a", calls.ui?.info.callId)
    }

    func testEndAndAnswerGoesStraightToTheNewCall() async {
        calls.onIncoming(incoming("a"))
        calls.onState("a", .active)
        calls.onIncoming(incoming("b"))
        calls.endAndAnswer()
        await settle()
        XCTAssertTrue(core.log.contains("endAndAnswer b"))
        calls.onState("a", .ended(reason: "hangup"))
        XCTAssertEqual("b", calls.ui?.info.callId)
        XCTAssertTrue(calls.ui?.answered == true)
        XCTAssertNil(calls.ended, "no Call ended screen in between")
    }

    func testEndedCallLeavesAnEndedScreen() {
        calls.onIncoming(incoming("a"))
        calls.onState("a", .active)
        calls.onState("a", .ended(reason: "hangup"))
        XCTAssertNil(calls.ui)
        XCTAssertEqual("Bea", calls.ended?.peerName)
        XCTAssertTrue(calls.ended?.wasActive == true)
    }

    func testStatesOfOtherCallsAreIgnored() {
        calls.onIncoming(incoming("a"))
        calls.onState("zzz", .active)
        XCTAssertTrue(calls.ui?.isRinging == true)
    }

    func testMuteFollowsTheButtonAndTheSystem() {
        calls.onIncoming(incoming("a"))
        calls.onState("a", .active)
        calls.toggleMute()
        XCTAssertTrue(calls.ui?.muted == true)
        calls.toggleMute()
        XCTAssertTrue(calls.ui?.muted == false)
    }

    func testHangupMarksTheTapAndCallsTheCore() async {
        calls.onIncoming(incoming("a"))
        calls.onState("a", .active)
        calls.hangup()
        await settle()
        XCTAssertGreaterThan(calls.endTapMs, 0)
        XCTAssertTrue(core.log.contains("hangup a"))
    }
}

@MainActor
final class FakeReadings: HostReadingSource {
    var current = HostReading(batteryPct: 80, plugged: true, thermal: .none, metered: false)
    func start(onChange: @escaping (HostReading) -> Void) { onChange(current) }
    func stop() {}
}

// MARK: - CallSession

@MainActor
final class FakeSystemCall: SystemCall {
    var ops: [String] = []
    func answer(video: Bool) async -> Bool { ops.append("answer \(video)"); return true }
    func setActive() async -> Bool { ops.append("active"); return true }
    func disconnect(_ cause: CallEndCause) async { ops.append("disconnect \(cause.rawValue)") }
    func select(_ route: AudioRoute) async -> Bool { ops.append("route \(route.key)"); return true }
}

@MainActor
final class FakeProvider: CallProvider {
    let call = FakeSystemCall()
    var gate: CheckedContinuation<Void, Never>?
    var hold = false
    func addCall(name: String, incoming: Bool, video: Bool, events: SystemCallEvents) async throws -> SystemCall {
        if hold { await withCheckedContinuation { gate = $0 } }
        return call
    }
}

@MainActor
final class CallSessionTests: XCTestCase {
    private func engine() -> CallAudioEngine { CallAudioEngine(core: { nil }) }
    private func settle() async { try? await Task.sleep(nanoseconds: 50_000_000) }

    func testOperationsBeforeTheSystemAnswersAreQueuedInOrder() async {
        let p = FakeProvider(); p.hold = true
        let s = CallSession(provider: p, engine: engine(), name: "Bea", incoming: true, preferSpeaker: false,
                            onAnswer: {}, onDisconnect: {}, onSystemMute: { _ in })
        s.answering()
        await settle()
        XCTAssertEqual([], p.call.ops)
        p.gate?.resume()
        await settle()
        XCTAssertEqual(["answer false"], p.call.ops)
        s.end(.local)
        await settle()
        XCTAssertEqual(["answer false", "disconnect local"], p.call.ops)
    }

    func testVideoAndGroupCallsMoveOffTheEarpieceOnce() async {
        let p = FakeProvider()
        let s = CallSession(provider: p, engine: engine(), name: "Bea", incoming: false, video: true, preferSpeaker: true,
                            onAnswer: {}, onDisconnect: {}, onSystemMute: { _ in })
        await settle()
        let ear = AudioRoute(kind: .earpiece, name: "", key: "ear"), spk = AudioRoute(kind: .speaker, name: "", key: "spk")
        s.systemRoutes(RouteState(current: ear, available: [ear, spk]))
        await settle()
        XCTAssertEqual(["route spk"], p.call.ops)
        // Back to the earpiece by hand: kept.
        s.systemRoutes(RouteState(current: ear, available: [ear, spk]))
        await settle()
        XCTAssertEqual(["route spk"], p.call.ops)
    }

    func testOnlySystemMuteChangesCount() {
        var seen: [Bool] = []
        let s = CallSession(provider: nil, engine: engine(), name: "Bea", incoming: false, preferSpeaker: false,
                            onAnswer: {}, onDisconnect: {}, onSystemMute: { seen.append($0) })
        s.systemMuted(false)
        s.systemMuted(false)
        s.systemMuted(true)
        XCTAssertEqual([true], seen)
    }

    func testARemoteDisconnectEndsTheCallOnce() {
        var disconnects = 0
        let s = CallSession(provider: nil, engine: engine(), name: "Bea", incoming: false, preferSpeaker: false,
                            onAnswer: {}, onDisconnect: { disconnects += 1 }, onSystemMute: { _ in })
        s.systemDisconnected()
        s.end(.remote)
        XCTAssertEqual(1, disconnects)
    }
}
