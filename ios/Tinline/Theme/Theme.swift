import SwiftUI

/// Design tokens (docs/design/Main.dc.html), light and dark. Mirrors android/.../Theme.kt `TinColors`: same names, same values.
struct TinColors {
    var bg, sf, sf2, sf3, ink, ink2, ln, ln2: Color
    var pr, onPr, prc, onPrc: Color
    /// Thread amber: the line motif and the "in call" state. Never body text (use `threadText`).
    var thread, threadText: Color
    var er, onEr, erc, onErc: Color
    var relay, relayC, onRelayC: Color
    var warn, onWarn, pk, onPk: Color
    var callAccept, callEnd: Color
    var bubIn, bubInLine, sfRaised: Color
    var dark: Bool

    /// Playground --danger-btn / --accent: call action fills.
    var callButtonEnd: Color { dark ? Color(hex: 0xC73A30) : Color(hex: 0xA12B23) }
    var callButtonAnswer: Color { dark ? Color(hex: 0x117E6B) : Color(hex: 0x0B6B5B) }
}

extension Color {
    init(hex: UInt32) {
        self.init(.sRGB, red: Double((hex >> 16) & 0xFF) / 255, green: Double((hex >> 8) & 0xFF) / 255,
                  blue: Double(hex & 0xFF) / 255, opacity: 1)
    }
}

extension TinColors {
    static let light = TinColors(
        bg: .init(hex: 0xF5F6F3), sf: .init(hex: 0xFFFFFF), sf2: .init(hex: 0xECEFEB), sf3: .init(hex: 0xE2E6E1),
        ink: .init(hex: 0x17201D), ink2: .init(hex: 0x4D5853), ln: .init(hex: 0xD3D9D4), ln2: .init(hex: 0x8E9A94),
        pr: .init(hex: 0x0B6B5B), onPr: .init(hex: 0xFFFFFF), prc: .init(hex: 0xCDEBE3), onPrc: .init(hex: 0x06352D),
        thread: .init(hex: 0xC98A1E), threadText: .init(hex: 0x845A10),
        er: .init(hex: 0xA12B23), onEr: .init(hex: 0xFFFFFF), erc: .init(hex: 0xF8E3E1), onErc: .init(hex: 0x410E0B),
        relay: .init(hex: 0x4B5AA8), relayC: .init(hex: 0xE1E4F7), onRelayC: .init(hex: 0x374582),
        warn: .init(hex: 0xFCEFD2), onWarn: .init(hex: 0x5C3B00), pk: .init(hex: 0xF3DDE6), onPk: .init(hex: 0x5E1F3A),
        callAccept: .init(hex: 0x0E7C69), callEnd: .init(hex: 0xC8372D),
        bubIn: .init(hex: 0xFFFFFF), bubInLine: .init(hex: 0xE2E7E1), sfRaised: .init(hex: 0xFFFFFF), dark: false)

    static let darkTheme = TinColors(
        bg: .init(hex: 0x0F1513), sf: .init(hex: 0x161D1B), sf2: .init(hex: 0x242D2A), sf3: .init(hex: 0x26302D),
        ink: .init(hex: 0xE6ECE9), ink2: .init(hex: 0xA6B2AD), ln: .init(hex: 0x33403B), ln2: .init(hex: 0x7C8984),
        pr: .init(hex: 0x6FD3BD), onPr: .init(hex: 0x00382E), prc: .init(hex: 0x0E4E43), onPrc: .init(hex: 0xBDEFE2),
        thread: .init(hex: 0xE8B04A), threadText: .init(hex: 0xE8B04A),
        er: .init(hex: 0xF2B8B5), onEr: .init(hex: 0x601410), erc: .init(hex: 0x4D201C), onErc: .init(hex: 0xFFDAD6),
        relay: .init(hex: 0xB8C1F5), relayC: .init(hex: 0x2F3A75), onRelayC: .init(hex: 0xE1E4F7),
        warn: .init(hex: 0x4A3500), onWarn: .init(hex: 0xFFDFA0), pk: .init(hex: 0x5A2A3E), onPk: .init(hex: 0xF3DDE6),
        callAccept: .init(hex: 0x0E7C69), callEnd: .init(hex: 0xC8372D),
        bubIn: .init(hex: 0x1C2422), bubInLine: .init(hex: 0x2A3531), sfRaised: .init(hex: 0x242D2A), dark: true)
}

private struct TinKey: EnvironmentKey { static let defaultValue = TinColors.light }
extension EnvironmentValues { var tin: TinColors { get { self[TinKey.self] } set { self[TinKey.self] = newValue } } }

private struct TinThemeModifier: ViewModifier {
    @Environment(\.colorScheme) private var scheme
    func body(content: Content) -> some View {
        content.environment(\.tin, scheme == .dark ? .darkTheme : .light)
    }
}
extension View { func tinTheme() -> some View { modifier(TinThemeModifier()) } }

/// Type scale from the tokens board (size/line-height weight). `relativeTo` keeps Dynamic Type working, as sp does on Android.
enum TinType {
    private static func figtree(_ size: CGFloat, _ weight: Font.Weight, _ style: Font.TextStyle) -> Font {
        .custom("Figtree", size: size, relativeTo: style).weight(weight)
    }
    static let display = figtree(34, .bold, .largeTitle)
    static let h1 = figtree(28, .bold, .title)
    static let titleL = figtree(24, .bold, .title2)
    static let titleM = figtree(18, .semibold, .title3)
    static let bodyL = figtree(16, .regular, .body)
    static let bodyM = figtree(14, .regular, .callout)
    static let label = figtree(14, .semibold, .callout)
    static let caption = figtree(12, .medium, .caption)
    static let mono = Font.custom("IBMPlexMono-Medium", size: 15, relativeTo: .body)
    static let button = figtree(16, .semibold, .body)
}
