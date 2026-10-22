import SwiftUI

/*
 * The people / gate / settings area's port of the playground's furniture (docs/design/playground/src), android PeopleKit.kt:
 * gate/StepShell, gate/Btn, gate/Field, gate/Notice, groups/Btn, lib/ui/Switch, t-chip, profile/profile.css (pf-*),
 * people/PassField, people/PersonRow and chat/Layer (dialog = bottom sheet). Values are the playground's computed CSS values
 * (tokens.css, motion.ts); names say which CSS they come from. Everything here is prefixed `People` so the other areas' kits
 * never collide with it.
 */

// MARK: - tokens (tokens.css) missing from TinColors

extension TinColors {
    /// --accent: filled primary surfaces. Dark is #117e6b, not `pr` (which is --accent-ink there).
    var peopleAccent: Color { dark ? Color(hex: 0x117E6B) : Color(hex: 0x0B6B5B) }
    var peopleOnAccent: Color { dark ? Color(hex: 0xF2FFFB) : .white }
    /// --accent-ink: teal as text and icons.
    var peopleAccentInk: Color { dark ? Color(hex: 0x6FD3BD) : Color(hex: 0x0A5D50) }
    /// --accent-soft: selected chips, active fills.
    var peopleAccentSoft: Color { dark ? Color(hex: 0x14433A) : Color(hex: 0xD7ECE6) }
    var peopleHairline: Color { dark ? Color(hex: 0x2A3531) : Color(hex: 0xE2E7E1) }
    /// --danger-btn: filled Decline / Block.
    var peopleDangerBtn: Color { dark ? Color(hex: 0xC73A30) : Color(hex: 0xA12B23) }
    /// --danger as text.
    var peopleDanger: Color { dark ? Color(hex: 0xF2B8B5) : Color(hex: 0xA12B23) }
}

/// Layer scrim: rgb(10 16 14 / .5).
let peopleScrim = Color(.sRGB, red: 10 / 255, green: 16 / 255, blue: 14 / 255, opacity: 0.5)

// MARK: - type (tokens.css type scale, Figtree incl. 650)

/// size / line-height / weight / letter-spacing (em). 650 is drawn as semibold: the closest weight SwiftUI names.
enum PeopleTextStyle {
    case h1, h1Center, h1Page, lead, title, barTitle, body, bodyB, sub, sub19, meta, label, btn, btnGroups, secHead, mono

    var spec: (size: CGFloat, lh: CGFloat, weight: Font.Weight, tracking: CGFloat, relativeTo: Font.TextStyle) {
        switch self {
        case .h1: return (26, 32, .bold, -0.01, .title)           // gate h1 (StepShell)
        case .h1Center: return (30, 36, .bold, -0.01, .largeTitle) // Welcome / Done centred h1
        case .h1Page: return (22, 28, .semibold, 0, .title2)      // pages' h1 (LinkDevice, PersonProfile)
        case .lead: return (15, 22, .regular, 0, .body)
        case .title: return (17, 24, .semibold, 0, .headline)
        case .barTitle: return (17, 24, .semibold, 0, .headline)
        case .body: return (15, 21, .regular, 0, .body)
        case .bodyB: return (15, 20, .semibold, 0, .body)
        case .sub: return (13, 18, .regular, 0, .footnote)
        case .sub19: return (13, 19, .regular, 0, .footnote)
        case .meta: return (11, 14, .regular, 0, .caption2)
        case .label: return (13, 18, .semibold, 0, .footnote)
        case .btn: return (15, 20, .semibold, 0, .body)
        case .btnGroups: return (15, 20, .semibold, 0, .body)
        case .secHead: return (12, 16, .semibold, 0.06, .caption)
        case .mono: return (15, 20, .medium, 0, .body)
        }
    }

    var font: Font {
        let s = spec
        if self == .mono { return Font.custom("IBMPlexMono-Medium", size: s.size, relativeTo: s.relativeTo) }
        return Font.custom("Figtree", size: s.size, relativeTo: s.relativeTo).weight(s.weight)
    }
}

private struct PeopleStyled: ViewModifier {
    let style: PeopleTextStyle
    let weight: Font.Weight?
    let lineHeight: CGFloat?
    let size: CGFloat?
    let tnum: Bool

    func body(content: Content) -> some View {
        let s = style.spec
        let px = size ?? s.size
        var font = size == nil ? style.font : Font.custom(style == .mono ? "IBMPlexMono-Medium" : "Figtree", size: px, relativeTo: s.relativeTo).weight(s.weight)
        if let weight { font = font.weight(weight) }
        if tnum { font = font.monospacedDigit() }
        // CSS line-height: SwiftUI adds spacing between lines, so ask only for what is beyond the font's own ~1.2 leading.
        return content.font(font).tracking(s.tracking * px).lineSpacing(max(0, (lineHeight ?? s.lh) - px * 1.2))
    }
}

extension View {
    func peopleStyle(_ style: PeopleTextStyle, weight: Font.Weight? = nil, size: CGFloat? = nil, lineHeight: CGFloat? = nil, tnum: Bool = false) -> some View {
        modifier(PeopleStyled(style: style, weight: weight, lineHeight: lineHeight, size: size, tnum: tnum))
    }
}

func peopleFirstName(_ name: String) -> String {
    let t = name.trimmingCharacters(in: .whitespacesAndNewlines)
    let first = t.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: true).first.map(String.init) ?? ""
    return first.isEmpty ? name : first
}

// MARK: - motion (motion.ts)

enum PeopleMotion {
    static let press = 0.100, release = 0.140, `switch` = 0.120, crossfade = 0.120
    static let enter = 0.200, exit = 0.140, bubbleIn = 0.160
    static let overlayOpen = 0.240, overlayClose = 0.220

    /// cubic-bezier(.2, .8, .2, 1)
    static func easeOut(_ d: Double) -> Animation { .timingCurve(0.2, 0.8, 0.2, 1, duration: d) }
    /// slide.ts: 1 - (1 - t)^3, close to the standard ease-out cubic.
    static func cubicOut(_ d: Double) -> Animation { .timingCurve(0.215, 0.61, 0.355, 1, duration: d) }
}

extension View {
    /// `animation` that is skipped under Reduce Motion (the app's reduced-motion switch, as "remove animations" is on Android).
    func peopleAnimation<V: Equatable>(_ duration: Double, value: V, curve: ((Double) -> Animation)? = nil) -> some View {
        modifier(PeopleAnimated(duration: duration, value: value, curve: curve ?? PeopleMotion.easeOut))
    }
}

private struct PeopleAnimated<V: Equatable>: ViewModifier {
    @Environment(\.accessibilityReduceMotion) private var reduced
    let duration: Double
    let value: V
    let curve: (Double) -> Animation
    func body(content: Content) -> some View { content.animation(reduced ? nil : curve(duration), value: value) }
}

/// app.css .press: scale .97 while pressed (100 ms in, 140 ms back, ease-out).
struct PeoplePressStyle: ButtonStyle {
    var enabled = true
    func makeBody(configuration: Configuration) -> some View { PeoplePressBody(configuration: configuration, enabled: enabled) }
}

private struct PeoplePressBody: View {
    @Environment(\.accessibilityReduceMotion) private var reduced
    let configuration: ButtonStyleConfiguration
    let enabled: Bool
    var body: some View {
        let on = configuration.isPressed && enabled && !reduced
        configuration.label
            .scaleEffect(on ? 0.97 : 1)
            .animation(PeopleMotion.easeOut(on ? PeopleMotion.press : PeopleMotion.release), value: on)
    }
}

// MARK: - depth ladder (tokens.css --shadow-lift/pop/sheet)

enum PeopleDepth {
    case flat, lift, pop, sheet
    var params: (contact: CGFloat, ambient: CGFloat, light: Double, dark: Double) {
        switch self {
        case .flat: return (0, 0, 0, 0)
        case .lift: return (1.5, 5, 0.30, 0.60)
        case .pop: return (3, 12, 0.32, 0.70)
        case .sheet: return (4, 22, 0.36, 0.75)
        }
    }
}

private struct PeopleDepthModifier: ViewModifier {
    @Environment(\.tin) private var c
    let level: PeopleDepth
    func body(content: Content) -> some View {
        let p = level.params
        let tint = (c.dark ? Color.black : Color(.sRGB, red: 9 / 255, green: 23 / 255, blue: 17 / 255)).opacity(c.dark ? p.dark : p.light)
        content
            .shadow(color: tint, radius: p.ambient / 2, x: 0, y: p.ambient / 4)
            .shadow(color: tint, radius: p.contact / 2, x: 0, y: p.contact / 2)
    }
}

extension View {
    /// Raise this element to `level`. Apply after the background so the shadow follows its shape.
    func peopleDepth(_ level: PeopleDepth) -> some View { level == .flat ? AnyView(self) : AnyView(modifier(PeopleDepthModifier(level: level))) }
}

// MARK: - icons

/// Stand-ins until the generated "Tinline line" icons exist (iconography decision): SF Symbols, named by the Android PgIcons they replace.
enum PeopleIcon: String {
    case back = "chevron.left", next = "arrow.right", eye, eyeoff = "eye.slash", sun = "sun.max", moon
    case info = "info.circle", warning = "exclamationmark.triangle", check = "checkmark", chevr = "chevron.right"
    case plus, edit = "pencil", trash, link, camera, x = "xmark", clock, phone, callend = "phone.down"
    case chat = "bubble.left", off = "nosign", share = "square.and.arrow.up", scan = "qrcode.viewfinder", image = "photo"
    case userPlus = "person.badge.plus", users = "person.2", callIn = "phone.arrow.down.left", callOut = "phone.arrow.up.right"
}

struct PeopleIconView: View {
    let icon: PeopleIcon
    var size: CGFloat = 20
    var color: Color? = nil
    @Environment(\.tin) private var c
    var body: some View {
        Image(systemName: icon.rawValue).font(.system(size: size * 0.82, weight: .regular))
            .foregroundStyle(color ?? c.ink).frame(width: size, height: size).accessibilityHidden(true)
    }
}

// MARK: - small pieces

struct PeopleSpinner: View {
    let size: CGFloat
    let color: Color
    var stroke: CGFloat = 2
    @Environment(\.accessibilityReduceMotion) private var reduced
    @State private var turn = 0.0
    var body: some View {
        Circle().trim(from: 0, to: 0.75).stroke(color, style: StrokeStyle(lineWidth: stroke, lineCap: .butt))
            .padding(stroke / 2).frame(width: size, height: size)
            .rotationEffect(.degrees(-90 + turn))
            .onAppear { if !reduced { withAnimation(.linear(duration: 0.8).repeatForever(autoreverses: false)) { turn = 360 } } }
            .accessibilityHidden(true)
    }
}

/// lib/ui/IconButton: 44 round, icon 22, ink.
struct PeopleIconButton: View {
    @Environment(\.tin) private var c
    let icon: PeopleIcon
    let label: String
    var tint: Color? = nil
    var size: CGFloat = 44
    var iconSize: CGFloat = 22
    var enabled = true
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            PeopleIconView(icon: icon, size: iconSize, color: (tint ?? c.ink).opacity(enabled ? 1 : 0.45))
                .frame(width: size, height: size).contentShape(Circle())
        }
        .buttonStyle(PeoplePressStyle(enabled: enabled)).disabled(!enabled).accessibilityLabel(label)
    }
}

// MARK: - buttons

/// gate/Btn.svelte. Primary / secondary / text; 48 tall, r-ctl 14, 650 15 px; busy shows a 16 px spinner.
struct PeopleGateButton: View {
    enum Kind { case primary, secondary, text }
    @Environment(\.tin) private var c
    let text: String
    var kind: Kind = .primary
    var enabled = true
    var busy = false
    var fill = true
    let action: () -> Void

    init(_ text: String, kind: Kind = .primary, enabled: Bool = true, busy: Bool = false, fill: Bool = true, action: @escaping () -> Void) {
        self.text = text; self.kind = kind; self.enabled = enabled; self.busy = busy; self.fill = fill; self.action = action
    }

    var body: some View {
        let on = enabled && !busy
        let (bg, fg): (Color, Color) = {
            switch kind {
            case .primary: return busy ? (c.peopleAccent.opacity(0.85), c.peopleOnAccent) : on ? (c.peopleAccent, c.peopleOnAccent) : (c.sf2, c.ink2)
            default: return (.clear, c.peopleAccentInk)
            }
        }()
        let shape = SmoothRoundedRectangle(cornerRadius: 14)
        Button(action: action) {
            HStack(spacing: 8) {
                if busy { PeopleSpinner(size: 16, color: fg) }
                Text(text).peopleStyle(.btn).foregroundStyle(fg).multilineTextAlignment(.center)
            }
            .padding(.horizontal, 20).frame(maxWidth: fill ? .infinity : nil, minHeight: 48)
            .background(shape.fill(bg))
            .overlay { if kind == .secondary { shape.stroke(c.ln, lineWidth: 1) } }
            .opacity(kind != .primary && !on ? 0.5 : 1)
            .contentShape(shape)
        }
        .buttonStyle(PeoplePressStyle(enabled: on)).disabled(!on)
    }
}

/// groups/Btn.svelte: the pill used in pages and dialogs. 44 tall, r 22, padding 0 18, 600 15 px.
struct PeopleButton: View {
    enum Variant { case primary, outline, quiet, danger, dangerQuiet }
    @Environment(\.tin) private var c
    let text: String
    var variant: Variant = .outline
    var icon: PeopleIcon? = nil
    var enabled = true
    var minWidth: CGFloat = 44
    var height: CGFloat = 44
    var padH: CGFloat = 18
    var color: Color? = nil
    var fill = false
    let action: () -> Void

    init(_ text: String, variant: Variant = .outline, icon: PeopleIcon? = nil, enabled: Bool = true, minWidth: CGFloat = 44,
         height: CGFloat = 44, padH: CGFloat = 18, color: Color? = nil, fill: Bool = false, action: @escaping () -> Void) {
        self.text = text; self.variant = variant; self.icon = icon; self.enabled = enabled; self.minWidth = minWidth
        self.height = height; self.padH = padH; self.color = color; self.fill = fill; self.action = action
    }

    var body: some View {
        let (bg, fg, line): (Color, Color, Color) = {
            switch variant {
            case .primary: return (c.peopleAccent, c.peopleOnAccent, .clear)
            case .outline: return (c.sf, c.ink, c.ln)
            case .quiet: return (.clear, c.ink, .clear)
            case .danger: return (c.peopleDangerBtn, .white, .clear)
            case .dangerQuiet: return (.clear, c.er, .clear)
            }
        }()
        let ink = color ?? fg
        let overridden = color != nil && variant == .outline
        Button(action: action) {
            HStack(spacing: 8) {
                if let icon { PeopleIconView(icon: icon, size: 18, color: ink) }
                Text(text).peopleStyle(.btnGroups).foregroundStyle(ink).lineLimit(1)
            }
            .padding(.horizontal, padH).frame(minWidth: minWidth, maxWidth: fill ? .infinity : nil, minHeight: height)
            .background(Capsule().fill(overridden ? .clear : bg))
            .overlay(Capsule().stroke(overridden ? color! : line, lineWidth: 1))
            .opacity(enabled ? 1 : 0.45)
            .contentShape(Capsule())
        }
        .buttonStyle(PeoplePressStyle(enabled: enabled)).disabled(!enabled)
    }
}

/// .pf-acts: buttons at the end, gap 8.
struct PeopleActs<Content: View>: View {
    var top: CGFloat = 18
    @ViewBuilder let content: Content
    var body: some View {
        HStack(spacing: 8) { Spacer(minLength: 0); content }.padding(.top, top).frame(maxWidth: .infinity)
    }
}

// MARK: - switch, chip, radio, segmented

/// lib/ui/Switch.svelte: 44 x 26 track (line / accent), 20 knob inset 3 with the lift shadow, 120 ms ease-out.
struct PeopleSwitch: View {
    @Environment(\.tin) private var c
    let checked: Bool
    let label: String
    var enabled = true
    let onChange: (Bool) -> Void
    var body: some View {
        // 44 px touch target around the 26 px track.
        Button { onChange(!checked) } label: {
            ZStack(alignment: .leading) {
                Capsule().fill(checked ? c.peopleAccent : c.ln).frame(width: 44, height: 26).opacity(enabled ? 1 : 0.55)
                Circle().fill(c.sf).frame(width: 20, height: 20).peopleDepth(.lift).offset(x: checked ? 21 : 3)
            }
            .frame(width: 44, height: 44).contentShape(Rectangle())
            .peopleAnimation(PeopleMotion.switch, value: checked)
        }
        .buttonStyle(.plain).disabled(!enabled)
        .accessibilityLabel(label).accessibilityValue(checked ? "On" : "Off").accessibilityAddTraits(.isButton)
    }
}

/// tokens.css .t-chip: 32 min (callers pass 36/44), r16, line border, 13 px 600; pressed = accent-soft, accent-ink.
struct PeopleChip: View {
    @Environment(\.tin) private var c
    let text: String
    let pressed: Bool
    var minHeight: CGFloat = 32
    var padH: CGFloat = 14
    var icon: PeopleIcon? = nil
    var mono = false
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if let icon { PeopleIconView(icon: icon, size: 16, color: pressed ? c.peopleAccentInk : c.ink) }
                Text(text).peopleStyle(mono ? .mono : .sub, weight: .semibold, size: mono ? 14 : nil)
                    .foregroundStyle(pressed ? c.peopleAccentInk : c.ink)
            }
            .padding(.horizontal, padH).frame(minHeight: minHeight)
            .background(Capsule().fill(pressed ? c.peopleAccentSoft : .clear))
            .overlay(Capsule().stroke(pressed ? .clear : c.ln, lineWidth: 1))
            .contentShape(Capsule())
            .peopleAnimation(PeopleMotion.switch, value: pressed)
        }
        .buttonStyle(PeoplePressStyle())
        .accessibilityValue(pressed ? "Selected" : "")
    }
}

/// Sounds.svelte .radio: 22 ring (2 px line / accent), 12 dot scaling in.
struct PeopleRadio: View {
    @Environment(\.tin) private var c
    let on: Bool
    var body: some View {
        Circle().stroke(on ? c.peopleAccent : c.ln, lineWidth: 2).frame(width: 22, height: 22)
            .overlay(Circle().fill(c.peopleAccent).frame(width: 12, height: 12).scaleEffect(on ? 1 : 0))
            .peopleAnimation(PeopleMotion.switch, value: on)
            .accessibilityHidden(true)
    }
}

/// A row of equal segments on surface-2 (Availability .seg, AddContact .tabs); the selected one is a lifted surface.
struct PeopleSegmented: View {
    @Environment(\.tin) private var c
    let options: [String]
    let selected: Int
    var height: CGFloat = 44
    var outerRadius: CGFloat = 16
    var innerRadius: CGFloat = 12
    var pad: CGFloat = 4
    var gap: CGFloat = 4
    var label = ""
    let onSelect: (Int) -> Void
    var body: some View {
        HStack(spacing: gap) {
            ForEach(Array(options.enumerated()), id: \.offset) { i, o in
                let on = i == selected
                let shape = SmoothRoundedRectangle(cornerRadius: innerRadius)
                Button { onSelect(i) } label: {
                    Text(o).peopleStyle(.bodyB).foregroundStyle(on ? c.ink : c.ink2)
                        .frame(maxWidth: .infinity, minHeight: height)
                        .background(shape.fill(on ? c.sf : c.sf2.opacity(0)))
                        .overlay { if on { shape.stroke(c.peopleHairline, lineWidth: 1) } }
                        .contentShape(shape)
                        .peopleAnimation(PeopleMotion.switch, value: on)
                }
                .buttonStyle(PeoplePressStyle())
                .accessibilityAddTraits(on ? [.isSelected] : [])
            }
        }
        .padding(pad)
        .background(SmoothRoundedRectangle(cornerRadius: outerRadius).fill(c.sf2))
        .accessibilityElement(children: .contain).accessibilityLabel(label)
    }
}

// MARK: - fields

/// gate/Field.svelte: label (13 px 650 ink) over a 48 tall input (r14, line border, surface). Focus: a 2 px accent-ink outline
/// 1 px outside, border hidden. Error: danger border. `reserveLines` keeps the message area at that many 18 px lines, so a
/// message appearing moves nothing below.
struct PeopleGateField: View {
    @Environment(\.tin) private var c
    @Binding var text: String
    let label: String
    var secret = false
    var mono = false
    var error: String? = nil
    var hint: String? = nil
    var reserveLines: CGFloat = 0
    var enabled = true
    var placeholder = ""
    var onDone: (() -> Void)? = nil
    @FocusState private var focused: Bool
    @State private var shown = false

    var body: some View {
        let shape = SmoothRoundedRectangle(cornerRadius: 14)
        VStack(alignment: .leading, spacing: 6) {
            Text(label).peopleStyle(.label).foregroundStyle(c.ink)
            HStack(spacing: 0) {
                Group {
                    if secret && !shown { SecureField(placeholder, text: $text) } else { TextField(placeholder, text: $text) }
                }
                .focused($focused).disabled(!enabled).submitLabel(.done).onSubmit { onDone?() }
                .textInputAutocapitalization(.never).autocorrectionDisabled(secret || mono)
                .peopleStyle(mono ? .mono : .body, size: mono ? 15 : nil, lineHeight: mono ? 21 : nil).foregroundStyle(c.ink).tint(c.peopleAccentInk)
                .padding(.leading, 14).padding(.trailing, secret ? 4 : 14).frame(maxWidth: .infinity, minHeight: 48)
                if secret {
                    PeopleIconButton(icon: shown ? .eyeoff : .eye, label: shown ? "Hide \(label.lowercased())" : "Show \(label.lowercased())",
                                     tint: c.ink2, iconSize: 20) { shown.toggle(); focused = true }
                }
            }
            .background(shape.fill(c.sf))
            .overlay(shape.stroke(focused ? .clear : error != nil ? c.er : c.ln, lineWidth: 1))
            .overlay { if focused { SmoothRoundedRectangle(cornerRadius: 16).stroke(c.peopleAccentInk, lineWidth: 2).padding(-2) } }
            .opacity(enabled ? 1 : 0.6)
            let msg = VStack(alignment: .leading, spacing: 2) {
                if let error { Text(error).peopleStyle(.sub, weight: .semibold).foregroundStyle(c.er).accessibilityAddTraits(.updatesFrequently) }
                if let hint { Text(hint).peopleStyle(.sub).foregroundStyle(c.ink2) }
            }
            if reserveLines > 0 { msg.frame(maxWidth: .infinity, minHeight: reserveLines * 18, alignment: .topLeading) }
            else if error != nil || hint != nil { msg }
        }
    }
}

/// people/PassField.svelte (pages and dialogs): label 13 px 600, a 44 tall input with the eye inside, focus = accent border plus a
/// 1 px accent ring, and a message line that is always there (19 px), so an error never moves the buttons.
struct PeopleField: View {
    @Environment(\.tin) private var c
    @Binding var text: String
    var label = "Passphrase"
    var error = ""
    var hint = ""
    var enabled = true
    var secret = true
    var mono = false
    var placeholder = ""
    var msgLines = 1
    var onDone: (() -> Void)? = nil
    @FocusState private var focused: Bool
    @State private var shown = false

    var body: some View {
        let shape = SmoothRoundedRectangle(cornerRadius: 14)
        let bad = !error.isEmpty
        VStack(alignment: .leading, spacing: 6) {
            Text(label).peopleStyle(.sub, weight: .semibold).foregroundStyle(c.ink)
            HStack(spacing: 0) {
                Group {
                    if secret && !shown { SecureField(placeholder, text: $text) } else { TextField(placeholder, text: $text) }
                }
                .focused($focused).disabled(!enabled).submitLabel(.done).onSubmit { onDone?() }
                .textInputAutocapitalization(.never).autocorrectionDisabled(secret || mono)
                .peopleStyle(mono ? .mono : .body).tracking(mono ? 0.04 * 15 : 0).foregroundStyle(c.ink).tint(c.peopleAccentInk)
                .padding(.leading, 14).padding(.trailing, secret ? 0 : 14).frame(maxWidth: .infinity, minHeight: 44)
                if secret {
                    PeopleIconButton(icon: shown ? .eyeoff : .eye, label: shown ? "Hide passphrase" : "Show passphrase", tint: c.ink2, iconSize: 20) {
                        shown.toggle(); focused = true
                    }
                }
            }
            .background(shape.fill(c.sf))
            .overlay(shape.stroke(bad ? c.er : focused ? c.peopleAccent : c.ln, lineWidth: focused && !bad ? 2 : 1))
            Text(bad ? error : hint).peopleStyle(.sub19).foregroundStyle(bad ? c.er : c.ink2)
                .frame(maxWidth: .infinity, minHeight: CGFloat(19 * msgLines), alignment: .topLeading)
                .accessibilityAddTraits(bad ? .updatesFrequently : [])
        }
    }
}

// MARK: - notices

/// gate/Notice.svelte: 12/14 padding, r14, 13/19; info = surface-2 ink, warn = warn-bg, error = danger-bg.
struct PeopleNotice: View {
    enum Kind { case info, warn, error }
    @Environment(\.tin) private var c
    let text: Text
    var kind: Kind = .info
    var icon: PeopleIcon? = nil

    init(_ text: String, kind: Kind = .info, icon: PeopleIcon? = nil) { self.text = Text(text); self.kind = kind; self.icon = icon }
    init(text: Text, kind: Kind = .info, icon: PeopleIcon? = nil) { self.text = text; self.kind = kind; self.icon = icon }

    var body: some View {
        let (bg, fg): (Color, Color) = {
            switch kind { case .info: return (c.sf2, c.ink); case .warn: return (c.warn, c.onWarn); case .error: return (c.erc, c.er) }
        }()
        HStack(alignment: .top, spacing: 12) {
            PeopleIconView(icon: icon ?? (kind == .info ? .info : .warning), size: 20, color: fg).padding(.top, 1)
            text.peopleStyle(.sub19).foregroundStyle(fg).frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, 14).padding(.vertical, 12)
        .background(SmoothRoundedRectangle(cornerRadius: 14).fill(bg))
        .accessibilityAddTraits(kind == .error ? .updatesFrequently : [])
    }
}

/// A bold lead and the rest of the sentence, in one Text.
func peopleBoldLead(_ bold: String, _ rest: String, boldFirst: Bool = true) -> Text {
    boldFirst ? Text(bold).bold() + Text(" " + rest) : Text(rest + " ") + Text(bold).bold()
}

/// Icon disc (accent-soft / accent-ink), used by Failed, Perms, Terms, Choose.
struct PeopleIconDisc: View {
    @Environment(\.tin) private var c
    let icon: PeopleIcon
    let size: CGFloat
    let iconSize: CGFloat
    var danger = false
    var body: some View {
        Circle().fill(danger ? c.erc : c.peopleAccentSoft).frame(width: size, height: size)
            .overlay(PeopleIconView(icon: icon, size: iconSize, color: danger ? c.er : c.peopleAccentInk))
    }
}

// MARK: - gate frame (Gate.svelte + StepShell.svelte)

/// The gate header: a 44 back slot, then the progress segments (14 x 6, r3, gap 4; line / accent) and "n of m" at the end.
/// Without progress or back it shrinks to 24 (Android `.head.empty`). `onToggleTheme` is the Shell's light/dark switch.
struct PeopleGateHead: View {
    @Environment(\.tin) private var c
    var onBack: (() -> Void)? = nil
    var progress: (step: Int, total: Int)? = nil
    var stepLabel = ""
    var onToggleTheme: (() -> Void)? = nil

    // Light/dark right in sign-up (Gate.svelte): the first screens are where someone decides if this feels like theirs.
    private var theme: some View {
        PeopleIconButton(icon: c.dark ? .sun : .moon, label: c.dark ? "Switch to light" : "Switch to dark", tint: c.ink2, size: 40, iconSize: 20) {
            onToggleTheme?()
        }
        .offset(x: 10).opacity(onToggleTheme == nil ? 0 : 1).disabled(onToggleTheme == nil)
    }

    var body: some View {
        if onBack == nil && progress == nil {
            HStack { Spacer(); theme }.frame(minHeight: 40).padding(.leading, 8).padding(.trailing, 20)
        } else {
            HStack(spacing: 12) {
                ZStack { if let onBack { PeopleIconButton(icon: .back, label: "Back", action: onBack) } }.frame(width: 44)
                Spacer(minLength: 0)
                if let progress {
                    HStack(spacing: 10) {
                        HStack(spacing: 4) {
                            ForEach(0..<progress.total, id: \.self) { i in
                                RoundedRectangle(cornerRadius: 3).fill(i < progress.step ? c.peopleAccent : c.ln).frame(width: 14, height: 6)
                                    .peopleAnimation(PeopleMotion.switch, value: i < progress.step)
                            }
                        }
                        // step 0 (Welcome, Choose): the count keeps its width, hidden, so "1 of 6" appears in place
                        Text("\(progress.step) of \(progress.total)").peopleStyle(.sub, tnum: true).foregroundStyle(c.ink2)
                            .frame(minWidth: 42, alignment: .trailing).opacity(progress.step == 0 ? 0 : 1)
                    }
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel("Setup progress, step \(progress.step) of \(progress.total)" + (stepLabel.isEmpty ? "" : ": \(stepLabel)"))
                }
                theme
            }
            .frame(minHeight: 56).padding(.leading, 8).padding(.trailing, 20)
        }
    }
}

/// gate/Btn.svelte `next` on Android: Continue is a 56 round accent button with an arrow at the end of the footer; the label stays
/// the accessible name, and busy swaps the arrow for a spinner so the size never changes. The arrow mirrors in RTL.
struct PeopleGateNext: View {
    @Environment(\.tin) private var c
    let label: String
    var enabled = true
    var busy = false
    let action: () -> Void
    var body: some View {
        let on = enabled && !busy
        let (bg, fg): (Color, Color) = busy ? (c.peopleAccent.opacity(0.85), c.peopleOnAccent) : on ? (c.peopleAccent, c.peopleOnAccent) : (c.sf2, c.ink2)
        Button(action: action) {
            Circle().fill(bg).frame(width: 56, height: 56)
                .overlay { if busy { PeopleSpinner(size: 20, color: fg) } else { PeopleIconView(icon: .next, size: 24, color: fg).flipsForRightToLeftLayoutDirection(true) } }
        }
        .buttonStyle(PeoplePressStyle(enabled: on)).disabled(!on).accessibilityLabel(label)
    }
}

/// The footer row when Continue is the round arrow: an optional secondary at the start, the arrow at the end.
struct PeopleGateNextRow<Next: View, Start: View>: View {
    @ViewBuilder let next: Next
    @ViewBuilder let start: Start
    var body: some View { HStack { start; Spacer(minLength: 0); next } }
}

/// gate/StepShell.svelte on Android: heading (26/32 bold), optional lead (15/22 muted), body (gap 16, padding 8 24 16) that scrolls,
/// footer (padding 12 24 24, gap 8, full-width buttons). `center` centres the body; with `anchor` it hangs from a fixed point
/// (min(14 % of the height, 96 pt) from the top) so an added line never re-centres the rest.
struct PeopleGateStep<Hero: View, Main: View, Foot: View>: View {
    @Environment(\.tin) private var c
    let title: String
    var lead: String? = nil
    var center = false
    var anchor = false
    var bigTitle = false
    /// The body fills the height instead of scrolling (Welcome's mark, Choose's options and Terms' consent line sit at the bottom).
    var fill = false
    @ViewBuilder let hero: Hero
    @ViewBuilder let content: Main
    @ViewBuilder let foot: Foot

    var body: some View {
        GeometryReader { geo in
            // The keyboard avoids the safe area itself, so the height already follows it: anchor from this height.
            let anchorTop = min(geo.size.height * 0.14, 96)
            VStack(spacing: 0) {
                let column = VStack(alignment: center ? .center : .leading, spacing: 16) {
                    hero
                    Text(title).peopleStyle(bigTitle ? .h1Center : .h1).foregroundStyle(c.ink).accessibilityAddTraits(.isHeader)
                        .frame(maxWidth: .infinity, alignment: center ? .center : .leading).multilineTextAlignment(center ? .center : .leading)
                    if let lead {
                        Text(lead).peopleStyle(.lead).foregroundStyle(c.ink2).frame(maxWidth: .infinity, alignment: center ? .center : .leading)
                            .multilineTextAlignment(center ? .center : .leading)
                    }
                    content
                }
                .padding(.horizontal, 24).padding(.top, center && anchor ? 8 + anchorTop : 8).padding(.bottom, 16)
                if fill { column.frame(maxHeight: .infinity, alignment: .top) }
                else if center && !anchor { column.frame(maxHeight: .infinity) }
                else { ScrollView { column } }
                VStack(spacing: 8) { foot }.padding(.horizontal, 24).padding(.top, 12).padding(.bottom, 24)
            }
        }
    }
}

/// The stage (Gate.svelte .stage): the step slides 36 pt in the inline direction and fades; enter 200 / exit 140, cubic-out.
struct PeopleGateStage<Step: Hashable, Content: View>: View {
    @Environment(\.accessibilityReduceMotion) private var reduced
    let step: Step
    /// +1 forward, -1 back.
    let dir: Int
    @ViewBuilder let content: (Step) -> Content
    var body: some View {
        PeoplePage {
            ZStack {
                content(step).id(step)
                    .transition(reduced ? .opacity : .asymmetric(
                        insertion: .modifier(active: PeopleSlide(dx: CGFloat(36 * dir), opacity: 0), identity: PeopleSlide(dx: 0, opacity: 1)),
                        removal: .modifier(active: PeopleSlide(dx: CGFloat(-36 * dir), opacity: 0), identity: PeopleSlide(dx: 0, opacity: 1))))
            }
            .animation(reduced ? nil : PeopleMotion.cubicOut(PeopleMotion.enter), value: step)
        }
    }
}

private struct PeopleSlide: ViewModifier {
    let dx: CGFloat
    let opacity: Double
    func body(content: Content) -> some View { content.offset(x: dx).opacity(opacity) }
}

// MARK: - pages (profile/profile.css)

/// The page surface: full screen on the background colour.
struct PeoplePage<Content: View>: View {
    @Environment(\.tin) private var c
    @ViewBuilder let content: Content
    var body: some View { VStack(spacing: 0) { content }.frame(maxWidth: .infinity, maxHeight: .infinity).background(c.bg.ignoresSafeArea()) }
}

/// Components.kt TopBar: 64 tall, back button, 20 px title.
struct PeopleTopBar<Actions: View>: View {
    @Environment(\.tin) private var c
    var title: String? = nil
    var onBack: (() -> Void)? = nil
    @ViewBuilder let actions: Actions
    init(title: String? = nil, onBack: (() -> Void)? = nil, @ViewBuilder actions: () -> Actions) { self.title = title; self.onBack = onBack; self.actions = actions() }
    var body: some View {
        HStack(spacing: 0) {
            if let onBack { PeopleIconButton(icon: .back, label: "Back", action: onBack) } else { Spacer().frame(width: 16) }
            Text(title ?? "").peopleStyle(.title, size: 20, lineHeight: 26).foregroundStyle(c.ink).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
            actions
        }
        .frame(minHeight: 64).padding(.leading, 4).padding(.trailing, 8)
    }
}
extension PeopleTopBar where Actions == EmptyView {
    init(title: String? = nil, onBack: (() -> Void)? = nil) { self.init(title: title, onBack: onBack) { EmptyView() } }
}

/// Components.kt Hint: a muted 13 px line.
struct PeopleHint: View {
    @Environment(\.tin) private var c
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View { Text(text).peopleStyle(.sub19).foregroundStyle(c.ink2).frame(maxWidth: .infinity, alignment: .leading) }
}

/// .pf-bar: 60 tall, surface, hairline below; back button then the title (17 px 600).
struct PeopleBar<Actions: View>: View {
    @Environment(\.tin) private var c
    let title: String
    let onBack: () -> Void
    @ViewBuilder let actions: Actions
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 4) {
                PeopleIconButton(icon: .back, label: "Back", action: onBack)
                Text(title).peopleStyle(.barTitle).foregroundStyle(c.ink).lineLimit(1).accessibilityAddTraits(.isHeader).frame(maxWidth: .infinity, alignment: .leading)
                actions
            }
            .frame(minHeight: 60).padding(.leading, 8).padding(.trailing, 12).padding(.vertical, 8)
            Rectangle().fill(c.peopleHairline).frame(height: 1)
        }
        .background(c.sf)
    }
}

/// .pf: the page (bg), bar, and a scrolling column (max 560, padding-bottom 40). `overlay` floats over the page without moving it.
struct PeopleProfilePage<Overlay: View, Content: View>: View {
    let title: String
    let onBack: () -> Void
    var scroll = true
    var bottomPad: CGFloat = 40
    @ViewBuilder let overlay: Overlay
    @ViewBuilder let content: Content
    var body: some View {
        PeoplePage {
            PeopleBar(title: title, onBack: onBack) { EmptyView() }
            ZStack {
                let column = VStack(spacing: 0) { content }.frame(maxWidth: 560).frame(maxWidth: .infinity).padding(.bottom, bottomPad)
                if scroll { ScrollView { column } } else { column }
                overlay
            }
            .frame(maxHeight: .infinity)
        }
    }
}

/// .pf-sec: padding 0 16, 24 above (16 for the first); optional uppercase heading (12 px 650, .06em, muted).
struct PeopleSection<Content: View>: View {
    @Environment(\.tin) private var c
    var title: String? = nil
    var first = false
    var top: CGFloat? = nil
    @ViewBuilder let content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let title {
                Text(title.uppercased()).peopleStyle(.secHead).foregroundStyle(c.ink2).accessibilityAddTraits(.isHeader)
                    .padding(.horizontal, 4).padding(.bottom, 8)
            }
            content
        }
        .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 16).padding(.top, top ?? (first ? 16 : 24))
    }
}

/// The hairline between a card's rows (.pf-card > * + *). Action rows (`.pf-act`) run together without it.
struct PeopleHairline: View {
    @Environment(\.tin) private var c
    var body: some View { Rectangle().fill(c.peopleHairline).frame(height: 1) }
}

/// .pf-card: r14 surface with an inset hairline. Callers put a `PeopleHairline()` between rows that need one.
struct PeopleCard<Content: View>: View {
    @Environment(\.tin) private var c
    @ViewBuilder let content: Content
    var body: some View {
        // profile.css .pf-card is a CSS border-radius (circular), not a shape.ts path.
        let shape = RoundedRectangle(cornerRadius: 14, style: .circular)
        VStack(spacing: 0) { content }.frame(maxWidth: .infinity)
            .background(shape.fill(c.sf)).overlay(shape.stroke(c.peopleHairline, lineWidth: 1)).clipShape(shape)
    }
}

/// .pf-note: 10 above, 4 inline, 13/19 muted.
struct PeopleNote: View {
    @Environment(\.tin) private var c
    let text: String
    var align: TextAlignment = .leading
    var top: CGFloat = 10
    init(_ text: String, align: TextAlignment = .leading, top: CGFloat = 10) { self.text = text; self.align = align; self.top = top }
    var body: some View {
        Text(text).peopleStyle(.sub19).foregroundStyle(c.ink2).multilineTextAlignment(align)
            .frame(maxWidth: .infinity, alignment: align == .center ? .center : .leading).padding(.horizontal, 4).padding(.top, top)
    }
}

/// .pf-act: a 60 tall row, icon (muted 20) - title (600, one line) / sub (13/18 muted) - trailing. Clickable when `action` is set.
struct PeopleAct: View {
    @Environment(\.tin) private var c
    let title: String
    var action: (() -> Void)? = nil
    var icon: PeopleIcon? = nil
    var sub: String? = nil
    var danger = false
    var chevron = false
    var enabled = true
    var subColor: Color? = nil
    var subBold = false
    var minHeight: CGFloat = 60
    var gap: CGFloat = 14
    var padV: CGFloat = 8
    var padH: CGFloat = 16
    var subMaxLines: Int? = nil
    /// No layout shift: every text this sub can switch to. They are laid out invisibly underneath, so the line keeps the
    /// height of the tallest variant at any width or font size.
    var subReserve: [String] = []
    var leading: AnyView? = nil
    var trailing: AnyView? = nil

    var body: some View {
        let row = HStack(spacing: gap) {
            if let leading { leading } else if let icon { PeopleIconView(icon: icon, size: 20, color: danger ? c.er : c.ink2) }
            VStack(alignment: .leading, spacing: 1) {
                Text(title).peopleStyle(.bodyB).foregroundStyle(danger ? c.er : c.ink).lineLimit(1)
                if let sub, !sub.isEmpty {
                    ZStack(alignment: .topLeading) {
                        ForEach(subReserve.filter { $0 != sub }, id: \.self) { alt in
                            Text(alt).peopleStyle(.sub, weight: subBold ? .semibold : .regular, tnum: true).lineLimit(subMaxLines).hidden().accessibilityHidden(true)
                        }
                        Text(sub).peopleStyle(.sub, weight: subBold ? .semibold : .regular, tnum: true).foregroundStyle(subColor ?? c.ink2).lineLimit(subMaxLines)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if let trailing { trailing }
            if chevron { PeopleIconView(icon: .chevr, size: 18, color: c.ink2) }
        }
        .padding(.horizontal, padH).padding(.vertical, padV).frame(minHeight: minHeight).opacity(enabled ? 1 : 0.55)
        if let action {
            Button(action: action) { row.contentShape(Rectangle()) }.buttonStyle(PeoplePressStyle(enabled: enabled)).disabled(!enabled)
        } else { row }
    }
}

/// .pf-strip: an inline status (surface-2, r14, 13/19).
struct PeopleStrip: View {
    @Environment(\.tin) private var c
    let text: String
    let icon: PeopleIcon
    var floating = false
    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 14, style: .circular)
        HStack(spacing: 8) {
            PeopleIconView(icon: icon, size: 16, color: c.ink2)
            Text(text).peopleStyle(.sub19).foregroundStyle(c.ink).frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
        .background(shape.fill(floating ? c.sfRaised : c.sf2))
        .overlay { if floating { shape.stroke(c.peopleHairline, lineWidth: 1) } }
        .peopleDepth(floating ? .pop : .flat)
        .accessibilityElement(children: .combine)
    }
}

/// .pf-err: danger-bg block, r12, 13/19.
struct PeopleErr: View {
    @Environment(\.tin) private var c
    let text: String
    var body: some View {
        Text(text).peopleStyle(.sub19).foregroundStyle(c.er).frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 12).padding(.vertical, 10)
            .background(RoundedRectangle(cornerRadius: 12, style: .circular).fill(c.erc))
            .padding(.top, 10).accessibilityAddTraits(.updatesFrequently)
    }
}

/// A line that is always there and cross-fades its text (reserved space: nothing moves).
struct PeopleReservedLine: View {
    @Environment(\.tin) private var c
    let text: String
    var style: PeopleTextStyle = .sub19
    var color: Color? = nil
    var align: TextAlignment = .leading
    var lines = 1
    var body: some View {
        Text(text).peopleStyle(style).foregroundStyle(color ?? c.ink2).multilineTextAlignment(align)
            .frame(maxWidth: .infinity, minHeight: style.spec.lh * CGFloat(lines), alignment: align == .center ? .top : .topLeading)
            .id(text).transition(.opacity).peopleAnimation(PeopleMotion.crossfade, value: text)
            .accessibilityAddTraits(.updatesFrequently)
    }
}

// MARK: - dialogs (chat/Layer.svelte, dialog = bottom sheet)

/// .pf-dlg h2 (17 px 650, 24) and paragraphs (13/19 muted, 8 above; lead = 15/21 ink).
struct PeopleDialogTitle: View {
    @Environment(\.tin) private var c
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text).peopleStyle(.title).foregroundStyle(c.ink).accessibilityAddTraits(.isHeader).padding(.bottom, 4).frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct PeopleDialogText: View {
    @Environment(\.tin) private var c
    let text: String
    var lead = false
    init(_ text: String, lead: Bool = false) { self.text = text; self.lead = lead }
    var body: some View {
        Text(text).peopleStyle(lead ? .body : .sub19).foregroundStyle(lead ? c.ink : c.ink2).frame(maxWidth: .infinity, alignment: .leading).padding(.top, 8)
    }
}

/// Dialog content padded as .pf-dlg (20 20 16) with its title.
struct PeopleDialogBody<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 0) { PeopleDialogTitle(title); content }
            .padding(.leading, 20).padding(.trailing, 20).padding(.top, 20).padding(.bottom, 16)
    }
}

private struct PeopleSheetHeight: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

/// A Layer dialog: a bottom sheet (surface-raised, r24 top, the notch) sized to its content, with the playground scrim.
private struct PeopleSheetContent<Content: View>: View {
    @Environment(\.tin) private var c
    @State private var height: CGFloat = 0
    @ViewBuilder let content: Content
    var body: some View {
        ScrollView {
            content.frame(maxWidth: .infinity)
                .background(GeometryReader { Color.clear.preference(key: PeopleSheetHeight.self, value: $0.size.height) })
        }
        .scrollBounceBehavior(.basedOnSize)
        .onPreferenceChange(PeopleSheetHeight.self) { height = $0 }
        .presentationDetents(height > 0 ? [.height(min(height + 28, 720)), .large] : [.medium, .large])
        .presentationDragIndicator(.visible).presentationCornerRadius(24).presentationBackground(c.sfRaised)
        .tinTheme()
    }
}

extension View {
    func peopleSheet<Content: View>(isPresented: Binding<Bool>, @ViewBuilder content: @escaping () -> Content) -> some View {
        sheet(isPresented: isPresented) { PeopleSheetContent { content() } }
    }
    func peopleSheet<Item: Identifiable, Content: View>(item: Binding<Item?>, @ViewBuilder content: @escaping (Item) -> Content) -> some View {
        sheet(item: item) { i in PeopleSheetContent { content(i) } }
    }
}

// MARK: - people

/// people/PersonRow.svelte: 64 tall, 44 avatar, gap 12, name (600, 21) over a one-line sub that always holds its 18 px.
struct PersonRowView<End: View>: View {
    @Environment(\.tin) private var c
    let name: String
    let did: String
    let sub: String
    var onClick: (() -> Void)? = nil
    var pad: CGFloat = 8
    var dim = false
    @ViewBuilder let end: End
    var body: some View {
        HStack(spacing: 8) {
            let main = HStack(spacing: 12) {
                TinAvatar(did: did, size: 44).opacity(dim ? 0.6 : 1)
                VStack(alignment: .leading, spacing: 1) {
                    Text(name).peopleStyle(.bodyB, lineHeight: 21).foregroundStyle(c.ink.opacity(dim ? 0.6 : 1)).lineLimit(1)
                    Text(sub).peopleStyle(.sub).foregroundStyle(c.ink2).lineLimit(1).frame(minHeight: 18, alignment: .topLeading)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minHeight: 64).contentShape(SmoothRoundedRectangle(cornerRadius: 14))
            if let onClick { Button(action: onClick) { main }.buttonStyle(.plain) } else { main }
            end
        }
        .padding(.horizontal, pad)
    }
}

extension PersonRowView where End == EmptyView {
    init(name: String, did: String, sub: String, onClick: (() -> Void)? = nil, pad: CGFloat = 8, dim: Bool = false) {
        self.init(name: name, did: did, sub: sub, onClick: onClick, pad: pad, dim: dim) { EmptyView() }
    }
}
