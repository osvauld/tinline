import SwiftUI

/// Corner geometry, a port of docs/design/playground/src/lib/geom/shape.ts (see GEOMETRY.md there; android Geometry.kt). Keep the
/// two identical: `SmoothGeom.svg` must produce the strings shape.ts produces (GeometryTests compares them).
/// Lines and cubic Beziers only, Figma-style corner smoothing; one-line bubbles degrade to a pill automatically.
enum SmoothGeom {
    /// Default corner smoothing (`--smooth` in tokens.css).
    static let smooth = 0.6

    /// One path command: "M" / "L" (1 point), "C" (3 points) or "Z" (none). Coordinates are x0, y0, x1, y1, ...
    struct Seg: Equatable {
        let op: Character
        let p: [Double]
    }

    private static let rad = Double.pi / 180

    struct Corner {
        var r = 0.0, s = 0.0, p = 0.0, a = 0.0, b = 0.0, c = 0.0, d = 0.0, arc = 0.0, alpha = 0.0, h = 0.0
    }

    static func cornerParams(_ radius: Double, _ smoothing: Double, budget: Double = .infinity) -> Corner {
        var r = max(0, radius)
        var s = min(1, max(0, smoothing))
        if r == 0 { return Corner() }
        if (1 + s) * r > budget {
            s = min(s, max(0, budget / r - 1))
            if r > budget { r = budget; s = 0 }
        }
        let p = (1 + s) * r
        let arcMeasure = 90 * (1 - s)
        let arc = sin((arcMeasure / 2) * rad) * r * 2.0.squareRoot()
        let alpha = (45 * s) * rad
        let p3p4 = r * tan(((90 - arcMeasure) / 4) * rad)
        let c = p3p4 * cos(alpha)
        let d = c * tan(alpha)
        let b = (p - arc - c - d) / 3
        let a = 2 * b
        let h = (4.0 / 3) * tan((arcMeasure / 4) * rad) * r
        return Corner(r: r, s: s, p: p, a: a, b: b, c: c, d: d, arc: arc, alpha: alpha, h: h)
    }

    private static func budgets(_ w: Double, _ h: Double, _ rad: [Double], _ s: Double) -> [Double] {
        let tl = rad[0], tr = rad[1], br = rad[2], bl = rad[3]
        func ext(_ r: Double) -> Double { (1 + s) * r }
        func side(_ len: Double, _ ra: Double, _ rb: Double) -> [Double] {
            let need = ext(ra) + ext(rb)
            if need <= len || need == 0 { return [.infinity, .infinity] }
            return [len * ra / (ra + rb), len * rb / (ra + rb)]
        }
        let top = side(w, tl, tr), right = side(h, tr, br), bottom = side(w, br, bl), left = side(h, bl, tl)
        return [min(top[0], left[1]), min(top[1], right[0]), min(right[1], bottom[0]), min(bottom[1], left[0])]
    }

    private static func corner(_ out: inout [Seg], _ sx: Double, _ sy: Double, _ inX: Double, _ inY: Double, _ outX: Double, _ outY: Double, _ k: Corner) {
        func pt(_ u: Double, _ v: Double) -> [Double] { [sx + inX * u + outX * v, sy + inY * u + outY * v] }
        let e1u = k.a + k.b + k.c, e1v = k.d
        let e2u = e1u + k.arc, e2v = e1v + k.arc
        if k.a + k.b + k.c + k.d > 1e-9 { out.append(Seg(op: "C", p: pt(k.a, 0) + pt(k.a + k.b, 0) + pt(e1u, e1v))) }
        if k.arc > 1e-9 {
            let ca = cos(k.alpha), sa = sin(k.alpha)
            out.append(Seg(op: "C", p: pt(e1u + k.h * ca, e1v + k.h * sa) + pt(e2u - k.h * sa, e2v - k.h * ca) + pt(e2u, e2v)))
        }
        if k.a + k.b + k.c + k.d > 1e-9 { out.append(Seg(op: "C", p: pt(e2u + k.d, e2v + k.c) + pt(e2u + k.d, e2v + k.b + k.c) + pt(k.p, k.p))) }
    }

    /// Round to 0.01 like shape.ts (`+ 0` turns -0 into 0).
    private static func r2(_ n: Double) -> Double { (n * 100 + 0.5).rounded(.down) / 100 + 0.0 }

    /// Radii are physical: topLeft, topRight, bottomRight, bottomLeft.
    static func roundedRect(_ w0: Double, _ h0: Double, _ radii: [Double], smoothing: Double = smooth, x: Double = 0, y: Double = 0) -> [Seg] {
        let w = max(0, w0), h = max(0, h0)
        let sm = min(1, max(0, smoothing))
        let bud = budgets(w, h, radii, sm)
        let k = (0..<4).map { cornerParams(min(radii[$0], min(w / 2, h / 2)), smoothing, budget: bud[$0]) }
        let tl = k[0], tr = k[1], br = k[2], bl = k[3]
        var out: [Seg] = [Seg(op: "M", p: [x + tl.p, y])]
        var cx = x + tl.p, cy = y
        func line(_ lx: Double, _ ly: Double) {
            if r2(lx) != r2(cx) || r2(ly) != r2(cy) { out.append(Seg(op: "L", p: [lx, ly])) }
            cx = lx; cy = ly
        }
        line(x + w - tr.p, y)
        if tr.r != 0 { corner(&out, x + w - tr.p, y, 1, 0, 0, 1, tr); cx = x + w; cy = y + tr.p }
        line(x + w, y + h - br.p)
        if br.r != 0 { corner(&out, x + w, y + h - br.p, 0, 1, -1, 0, br); cx = x + w - br.p; cy = y + h }
        line(x + bl.p, y + h)
        if bl.r != 0 { corner(&out, x + bl.p, y + h, -1, 0, 0, -1, bl); cx = x; cy = y + h - bl.p }
        line(x, y + tl.p)
        if tl.r != 0 { corner(&out, x, y + tl.p, 0, -1, 1, 0, tl); cx = x + tl.p; cy = y }
        out.append(Seg(op: "Z", p: []))
        return out
    }

    /// Same shape shrunk by `inset` (negative = grown), radius - inset, same smoothing, kept in the original box's space.
    static func inset(_ w: Double, _ h: Double, _ radii: [Double], _ smoothing: Double, _ inset: Double) -> [Seg] {
        roundedRect(w - 2 * inset, h - 2 * inset, radii.map { max(0, $0 - inset) }, smoothing: smoothing, x: inset, y: inset)
    }

    /// The SVG path string, rounded to 0.01 exactly like shape.ts.
    static func svg(_ segs: [Seg]) -> String {
        var s = ""
        for seg in segs {
            s.append(seg.op)
            if seg.op == "Z" { continue }
            for i in stride(from: 0, to: seg.p.count, by: 2) {
                if i > 0 { s += " " }
                s += fmt(seg.p[i]) + " " + fmt(seg.p[i + 1])
            }
        }
        return s
    }

    private static func fmt(_ n: Double) -> String {
        let v = r2(n)
        return v == v.rounded(.down) && abs(v) < 1e15 ? String(Int64(v)) : "\(v)"
    }

    /// Bubble corner radii [tl, tr, br, bl]; `right` = the side facing the screen edge the stack sits against.
    static func bubbleRadii(_ pos: BubblePos, right: Bool, outer: Double = 18, tight: Double = 6) -> [Double] {
        let top = pos == .mid || pos == .last ? tight : outer
        let bot = pos == .mid || pos == .first ? tight : outer
        return right ? [outer, top, bot, outer] : [top, outer, outer, bot]
    }

    static func toPath(_ segs: [Seg]) -> Path {
        var path = Path()
        for s in segs {
            let p = s.p
            switch s.op {
            case "M": path.move(to: CGPoint(x: p[0], y: p[1]))
            case "L": path.addLine(to: CGPoint(x: p[0], y: p[1]))
            case "C": path.addCurve(to: CGPoint(x: p[4], y: p[5]), control1: CGPoint(x: p[0], y: p[1]), control2: CGPoint(x: p[2], y: p[3]))
            default: path.closeSubpath()
            }
        }
        return path
    }
}

/// Position in a run of same-sender messages.
enum BubblePos { case solo, first, mid, last }

/// A rounded rectangle with Figma-style corner smoothing, radii in points. Leading/trailing follow `layoutDirection`: SwiftUI shapes
/// cannot read the environment, so a caller in a right-to-left layout passes `@Environment(\.layoutDirection)` in.
struct SmoothCornerShape: InsettableShape {
    var topLeading: CGFloat = 0, topTrailing: CGFloat = 0, bottomTrailing: CGFloat = 0, bottomLeading: CGFloat = 0
    var smoothing: Double = SmoothGeom.smooth
    var layoutDirection: LayoutDirection = .leftToRight
    var insetAmount: CGFloat = 0

    init(topLeading: CGFloat = 0, topTrailing: CGFloat = 0, bottomTrailing: CGFloat = 0, bottomLeading: CGFloat = 0,
         smoothing: Double = SmoothGeom.smooth, layoutDirection: LayoutDirection = .leftToRight) {
        self.topLeading = topLeading; self.topTrailing = topTrailing; self.bottomTrailing = bottomTrailing; self.bottomLeading = bottomLeading
        self.smoothing = smoothing; self.layoutDirection = layoutDirection
    }

    init(all: CGFloat, smoothing: Double = SmoothGeom.smooth) {
        self.init(topLeading: all, topTrailing: all, bottomTrailing: all, bottomLeading: all, smoothing: smoothing)
    }

    func inset(by amount: CGFloat) -> SmoothCornerShape {
        var s = self
        s.insetAmount += amount
        return s
    }

    func path(in rect: CGRect) -> Path {
        let ts = Double(topLeading), te = Double(topTrailing), be = Double(bottomTrailing), bs = Double(bottomLeading)
        let radii = layoutDirection == .leftToRight ? [ts, te, be, bs] : [te, ts, bs, be]
        let w = Double(rect.width), h = Double(rect.height)
        let segs = insetAmount == 0
            ? SmoothGeom.roundedRect(w, h, radii, smoothing: smoothing)
            : SmoothGeom.inset(w, h, radii, smoothing, Double(insetAmount))
        return SmoothGeom.toPath(segs).offsetBy(dx: rect.minX, dy: rect.minY)
    }
}

/// `SmoothRoundedRectangle(cornerRadius: 14)`: the same radius on all four corners; drop-in for `RoundedRectangle` in `clipShape`,
/// `background(_, in:)`, `overlay(shape.strokeBorder(...))`.
struct SmoothRoundedRectangle: InsettableShape {
    var cornerRadius: CGFloat
    var smoothing: Double = SmoothGeom.smooth
    var insetAmount: CGFloat = 0

    init(cornerRadius: CGFloat, smoothing: Double = SmoothGeom.smooth) {
        self.cornerRadius = cornerRadius; self.smoothing = smoothing
    }

    func inset(by amount: CGFloat) -> SmoothRoundedRectangle {
        var s = self
        s.insetAmount += amount
        return s
    }

    func path(in rect: CGRect) -> Path {
        var s = SmoothCornerShape(all: cornerRadius, smoothing: smoothing)
        s.insetAmount = insetAmount
        return s.path(in: rect)
    }
}

/// Bubble outline for a message at `pos` in its run. `mine`: stack against the trailing edge, else the leading edge.
/// `inset`: the same outline `inset` in (a photo inside its 3 pt rim), each corner at least 3 pt.
func bubbleShape(_ pos: BubblePos, mine: Bool, inset: CGFloat = 0, layoutDirection: LayoutDirection = .leftToRight) -> SmoothCornerShape {
    let o = max(18 - inset, 3), t = max(6 - inset, 3)
    let top = pos == .mid || pos == .last ? t : o
    let bot = pos == .mid || pos == .first ? t : o
    return mine
        ? SmoothCornerShape(topLeading: o, topTrailing: top, bottomTrailing: bot, bottomLeading: o, layoutDirection: layoutDirection)
        : SmoothCornerShape(topLeading: top, topTrailing: o, bottomTrailing: o, bottomLeading: bot, layoutDirection: layoutDirection)
}
