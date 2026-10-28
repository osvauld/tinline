import AVFoundation
import Combine
import SwiftUI
import UIKit

// Voice messages: record and play Ogg Opus through the core's VoiceRecorder / VoiceDecoder (android VoiceMessage.kt).
// The core does the Opus work; this file feeds it 16 kHz mono PCM from the microphone and plays what it decodes. The
// platform audio glue sits behind `VoiceMicInput` and `VoiceSpeaker` so the state machines below are plain Swift.

let voiceRate = 16_000
let voiceMinMs = 500
let voiceMaxMs = 15 * 60 * 1000

/// A finished recording: the Ogg Opus file plus the metadata the chat message carries.
struct RecordedVoice { let path: String; let durationMs: Int; let waveform: Data }

// MARK: - Platform seams

/// The microphone: 16 kHz mono signed 16-bit chunks, with echo and noise processing on (voice chat mode).
protocol VoiceMicInput: AnyObject {
    /// Starts delivering chunks (any size, on an audio thread). False if the mic cannot be opened (permission missing, or
    /// a call holds it).
    func start(onChunk: @escaping @Sendable ([Int16]) -> Void) -> Bool
    func stop()
}

/// The speaker for voice playback: the player pulls decoded PCM through `pull` until it returns empty.
protocol VoiceSpeaker: AnyObject {
    /// Plays until `pull` returns empty (then calls `finished(true)`) or `stop()` (then `finished(false)`). Runs on its own
    /// thread; `pull` and `finished` are called from there.
    func play(sampleRate: Int, pull: @escaping @Sendable (Int) -> [Int16], finished: @escaping @Sendable (Bool) -> Void)
    func stop()
}

/// The core's recorder as the capture needs it (`uniffi VoiceRecorder`), so tests can fake it.
protocol VoiceCoreRecorder: AnyObject {
    func push(_ pcm: [Int16]) throws
    var elapsedMs: Int { get }
    func finish() throws -> (durationMs: Int, waveform: Data)
    func cancel()
}

final class CoreVoiceRecorder: VoiceCoreRecorder {
    private let r: VoiceRecorder
    init(path: String) throws { r = try VoiceRecorder.start(path: path) }
    func push(_ pcm: [Int16]) throws { try r.push(pcm: pcm) }
    var elapsedMs: Int { Int(r.elapsedMs()) }
    func finish() throws -> (durationMs: Int, waveform: Data) { let i = try r.finish(); return (Int(i.durationMs), i.waveform) }
    func cancel() { r.cancel() }
}

/// The core's decoder as the player needs it (`uniffi VoiceDecoder`).
protocol VoiceCoreDecoder: AnyObject {
    var durationMs: Int { get }
    var positionMs: Int { get }
    var sampleRate: Int { get }
    func read(maxSamples: Int) throws -> [Int16]
    func seek(ms: Int)
}

final class CoreVoiceDecoder: VoiceCoreDecoder {
    private let d: VoiceDecoder
    init(path: String) throws { d = try VoiceDecoder.open(path: path) }
    var durationMs: Int { Int(d.durationMs()) }
    var positionMs: Int { Int(d.positionMs()) }
    var sampleRate: Int { Int(d.sampleRate()) }
    func read(maxSamples: Int) throws -> [Int16] { try d.read(maxSamples: UInt32(maxSamples)) }
    func seek(ms: Int) { d.seek(ms: UInt32(ms)) }
}

/// Real audio. // TODO(ios-device): written from Apple's documented AVAudioEngine patterns and never run; verify on a
/// device (route changes, Bluetooth, interruptions by calls, the voice-processing mic, converter drift).
final class AVVoiceMic: VoiceMicInput {
    private let engine = AVAudioEngine()
    private var running = false

    func start(onChunk: @escaping @Sendable ([Int16]) -> Void) -> Bool {
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.playAndRecord, mode: .voiceChat, options: [.defaultToSpeaker, .allowBluetooth])
            try session.setActive(true)
        } catch { return false }
        let input = engine.inputNode
        let inFormat = input.outputFormat(forBus: 0)
        guard inFormat.sampleRate > 0,
              let outFormat = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: Double(voiceRate), channels: 1, interleaved: true),
              let converter = AVAudioConverter(from: inFormat, to: outFormat) else { return false }
        input.installTap(onBus: 0, bufferSize: 1024, format: inFormat) { buffer, _ in
            let capacity = AVAudioFrameCount(Double(buffer.frameLength) * outFormat.sampleRate / inFormat.sampleRate) + 32
            guard let out = AVAudioPCMBuffer(pcmFormat: outFormat, frameCapacity: capacity) else { return }
            var fed = false
            var error: NSError?
            converter.convert(to: out, error: &error) { _, status in
                if fed { status.pointee = .noDataNow; return nil }
                fed = true; status.pointee = .haveData; return buffer
            }
            if error == nil, out.frameLength > 0, let p = out.int16ChannelData {
                onChunk(Array(UnsafeBufferPointer(start: p[0], count: Int(out.frameLength))))
            }
        }
        do { try engine.start() } catch { input.removeTap(onBus: 0); return false }
        running = true
        return true
    }

    func stop() {
        guard running else { return }
        running = false
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
    }
}

/// // TODO(ios-device): as `AVVoiceMic`: AVAudioEngine + AVAudioPlayerNode, buffers scheduled four deep, never run.
final class AVVoiceSpeaker: VoiceSpeaker {
    private let lock = NSLock()
    private var generation = 0
    private var engine: AVAudioEngine?

    func play(sampleRate: Int, pull: @escaping @Sendable (Int) -> [Int16], finished: @escaping @Sendable (Bool) -> Void) {
        lock.lock(); generation += 1; let my = generation; lock.unlock()
        let session = AVAudioSession.sharedInstance()
        try? session.setCategory(.playback, mode: .spokenAudio)
        try? session.setActive(true)
        Thread.detachNewThread { [weak self] in
            guard let self, let format = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: Double(sampleRate), channels: 1, interleaved: false) else { finished(false); return }
            let engine = AVAudioEngine(), node = AVAudioPlayerNode()
            engine.attach(node)
            engine.connect(node, to: engine.mainMixerNode, format: format)
            self.lock.lock(); self.engine = engine; self.lock.unlock()
            guard (try? engine.start()) != nil else { finished(false); return }
            node.play()
            let inFlight = DispatchSemaphore(value: 4)
            var completed = false
            while self.current == my {
                let pcm = pull(sampleRate / 20)
                if pcm.isEmpty { completed = true; break }
                guard let buf = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(pcm.count)), let ch = buf.floatChannelData else { break }
                buf.frameLength = AVAudioFrameCount(pcm.count)
                for i in 0..<pcm.count { ch[0][i] = Float(pcm[i]) / 32768 }
                inFlight.wait()
                node.scheduleBuffer(buf) { inFlight.signal() }
            }
            if completed { for _ in 0..<4 { inFlight.wait() } }   // let the last buffers play out
            node.stop(); engine.stop()
            finished(completed && self.current == my)
        }
    }

    private var current: Int { lock.lock(); defer { lock.unlock() }; return generation }

    func stop() { lock.lock(); generation += 1; engine?.stop(); engine = nil; lock.unlock() }
}

/// Where the voice code gets real audio from; tests replace the factories.
enum VoiceAudio {
    static var mic: () -> VoiceMicInput = { AVVoiceMic() }
    static var speaker: () -> VoiceSpeaker = { AVVoiceSpeaker() }
    static var recorder: (String) throws -> VoiceCoreRecorder = { try CoreVoiceRecorder(path: $0) }
    static var decoder: (String) throws -> VoiceCoreDecoder = { try CoreVoiceDecoder(path: $0) }
}

// MARK: - Recording

/// One mic feeding the core recorder; ends by itself at 15 minutes (`onLimit`).
final class VoiceCapture: @unchecked Sendable {
    private let mic: VoiceMicInput
    private let core: VoiceCoreRecorder
    private let file: URL
    private let lock = NSLock()
    private var running = true
    /// Called once on an audio thread when the recording reaches `voiceMaxMs`.
    var onLimit: (@Sendable () -> Void)?

    private init(mic: VoiceMicInput, core: VoiceCoreRecorder, file: URL) { self.mic = mic; self.core = core; self.file = file }

    var elapsedMs: Int { core.elapsedMs }

    /// Nil if the mic cannot be opened (permission missing, or a call holds it).
    static func start() -> VoiceCapture? {
        let dir = ChatMedia.voiceDir
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let file = dir.appendingPathComponent("rec-\(Int(Date().timeIntervalSince1970 * 1000)).opus")
        guard let core = try? VoiceAudio.recorder(file.path) else { return nil }
        let mic = VoiceAudio.mic()
        let cap = VoiceCapture(mic: mic, core: core, file: file)
        let ok = mic.start { [weak cap] pcm in cap?.feed(pcm) }
        if !ok { core.cancel(); return nil }
        return cap
    }

    private func feed(_ pcm: [Int16]) {
        lock.lock(); let go = running; lock.unlock()
        guard go, !pcm.isEmpty else { return }
        try? core.push(pcm)
        if core.elapsedMs >= voiceMaxMs {
            lock.lock(); let was = running; running = false; lock.unlock()
            if was { onLimit?() }
        }
    }

    private func halt() { lock.lock(); running = false; lock.unlock(); mic.stop() }

    /// Stops and returns the file, or nil (file deleted) if it is shorter than `voiceMinMs`.
    func finish() -> RecordedVoice? {
        halt()
        do {
            let info = try core.finish()
            if info.durationMs < voiceMinMs { try? FileManager.default.removeItem(at: file); return nil }
            return RecordedVoice(path: file.path, durationMs: info.durationMs, waveform: info.waveform)
        } catch { try? FileManager.default.removeItem(at: file); return nil }
    }

    func cancel() { halt(); core.cancel() }
}

/// Hold-to-record state for one composer (a 1:1 chat or a group). `onSend` gets the finished file. Drive it with
/// `VoiceMicButton` and show `VoiceRecordingBar` in place of the text field while `recording`. One records at a time in
/// the app, and starting stops the voice message playing.
@MainActor
final class VoiceRecState: ObservableObject {
    @Published private(set) var recording = false
    @Published var locked = false
    /// 0...1 how far the finger has slid left; at 1 releasing cancels.
    @Published var cancelPull: CGFloat = 0
    @Published private(set) var elapsedMs = 0
    private var capture: VoiceCapture?
    private var ticker: Task<Void, Never>?
    /// Gets the finished file; the owner of the composer sets it (it needs the conversation model).
    var onSend: (RecordedVoice) -> Void
    private let canRecord: () -> Bool
    private let makeCapture: () -> VoiceCapture?
    /// The composer recording now, if any: a second one does not start.
    private static weak var active: VoiceRecState?

    init(canRecord: @escaping () -> Bool = { true }, makeCapture: @escaping () -> VoiceCapture? = { VoiceCapture.start() }, onSend: @escaping (RecordedVoice) -> Void = { _ in }) {
        self.canRecord = canRecord; self.makeCapture = makeCapture; self.onSend = onSend
    }

    /// False if the mic is busy (in a call) or could not be opened.
    func begin() -> Bool {
        if recording || !canRecord() { return false }
        if let a = Self.active, a !== self, a.recording { return false }
        guard let c = makeCapture() else { return false }
        Self.active = self
        VoicePlayer.shared.stop()
        c.onLimit = { [weak self] in Task { @MainActor in self?.send() } }
        capture = c; recording = true; locked = false; cancelPull = 0; elapsedMs = 0
        ticker = Task { [weak self] in
            while let s = self, s.recording { s.tick(); try? await Task.sleep(nanoseconds: 100_000_000) }
        }
        return true
    }

    func send() {
        guard let c = capture else { return }
        reset()
        if let v = c.finish() { onSend(v) }
    }

    func cancel() {
        guard let c = capture else { return }
        reset()
        c.cancel()
    }

    func tick() { elapsedMs = capture?.elapsedMs ?? 0 }

    private func reset() { capture = nil; recording = false; locked = false; cancelPull = 0; ticker?.cancel(); ticker = nil }
}

/// The mic button. Hold to record, release to send, slide left to cancel, up to lock (then it becomes Send).
struct VoiceMicButton: View {
    @ObservedObject var state: VoiceRecState
    @Environment(\.tin) private var tin
    @State private var touching = false
    @State private var began = false
    @State private var cancelled = false
    @State private var tapWhenLocked = false
    @State private var asking = false
    private let cancelPx: CGFloat = 96, lockPx: CGFloat = 64

    var body: some View {
        let big = state.recording && !state.locked
        let size: CGFloat = big ? 64 : 48
        ZStack {
            if big {
                // The lock pill the finger slides up into.
                VStack {
                    ChatIcon(glyph: .lock, size: 20, color: tin.ink2)
                    Spacer()
                    ChatIcon(glyph: .chevUp, size: 18, color: tin.ink2)
                }
                .padding(.vertical, 12).frame(width: 40, height: 96).background(tin.sf, in: Capsule())
                .overlay(Capsule().strokeBorder(tin.ln, lineWidth: 1)).offset(y: -120)
            }
            Circle().fill(tin.pr.opacity(big ? 0.18 : 0)).frame(width: size + (big ? 16 : 0), height: size + (big ? 16 : 0))
            Circle().fill(tin.pr).frame(width: size, height: size)
            ChatIcon(glyph: state.locked ? .send : .mic, size: big ? 28 : 24, color: tin.onPr)
        }
        .frame(width: 48, height: 48).contentShape(Rectangle())
        .gesture(DragGesture(minimumDistance: 0).onChanged(changed).onEnded(ended))
        .accessibilityElement().accessibilityAddTraits(.isButton)
        .accessibilityLabel(state.locked ? tr("chat_voice_send") : state.recording ? tr("chat_voice_recording") : tr("chat_voice_hold"))
        .accessibilityAction { if state.locked { state.send() } }
    }

    private func changed(_ v: DragGesture.Value) {
        if !touching {
            touching = true; began = false; cancelled = false; tapWhenLocked = false
            if state.locked { tapWhenLocked = true; return }
            Task { await startRecording() }
            return
        }
        guard began, !cancelled, !state.locked else { return }
        let dx = v.translation.width, dy = v.translation.height
        state.cancelPull = min(1, max(0, -dx / cancelPx))
        if -dx >= cancelPx { cancelled = true; haptic(.heavy); state.cancel() }
        else if -dy >= lockPx && abs(dy) > abs(dx) { state.locked = true; state.cancelPull = 0; haptic(.light) }
    }

    private func ended(_ v: DragGesture.Value) {
        defer { touching = false }
        if tapWhenLocked { state.send(); return }
        if began && !cancelled && !state.locked { state.send() }
    }

    private func startRecording() async {
        guard AVAudioApplication.shared.recordPermission == .granted else {
            // The first press asks; the press itself records nothing, as on Android.
            if !asking && AVAudioApplication.shared.recordPermission == .undetermined {
                asking = true
                _ = await AVAudioApplication.requestRecordPermission()
                asking = false
            }
            return
        }
        if state.begin() { began = true; haptic(.heavy) } else { haptic(.light) }
    }

    private func haptic(_ s: UIImpactFeedbackGenerator.FeedbackStyle) { UIImpactFeedbackGenerator(style: s).impactOccurred() }
}

/// Replaces the text field while recording: red dot, timer, "Slide to cancel" (or a trash button once locked).
struct VoiceRecordingBar: View {
    @ObservedObject var state: VoiceRecState
    @Environment(\.tin) private var tin
    var body: some View {
        HStack(spacing: 10) {
            Circle().fill(tin.er).frame(width: 10, height: 10)
            Text(clockMs(ms: UInt32(max(0, state.elapsedMs)))).font(TinType.mono).foregroundStyle(tin.ink).monospacedDigit()
            if state.locked {
                Spacer(minLength: 0)
                Button { state.cancel() } label: { ChatIcon(glyph: .trash, size: 22, color: tin.er).frame(width: 40, height: 40) }
                    .accessibilityLabel(tr("chat_voice_discard"))
            } else {
                HStack(spacing: 4) {
                    ChatIcon(glyph: .back, size: 16, color: tin.ink2)
                    Text(tr("chat_voice_slide_cancel")).font(TinType.bodyM).foregroundStyle(tin.ink2)
                }
                .frame(maxWidth: .infinity).offset(x: -state.cancelPull * 48)
            }
        }
        .padding(.leading, 16).padding(.trailing, 6).frame(minHeight: 48)
        .background(tin.sf, in: SmoothRoundedRectangle(cornerRadius: 24))
        .overlay(SmoothRoundedRectangle(cornerRadius: 24).stroke(tin.ln, lineWidth: 1))
    }
}

// MARK: - Playback

struct PlayState: Equatable { let id: String; let playing: Bool; let positionMs: Int; let durationMs: Int }

/// One message plays at a time. A call starting (`pause()` from the Calls area) or an audio interruption pauses it.
/// Not done: routing to the earpiece when the phone is held to the ear (proximity sensor), as on Android.
@MainActor
final class VoicePlayer: ObservableObject {
    static let shared = VoicePlayer()
    @Published private(set) var state: PlayState?

    private var decoder: VoiceCoreDecoder?
    private var id: String?
    private var speaker: VoiceSpeaker?
    private var gen = 0
    private var interruption: AnyCancellable?

    private init() {
        interruption = NotificationCenter.default.publisher(for: AVAudioSession.interruptionNotification).sink { [weak self] n in
            if let raw = n.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt, AVAudioSession.InterruptionType(rawValue: raw) == .began {
                Task { @MainActor in self?.pause() }
            }
        }
    }

    /// Starts `id`'s file at `fromMs` (or continues if it is the current message and `fromMs` is nil).
    func play(id: String, path: String, fromMs: Int? = nil) {
        if self.id != id {
            stopPlayback()
            guard let d = try? VoiceAudio.decoder(path) else { return }
            decoder = d; self.id = id
        }
        guard let d = decoder else { return }
        if let fromMs { stopPlayback(); d.seek(ms: fromMs) } else if speaker != nil { return }
        if d.positionMs >= d.durationMs { d.seek(ms: 0) }
        start(d, id)
    }

    func pause() {
        stopPlayback()
        guard let d = decoder, let id else { return }
        state = PlayState(id: id, playing: false, positionMs: d.positionMs, durationMs: d.durationMs)
    }

    func stop() { stopPlayback(); decoder = nil; id = nil; state = nil }

    private func stopPlayback() { gen += 1; speaker?.stop(); speaker = nil }

    private func start(_ d: VoiceCoreDecoder, _ id: String) {
        gen += 1
        let my = gen
        let dur = d.durationMs
        state = PlayState(id: id, playing: true, positionMs: d.positionMs, durationMs: dur)
        let sp = VoiceAudio.speaker()
        speaker = sp
        let box = DecoderBox(d)
        sp.play(sampleRate: d.sampleRate, pull: { n in (try? box.d.read(maxSamples: n)) ?? [] }, finished: { [weak self] done in
            Task { @MainActor in
                guard let self, self.gen == my else { return }
                if done {
                    box.d.seek(ms: 0)
                    self.speaker = nil
                    self.state = PlayState(id: id, playing: false, positionMs: 0, durationMs: dur)
                }
            }
        })
        // The position bar follows the decoder every 100 ms while this playback is current.
        Task { [weak self] in
            while let s = self, s.gen == my, s.speaker != nil {
                s.state = PlayState(id: id, playing: true, positionMs: box.d.positionMs, durationMs: dur)
                try? await Task.sleep(nanoseconds: 100_000_000)
            }
        }
    }
}

/// The decoder is read from the audio thread only while playing; the player never touches it concurrently.
private final class DecoderBox: @unchecked Sendable { let d: VoiceCoreDecoder; init(_ d: VoiceCoreDecoder) { self.d = d } }

// MARK: - Bubble

/// The inside of a voice message bubble (the chat supplies the bubble itself): play/pause, waveform that fills with
/// progress, and the time. Tap the waveform to seek. `id` identifies the message so only one plays at a time.
struct VoiceBubbleContent: View {
    let id: String
    let path: String
    let durationMs: Int
    let waveform: Data
    let outgoing: Bool
    @ObservedObject private var player = VoicePlayer.shared
    @Environment(\.tin) private var tin

    var body: some View {
        let mine = player.state?.id == id ? player.state : nil
        let playing = mine?.playing == true
        let progress: CGFloat = (mine != nil && durationMs > 0) ? min(1, max(0, CGFloat(mine!.positionMs) / CGFloat(durationMs))) : 0
        let idle = outgoing ? tin.onPrc.opacity(0.35) : tin.ln2
        let shown = (mine != nil && (playing || mine!.positionMs > 0)) ? mine!.positionMs : durationMs
        HStack(spacing: 10) {
            Button { if playing { player.pause() } else { player.play(id: id, path: path) } } label: {
                ChatIcon(glyph: playing ? .pause : .play, size: 22, color: tin.onPr).frame(width: 40, height: 40).background(tin.pr, in: Circle())
            }
            .accessibilityLabel(playing ? tr("chat_voice_pause") : tr("chat_voice_play"))
            GeometryReader { geo in
                Canvas { ctx, size in
                    let barW: CGFloat = 3, gap: CGFloat = 2
                    let n = max(8, Int((size.width + gap) / (barW + gap)))
                    let bars = ChatFmt.bars(waveform, n)
                    let minH: CGFloat = 4
                    let used = CGFloat(n) * (barW + gap) - gap
                    let x0 = (size.width - used) / 2
                    for i in 0..<n {
                        let h = minH + (size.height - minH) * CGFloat(bars[i].squareRoot())
                        let r = CGRect(x: x0 + CGFloat(i) * (barW + gap), y: (size.height - h) / 2, width: barW, height: h)
                        ctx.fill(Path(roundedRect: r, cornerRadius: barW / 2), with: .color((CGFloat(i) + 0.5) / CGFloat(n) <= progress ? tin.pr : idle))
                    }
                }
                .contentShape(Rectangle())
                .gesture(SpatialTapGesture().onEnded { t in
                    let ms = Int((t.location.x / max(1, geo.size.width) * CGFloat(durationMs)).rounded())
                    player.play(id: id, path: path, fromMs: min(max(0, ms), durationMs))
                })
            }
            .frame(height: 28).accessibilityLabel(tr("chat_voice_waveform"))
            Text(clockMs(ms: UInt32(max(0, shown)))).font(.custom("IBMPlexMono-Medium", size: 12, relativeTo: .caption))
                .foregroundStyle(outgoing ? tin.onPrc : tin.ink2).monospacedDigit()
        }
        .frame(minWidth: 230)
    }
}
