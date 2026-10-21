import UIKit
import ImageIO

/*
 * Picture preparation, ported from playground src/lib/model/images.ts (android ProfileCrop.kt): input checks (the packet's
 * limits), the crop, and the square PNG the core receives. Same numbers, same messages.
 */

enum ImageLimits {
    static let maxBytes = 2 * 1024 * 1024
    static let maxSide = 4096
    static let out = 512
    static let outMaxBytes = 1024 * 1024
}

/// The crop viewport is `view` pt square; the picture sits at (ox, oy) scaled so it always covers it.
struct Crop: Equatable {
    var ox: CGFloat
    var oy: CGFloat
    var zoom: CGFloat
}

func coverScale(_ w: Int, _ h: Int, view: CGFloat, zoom: CGFloat) -> CGFloat { max(view / CGFloat(w), view / CGFloat(h)) * zoom }

func clampCrop(_ c: Crop, _ w: Int, _ h: Int, view: CGFloat) -> Crop {
    let s = coverScale(w, h, view: view, zoom: c.zoom)
    let minX = view - CGFloat(w) * s, minY = view - CGFloat(h) * s
    return Crop(ox: min(0, max(minX, c.ox)), oy: min(0, max(minY, c.oy)), zoom: c.zoom)
}

/// Centre the picture in the viewport at a zoom.
func centred(_ w: Int, _ h: Int, view: CGFloat, zoom: CGFloat = 1) -> Crop {
    let s = coverScale(w, h, view: view, zoom: zoom)
    return Crop(ox: (view - CGFloat(w) * s) / 2, oy: (view - CGFloat(h) * s) / 2, zoom: zoom)
}

/// Zoom about the viewport centre, keeping the point under the centre where it is.
func zoomAt(_ c: Crop, _ zoom: CGFloat, _ w: Int, _ h: Int, view: CGFloat) -> Crop {
    let s0 = coverScale(w, h, view: view, zoom: c.zoom), s1 = coverScale(w, h, view: view, zoom: zoom)
    let cx = (view / 2 - c.ox) / s0, cy = (view / 2 - c.oy) / s0
    return clampCrop(Crop(ox: view / 2 - cx * s1, oy: view / 2 - cy * s1, zoom: zoom), w, h, view: view)
}

/// A decoded, checked picture ready for the crop step.
struct Picked: Identifiable {
    let id = UUID()
    let image: UIImage
    var w: Int { Int(image.size.width) }
    var h: Int { Int(image.size.height) }
}

enum PickResult {
    case ok(Picked)
    case refused(String)
}

private func mb(_ n: Int) -> String {
    let s = String(format: "%.1f", Double(n) / (1024 * 1024))
    return s.hasSuffix(".0") ? String(s.dropLast(2)) : s
}

/// Look at the first bytes, not the file name or the claimed type.
func sniff(_ h: Data) -> String? {
    func at(_ i: Int, _ n: UInt8) -> Bool { i < h.count && h[h.startIndex + i] == n }
    if at(0, 0x89) && at(1, 0x50) && at(2, 0x4e) && at(3, 0x47) { return "png" }
    if at(0, 0xff) && at(1, 0xd8) && at(2, 0xff) { return "jpeg" }
    if at(0, 0x52) && at(1, 0x49) && at(2, 0x46) && at(3, 0x46) && at(8, 0x57) && at(9, 0x45) && at(10, 0x42) && at(11, 0x50) { return "webp" }
    return nil
}

/// images.ts inspectImage, for bytes already read. Blocking (decode): call off the main thread. `size` is the file's real size
/// when `bytes` was cut short.
func inspectImage(_ bytes: Data, size: Int? = nil) -> PickResult {
    let total = size ?? bytes.count
    if total > ImageLimits.maxBytes {
        return .refused("That picture is \(mb(total)) MB. Pictures can be up to \(mb(ImageLimits.maxBytes)) MB.")
    }
    if sniff(bytes) == nil { return .refused("Tinline can use PNG, JPEG or WebP pictures. That file is a different kind of file.") }
    let unreadable = "That file couldn\u{2019}t be read as a picture. It may be damaged."
    guard let src = CGImageSourceCreateWithData(bytes as CFData, nil),
          let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any],
          let pw = props[kCGImagePropertyPixelWidth] as? Int, let ph = props[kCGImagePropertyPixelHeight] as? Int, pw > 0, ph > 0
    else { return .refused(unreadable) }
    if pw > ImageLimits.maxSide || ph > ImageLimits.maxSide {
        return .refused("That picture is \(pw) \u{00D7} \(ph) pixels. Tinline accepts up to \(ImageLimits.maxSide) pixels on a side.")
    }
    guard let img = UIImage(data: bytes) else { return .refused(unreadable) }
    return .ok(Picked(image: img))
}

/// App-side preparation: crop the chosen square and scale it to a PNG of at most 512 px. Blocking.
func renderSquare(_ p: Picked, _ crop: Crop, view: CGFloat) -> Data? {
    let s = coverScale(p.w, p.h, view: view, zoom: crop.zoom)
    let side = view / s
    let out = min(ImageLimits.out, max(1, Int(side.rounded())))
    let k = CGFloat(out) / side
    let fmt = UIGraphicsImageRendererFormat()
    fmt.scale = 1; fmt.opaque = false
    let renderer = UIGraphicsImageRenderer(size: CGSize(width: out, height: out), format: fmt)
    let img = renderer.image { _ in
        // The source square starts at (-ox/s, -oy/s) in picture pixels; draw the whole picture so that square fills the output.
        p.image.draw(in: CGRect(x: crop.ox / s * k, y: crop.oy / s * k, width: CGFloat(p.w) * k, height: CGFloat(p.h) * k))
    }
    return img.pngData()
}
