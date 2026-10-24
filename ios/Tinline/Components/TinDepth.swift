import SwiftUI

/// The depth ladder (PHILOSOPHY section 9; tokens.css --shadow-lift/pop/sheet), same as android `Depth`. Flat is no shadow, only a hairline.
/// Each step is two layers, a tight contact shadow and a wide ambient one; light tints them with the palette green-black (9, 23, 17),
/// dark uses black and leans on `sfRaised`.
enum TinDepth {
    case flat, lift, pop, sheet

    /// CSS blur radii (contact, ambient) in points and the CSS alphas (light, dark).
    var contact: CGFloat { switch self { case .flat: 0; case .lift: 1.5; case .pop: 3; case .sheet: 4 } }
    var ambient: CGFloat { switch self { case .flat: 0; case .lift: 5; case .pop: 12; case .sheet: 22 } }
    var lightAlpha: Double { switch self { case .flat: 0; case .lift: 0.30; case .pop: 0.32; case .sheet: 0.36 } }
    var darkAlpha: Double { switch self { case .flat: 0; case .lift: 0.60; case .pop: 0.70; case .sheet: 0.75 } }
}

private struct TinDepthModifier<S: Shape>: ViewModifier {
    @Environment(\.tin) private var tin
    let level: TinDepth
    let shape: S

    func body(content: Content) -> some View {
        if level == .flat {
            content
        } else {
            let tint = tin.dark ? Color.black.opacity(level.darkAlpha) : Color(.sRGB, red: 9 / 255, green: 23 / 255, blue: 17 / 255, opacity: level.lightAlpha)
            content
                // Android draws shadows from an elevation; SwiftUI takes a blur radius and an offset, so the contact layer sits
                // half its radius below and the ambient layer 40 % of its radius (design guess, judge on a device).
                .shadow(color: tint, radius: level.ambient / 2, x: 0, y: level.ambient * 0.4)
                .shadow(color: tint, radius: level.contact / 2, x: 0, y: level.contact * 0.5)
                .overlay(alignment: .top) {
                    // tokens.css --rim (dark only): a 1 px light edge along the top, as if lit from above, clipped to the outline.
                    if tin.dark {
                        Color.white.opacity(0.08).frame(height: 1).frame(maxHeight: .infinity, alignment: .top).clipShape(shape).allowsHitTesting(false)
                    }
                }
        }
    }
}

extension View {
    /// Raise this element to `level`. Apply after the background; `shape` is the outline the shadow and the rim follow.
    func tinDepth<S: Shape>(_ level: TinDepth, _ shape: S) -> some View { modifier(TinDepthModifier(level: level, shape: shape)) }
}
