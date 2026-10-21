import SwiftUI
import UIKit

/*
 * The calls area's small design kit: the few tokens and widgets the call screens use that the shared Theme does not
 * carry yet (accent fills, the type sizes the call screens copy from the playground, depth shadows, buttons, badges,
 * signal bars, End's glyph). Prefixed `Call` so nothing collides with other areas' kits. Values are copied from
 * android/.../Theme.kt, Components.kt, SignalBars.kt and HangupGlyph.kt.
 */

// MARK: - colours

extension TinColors {
    /** --accent: filled primary surfaces (badges, Answer). Dark is #117e6b, not `pr` (which is --accent-ink there). */
    var callAccent: Color { dark ? Color(hex: 0x117E6B) : Color(hex: 0x0B6B5B) }
    var callOnAccent: Color { dark ? Color(hex: 0xF2FFFB) : .white }
    /** --accent-ink: teal as text and icons. */
    var callAccentInk: Color { dark ? Color(hex: 0x6FD3BD) : Color(hex: 0x0A5D50) }
    /** --accent-soft: selected chips, active fills. */
    var callAccentSoft: Color { dark ? Color(hex: 0x14433A) : Color(hex: 0xD7ECE6) }
    /** The round control fill: the surface, lifted toward ink in dark mode (CallParts.kt `fill`). */
    var callControlFill: Color { dark ? Color.callLerp(sf, ink, 0.14) : sf }
}

extension Color {
    /** Straight sRGB interpolation, as Compose's `lerp(Color, Color, Float)`. */
    static func callLerp(_ a: Color, _ b: Color, _ t: Double) -> Color {
        var ar: CGFloat = 0, ag: CGFloat = 0, ab: CGFloat = 0, aa: CGFloat = 0
        var br: CGFloat = 0, bg: CGFloat = 0, bb: CGFloat = 0, ba: CGFloat = 0
        UIColor(a).getRed(&ar, green: &ag, blue: &ab, alpha: &aa)
        UIColor(b).getRed(&br, green: &bg, blue: &bb, alpha: &ba)
        let k = CGFloat(t)
        return Color(.sRGB, red: ar + (br - ar) * k, green: ag + (bg - ag) * k, blue: ab + (bb - ab) * k, opacity: aa + (ba - aa) * k)
    }
}

/** The fixed video-screen background (CallScreens.kt `VideoBg`). */
let callVideoBg = Color(hex: 0x0B100F)

// MARK: - type

/** The call screens' type: Figtree at the playground's sizes, scaling with Dynamic Type like sp. */
enum CallFont {
    static func figtree(_ size: CGFloat, _ weight: Font.Weight = .regular, _ style: Font.TextStyle = .body) -> Font {
        .custom("Figtree", size: size, relativeTo: style).weight(weight)
    }
    static func mono(_ size: CGFloat, _ weight: Font.Weight = .medium) -> Font {
        .custom("IBMPlexMono-Medium", size: size, relativeTo: .body).weight(weight)
    }
    /** TinFs.body: 15/21. */
    static let body = figtree(15)
    /** TinFs.sub: 13/18. */
    static let sub = figtree(13, .regular, .footnote)
    /** TinFs.meta: 11/14, tabular. */
    static let meta = figtree(11, .regular, .caption2).monospacedDigit()
}

// MARK: - motion

enum CallMotion {
    /** --ease-out cubic-bezier(.2,.8,.2,1). */
    static func easeOut(_ ms: Double) -> Animation { .timingCurve(0.2, 0.8, 0.2, 1, duration: ms / 1000) }
    /** svelte cubicIn. */
    static func cubicIn(_ ms: Double) -> Animation { .timingCurve(0.55, 0.055, 0.675, 0.19, duration: ms / 1000) }
    static let switchMs = 120.0
}

// MARK: - depth

/** Theme.kt `Depth`: contact + ambient shadows, stronger in dark mode. */
enum CallDepth {
    case lift, pop, sheet
    var contact: CGFloat { switch self { case .lift: 1.5; case .pop: 3; case .sheet: 4 } }
    var ambient: CGFloat { switch self { case .lift: 5; case .pop: 12; case .sheet: 22 } }
    func alpha(dark: Bool) -> Double {
        switch self {
        case .lift: dark ? 0.60 : 0.30
        case .pop: dark ? 0.70 : 0.32
        case .sheet: dark ? 0.75 : 0.36
        }
    }
}

private let callShadowTint = Color(.sRGB, red: 9 / 255, green: 23 / 255, blue: 17 / 255, opacity: 1)

extension View {
    func callDepth(_ d: CallDepth, dark: Bool) -> some View {
        let a = d.alpha(dark: dark)
        return self
            .shadow(color: callShadowTint.opacity(a * 0.5), radius: d.contact, y: d.contact / 2)
            .shadow(color: callShadowTint.opacity(a * 0.35), radius: d.ambient, y: d.ambient / 3)
    }
}

// MARK: - icons

/**
 * Stand-ins for the Tinline line icons (crates/tin-icons, CallGlyphs.kt / PgIcons.kt). SF Symbols until the
 * generated icon set reaches iOS; End is drawn exactly ([CallHangupGlyph]).
 */
enum CallIcon: String {
    case phone = "phone"
    case mic = "mic"
    case micOff = "mic.slash"
    case speaker = "speaker.wave.2"
    case video = "video"
    case videoOff = "video.slash"
    case flip = "arrow.triangle.2.circlepath.camera"
    case back = "chevron.left"
    case chat = "bubble.left"
    case bluetooth = "headphones"
    case vol = "speaker.wave.1"
    case check = "checkmark"
    case info = "info.circle"
    case users = "person.2"
    case close = "xmark"

    var image: Image { Image(systemName: rawValue) }
}

// MARK: - buttons and badges

/** Components.kt `BtnStyle` (the ones the call screens use). */
enum CallBtnStyle { case filled, tonal, outlined, text }

/** Components.kt `TinButton`: a full-width pill, 52 tall unless told otherwise. */
struct CallButton: View {
    @Environment(\.tin) var tin
    let title: String
    var style: CallBtnStyle = .filled
    var height: CGFloat = 52
    let action: () -> Void

    init(_ title: String, style: CallBtnStyle = .filled, height: CGFloat = 52, action: @escaping () -> Void) {
        self.title = title; self.style = style; self.height = height; self.action = action
    }

    var body: some View {
        let (bg, fg): (Color, Color) = switch style {
        case .filled: (tin.pr, tin.onPr)
        case .tonal: (tin.prc, tin.onPrc)
        case .outlined, .text: (.clear, tin.pr)
        }
        Button(action: action) {
            Text(title).font(TinType.button).foregroundStyle(fg)
                .frame(maxWidth: .infinity, minHeight: height)
                .padding(.horizontal, 24)
                .background(Capsule().fill(bg))
                .overlay { if style == .outlined { Capsule().strokeBorder(tin.ln2, lineWidth: 1) } }
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
    }
}

/** Components.kt `Badge` kinds used on the ended screen. */
enum CallBadgeKind { case direct, relayed, neutral, mono }

struct CallBadge: View {
    @Environment(\.tin) var tin
    let text: String
    let kind: CallBadgeKind
    var body: some View {
        let (bg, fg): (Color, Color) = switch kind {
        case .direct: (tin.prc, tin.onPrc)
        case .relayed: (tin.relayC, tin.onRelayC)
        case .neutral, .mono: (tin.sf2, tin.ink)
        }
        Text(text)
            .font(kind == .mono ? CallFont.mono(14) : CallFont.figtree(13, .semibold))
            .foregroundStyle(fg)
            .padding(.horizontal, 12)
            .frame(minHeight: 30)
            .background(Capsule().fill(bg))
    }
}

/** Components.kt `Hint`: 13/18, ink2. */
struct CallHint: View {
    @Environment(\.tin) var tin
    let text: String
    var align: TextAlignment = .leading
    var body: some View {
        Text(text).font(CallFont.figtree(13)).foregroundStyle(tin.ink2).multilineTextAlignment(align)
            .frame(maxWidth: .infinity, alignment: align == .center ? .center : .leading)
    }
}

// MARK: - signal bars

/** link.svelte.ts ghost opacity. */
private let callBarGhost = 0.28

/**
 * SignalBars.kt: four rising bars, `level` lit (1-4); `wave` (reconnecting) runs a ghost wave left to right every
 * 1.6 s, each bar 160 ms after the last. Reduce Motion: fades are cuts and the wave does not run.
 */
struct CallSignalBars: View {
    let level: Int
    let color: Color
    var wave: Bool = false
    var size: CGFloat = 24
    @Environment(\.accessibilityReduceMotion) var reduce

    var body: some View {
        if wave && !reduce {
            TimelineView(.animation) { tl in
                bars { i in Self.waveAlpha(ms: tl.date.timeIntervalSinceReferenceDate * 1000 - Double(i) * 160) }
            }
        } else {
            bars { i in !wave && i < level ? 1 : callBarGhost }
                .animation(reduce ? nil : CallMotion.easeOut(300), value: level)
        }
    }

    private func bars(_ alpha: @escaping (Int) -> Double) -> some View {
        Canvas { ctx, sz in
            let k = sz.width / 24
            let h: [CGFloat] = [6, 10, 14, 18]
            for i in 0..<4 {
                let r = CGRect(x: (3.5 + CGFloat(i) * 5) * k, y: (20 - h[i]) * k, width: 3 * k, height: h[i] * k)
                ctx.fill(Path(roundedRect: r, cornerRadius: 1.5 * k), with: .color(color.opacity(alpha(i))))
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }

    /** The keyframes: ghost at 0, 0.7 at 400 ms (in-out), back to ghost at 960 ms, rest until 1600. */
    static func waveAlpha(ms: Double) -> Double {
        let t = ms.truncatingRemainder(dividingBy: 1600)
        let p = t < 0 ? t + 1600 : t
        func inOut(_ x: Double) -> Double { x < 0.5 ? 2 * x * x : 1 - pow(-2 * x + 2, 2) / 2 }
        if p < 400 { return callBarGhost + (0.7 - callBarGhost) * inOut(p / 400) }
        if p < 960 { return 0.7 + (callBarGhost - 0.7) * ((p - 400) / 560) }
        return callBarGhost
    }
}

// MARK: - End's glyph

/** How long the call screen stays after my own End, so the glyph is seen (HangupGlyph.kt). */
let callEndHoldMs: Double = 320

/*
 * End's glyph (crates/tin-icons/src/cut.rs; playground EndGlyph.svelte `xcut`): two strings crossed as a swirl X, one
 * cut where the other crosses it. Pressing End morphs `hangup` into `hangup-go` (the cut string goes slack, its halves
 * fall back from the cut), once, in the switch time with the playground's little overshoot. Both shapes are the same
 * three quadratics, so the morph moves each number in a straight line.
 */
private let hangupRest: [CGFloat] = [5.4, 5.4, 15.39, 8.61, 18.6, 18.6, 18.6, 5.4, 18, 7.28, 17.15, 8.93, 12.75, 14.58, 9.7, 17.22, 5.4, 18.6]
private let hangupGo: [CGFloat] = [5.4, 5.4, 15.39, 8.61, 18.6, 18.6, 18.6, 5.4, 18.52, 6.94, 18.26, 8.3, 12.97, 16.54, 9.97, 18.36, 5.4, 18.6]

/** The three `M x y Q cx cy x y` strokes at morph progress `p` (0 rest, 1 gone), in a 24 x 24 box. */
struct CallHangupShape: Shape {
    var p: CGFloat
    var animatableData: CGFloat { get { p } set { p = newValue } }
    func path(in rect: CGRect) -> Path {
        let k = rect.width / 24
        func v(_ i: Int) -> CGFloat { (hangupRest[i] + (hangupGo[i] - hangupRest[i]) * p) * k }
        var path = Path()
        for s in 0..<3 {
            let o = s * 6
            path.move(to: CGPoint(x: v(o), y: v(o + 1)))
            path.addQuadCurve(to: CGPoint(x: v(o + 4), y: v(o + 5)), control: CGPoint(x: v(o + 2), y: v(o + 3)))
        }
        return path
    }
}

struct CallHangupGlyph: View {
    let gone: Bool
    let tint: Color
    @Environment(\.accessibilityReduceMotion) var reduce
    var body: some View {
        CallHangupShape(p: gone ? 1 : 0)
            .stroke(tint, style: StrokeStyle(lineWidth: 1.75 * 28 / 24, lineCap: .round, lineJoin: .round))
            // GIVE: cubic-bezier(0.34, 1.4, 0.64, 1), the playground's overshoot.
            .animation(reduce ? nil : .timingCurve(0.34, 1.4, 0.64, 1, duration: CallMotion.switchMs / 1000), value: gone)
    }
}

// MARK: - scrim card

/**
 * The calm card over a 24 % scrim used by About this call (1:1 and group): 16 from the sides, 24 above the bottom, at
 * most 420 wide; tapping the scrim dismisses it.
 */
struct CallScrimCard<Content: View>: View {
    @Environment(\.tin) var tin
    let onDismiss: () -> Void
    @ViewBuilder let content: () -> Content
    var body: some View {
        ZStack(alignment: .bottom) {
            Color.black.opacity(0.24).ignoresSafeArea().onTapGesture(perform: onDismiss)
            ScrollView {
                VStack(alignment: .leading, spacing: 0, content: content)
                    .padding(EdgeInsets(top: 20, leading: 20, bottom: 16, trailing: 20))
            }
            .scrollBounceBehavior(.basedOnSize)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: 420)
            .background(SmoothRoundedRectangle(cornerRadius: 24).fill(tin.sfRaised))
            .clipShape(SmoothRoundedRectangle(cornerRadius: 24))
            .callDepth(.sheet, dark: tin.dark)
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
            .accessibilityElement(children: .contain)
            .accessibilityLabel("About this call")
        }
    }
}
