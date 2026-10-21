import Foundation
import ImageIO
import Photos
import UIKit
import UniformTypeIdentifiers

/// Files around attachments (android ChatMedia.kt). The core keeps attachments encrypted; anything shown or shared is
/// decrypted into `Caches/chat_media` (wiped at app start) with `Node.saveAttachment`.
enum ChatMedia {
    private static var cache: URL { FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0] }
    static var mediaDir: URL { cache.appendingPathComponent("chat_media", isDirectory: true) }
    static var outDir: URL { cache.appendingPathComponent("chat_out", isDirectory: true) }
    static var voiceDir: URL { cache.appendingPathComponent("voice", isDirectory: true) }

    static func clear() {
        for d in [mediaDir, outDir, cache.appendingPathComponent("chat_camera", isDirectory: true)] { try? FileManager.default.removeItem(at: d) }
    }

    /// Bytes of decrypted copies and scratch files (what `clear` removes).
    static func size() -> Int64 {
        let fm = FileManager.default
        return [mediaDir, outDir, cache.appendingPathComponent("chat_camera", isDirectory: true)].reduce(0) { total, d in
            let files = fm.enumerator(at: d, includingPropertiesForKeys: [.fileSizeKey])
            var n: Int64 = 0
            while let u = files?.nextObject() as? URL { n += Int64((try? u.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0) }
            return total + n
        }
    }

    static func safe(_ name: String) -> String {
        let allowed = Set("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._ -")
        let s = String(String(name.map { allowed.contains($0) ? $0 : "_" }).prefix(80))
        return s.trimmingCharacters(in: .whitespaces).isEmpty ? "file" : s
    }

    /// The decrypted copy of `m`'s attachment, made on first use.
    static func decrypt(source: ChatSource, message m: Message) async throws -> URL {
        guard let a = m.attachment else { throw ChatMediaError.noAttachment }
        let f = mediaDir.appendingPathComponent("\(a.hash.prefix(16))_\(safe(a.name))")
        let fm = FileManager.default
        let size = (try? fm.attributesOfItem(atPath: f.path)[.size] as? Int) ?? 0
        if size == 0 {
            try fm.createDirectory(at: mediaDir, withIntermediateDirectories: true)
            let tmp = URL(fileURLWithPath: f.path + ".part")
            try? fm.removeItem(at: tmp)
            try await source.save(peerDid: m.peerDid, messageId: m.id, destPath: tmp.path)
            try? fm.removeItem(at: f)
            try fm.moveItem(at: tmp, to: f)
        }
        return f
    }

    /// Decodes `file` down to about `maxPx` on the long side (ImageIO thumbnail: never loads the full bitmap).
    static func image(_ file: URL, maxPx: Int = 1280) -> UIImage? {
        guard let src = CGImageSourceCreateWithURL(file as CFURL, nil) else { return nil }
        let opts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true, kCGImageSourceThumbnailMaxPixelSize: maxPx,
        ]
        guard let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary) else { return nil }
        return UIImage(cgImage: cg)
    }

    /// Copies a picked file URL into app cache under its own name; returns the copy and its MIME type.
    static func copyIn(_ url: URL) throws -> (URL, String) {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let dir = outDir.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let out = dir.appendingPathComponent(safe(url.lastPathComponent))
        try FileManager.default.copyItem(at: url, to: out)
        return (out, mime(for: out))
    }

    static func mime(for url: URL) -> String {
        UTType(filenameExtension: url.pathExtension.lowercased())?.preferredMIMEType ?? "application/octet-stream"
    }

    /// Removes the scratch folder a sent file lived in (`chat_out/<uuid>/name`).
    static func discardOut(_ file: URL) {
        let dir = file.deletingLastPathComponent()
        if dir.deletingLastPathComponent().lastPathComponent == "chat_out" { try? FileManager.default.removeItem(at: dir) }
    }

    /// "Save to phone": images and videos go to the photo library (add-only access); anything else is offered to Files.
    @MainActor
    static func saveToPhone(_ file: URL, name: String, mime: String) async -> Bool {
        if mime.hasPrefix("image/") || mime.hasPrefix("video/") {
            let status = await PHPhotoLibrary.requestAuthorization(for: .addOnly)
            guard status == .authorized || status == .limited else { return false }
            return await withCheckedContinuation { c in
                PHPhotoLibrary.shared().performChanges({
                    if mime.hasPrefix("image/") { PHAssetChangeRequest.creationRequestForAssetFromImage(atFileURL: file) }
                    else { PHAssetChangeRequest.creationRequestForAssetFromVideo(atFileURL: file) }
                }, completionHandler: { ok, _ in c.resume(returning: ok) })
            }
        }
        // The exporter wants the file under its display name.
        let named = file.deletingLastPathComponent().appendingPathComponent(safe(name))
        if named != file { try? FileManager.default.removeItem(at: named); try? FileManager.default.copyItem(at: file, to: named) }
        return ChatPresenter.present(UIDocumentPickerViewController(forExporting: [named], asCopy: true))
    }

    /// The system share sheet for a decrypted copy.
    @MainActor
    @discardableResult
    static func share(_ file: URL) -> Bool { ChatPresenter.present(UIActivityViewController(activityItems: [file], applicationActivities: nil)) }
}

enum ChatMediaError: Swift.Error { case noAttachment }

/// Presents UIKit controllers (share sheet, exporter) from the foremost view controller.
enum ChatPresenter {
    @MainActor
    static func present(_ vc: UIViewController) -> Bool {
        let scene = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first { $0.activationState == .foregroundActive }
        guard var top = scene?.windows.first(where: \.isKeyWindow)?.rootViewController else { return false }
        while let p = top.presentedViewController { top = p }
        vc.popoverPresentationController?.sourceView = top.view
        top.present(vc, animated: true)
        return true
    }
}
