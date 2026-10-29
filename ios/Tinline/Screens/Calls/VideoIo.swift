import AVFoundation
import CoreGraphics
import Foundation
import QuartzCore
import SwiftUI
import UIKit

/*
 * VideoIo.kt on iOS: the only place call video frames cross into and out of the core. Android has a JNI fast path
 * (direct buffers, RGBA out of Rust); iOS uses the UniFFI path Android keeps for comparison: camera planes go in as
 * `Data` through `pushVideoFrameYuv`, decoded I420 comes out through `pullVideoFrame` and is converted here
 * ([i420ToBgra]) into a CGImage. Camera capture itself is behind [CallCamera].
 */

/** One camera frame's planes, as `pushVideoFrameYuv` takes them (full range). */
struct CameraFrame {
    var width: Int
    var height: Int
    /** Clockwise degrees to turn the picture upright. */
    var rotation: Int
    var y: Data
    var yRowStride: Int
    /** NV12 (the iPhone camera's bi-planar format): the whole interleaved CbCr plane here, `v` empty, pixel stride 2. */
    var u: Data
    var v: Data
    var uvRowStride: Int
    var uvPixelStride: Int
}

/** The camera for the self-view and the encoder. Main actor; frames arrive on the camera's own queue. */
@MainActor
protocol CallCamera: AnyObject {
    /** Starts capture at `preference`; throws [CaptureUnsupported] when this size cannot be had (one fallback follows). */
    func start(front: Bool, preference: CapturePreference, onFrame: @escaping @Sendable (CameraFrame) -> Void) throws
    func stop()
    /** The live preview for the self-view (nil while stopped). */
    var previewLayer: CALayer? { get }
}

struct CameraUnavailable: Swift.Error {}

/**
 * TODO(ios-device): an AVCaptureSession (`.vga640x480` for SD, `.hd1280x720` for HD; CaptureUnsupported when
 * `canSetSessionPreset` refuses), the front or back wide-angle AVCaptureDevice, an AVCaptureVideoDataOutput with
 * `kCVPixelFormatType_420YpCbCr8BiPlanarFullRange` and `alwaysDiscardsLateVideoFrames`, on a serial queue; each
 * CVPixelBuffer is locked and its two planes copied into a [CameraFrame] (rotation from the device orientation and the
 * camera position), and an AVCaptureVideoPreviewLayer (mirrored for the front camera) for [previewLayer]. Not written
 * blind: until then starting fails and the call screen says "Camera unavailable"; the call goes on as voice.
 */
@MainActor
final class CallCameraStub: CallCamera {
    func start(front: Bool, preference: CapturePreference, onFrame: @escaping @Sendable (CameraFrame) -> Void) throws {
        throw CameraUnavailable()
    }
    func stop() {}
    var previewLayer: CALayer? { nil }
}

/**
 * The real camera: an AVCaptureSession at `.vga640x480` (SD) or `.hd1280x720` (HD), NV12 full range, late frames
 * discarded. The core turns the picture upright from `rotation`, so buffers are not rotated here; the preview layer
 * mirrors the front camera by itself, the frames sent are not mirrored.
 */
@MainActor
final class AVCallCamera: CallCamera {
    private var session: AVCaptureSession?
    private var preview: AVCaptureVideoPreviewLayer?
    private var sink: CameraSink?
    private var orientationObserver: NSObjectProtocol?
    private let queue = DispatchQueue(label: "tl-camera", qos: .userInteractive)

    var previewLayer: CALayer? { preview }

    func start(front: Bool, preference: CapturePreference, onFrame: @escaping @Sendable (CameraFrame) -> Void) throws {
        stop()
        let preset: AVCaptureSession.Preset = preference == .hd ? .hd1280x720 : .vga640x480
        let s = AVCaptureSession()
        guard s.canSetSessionPreset(preset) else { throw CaptureUnsupported(message: "preset \(preference.width)x\(preference.height) refused") }
        guard let dev = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: front ? .front : .back),
              let input = try? AVCaptureDeviceInput(device: dev), s.canAddInput(input) else { throw CameraUnavailable() }
        s.beginConfiguration()
        s.sessionPreset = preset
        s.addInput(input)
        let out = AVCaptureVideoDataOutput()
        out.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarFullRange]
        out.alwaysDiscardsLateVideoFrames = true
        guard s.canAddOutput(out) else { s.commitConfiguration(); throw CameraUnavailable() }
        s.addOutput(out)
        s.commitConfiguration()

        let sink = CameraSink(front: front, orientation: UIDevice.current.orientation, onFrame: onFrame)
        out.setSampleBufferDelegate(sink, queue: queue)
        let layer = AVCaptureVideoPreviewLayer(session: s)
        layer.videoGravity = .resizeAspectFill
        UIDevice.current.beginGeneratingDeviceOrientationNotifications()
        orientationObserver = NotificationCenter.default.addObserver(forName: UIDevice.orientationDidChangeNotification, object: nil, queue: .main) { _ in
            sink.setOrientation(UIDevice.current.orientation)
        }
        session = s; preview = layer; self.sink = sink
        queue.async { s.startRunning() }
    }

    func stop() {
        sink?.stop()
        sink = nil
        if let o = orientationObserver {
            NotificationCenter.default.removeObserver(o)
            UIDevice.current.endGeneratingDeviceOrientationNotifications()
        }
        orientationObserver = nil
        preview?.removeFromSuperlayer()
        preview = nil
        if let s = session { session = nil; queue.async { s.stopRunning() } }
    }
}

/** Receives the camera's buffers on its queue and copies each into a [CameraFrame]. */
private final class CameraSink: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate, @unchecked Sendable {
    private let front: Bool
    private let onFrame: @Sendable (CameraFrame) -> Void
    private let lock = NSLock()
    private var orientation: UIDeviceOrientation
    private var live = true

    init(front: Bool, orientation: UIDeviceOrientation, onFrame: @escaping @Sendable (CameraFrame) -> Void) {
        self.front = front; self.orientation = orientation; self.onFrame = onFrame
    }

    func setOrientation(_ o: UIDeviceOrientation) { lock.lock(); if o.isValidInterfaceOrientation { orientation = o }; lock.unlock() }
    func stop() { lock.lock(); live = false; lock.unlock() }

    /** Clockwise degrees to turn the sensor's landscape buffer upright (both cameras deliver landscape-right natively). */
    private func rotation() -> Int {
        lock.lock(); defer { lock.unlock() }
        switch orientation {
        case .portraitUpsideDown: return 270
        case .landscapeLeft: return 0      // home button on the right: the sensor's own orientation
        case .landscapeRight: return 180
        default: return 90                 // portrait
        }
    }

    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection) {
        lock.lock(); let go = live; lock.unlock()
        guard go, let pb = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        CVPixelBufferLockBaseAddress(pb, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pb, .readOnly) }
        guard CVPixelBufferGetPlaneCount(pb) == 2,
              let yBase = CVPixelBufferGetBaseAddressOfPlane(pb, 0), let uvBase = CVPixelBufferGetBaseAddressOfPlane(pb, 1) else { return }
        let yStride = CVPixelBufferGetBytesPerRowOfPlane(pb, 0), uvStride = CVPixelBufferGetBytesPerRowOfPlane(pb, 1)
        let h = CVPixelBufferGetHeightOfPlane(pb, 0), uvH = CVPixelBufferGetHeightOfPlane(pb, 1)
        onFrame(CameraFrame(width: CVPixelBufferGetWidthOfPlane(pb, 0), height: h, rotation: rotation(),
                            y: Data(bytes: yBase, count: yStride * h), yRowStride: yStride,
                            u: Data(bytes: uvBase, count: uvStride * uvH), v: Data(), uvRowStride: uvStride, uvPixelStride: 2))
    }
}

final class VideoIo: @unchecked Sendable {
    private let core: @Sendable () -> CallCore?
    private let lock = NSLock()
    init(core: @escaping @Sendable () -> CallCore?) { self.core = core }

    // ---- in: camera queue only
    /** Average time of the push call (FFI), ns. */
    private(set) var pushNs: Int64 = 0
    private(set) var pushed: Int64 = 0
    private(set) var inSize = "-"

    /** One camera frame to the encoder; the core converts it. False = dropped by the core (camera off, no video). */
    @discardableResult
    func push(_ f: CameraFrame) -> Bool {
        guard let core = core() else { return false }
        let t0 = DispatchTime.now().uptimeNanoseconds
        let ok = core.pushVideoFrameYuv(width: UInt32(f.width), height: UInt32(f.height), rotation: UInt32(f.rotation), y: f.y,
                                        yRowStride: UInt32(f.yRowStride), u: f.u, v: f.v, uvRowStride: UInt32(f.uvRowStride),
                                        uvPixelStride: UInt32(f.uvPixelStride), fullRange: true)
        let dt = Int64(DispatchTime.now().uptimeNanoseconds - t0)
        lock.lock()
        pushNs = Self.avg(pushNs, dt)
        inSize = "\(f.width)x\(f.height)"
        if ok { pushed += 1 }
        lock.unlock()
        return ok
    }

    // ---- out: one render at a time (the call screen and the strip's tile each have a RemoteRenderer on this VideoIo)
    private let pullLock = NSLock()
    private var bgra: [UInt32] = []
    /** Average time of a pull that returned a frame / of its conversion, ns. */
    private(set) var pullNs: Int64 = 0
    private(set) var convertNs: Int64 = 0

    /** The newest decoded frame as an image, or nil if none arrived since the last call. */
    func pull() -> CGImage? {
        pullLock.lock(); defer { pullLock.unlock() }
        guard let core = core() else { return nil }
        let tp = DispatchTime.now().uptimeNanoseconds
        guard let f = core.pullVideoFrame() else { return nil }
        let w = Int(f.width), h = Int(f.height)
        let t0 = DispatchTime.now().uptimeNanoseconds
        if bgra.count != w * h { bgra = [UInt32](repeating: 0, count: w * h) }
        Self.i420ToBgra(f.data, w, h, &bgra)
        let img = Self.image(bgra, w, h)
        let t1 = DispatchTime.now().uptimeNanoseconds
        lock.lock()
        pullNs = Self.avg(pullNs, Int64(t0 - tp))
        convertNs = Self.avg(convertNs, Int64(t1 - t0))
        lock.unlock()
        // The core adds this render cost (pull + conversion) to the decode cost when it steers the frame rate.
        core.reportVideoRenderMs(ms: Float(t1 - tp) / 1e6)
        return img
    }

    /** 32-bit little-endian 0xAARRGGBB words are B, G, R, A in memory: BGRA with alpha first in the word. */
    private static func image(_ px: [UInt32], _ w: Int, _ h: Int) -> CGImage? {
        let data = px.withUnsafeBufferPointer { Data(buffer: $0) }
        guard let provider = CGDataProvider(data: data as CFData) else { return nil }
        let info = CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        return CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                       space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: info, provider: provider, decode: nil,
                       shouldInterpolate: true, intent: .defaultIntent)
    }

    /** Moving average (1/10 weight) of per-frame times, for the stats line. */
    private static func avg(_ old: Int64, _ x: Int64) -> Int64 { old == 0 ? x : (old * 9 + x) / 10 }

    /** BT.601 limited range, fixed point (x1024), two pixels at a time sharing chroma. Out: 0xFFRRGGBB words. */
    static func i420ToBgra(_ src: Data, _ w: Int, _ h: Int, _ out: inout [UInt32]) {
        let cw = (w + 1) / 2
        let uOff = w * h
        let vOff = uOff + cw * ((h + 1) / 2)
        guard src.count >= vOff + cw * ((h + 1) / 2), out.count >= w * h else { return }
        src.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            let s = raw.bindMemory(to: UInt8.self)
            @inline(__always) func clamp(_ v: Int) -> UInt32 { UInt32(v < 0 ? 0 : (v > 255 ? 255 : v)) }
            for y in 0..<h {
                let yRow = y * w
                let cRow = (y >> 1) * cw
                var x = 0
                while x < w {
                    let ci = cRow + (x >> 1)
                    let u = Int(s[uOff + ci]) - 128
                    let v = Int(s[vOff + ci]) - 128
                    let rd = 1634 * v
                    let gd = -401 * u - 832 * v
                    let bd = 2066 * u
                    var k = 0
                    while k < 2 && x + k < w {
                        let yy = (Int(s[yRow + x + k]) - 16) * 1192
                        let r = clamp((yy + rd) >> 10), g = clamp((yy + gd) >> 10), b = clamp((yy + bd) >> 10)
                        out[yRow + x + k] = 0xFF00_0000 | (r << 16) | (g << 8) | b
                        k += 1
                    }
                    x += 2
                }
            }
        }
    }
}

/**
 * Pulls the peer's frames on the display clock (a CADisplayLink, as Android's Choreographer), converts them off the
 * main thread and publishes the image; latest wins, so a slow frame is just skipped.
 */
@MainActor
final class RemoteRenderer {
    private let io: VideoIo
    private let onFrame: (CGImage) -> Void
    private var link: CADisplayLink?
    private var busy = false
    private let queue = DispatchQueue(label: "tl-video-render", qos: .userInteractive)
    private(set) var frames: Int64 = 0

    init(io: VideoIo, onFrame: @escaping (CGImage) -> Void) {
        self.io = io; self.onFrame = onFrame
        let l = CADisplayLink(target: DisplayTarget { [weak self] in self?.tick() }, selector: #selector(DisplayTarget.fire))
        l.add(to: .main, forMode: .common)
        link = l
    }

    private func tick() {
        if busy { return }
        busy = true
        let io = io
        queue.async { [weak self] in
            let img = io.pull()
            Task { @MainActor in
                guard let self else { return }
                self.busy = false
                if let img, self.link != nil { self.frames += 1; self.onFrame(img) }
            }
        }
    }

    func stop() { link?.invalidate(); link = nil }

    /** CADisplayLink retains its target; this small one holds the renderer weakly. */
    private final class DisplayTarget: NSObject {
        let f: () -> Void
        init(_ f: @escaping () -> Void) { self.f = f }
        @objc func fire() { f() }
    }
}

/** The camera's live preview as a SwiftUI view (filling, cropped), for the self-view. */
struct CallPreviewLayerView: UIViewRepresentable {
    let layer: CALayer?
    func makeUIView(context: Context) -> PreviewHost { PreviewHost() }
    func updateUIView(_ v: PreviewHost, context: Context) { v.show(layer) }

    final class PreviewHost: UIView {
        private var shown: CALayer?
        func show(_ l: CALayer?) {
            if shown === l { return }
            shown?.removeFromSuperlayer()
            shown = l
            if let l { layer.addSublayer(l); setNeedsLayout() }
        }
        override func layoutSubviews() { super.layoutSubviews(); shown?.frame = bounds }
    }
}
