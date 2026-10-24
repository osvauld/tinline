import SwiftUI

/// SVG path data (the subset the icon generator emits: absolute M, L, C, Q, Z, plus H, V and the relative forms for hand-written
/// glyphs such as android `Pg.X`) as a SwiftUI `Path` in the data's own units.
enum TinSVGPath {
    static func parse(_ d: String) -> Path {
        var path = Path()
        let chars = Array(d.unicodeScalars)
        var i = 0
        var cur = CGPoint.zero, start = CGPoint.zero
        func skipSep() { while i < chars.count, chars[i] == " " || chars[i] == "," || chars[i] == "\n" { i += 1 } }
        func number() -> CGFloat? {
            skipSep()
            let from = i
            if i < chars.count, chars[i] == "-" || chars[i] == "+" { i += 1 }
            var dot = false
            while i < chars.count {
                let c = chars[i]
                if c.properties.numericType != nil && c.value < 128 { i += 1 }
                else if c == ".", !dot { dot = true; i += 1 }
                else if c == "e" || c == "E" {
                    i += 1
                    if i < chars.count, chars[i] == "-" || chars[i] == "+" { i += 1 }
                } else { break }
            }
            if i == from { return nil }
            return CGFloat(Double(String(String.UnicodeScalarView(chars[from..<i]))) ?? 0)
        }
        var cmd: Character = " "
        while true {
            skipSep()
            guard i < chars.count else { break }
            let c = chars[i]
            if c.properties.isAlphabetic { cmd = Character(c); i += 1 }
            else if cmd == " " { break }
            let rel = cmd.isLowercase
            let ox = rel ? cur.x : 0, oy = rel ? cur.y : 0
            switch Character(cmd.uppercased()) {
            case "Z":
                path.closeSubpath(); cur = start; cmd = " "
            case "M":
                guard let x = number(), let y = number() else { return path }
                cur = CGPoint(x: x + ox, y: y + oy); start = cur; path.move(to: cur)
                cmd = rel ? "l" : "L"   // further pairs after M are implicit lineto
            case "L":
                guard let x = number(), let y = number() else { return path }
                cur = CGPoint(x: x + ox, y: y + oy); path.addLine(to: cur)
            case "H":
                guard let x = number() else { return path }
                cur = CGPoint(x: x + ox, y: cur.y); path.addLine(to: cur)
            case "V":
                guard let y = number() else { return path }
                cur = CGPoint(x: cur.x, y: y + oy); path.addLine(to: cur)
            case "C":
                guard let x1 = number(), let y1 = number(), let x2 = number(), let y2 = number(), let x = number(), let y = number() else { return path }
                let to = CGPoint(x: x + ox, y: y + oy)
                path.addCurve(to: to, control1: CGPoint(x: x1 + ox, y: y1 + oy), control2: CGPoint(x: x2 + ox, y: y2 + oy)); cur = to
            case "Q":
                guard let x1 = number(), let y1 = number(), let x = number(), let y = number() else { return path }
                let to = CGPoint(x: x + ox, y: y + oy)
                path.addQuadCurve(to: to, control: CGPoint(x: x1 + ox, y: y1 + oy)); cur = to
            default:
                return path
            }
        }
        return path
    }
}

/// One icon at one size tier, as paths in the 24-unit box. `all` is the whole glyph in one colour; `body` and `string` split it so the
/// string can be gold.
struct TinGlyph {
    let lines: Path, strings: Path, fills: Path
    var hasString: Bool { !strings.isEmpty }
}

/// A Tinline line icon (crates/tin-icons, generated `TinIcons`). Tiers are designed for 16, 20 and 24 pt; `at` picks the nearest.
/// Each tier is `[lines, fills, strings]` path data; lines and strings are stroked (1.75, round), fills are filled.
final class TinIconData: @unchecked Sendable {
    let name: String
    private let tiers: [[String]]
    private let lock = NSLock()
    private var cache: [TinGlyph?] = [nil, nil, nil]

    init(_ name: String, _ tiers: [[String]]) {
        self.name = name
        self.tiers = tiers
    }

    func at(_ size: CGFloat) -> TinGlyph {
        let t = size < 18 ? 0 : size < 22 ? 1 : 2
        lock.lock(); defer { lock.unlock() }
        if let g = cache[t] { return g }
        let p = tiers[t]
        let g = TinGlyph(lines: TinSVGPath.parse(p[0]), strings: TinSVGPath.parse(p[2]), fills: TinSVGPath.parse(p[1]))
        cache[t] = g
        return g
    }
}

/// An icon at `size` points, tinted `tint` (default `ink`). Give `stringTint` to draw the glyph's string (the mark's line) in another
/// colour, usually `tin.thread`. Decorative unless `label` is set (the icon alone carries meaning).
struct TinIcon: View {
    var icon: TinIconData
    var size: CGFloat
    var tint: Color?
    var stringTint: Color?
    var label: String?
    /// The design tier drawn (16, 20 or 24). Default 20, whatever `size` is: android's `PgIcons` draw the 20 dp tier in every slot ("one tier serves every place").
    var tier: CGFloat?

    @Environment(\.tin) private var tin

    init(_ icon: TinIconData, size: CGFloat = 24, tint: Color? = nil, stringTint: Color? = nil, label: String? = nil, tier: CGFloat? = nil) {
        self.icon = icon; self.size = size; self.tint = tint; self.stringTint = stringTint; self.label = label; self.tier = tier
    }

    var body: some View {
        let g = icon.at(tier ?? 20)
        let ink = tint ?? tin.ink
        Canvas { ctx, sz in
            let s = sz.width / 24
            let scale = CGAffineTransform(scaleX: s, y: s)
            let style = StrokeStyle(lineWidth: 1.75 * s, lineCap: .round, lineJoin: .round)
            ctx.stroke(g.lines.applying(scale), with: .color(ink), style: style)
            ctx.stroke(g.strings.applying(scale), with: .color(stringTint ?? ink), style: style)
            ctx.fill(g.fills.applying(scale), with: .color(ink))
        }
        .frame(width: size, height: size)
        .accessibilityElement()
        .accessibilityLabel(label ?? "")
        .accessibilityHidden(label == nil)
    }
}

/// `[dir='rtl'] ... svg { transform: scaleX(-1) }`: directional glyphs (back, chevron) point the other way in right-to-left.
extension View {
    func tinMirrorRTL() -> some View { modifier(TinMirrorRTL()) }
}

private struct TinMirrorRTL: ViewModifier {
    @Environment(\.layoutDirection) private var dir
    func body(content: Content) -> some View { content.scaleEffect(x: dir == .rightToLeft ? -1 : 1, y: 1) }
}
