import XCTest
@testable import Tinline

/// The hold-to-record state machine, with the core recorder and the mic faked.
@MainActor
final class VoiceRecStateTests: XCTestCase {
    private final class FakeMic: VoiceMicInput {
        var started = false, stopped = false
        func start(onChunk: @escaping @Sendable ([Int16]) -> Void) -> Bool { started = true; return true }
        func stop() { stopped = true }
    }
    private final class FakeRecorder: VoiceCoreRecorder {
        var elapsedMs = 0, canceled = false, durationOnFinish = 2000
        func push(_ pcm: [Int16]) throws {}
        func finish() throws -> (durationMs: Int, waveform: Data) { (durationOnFinish, Data(repeating: 9, count: 64)) }
        func cancel() { canceled = true }
    }

    private var mic = FakeMic()
    private var rec = FakeRecorder()

    override func setUp() {
        mic = FakeMic(); rec = FakeRecorder()
        VoiceAudio.mic = { [unowned self] in self.mic }
        VoiceAudio.recorder = { [unowned self] _ in self.rec }
    }

    func testReleaseSendsAFileOfEnoughLength() {
        var sent: RecordedVoice?
        let s = VoiceRecState(onSend: { sent = $0 })
        XCTAssertTrue(s.begin()); XCTAssertTrue(s.recording); XCTAssertTrue(mic.started)
        s.send()
        XCTAssertFalse(s.recording); XCTAssertTrue(mic.stopped)
        XCTAssertEqual(sent?.durationMs, 2000); XCTAssertEqual(sent?.waveform.count, 64)
    }

    func testAnAccidentalTapShorterThanHalfASecondIsDropped() {
        rec.durationOnFinish = 300
        var sent: RecordedVoice?
        let s = VoiceRecState(onSend: { sent = $0 })
        XCTAssertTrue(s.begin()); s.send()
        XCTAssertNil(sent)
    }

    func testCancelDiscardsAndAnActiveCallBlocksRecording() {
        var sent: RecordedVoice?
        let s = VoiceRecState(canRecord: { false }, onSend: { sent = $0 })
        XCTAssertFalse(s.begin())
        let s2 = VoiceRecState(onSend: { sent = $0 })
        XCTAssertTrue(s2.begin()); s2.cancel()
        XCTAssertTrue(rec.canceled); XCTAssertNil(sent); XCTAssertFalse(s2.recording)
    }

    func testCannotBeginTwice() {
        let s = VoiceRecState(onSend: { _ in })
        XCTAssertTrue(s.begin()); XCTAssertFalse(s.begin())
        s.cancel()
    }
}
