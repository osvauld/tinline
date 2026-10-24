import SwiftUI
import UIKit

/// Motion tokens (playground src/lib/fx/motion.ts, android PgMotion): durations in seconds and the easings.
/// The area kits below read these instead of repeating numbers.
enum TinMotion {
    static let press = 0.100, release = 0.140, switchTime = 0.120, crossfade = 0.120
    static let enter = 0.200, exit = 0.140, bubbleIn = 0.160
    static let overlayOpen = 0.240, overlayClose = 0.220, indicator = 0.200, tabOut = 0.090, tabIn = 0.210

    /// cubic-bezier(.2, .8, .2, 1), the app's ease-out.
    static let out = CubicBezier(0.2, 0.8, 0.2, 1)
    static let `in` = CubicBezier(0.4, 0, 1, 1)
    static let inOut = CubicBezier(0.4, 0, 0.2, 1)

    /// `.timingCurve` with the ease-out token.
    static func easeOut(_ duration: Double) -> Animation { .timingCurve(0.2, 0.8, 0.2, 1, duration: duration) }
}

/// A CSS cubic-bezier easing: x(t) is inverted numerically, y at that parameter is the eased progress.
struct CubicBezier {
    let x1, y1, x2, y2: Double
    init(_ x1: Double, _ y1: Double, _ x2: Double, _ y2: Double) { self.x1 = x1; self.y1 = y1; self.x2 = x2; self.y2 = y2 }

    private func coord(_ t: Double, _ a: Double, _ b: Double) -> Double {
        let u = 1 - t
        return 3 * u * u * t * a + 3 * u * t * t * b + t * t * t
    }

    func transform(_ x: Double) -> Double {
        if x <= 0 { return 0 }
        if x >= 1 { return 1 }
        var lo = 0.0, hi = 1.0, t = x
        for _ in 0..<40 {
            let cx = coord(t, x1, x2)
            if abs(cx - x) < 1e-6 { break }
            if cx < x { lo = t } else { hi = t }
            t = (lo + hi) / 2
        }
        return coord(t, y1, y2)
    }
}

/// svelte/easing cubicOut and cubicIn, exactly (transitions.ts uses these, not the CSS beziers).
enum SvelteEase {
    static func cubicOut(_ t: Double) -> Double { let f = t - 1; return f * f * f + 1 }
    static func cubicIn(_ t: Double) -> Double { t * t * t }
}

/// A SwiftUI animation with a fixed duration and an exact easing function (the system curves only offer approximations).
struct TinEasedAnimation: CustomAnimation {
    var duration: Double
    var ease: @Sendable (Double) -> Double
    /// Identity of the easing (CustomAnimation is Hashable and a closure is not).
    var name: String

    static func == (a: TinEasedAnimation, b: TinEasedAnimation) -> Bool { a.duration == b.duration && a.name == b.name }
    func hash(into h: inout Hasher) { h.combine(duration); h.combine(name) }

    func animate<V: VectorArithmetic>(value: V, time: TimeInterval, context: inout AnimationContext<V>) -> V? {
        if time >= duration { return nil }
        return value.scaled(by: ease(time / duration))
    }
}

extension Animation {
    static func tinCubicOut(_ duration: Double) -> Animation { Animation(TinEasedAnimation(duration: duration, ease: SvelteEase.cubicOut, name: "cubicOut")) }
    static func tinCubicIn(_ duration: Double) -> Animation { Animation(TinEasedAnimation(duration: duration, ease: SvelteEase.cubicIn, name: "cubicIn")) }
}

extension Color {
    /// `lerp(a, b, t)` in sRGB, like Compose's `lerp(Color, Color, Float)`. Both colours are resolved in the light trait set;
    /// the Tin tokens are fixed sRGB values, so that is exact for them.
    static func tinMix(_ a: Color, _ b: Color, by t: Double) -> Color {
        func rgba(_ c: Color) -> (Double, Double, Double, Double) {
            var r: CGFloat = 0, g: CGFloat = 0, bl: CGFloat = 0, al: CGFloat = 0
            UIColor(c).getRed(&r, green: &g, blue: &bl, alpha: &al)
            return (Double(r), Double(g), Double(bl), Double(al))
        }
        let (r1, g1, b1, a1) = rgba(a), (r2, g2, b2, a2) = rgba(b)
        let k = min(1, max(0, t))
        return Color(.sRGB, red: r1 + (r2 - r1) * k, green: g1 + (g2 - g1) * k, blue: b1 + (b2 - b1) * k, opacity: a1 + (a2 - a1) * k)
    }
}
