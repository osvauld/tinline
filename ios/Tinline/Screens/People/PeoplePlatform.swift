import SwiftUI
import UIKit
import Vision
import CoreImage.CIFilterBuiltins

/*
 * Platform pieces the people area needs (android Qr.kt, ContactQrDecoder.kt, CardShare.kt, Support.kt clipboard). The QR
 * generator, the QR-from-image decoder, the clipboard and the share sheet use ordinary documented APIs. Live camera scanning is
 * a device-only piece and is stubbed behind `PeopleQRCameraView`.
 */

enum PeopleClipboard {
    static var text: String? { UIPasteboard.general.string }
    static func copy(_ s: String) { UIPasteboard.general.string = s }
}

/// The QR image of a contact code (ZXing in Qr.kt): dark modules on white with a quiet zone, drawn without smoothing.
enum PeopleQR {
    static func image(_ text: String, dark: UIColor = UIColor(red: 0x17 / 255, green: 0x20 / 255, blue: 0x1D / 255, alpha: 1)) -> UIImage? {
        let f = CIFilter.qrCodeGenerator()
        f.message = Data(text.utf8)
        f.correctionLevel = "L"
        guard let out = f.outputImage else { return nil }
        let tint = CIFilter.falseColor()
        tint.inputImage = out
        tint.color0 = CIColor(color: dark)
        tint.color1 = CIColor(color: .white)
        guard let tinted = tint.outputImage else { return nil }
        let scaled = tinted.transformed(by: CGAffineTransform(scaleX: 8, y: 8))
        guard let cg = CIContext().createCGImage(scaled, from: scaled.extent) else { return nil }
        return UIImage(cgImage: cg)
    }
}

/// What reading a QR out of a chosen picture gave (ContactQrImageResult).
enum PeopleQRImageResult: Equatable {
    case decoded(String)
    case noQr
    case multiple
    case unreadable
}

/// Reads exactly one QR code out of an image (Vision). More than one is refused so the wrong person is never added.
enum PeopleQRImageDecoder {
    static func decode(_ data: Data) -> PeopleQRImageResult {
        guard let img = UIImage(data: data), let cg = img.cgImage else { return .unreadable }
        let req = VNDetectBarcodesRequest()
        req.symbologies = [.qr]
        let handler = VNImageRequestHandler(cgImage: cg, orientation: CGImagePropertyOrientation(img.imageOrientation), options: [:])
        do { try handler.perform([req]) } catch { return .unreadable }
        let payloads = (req.results ?? []).compactMap { $0.payloadStringValue }
        switch payloads.count {
        case 0: return .noQr
        case 1: return .decoded(payloads[0])
        default: return Set(payloads).count == 1 ? .decoded(payloads[0]) : .multiple
        }
    }
}

private extension CGImagePropertyOrientation {
    init(_ o: UIImage.Orientation) {
        switch o {
        case .up: self = .up
        case .upMirrored: self = .upMirrored
        case .down: self = .down
        case .downMirrored: self = .downMirrored
        case .left: self = .left
        case .leftMirrored: self = .leftMirrored
        case .right: self = .right
        case .rightMirrored: self = .rightMirrored
        @unknown default: self = .up
        }
    }
}

/// The system share sheet for a code (CardShare.kt).
struct PeopleShareSheet: UIViewControllerRepresentable {
    let text: String
    func makeUIViewController(context: Context) -> UIActivityViewController { UIActivityViewController(activityItems: [text], applicationActivities: nil) }
    func updateUIViewController(_ vc: UIActivityViewController, context: Context) {}
}

/// Live camera QR scanning (Android `QrCameraBox`): 260 pt square preview, calls `onCode` once per code read.
// TODO(ios-device): AVCaptureSession + AVCaptureMetadataOutput(.qr) in a UIViewControllerRepresentable with the camera permission
// prompt (NSCameraUsageDescription); stop the session when the view disappears. This stub only draws the frame.
struct PeopleQRCameraView: View {
    @Environment(\.tin) private var c
    let label: String
    let onCode: (String) -> Void
    var body: some View {
        ZStack {
            Color.black
            Text(label).peopleStyle(.sub19).foregroundStyle(.white.opacity(0.85)).multilineTextAlignment(.center).padding(24)
        }
        .accessibilityLabel(label)
    }
}
