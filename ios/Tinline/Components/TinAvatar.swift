import SwiftUI

/// tokens.css --avN-bg / --avN-ink: the six stable hue pairs (initials fallback for a name without a DID).
func tinAvatarColors(_ c: TinColors, hue: Int) -> (bg: Color, fg: Color) {
    let h = ((hue % 6) + 6) % 6
    if c.dark {
        switch h {
        case 0: return (Color(hex: 0x164F45), Color(hex: 0xB7E6D8))
        case 1: return (Color(hex: 0x2F3A75), Color(hex: 0xDFE3FF))
        case 2: return (Color(hex: 0x5C2A35), Color(hex: 0xFFD9E0))
        case 3: return (Color(hex: 0x58431A), Color(hex: 0xFFE2AE))
        case 4: return (Color(hex: 0x3A4F26), Color(hex: 0xD8F0C0))
        default: return (Color(hex: 0x4A3458), Color(hex: 0xECD9FA))
        }
    }
    switch h {
    case 0: return (Color(hex: 0xD7ECE6), Color(hex: 0x0B5447))
    case 1: return (Color(hex: 0xE1E4F7), Color(hex: 0x374582))
    case 2: return (Color(hex: 0xF6E3E6), Color(hex: 0x8C3A4A))
    case 3: return (Color(hex: 0xF3E8D2), Color(hex: 0x6D4E12))
    case 4: return (Color(hex: 0xE3EDD9), Color(hex: 0x44622A))
    default: return (Color(hex: 0xECE2F2), Color(hex: 0x5D3F73))
    }
}

/// The fingerprint mandala of a DID (crates/core avatar.rs via UniFFI), built once per DID and size class into SwiftUI paths
/// (100 x 100 box units, theme independent). The colours are picked per theme at draw time. See docs/design/FINGERPRINT-AVATARS.md.
final class FingerprintDrawing {
    struct Stroke { let path: Path; let gold: Bool; let alpha: Double }
    let strokes: [Stroke]
    let strokeWidth: CGFloat
    let lightBg: Color, lightFg: Color, darkBg: Color, darkFg: Color

    init(_ a: AvatarArt) {
        strokes = a.strokes.map { s in
            var p = Path()
            for i in 0..<(s.points.count / 2) {
                let pt = CGPoint(x: CGFloat(s.points[2 * i]), y: CGFloat(s.points[2 * i + 1]))
                if i == 0 { p.move(to: pt) } else { p.addLine(to: pt) }
            }
            if s.closed { p.closeSubpath() }
            return Stroke(path: p, gold: s.gold, alpha: Double(s.alpha))
        }
        strokeWidth = CGFloat(a.strokeWidth)
        func col(_ v: UInt32) -> Color { Color(.sRGB, red: Double((v >> 16) & 0xFF) / 255, green: Double((v >> 8) & 0xFF) / 255, blue: Double(v & 0xFF) / 255, opacity: 1) }
        lightBg = col(a.light.bg); lightFg = col(a.light.fg); darkBg = col(a.dark.bg); darkFg = col(a.dark.fg)
    }

    /// Draws the strokes filling `size` square: `fg`, and the gold layer in `gold`.
    func draw(_ ctx: inout GraphicsContext, size: CGSize, fg: Color, gold: Color) {
        let s = min(size.width, size.height) / 100
        let style = StrokeStyle(lineWidth: strokeWidth * s, lineCap: .round, lineJoin: .round)
        let scale = CGAffineTransform(scaleX: s, y: s)
        for st in strokes {
            ctx.opacity = st.alpha
            ctx.stroke(st.path.applying(scale), with: .color(st.gold ? gold : fg), style: style)
        }
    }
}

enum FingerprintCache {
    /// Below this many points the small variant is drawn (the core's SMALL_BELOW_PX).
    static let smallBelow: CGFloat = 40
    private static let lock = NSLock()
    private static var cache: [String: FingerprintDrawing?] = [:]

    private static func get(_ key: String, _ make: () -> FingerprintDrawing?) -> FingerprintDrawing? {
        lock.lock(); defer { lock.unlock() }
        if let hit = cache[key] { return hit }
        let v = make()
        cache[key] = .some(v)
        return v
    }

    /// The drawing for `did`, or nil when it is not a DID.
    static func of(_ did: String?, size: CGFloat) -> FingerprintDrawing? {
        guard let did, did.hasPrefix("did:") else { return nil }
        let small = size < smallBelow
        return get((small ? "s" : "l") + did) { avatarArt(did: did, small: small).map(FingerprintDrawing.init) }
    }

    /// The drawing for a group id (its own fingerprint context; never a DID).
    static func ofGroup(_ gid: String?, size: CGFloat) -> FingerprintDrawing? {
        guard let gid, !gid.isEmpty else { return nil }
        let small = size < smallBelow
        return get((small ? "gs" : "gl") + gid) { FingerprintDrawing(groupAvatarArt(gid: gid, small: small)) }
    }
}

/// lib/ui/Avatar.svelte, the one person avatar (android PgAvatar). With a DID it is the fingerprint mandala on the DID's own colours;
/// without one (a name only) it is initials (round(size x .34) pt, 700, +.02em) on the hue pair of `avatarSlot(key)`.
/// A person's picture, when the profile layer has one ready, is passed as `image`: it fades in over the avatar (crossfade 120 ms ease-out)
/// with a 1 pt inner hairline. `awaiting` = a picture is selected but not cached yet: a faint ring breathes .5 -> .12 -> .5 every 2.88 s
/// (static under reduced motion). Removed, broken or undecodable pictures are simply `image: nil`. Never a spinner or percentage.
struct TinAvatar: View {
    var did: String
    var size: CGFloat
    var name: String = ""
    var image: Image?
    var awaiting = false
    var alpha: Double = 1

    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    @State private var imageIn = false

    var body: some View {
        let art = FingerprintCache.of(did, size: size)
        let hue = Int(avatarSlot(key: did, slots: avatarSlotCount()))
        let pair = tinAvatarColors(tin, hue: hue)
        let bg = art.map { tin.dark ? $0.darkBg : $0.lightBg } ?? pair.bg
        let fg = art.map { tin.dark ? $0.darkFg : $0.lightFg } ?? pair.fg
        let fontPx = (size * 0.34).rounded()
        ZStack {
            bg
            if let art {
                // A person with a DID: the fingerprint mandala instead of initials (a picture, when ready, covers it below).
                Canvas { ctx, sz in art.draw(&ctx, size: sz, fg: fg, gold: tin.thread) }
            } else {
                Text(initials(name: name))
                    .font(.custom("Figtree", fixedSize: fontPx).weight(.bold))
                    .tracking(fontPx * 0.02)
                    .foregroundStyle(fg)
                    .lineLimit(1)
                    .fixedSize()
            }
            if awaiting { AwaitingRing(fg: fg, fontPx: fontPx, reduced: reduced) }
            if let image {
                image.resizable().scaledToFill().frame(width: size, height: size).clipped()
                    .opacity(imageIn ? 1 : 0)
                Circle().strokeBorder(tin.dark ? Color.white.opacity(0.12) : Color.black.opacity(0.1), lineWidth: 1)
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .opacity(alpha)
        .accessibilityHidden(true)
        .onChange(of: image != nil, initial: true) { _, has in
            if !has { imageIn = false } else if reduced { imageIn = true } else { withAnimation(TinMotion.easeOut(TinMotion.crossfade)) { imageIn = true } }
        }
    }
}

/// The faint breathing ring of a picture that is on its way: .5 -> .12 (ease-in-out over half the period) -> .5 (linear).
private struct AwaitingRing: View {
    let fg: Color, fontPx: CGFloat, reduced: Bool

    var body: some View {
        let w = max(1.5, fontPx * 0.03)
        if reduced {
            ring(0.5, w)
        } else {
            TimelineView(.animation) { ctx in
                let period = TinMotion.overlayOpen * 12
                let t = ctx.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: period) / period
                let a = t < 0.5 ? 0.5 + (0.12 - 0.5) * CubicBezier(0.42, 0, 0.58, 1).transform(t * 2) : 0.12 + (0.5 - 0.12) * (t - 0.5) * 2
                ring(a, w)
            }
        }
    }

    private func ring(_ alpha: Double, _ w: CGFloat) -> some View {
        Circle().strokeBorder(fg.opacity(0.45 * alpha), lineWidth: w)
    }
}

/// A group's avatar: its own fingerprint mandala in a rounded square (corner radius 28 % of the size), so a group never reads as a person.
struct TinGroupAvatar: View {
    var groupID: String
    var size: CGFloat

    @Environment(\.tin) private var tin

    var body: some View {
        let art = FingerprintCache.ofGroup(groupID, size: size)
        let bg = art.map { tin.dark ? $0.darkBg : $0.lightBg } ?? tin.sf3
        let fg = art.map { tin.dark ? $0.darkFg : $0.lightFg } ?? tin.ink2
        ZStack {
            bg
            if let art { Canvas { ctx, sz in art.draw(&ctx, size: sz, fg: fg, gold: tin.thread) } }
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: size * 0.28, style: .continuous))
        .accessibilityHidden(true)
    }
}

/// Opens a ready photo or the identity's fingerprint geometry full screen. Used on profiles and during signup.
/// Tap anywhere or the close button to dismiss.
struct TinViewableAvatar: View {
    var did: String
    var name: String
    var size: CGFloat
    var image: Image?

    @State private var open = false

    var body: some View {
        let viewable = image != nil || FingerprintCache.of(did, size: 280) != nil
        Button { open = true } label: { TinAvatar(did: did, size: size, name: name, image: image) }
            .buttonStyle(.plain)
            .disabled(!viewable)
            .accessibilityLabel(name)
            .accessibilityHint(viewable ? (image != nil ? "View photo" : "View identity geometry") : "")
            .fullScreenCover(isPresented: $open) { viewer }
    }

    private var viewer: some View {
        ZStack(alignment: .topTrailing) {
            Color.black.ignoresSafeArea().onTapGesture { open = false }
            if let image {
                image.resizable().scaledToFit().accessibilityLabel("\(name)’s photo")
            } else {
                TinAvatar(did: did, size: 280, name: name)
            }
            HStack(alignment: .top) {
                Text(name).font(.custom("Figtree", size: 17, relativeTo: .headline).weight(.semibold)).foregroundStyle(.white).lineLimit(1)
                    .padding(.leading, 12).padding(.top, 12)
                Spacer()
                Button { open = false } label: {
                    TinIcon(TinIcons.x, size: 24, tint: .white).frame(width: 48, height: 48).contentShape(Circle())
                }
                .accessibilityLabel("Close")
            }
            .padding(.horizontal, 8).padding(.vertical, 12)
        }
    }
}
