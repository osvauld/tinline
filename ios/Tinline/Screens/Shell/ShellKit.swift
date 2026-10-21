import SwiftUI

/*
 * The Shell's UI kit: the playground pieces the shell, sign-up and settings screens share (android PeopleKit.kt gate and page
 * furniture, Components.kt's older page parts, Shell.kt's press and badges). Sizes and copy are the Kotlin's; colours are the
 * `tin` tokens plus the tokens.css extras Theme.kt adds (`ShellPalette`).
 */

/// tokens.css colours beyond `TinColors` (android Theme.kt extension properties), resolved for light or dark.
struct ShellPalette {
    let c: TinColors
    init(_ c: TinColors) { self.c = c }
    var accent: Color { c.dark ? Color(hex: 0x117E6B) : Color(hex: 0x0B6B5B) }
    var onAccent: Color { c.dark ? Color(hex: 0xF2FFFB) : .white }
    var accentInk: Color { c.dark ? Color(hex: 0x6FD3BD) : Color(hex: 0x0A5D50) }
    var accentSoft: Color { c.dark ? Color(hex: 0x14433A) : Color(hex: 0xD7ECE6) }
    var hairline: Color { c.dark ? Color(hex: 0x2A3531) : Color(hex: 0xE2E7E1) }
    var dangerBtn: Color { c.dark ? Color(hex: 0xC73A30) : Color(hex: 0xA12B23) }
    var danger: Color { c.dark ? Color(hex: 0xF2B8B5) : Color(hex: 0xA12B23) }
    var goldInk: Color { c.dark ? Color(hex: 0xE8B04A) : Color(hex: 0x845A10) }
}

// ---------------------------------------------------------------- type (PgType, TinFs)

/// The playground's type scale (PeopleKit.kt PgType, Theme.kt TinFs): size / line height / weight.
enum ShellStyle {
    case h1, h1Center, h1Page, lead, title, barTitle, body, bodyB, sub, sub19, meta, label, btn, btnGroups, secHead, mono
    /// Components.kt TopBar title: titleM at 20/26.
    case topBar
    /// TinType.bodyL / bodyM on the older pages.
    case bodyL, bodyM

    var size: CGFloat {
        switch self {
        case .h1: 26; case .h1Center: 30; case .h1Page: 22; case .lead: 15; case .title, .barTitle: 17
        case .body, .bodyB, .btn, .btnGroups, .mono: 15; case .sub, .sub19, .label: 13; case .meta: 11; case .secHead: 12
        case .topBar: 20; case .bodyL: 16; case .bodyM: 14
        }
    }
    var lineHeight: CGFloat {
        switch self {
        case .h1: 32; case .h1Center: 36; case .h1Page: 28; case .lead: 22; case .title, .barTitle: 24
        case .body: 21; case .bodyB, .btn, .btnGroups, .mono: 20; case .sub, .label: 18; case .sub19: 19; case .meta: 14; case .secHead: 16
        case .topBar: 26; case .bodyL: 22; case .bodyM: 20
        }
    }
    /// 650 has no SwiftUI weight; it maps to semibold.
    var weight: Font.Weight {
        switch self {
        case .h1, .h1Center: .bold
        case .h1Page, .title, .barTitle, .bodyB, .label, .btn, .btnGroups, .secHead, .topBar: .semibold
        default: .regular
        }
    }
    var textStyle: Font.TextStyle {
        switch self {
        case .h1, .h1Center, .h1Page: .title; case .title, .barTitle, .topBar: .headline
        case .sub, .sub19, .label, .secHead: .footnote; case .meta: .caption2; default: .body
        }
    }
    var tracking: CGFloat {
        switch self { case .h1, .h1Center: -0.01 * size; case .secHead: 0.06 * size; default: 0 }
    }
}

enum ShellFont {
    static func of(_ s: ShellStyle, weight: Font.Weight? = nil) -> Font {
        if s == .mono { return .custom("IBMPlexMono-Medium", size: s.size, relativeTo: .body) }
        return .custom("Figtree", size: s.size, relativeTo: s.textStyle).weight(weight ?? s.weight)
    }
    static func figtree(_ size: CGFloat, _ weight: Font.Weight = .regular, relativeTo: Font.TextStyle = .body) -> Font {
        .custom("Figtree", size: size, relativeTo: relativeTo).weight(weight)
    }
    static func mono(_ size: CGFloat, _ weight: Font.Weight = .medium) -> Font {
        .custom(weight == .regular ? "IBMPlexMono-Regular" : "IBMPlexMono-Medium", size: size, relativeTo: .body)
    }
}

extension View {
    /// Font, CSS line height (as line spacing) and tracking of a `ShellStyle`.
    func shellStyle(_ s: ShellStyle, weight: Font.Weight? = nil) -> some View {
        font(ShellFont.of(s, weight: weight)).lineSpacing(max(0, s.lineHeight - s.size * 1.2)).tracking(s.tracking)
    }
}

// ---------------------------------------------------------------- icons

/// The icons the Shell draws (android PgIcons), each a generated Tinline line icon (Components/TinIcons.swift).
enum ShellGlyph {
    case micoff, back, next, chev, chevup, chevr, search, gear, plus, x, check, chat, users, phone, contact, userPlus, callIn, callOut, callEnd, clock, off, mute, bell, battery, lock, mic, qr, key, devices, laptop, smartphone, info, warning, shield, eye, eyeoff, vol, video, save, licence, file, code, globe, copy, share, trash, edit, retry, camera, torch, torchoff, link, scan, paste, moon, sun

    var data: TinIconData {
        switch self {
        case .micoff: TinIcons.micoff
        case .back: TinIcons.back
        case .next: TinIcons.next
        case .chev: TinIcons.chev
        case .chevup: TinIcons.chevup
        case .chevr: TinIcons.chevr
        case .search: TinIcons.search
        case .gear: TinIcons.gear
        case .plus: TinIcons.plus
        case .x: TinIcons.x
        case .check: TinIcons.check
        case .chat: TinIcons.chat
        case .users: TinIcons.users
        case .phone: TinIcons.phone
        case .contact: TinIcons.contact
        case .userPlus: TinIcons.userplus
        case .callIn: TinIcons.callIn
        case .callOut: TinIcons.callOut
        case .callEnd: TinIcons.callend
        case .clock: TinIcons.clock
        case .off: TinIcons.off
        case .mute: TinIcons.mute
        case .bell: TinIcons.bell
        case .battery: TinIcons.battery
        case .lock: TinIcons.lock
        case .mic: TinIcons.mic
        case .qr: TinIcons.qr
        case .key: TinIcons.key
        case .devices: TinIcons.devices
        case .laptop: TinIcons.laptop
        case .smartphone: TinIcons.smartphone
        case .info: TinIcons.info
        case .warning: TinIcons.warning
        case .shield: TinIcons.shield
        case .eye: TinIcons.eye
        case .eyeoff: TinIcons.eyeoff
        case .vol: TinIcons.vol
        case .video: TinIcons.video
        case .save: TinIcons.save
        case .licence: TinIcons.licence
        case .file: TinIcons.file
        case .code: TinIcons.code
        case .globe: TinIcons.globe
        case .copy: TinIcons.copy
        case .share: TinIcons.share
        case .trash: TinIcons.trash
        case .edit: TinIcons.edit
        case .retry: TinIcons.retry
        case .camera: TinIcons.camera
        case .torch: TinIcons.torch
        case .torchoff: TinIcons.torchoff
        case .link: TinIcons.link
        case .scan: TinIcons.scan
        case .paste: TinIcons.paste
        case .moon: TinIcons.moon
        case .sun: TinIcons.sun
        }
    }
}

/// One icon, decorative (the control around it carries the label).
struct ShellIcon: View {
    let glyph: ShellGlyph
    var size: CGFloat = 22
    var color: Color
    var body: some View { TinIcon(glyph.data, size: size, tint: color) }
}

// ---------------------------------------------------------------- press

/// `.press`: scale .97 while pressed (100 ms in, 140 ms back), plus the hover wash as the pressed fill.
struct ShellPressStyle: ButtonStyle {
    var wash: Color? = nil
    var rest: Color = .clear
    var radius: CGFloat = 14
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    func makeBody(configuration: Configuration) -> some View {
        let down = configuration.isPressed
        configuration.label
            .background(RoundedRectangle(cornerRadius: radius, style: .continuous).fill(down ? (wash ?? tin.sf2) : rest))
            .scaleEffect(down && !reduced ? 0.97 : 1)
            .animation(TinMotion.easeOut(down ? TinMotion.press : TinMotion.release), value: down)
            .contentShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
    }
}

/// lib/ui/IconButton: 44 circle, 22 icon, hover wash.
struct ShellIconButton: View {
    let glyph: ShellGlyph
    let label: String
    var tint: Color? = nil
    var size: CGFloat = 44
    var iconSize: CGFloat = 22
    var enabled = true
    let action: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        Button(action: action) {
            ShellIcon(glyph: glyph, size: iconSize, color: tint ?? tin.ink).opacity(enabled ? 1 : 0.45).frame(width: size, height: size)
        }
        .buttonStyle(ShellPressStyle(radius: size / 2)).disabled(!enabled).accessibilityLabel(label)
    }
}

/// An arc spinning once per 800 ms (still when motion is reduced).
struct ShellSpinner: View {
    var size: CGFloat = 16
    var color: Color
    var stroke: CGFloat = 2
    @Environment(\.accessibilityReduceMotion) private var reduced
    @State private var spin = false
    var body: some View {
        Circle().trim(from: 0, to: 0.75).stroke(color, style: StrokeStyle(lineWidth: stroke, lineCap: .butt))
            .frame(width: size, height: size).rotationEffect(.degrees(spin ? 360 : 0))
            .animation(reduced ? nil : .linear(duration: 0.8).repeatForever(autoreverses: false), value: spin)
            .onAppear { spin = true }.accessibilityHidden(true)
    }
}

// ---------------------------------------------------------------- buttons

/// gate/Btn.svelte. Primary / secondary / text; 48 tall, r 14, 650 15 px; busy shows a 16 px spinner.
enum ShellGKind { case primary, secondary, text }

struct ShellGBtn: View {
    let text: String
    var kind: ShellGKind = .primary
    var enabled = true
    var busy = false
    var fill = true
    let action: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin), on = enabled && !busy
        let (bg, fg): (Color, Color) = {
            switch kind {
            case .primary: return busy ? (p.accent.opacity(0.85), p.onAccent) : on ? (p.accent, p.onAccent) : (tin.sf2, tin.ink2)
            case .secondary, .text: return (.clear, p.accentInk)
            }
        }()
        Button(action: action) {
            HStack(spacing: 8) {
                if busy { ShellSpinner(size: 16, color: fg) }
                Text(text).shellStyle(.btn).foregroundStyle(fg).multilineTextAlignment(.center)
            }
            .padding(.horizontal, 20).frame(maxWidth: fill ? .infinity : nil, minHeight: 48).frame(minWidth: 44)
            .background(SmoothRoundedRectangle(cornerRadius: 14).fill(bg))
            .overlay { if kind == .secondary { SmoothRoundedRectangle(cornerRadius: 14).stroke(tin.ln, lineWidth: 1) } }
            .opacity(kind != .primary && !on ? 0.5 : 1)
        }
        .buttonStyle(ShellPressStyle(wash: .clear)).disabled(!on)
    }
}

/// groups/Btn.svelte: the pill used in pages and dialogs. 44 tall, r 22, padding 0 18, 600 15 px.
enum ShellPVariant { case primary, outline, quiet, danger, dangerQuiet }

struct ShellPBtn: View {
    let text: String
    var variant: ShellPVariant = .outline
    var enabled = true
    var glyph: ShellGlyph? = nil
    var height: CGFloat = 44
    var padH: CGFloat = 18
    var colorOverride: Color? = nil
    var minWidth: CGFloat = 44
    let action: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin)
        let (bg, fg, line): (Color, Color, Color) = {
            switch variant {
            case .primary: return (p.accent, p.onAccent, .clear)
            case .outline: return (tin.sf, tin.ink, tin.ln)
            case .quiet: return (.clear, tin.ink, .clear)
            case .danger: return (p.dangerBtn, .white, .clear)
            case .dangerQuiet: return (.clear, tin.er, .clear)
            }
        }()
        let ink = colorOverride ?? fg
        let outlined = colorOverride != nil && variant == .outline
        Button(action: action) {
            HStack(spacing: 8) {
                if let glyph { ShellIcon(glyph: glyph, size: 18, color: ink) }
                Text(text).shellStyle(.btnGroups).foregroundStyle(ink).lineLimit(1)
            }
            .padding(.horizontal, padH).frame(minWidth: minWidth, minHeight: height)
            .background(Capsule().fill(outlined ? .clear : bg))
            .overlay(Capsule().stroke(outlined ? colorOverride! : line, lineWidth: 1))
            .opacity(enabled ? 1 : 0.45)
        }
        .buttonStyle(ShellPressStyle(wash: .clear, radius: height / 2)).disabled(!enabled)
    }
}

/// Components.kt TinButton: the older 52 tall pill on Settings and Devices pages.
enum ShellBtnStyle { case filled, outlined, text, danger }

struct ShellTinButton: View {
    let text: String
    var style: ShellBtnStyle = .filled
    var glyph: ShellGlyph? = nil
    var enabled = true
    var fill = true
    let action: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let (bg, fg): (Color, Color) = {
            switch style {
            case .filled: return enabled ? (tin.pr, tin.onPr) : (tin.sf3, tin.ink2)
            case .outlined, .text: return (.clear, enabled ? tin.pr : tin.ink2)
            case .danger: return (tin.erc, tin.er)
            }
        }()
        Button(action: action) {
            HStack(spacing: 8) {
                if let glyph { ShellIcon(glyph: glyph, size: 20, color: fg) }
                Text(text).font(TinType.button).foregroundStyle(fg).multilineTextAlignment(.center)
            }
            .padding(.horizontal, style == .text && !fill ? 12 : 24).frame(maxWidth: fill ? .infinity : nil, minHeight: 52)
            .background(Capsule().fill(bg))
            .overlay { if style == .outlined { Capsule().stroke(tin.ln2, lineWidth: 1) } }
        }
        .buttonStyle(ShellPressStyle(wash: .clear, radius: 26)).disabled(!enabled)
    }
}

/// gate/Btn.svelte `next` on Android: Continue is a 56 round accent button with an arrow at the end of the footer; the label stays
/// the accessible name, and busy swaps the arrow for a spinner so the size never changes. The arrow mirrors in RTL.
struct ShellGNext: View {
    let label: String
    var enabled = true
    var busy = false
    let action: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin), on = enabled && !busy
        let (bg, fg): (Color, Color) = busy ? (p.accent.opacity(0.85), p.onAccent) : on ? (p.accent, p.onAccent) : (tin.sf2, tin.ink2)
        Button(action: action) {
            ZStack {
                Circle().fill(bg)
                if busy { ShellSpinner(size: 20, color: fg) } else { ShellIcon(glyph: .next, size: 24, color: fg).flipsForRightToLeftLayoutDirection(true) }
            }
            .frame(width: 56, height: 56)
        }
        .buttonStyle(ShellPressStyle(wash: .clear, radius: 28)).disabled(!on).accessibilityLabel(label)
    }
}

/// The footer row when Continue is the round arrow: an optional secondary at the start, the arrow at the end.
struct ShellGNextRow<Start: View>: View {
    let next: ShellGNext
    @ViewBuilder var start: () -> Start
    var body: some View {
        HStack { start(); Spacer(minLength: 0); next }
    }
}

extension ShellGNextRow where Start == EmptyView {
    init(next: ShellGNext) { self.next = next; self.start = { EmptyView() } }
}

// ---------------------------------------------------------------- fields

/// gate/Field.svelte: label (13 px 650 ink) over a 48 tall input (r 14, line border, surface). Focus: a 2 px accent-ink outline,
/// border hidden. Error: danger border. `reserveLines` keeps the message area at that many 18 px lines, so a message appearing
/// moves nothing below.
struct ShellGField: View {
    let label: String
    @Binding var text: String
    var secret = false
    var mono = false
    var error: String? = nil
    var hint: String? = nil
    var reserveLines: CGFloat = 0
    var enabled = true
    var placeholder = ""
    var keyboard: UIKeyboardType = .default
    var capitalization: TextInputAutocapitalization = .sentences
    var submit: SubmitLabel = .done
    var autofocus = false
    var onDone: (() -> Void)? = nil
    @Environment(\.tin) private var tin
    @FocusState private var focused: Bool
    @State private var shown = false

    var body: some View {
        let p = ShellPalette(tin)
        VStack(alignment: .leading, spacing: 6) {
            Text(label).shellStyle(.label).foregroundStyle(tin.ink)
            HStack(spacing: 0) {
                Group {
                    if secret && !shown { SecureField(placeholder, text: $text) } else { TextField(placeholder, text: $text) }
                }
                .font(mono ? ShellFont.mono(15) : ShellFont.of(.body)).foregroundStyle(tin.ink).tint(p.accentInk)
                .keyboardType(secret ? .asciiCapable : keyboard)
                .textInputAutocapitalization(secret ? .never : capitalization).autocorrectionDisabled(secret)
                .submitLabel(submit).onSubmit { onDone?() }
                .focused($focused).disabled(!enabled)
                .padding(.leading, 14).padding(.trailing, secret ? 4 : 14)
                if secret {
                    ShellIconButton(glyph: shown ? .eyeoff : .eye, label: shown ? "Hide \(label.lowercased())" : "Show \(label.lowercased())",
                                    tint: tin.ink2, iconSize: 20) { shown.toggle() }
                }
            }
            .frame(minHeight: 48)
            .background(SmoothRoundedRectangle(cornerRadius: 14).fill(tin.sf))
            .overlay(SmoothRoundedRectangle(cornerRadius: 14).stroke(focused ? .clear : (error != nil ? tin.er : tin.ln), lineWidth: 1))
            .overlay(SmoothRoundedRectangle(cornerRadius: 15).stroke(focused ? p.accentInk : .clear, lineWidth: 2).padding(-1))
            .opacity(enabled ? 1 : 0.6)
            if reserveLines > 0 || error != nil || hint != nil {
                VStack(alignment: .leading, spacing: 2) {
                    if let error { Text(error).shellStyle(.sub, weight: .semibold).foregroundStyle(tin.er) }
                    if let hint { Text(hint).shellStyle(.sub).foregroundStyle(tin.ink2) }
                }
                .frame(maxWidth: .infinity, minHeight: reserveLines * 18, alignment: .topLeading)
                .accessibilityElement(children: .combine)
            }
        }
        .task(id: autofocus) {
            // Focus on the first laid-out frame, during the entrance rather than after it.
            if autofocus { await Task.yield(); focused = true }
        }
    }
}

/// people/PassField.svelte (pages and dialogs): label 13 px 600, a 44 tall input with the eye inside, focus = accent border, and a
/// message line that is always there (19 px), so an error never moves the buttons.
struct ShellPassField: View {
    var label = "Passphrase"
    @Binding var text: String
    var error = ""
    var hint = ""
    var enabled = true
    var secret = true
    var mono = false
    var placeholder = ""
    var msgLines = 1
    var autofocus = false
    var onDone: (() -> Void)? = nil
    @Environment(\.tin) private var tin
    @FocusState private var focused: Bool
    @State private var shown = false

    var body: some View {
        let p = ShellPalette(tin), bad = !error.isEmpty
        VStack(alignment: .leading, spacing: 6) {
            if !label.isEmpty { Text(label).shellStyle(.sub, weight: .semibold).foregroundStyle(tin.ink) }
            HStack(spacing: 0) {
                Group {
                    if secret && !shown { SecureField(placeholder, text: $text) } else { TextField(placeholder, text: $text) }
                }
                .font(mono ? ShellFont.mono(15) : ShellFont.of(.body)).foregroundStyle(tin.ink).tint(p.accentInk)
                .textInputAutocapitalization(.never).autocorrectionDisabled(true)
                .submitLabel(.done).onSubmit { onDone?() }.focused($focused).disabled(!enabled)
                .padding(.leading, 14).padding(.trailing, secret ? 0 : 14)
                if secret {
                    ShellIconButton(glyph: shown ? .eyeoff : .eye, label: shown ? "Hide passphrase" : "Show passphrase", tint: tin.ink2, iconSize: 20) { shown.toggle() }
                }
            }
            .frame(minHeight: 44)
            .background(SmoothRoundedRectangle(cornerRadius: 14).fill(tin.sf))
            .overlay(SmoothRoundedRectangle(cornerRadius: 14).stroke(bad ? tin.er : (focused ? p.accent : tin.ln), lineWidth: focused && !bad ? 2 : 1))
            if msgLines > 0 {
                Text(bad ? error : hint).shellStyle(.sub19).foregroundStyle(bad ? tin.er : tin.ink2)
                    .frame(maxWidth: .infinity, minHeight: CGFloat(19 * msgLines), alignment: .topLeading)
            }
        }
        .task(id: autofocus) { if autofocus { await Task.yield(); focused = true } }
    }
}

// ---------------------------------------------------------------- notices, discs

/// gate/Notice.svelte: 12/14 padding, r 14, 13/19; info = surface-2 ink, warn = warn-bg, error = danger-bg.
enum ShellNKind { case info, warn, error }

struct ShellNotice: View {
    let text: Text
    var kind: ShellNKind = .info
    var glyph: ShellGlyph? = nil
    @Environment(\.tin) private var tin

    init(_ text: String, kind: ShellNKind = .info, glyph: ShellGlyph? = nil) { self.text = Text(text); self.kind = kind; self.glyph = glyph }
    init(text: Text, kind: ShellNKind = .info, glyph: ShellGlyph? = nil) { self.text = text; self.kind = kind; self.glyph = glyph }

    var body: some View {
        let (bg, fg): (Color, Color) = { switch kind { case .info: (tin.sf2, tin.ink); case .warn: (tin.warn, tin.onWarn); case .error: (tin.erc, tin.er) } }()
        HStack(alignment: .top, spacing: 12) {
            ShellIcon(glyph: glyph ?? (kind == .info ? .info : .warning), size: 20, color: fg).padding(.top, 1)
            text.shellStyle(.sub19).foregroundStyle(fg).frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, 14).padding(.vertical, 12)
        .background(SmoothRoundedRectangle(cornerRadius: 14).fill(bg))
        .accessibilityElement(children: .combine)
    }
}

/// Icon disc (accent-soft / accent-ink), used by Failed, Perms, Terms, Choose.
struct ShellIconDisc: View {
    let glyph: ShellGlyph
    let size: CGFloat
    let iconSize: CGFloat
    var danger = false
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin)
        ShellIcon(glyph: glyph, size: iconSize, color: danger ? tin.er : p.accentInk)
            .frame(width: size, height: size).background(Circle().fill(danger ? tin.erc : p.accentSoft))
    }
}

/// A line that is always there and cross-fades its text (reserved space: nothing moves).
struct ShellReservedLine: View {
    let text: String
    var style: ShellStyle = .sub19
    var color: Color? = nil
    var alignment: TextAlignment = .leading
    var lines = 1
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    var body: some View {
        Text(text).shellStyle(style).foregroundStyle(color ?? tin.ink2).multilineTextAlignment(alignment)
            .frame(maxWidth: .infinity, minHeight: style.lineHeight * CGFloat(lines), alignment: alignment == .center ? .top : .topLeading)
            .id(text).transition(.opacity)
            .animation(reduced ? nil : TinMotion.easeOut(TinMotion.crossfade), value: text)
    }
}

// ---------------------------------------------------------------- gate frame (Gate.svelte + StepShell.svelte)

/// The gate header: a 44 back slot, then the progress segments (14 x 6, r 3, gap 4; line / accent) and "n of m" at the end.
/// Without progress or back it shrinks to the theme switch alone (Android `.head.empty`).
struct ShellGateHead: View {
    var onBack: (() -> Void)?
    var progress: (Int, Int)?
    var label = ""
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    @AppStorage("appearance.dark") private var appearanceDark: Bool?

    var body: some View {
        let p = ShellPalette(tin)
        // Light/dark right in sign-up (Gate.svelte): the first screens are where someone decides if this feels like theirs.
        let theme = ShellIconButton(glyph: tin.dark ? .sun : .moon, label: tin.dark ? "Switch to light" : "Switch to dark",
                                    tint: tin.ink2, size: 40, iconSize: 20) { appearanceDark = !tin.dark }
            .offset(x: 10)
        if onBack == nil && progress == nil {
            HStack { Spacer(); theme }.frame(minHeight: 40).padding(.leading, 8).padding(.trailing, 20)
        } else {
            HStack(spacing: 12) {
                ZStack { if let onBack { ShellIconButton(glyph: .back, label: "Back", action: onBack) } }.frame(width: 44)
                Spacer(minLength: 0)
                if let progress {
                    HStack(spacing: 10) {
                        HStack(spacing: 4) {
                            ForEach(0..<progress.1, id: \.self) { i in
                                RoundedRectangle(cornerRadius: 3).fill(i < progress.0 ? p.accent : tin.ln).frame(width: 14, height: 6)
                                    .animation(reduced ? nil : TinMotion.easeOut(TinMotion.switchTime), value: progress.0)
                            }
                        }
                        // step 0 (Welcome, Choose): the count keeps its width, hidden, so "1 of 6" appears in place
                        Text("\(progress.0) of \(progress.1)").shellStyle(.sub).monospacedDigit().foregroundStyle(tin.ink2)
                            .frame(minWidth: 42).opacity(progress.0 == 0 ? 0 : 1)
                    }
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel("Setup progress, step \(progress.0) of \(progress.1)" + (label.isEmpty ? "" : ": \(label)"))
                }
                theme
            }
            .frame(minHeight: 56).padding(.leading, 8).padding(.trailing, 20)
        }
    }
}

/// gate/StepShell.svelte on Android: heading (26/32 bold), optional lead (15/22 muted), body (gap 16, padding 8 24 16) that
/// scrolls, footer (padding 12 24 24, gap 8, full-width buttons). `center` centres the body; with `anchor` it hangs from a fixed
/// point (min(14 % of the height, 96)) so an added line never re-centres the rest. `fill`: the body fills the height instead of
/// scrolling (Welcome's mark, Choose's options and Terms' consent line sit at the bottom).
struct ShellGateStep<Hero: View, Content: View, Foot: View, TitleSlot: View>: View {
    let title: String
    var lead: String? = nil
    var center = false
    var anchor = false
    var bigTitle = false
    var fill = false
    @ViewBuilder var hero: () -> Hero
    @ViewBuilder var titleSlot: () -> TitleSlot
    var hasTitleSlot = false
    @ViewBuilder var content: () -> Content
    @ViewBuilder var foot: () -> Foot
    @Environment(\.tin) private var tin

    var body: some View {
        GeometryReader { geo in
            let anchorTop = min(geo.size.height * 0.14, 96)
            VStack(spacing: 0) {
                let column = VStack(alignment: center ? .center : .leading, spacing: 16) {
                    hero()
                    if hasTitleSlot { titleSlot() } else {
                        Text(title).shellStyle(bigTitle ? .h1Center : .h1).foregroundStyle(tin.ink)
                            .multilineTextAlignment(center ? .center : .leading)
                            .frame(maxWidth: .infinity, alignment: center ? .center : .leading).accessibilityAddTraits(.isHeader)
                    }
                    if let lead {
                        Text(lead).shellStyle(.lead).foregroundStyle(tin.ink2).multilineTextAlignment(center ? .center : .leading)
                            .frame(maxWidth: .infinity, alignment: center ? .center : .leading)
                    }
                    content()
                }
                .padding(.horizontal, 24).padding(.top, center && anchor ? 8 + anchorTop : 8).padding(.bottom, 16)
                if fill {
                    column.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                } else if center && !anchor {
                    ScrollView { column.frame(minHeight: geo.size.height - 120) }.scrollBounceBehavior(.basedOnSize)
                } else {
                    ScrollView { column.frame(maxWidth: .infinity, alignment: .top) }.scrollBounceBehavior(.basedOnSize)
                }
                VStack(spacing: 8) { foot() }.padding(.horizontal, 24).padding(.top, 12).padding(.bottom, 24)
            }
        }
    }
}

extension ShellGateStep where TitleSlot == EmptyView {
    init(_ title: String, lead: String? = nil, center: Bool = false, anchor: Bool = false, bigTitle: Bool = false, fill: Bool = false,
         @ViewBuilder hero: @escaping () -> Hero, @ViewBuilder content: @escaping () -> Content, @ViewBuilder foot: @escaping () -> Foot) {
        self.title = title; self.lead = lead; self.center = center; self.anchor = anchor; self.bigTitle = bigTitle; self.fill = fill
        self.hero = hero; self.titleSlot = { EmptyView() }; self.content = content; self.foot = foot
    }
}

extension ShellGateStep where TitleSlot == EmptyView, Hero == EmptyView {
    init(_ title: String, lead: String? = nil, center: Bool = false, anchor: Bool = false, bigTitle: Bool = false, fill: Bool = false,
         @ViewBuilder content: @escaping () -> Content, @ViewBuilder foot: @escaping () -> Foot) {
        self.init(title, lead: lead, center: center, anchor: anchor, bigTitle: bigTitle, fill: fill, hero: { EmptyView() }, content: content, foot: foot)
    }
}

/// The stage (Gate.svelte .stage): the step slides 36 px in the inline direction and fades; enter 200 / exit 140, cubic-out.
/// Back slides the other way; with reduced motion it is a cut.
extension AnyTransition {
    static func shellGate(dir: Int, reduced: Bool) -> AnyTransition {
        if reduced { return .identity }
        let x = CGFloat(36 * dir)
        return .asymmetric(
            insertion: .offset(x: x).combined(with: .opacity).animation(.tinCubicOut(TinMotion.enter)),
            removal: .offset(x: -x).combined(with: .opacity).animation(.tinCubicOut(TinMotion.exit)))
    }
}

/// Full-screen surface in the page colour (android Page).
struct ShellPage<Content: View>: View {
    @ViewBuilder var content: () -> Content
    @Environment(\.tin) private var tin
    var body: some View {
        VStack(spacing: 0) { content() }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top).background(tin.bg.ignoresSafeArea())
    }
}

/// Components.kt TopBar: 64 tall, back (48) then the 20/26 title.
struct ShellTopBar<Actions: View>: View {
    let title: String
    var onBack: (() -> Void)?
    @ViewBuilder var actions: () -> Actions
    @Environment(\.tin) private var tin
    var body: some View {
        HStack(spacing: 0) {
            if let onBack { ShellIconButton(glyph: .back, label: "Back", size: 48, iconSize: 24, action: onBack) } else { Spacer().frame(width: 16) }
            Text(title).shellStyle(.topBar).foregroundStyle(tin.ink).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityAddTraits(.isHeader)
            actions()
        }
        .frame(minHeight: 64).padding(.leading, 4).padding(.trailing, 8)
    }
}

extension ShellTopBar where Actions == EmptyView {
    init(_ title: String, onBack: (() -> Void)?) { self.title = title; self.onBack = onBack; self.actions = { EmptyView() } }
}

/// 13/18 muted helper line (Components.kt Hint).
struct ShellHint: View {
    let text: String
    var color: Color? = nil
    var alignment: TextAlignment = .leading
    @Environment(\.tin) private var tin
    init(_ text: String, color: Color? = nil, alignment: TextAlignment = .leading) { self.text = text; self.color = color; self.alignment = alignment }
    var body: some View {
        Text(text).font(ShellFont.figtree(13)).lineSpacing(2).foregroundStyle(color ?? tin.ink2).multilineTextAlignment(alignment)
    }
}

/// Components.kt SectionLabel: 13 semibold muted, 18 above.
struct ShellSectionLabel: View {
    let text: String
    @Environment(\.tin) private var tin
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text).font(ShellFont.figtree(13, .semibold)).tracking(0.04).foregroundStyle(tin.ink2)
            .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 20).padding(.top, 18).padding(.bottom, 6)
    }
}

/// Components.kt ListItem: icon, title, optional sub-line, trailing slot (chevron by default).
struct ShellListItem<Trailing: View>: View {
    let title: String
    var sub: String? = nil
    var glyph: ShellGlyph? = nil
    var titleColor: Color? = nil
    var action: (() -> Void)? = nil
    @ViewBuilder var trailing: () -> Trailing
    @Environment(\.tin) private var tin
    var body: some View {
        let row = HStack(spacing: 16) {
            if let glyph { ShellIcon(glyph: glyph, size: 24, color: tin.ink2) }
            VStack(alignment: .leading, spacing: 0) {
                Text(title).font(TinType.bodyL).foregroundStyle(titleColor ?? tin.ink)
                if let sub { Text(sub).font(ShellFont.figtree(13)).foregroundStyle(tin.ink2) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            trailing()
        }
        .frame(minHeight: 60).padding(.leading, 20).padding(.trailing, 16).padding(.vertical, 6).contentShape(Rectangle())
        if let action { Button(action: action) { row }.buttonStyle(ShellPressStyle(radius: 0)) } else { row }
    }
}

extension ShellListItem where Trailing == ShellIcon {
    /// The default trailing chevron.
    init(_ title: String, sub: String? = nil, glyph: ShellGlyph? = nil, titleColor: Color? = nil, chevronColor: Color, action: (() -> Void)? = nil) {
        self.title = title; self.sub = sub; self.glyph = glyph; self.titleColor = titleColor; self.action = action
        self.trailing = { ShellIcon(glyph: .chevr, size: 24, color: chevronColor) }
    }
}

extension ShellListItem where Trailing == EmptyView {
    init(plain title: String, sub: String? = nil, glyph: ShellGlyph? = nil, titleColor: Color? = nil, action: (() -> Void)? = nil) {
        self.title = title; self.sub = sub; self.glyph = glyph; self.titleColor = titleColor; self.action = action
        self.trailing = { EmptyView() }
    }
}

/// Components.kt InfoCard: rounded info strip with an optional leading icon and a bold lead-in.
struct ShellInfoCard: View {
    let text: String
    var glyph: ShellGlyph? = nil
    var kind: ShellNKind = .info
    var bold: String? = nil
    @Environment(\.tin) private var tin
    var body: some View {
        let (bg, fg): (Color, Color) = { switch kind { case .warn: (tin.warn, tin.onWarn); case .error: (tin.erc, tin.onErc); case .info: (tin.sf2, tin.ink2) } }()
        HStack(alignment: .top, spacing: 10) {
            if let glyph { ShellIcon(glyph: glyph, size: 22, color: fg) }
            (bold.map { Text($0 + " ").bold() } ?? Text("")) + Text(text)
        }
        .font(TinType.bodyM).foregroundStyle(fg).frame(maxWidth: .infinity, alignment: .leading).padding(14)
        .background(SmoothRoundedRectangle(cornerRadius: 14).fill(bg))
    }
}

// ---------------------------------------------------------------- pages (profile/profile.css)

/// .pf-bar + .pf: 60 tall bar (surface, hairline below; back then the 17 px title) over a scrolling column (max 560, 40 below).
struct ShellPfPage<Content: View>: View {
    let title: String
    let onBack: () -> Void
    var scroll = true
    @ViewBuilder var content: () -> Content
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin)
        ShellPage {
            VStack(spacing: 0) {
                HStack(spacing: 4) {
                    ShellIconButton(glyph: .back, label: "Back", action: onBack)
                    Text(title).shellStyle(.barTitle).foregroundStyle(tin.ink).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                        .accessibilityAddTraits(.isHeader)
                }
                .frame(minHeight: 60).padding(.leading, 8).padding(.trailing, 12).padding(.vertical, 8)
                p.hairline.frame(height: 1)
            }
            .background(tin.sf)
            let column = VStack(alignment: .leading, spacing: 0) { content() }.frame(maxWidth: 560).frame(maxWidth: .infinity).padding(.bottom, 40)
            if scroll { ScrollView { column } } else { column.frame(maxHeight: .infinity, alignment: .top) }
        }
    }
}

/// .pf-sec: padding 0 16, 24 above (16 for the first); optional uppercase heading (12 px 650, .06em, muted).
struct ShellPfSec<Content: View>: View {
    var title: String? = nil
    var first = false
    var top: CGFloat? = nil
    @ViewBuilder var content: () -> Content
    @Environment(\.tin) private var tin
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let title {
                Text(title.uppercased()).shellStyle(.secHead).foregroundStyle(tin.ink2).padding(.horizontal, 4).padding(.bottom, 8).accessibilityAddTraits(.isHeader)
            }
            content()
        }
        .padding(.horizontal, 16).padding(.top, top ?? (first ? 16 : 24))
    }
}

/// .pf-card: r 14 surface with an inset hairline. Rows that need the hairline between them put `ShellPfHairline()` there.
struct ShellPfCard<Content: View>: View {
    @ViewBuilder var content: () -> Content
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin)
        VStack(spacing: 0) { content() }
            .background(RoundedRectangle(cornerRadius: 14).fill(tin.sf))
            .overlay(RoundedRectangle(cornerRadius: 14).stroke(p.hairline, lineWidth: 1))
            .clipShape(RoundedRectangle(cornerRadius: 14))
    }
}

struct ShellPfHairline: View {
    @Environment(\.tin) private var tin
    var body: some View { ShellPalette(tin).hairline.frame(height: 1) }
}

/// .pf-note: 10 above, 4 inline, 13/19 muted.
struct ShellPfNote: View {
    let text: String
    var top: CGFloat = 10
    var alignment: TextAlignment = .leading
    @Environment(\.tin) private var tin
    init(_ text: String, top: CGFloat = 10, alignment: TextAlignment = .leading) { self.text = text; self.top = top; self.alignment = alignment }
    var body: some View {
        Text(text).shellStyle(.sub19).foregroundStyle(tin.ink2).multilineTextAlignment(alignment)
            .frame(maxWidth: .infinity, alignment: alignment == .center ? .center : .leading).padding(.horizontal, 4).padding(.top, top)
    }
}

/// .pf-act: a 60 tall row, icon (muted 20) - title (600, one line) / sub (13/18 muted) - trailing. Clickable when `action` is set.
/// `subReserve` lists every text the sub can switch to; they are laid out invisibly underneath, so the line keeps the height of
/// the tallest variant at any width or text size (no layout shift).
struct ShellPfAct<Leading: View, Trailing: View, Extra: View>: View {
    let title: String
    var glyph: ShellGlyph? = nil
    var sub: String? = nil
    var danger = false
    var chevron = false
    var enabled = true
    var subColor: Color? = nil
    var subBold = false
    var minHeight: CGFloat = 60
    var titleStyle: ShellStyle = .bodyB
    var titleSize: CGFloat? = nil
    var gap: CGFloat = 14
    var padV: CGFloat = 8
    var padH: CGFloat = 16
    var subReserve: [String] = []
    var traits: AccessibilityTraits = .isButton
    var action: (() -> Void)? = nil
    @ViewBuilder var leading: () -> Leading
    @ViewBuilder var trailing: () -> Trailing
    @ViewBuilder var extra: () -> Extra
    @Environment(\.tin) private var tin

    var body: some View {
        let row = HStack(spacing: gap) {
            if Leading.self != EmptyView.self { leading() }
            else if let glyph { ShellIcon(glyph: glyph, size: 20, color: danger ? tin.er : tin.ink2) }
            VStack(alignment: .leading, spacing: 1) {
                Text(title).font(titleSize.map { ShellFont.figtree($0, .semibold) } ?? ShellFont.of(titleStyle))
                    .foregroundStyle(danger ? tin.er : tin.ink).lineLimit(1)
                if let sub, !sub.isEmpty {
                    ZStack(alignment: .topLeading) {
                        ForEach(subReserve.filter { $0 != sub }, id: \.self) { alt in
                            Text(alt).shellStyle(.sub, weight: subBold ? .semibold : .regular).hidden().accessibilityHidden(true)
                        }
                        Text(sub).shellStyle(.sub, weight: subBold ? .semibold : .regular).monospacedDigit().foregroundStyle(subColor ?? tin.ink2)
                    }
                }
                extra()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            trailing()
            if chevron { ShellIcon(glyph: .chevr, size: 18, color: tin.ink2).flipsForRightToLeftLayoutDirection(true) }
        }
        .frame(minHeight: minHeight).padding(.horizontal, padH).padding(.vertical, padV).contentShape(Rectangle())
        .opacity(enabled ? 1 : 0.55)
        if let action {
            Button(action: action) { row }.buttonStyle(ShellPressStyle(radius: 0)).disabled(!enabled).accessibilityAddTraits(traits)
        } else {
            row.accessibilityElement(children: .combine)
        }
    }
}

extension ShellPfAct where Leading == EmptyView, Trailing == EmptyView, Extra == EmptyView {
    init(_ title: String, glyph: ShellGlyph? = nil, sub: String? = nil, danger: Bool = false, chevron: Bool = false, subColor: Color? = nil,
         subBold: Bool = false, minHeight: CGFloat = 60, action: (() -> Void)? = nil) {
        self.init(title: title, glyph: glyph, sub: sub, danger: danger, chevron: chevron, subColor: subColor, subBold: subBold, minHeight: minHeight,
                  action: action, leading: { EmptyView() }, trailing: { EmptyView() }, extra: { EmptyView() })
    }
}

extension ShellPfAct where Leading == EmptyView, Extra == EmptyView {
    init(_ title: String, glyph: ShellGlyph? = nil, sub: String? = nil, subColor: Color? = nil, subBold: Bool = false, minHeight: CGFloat = 60,
         subReserve: [String] = [], traits: AccessibilityTraits = .isButton, action: (() -> Void)? = nil, @ViewBuilder trailing: @escaping () -> Trailing) {
        self.init(title: title, glyph: glyph, sub: sub, subColor: subColor, subBold: subBold, minHeight: minHeight, subReserve: subReserve,
                  traits: traits, action: action, leading: { EmptyView() }, trailing: trailing, extra: { EmptyView() })
    }
}

/// One choice among several (Settings radio rows): ring + dot, title, optional muted line (android PfRadioRow).
extension ShellPfAct where Leading == ShellRadio, Trailing == EmptyView, Extra == EmptyView {
    init(radio title: String, sub: String? = nil, on: Bool, minHeight: CGFloat = 56, action: @escaping () -> Void) {
        self.init(title: title, sub: sub, minHeight: minHeight, traits: on ? [.isButton, .isSelected] : .isButton, action: action,
                  leading: { ShellRadio(on: on) }, trailing: { EmptyView() }, extra: { EmptyView() })
    }
}

// ---------------------------------------------------------------- switch, chip, radio

/// lib/ui/Switch.svelte: 44 x 26 track (line / accent), 20 knob inset 3 with the lift shadow, 120 ms ease-out; 44 touch target.
struct ShellSwitch: View {
    let isOn: Bool
    let label: String
    var enabled = true
    let onChange: (Bool) -> Void
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    var body: some View {
        let p = ShellPalette(tin)
        Button { onChange(!isOn) } label: {
            ZStack(alignment: isOn ? .trailing : .leading) {
                Capsule().fill(isOn ? p.accent : tin.ln).frame(width: 44, height: 26)
                Circle().fill(tin.sf).frame(width: 20, height: 20).tinDepth(.lift, Circle()).padding(3)
            }
            .frame(width: 44, height: 26).opacity(enabled ? 1 : 0.55)
            .animation(reduced ? nil : TinMotion.easeOut(TinMotion.switchTime), value: isOn)
            .frame(width: 44, height: 44).contentShape(Rectangle())
        }
        .buttonStyle(.plain).disabled(!enabled)
        .accessibilityLabel(label).accessibilityValue(isOn ? "On" : "Off").accessibilityAddTraits(.isToggle)
    }
}

/// tokens.css .t-chip: 32 min (callers pass 36/44), r 16, line border, 13 px 600; pressed = accent-soft, accent-ink.
struct ShellChip: View {
    let text: String
    var pressed = false
    var minHeight: CGFloat = 32
    var padH: CGFloat = 14
    var glyph: ShellGlyph? = nil
    let action: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin), fg = pressed ? p.accentInk : tin.ink
        Button(action: action) {
            HStack(spacing: 6) {
                if let glyph { ShellIcon(glyph: glyph, size: 16, color: fg) }
                Text(text).shellStyle(.sub, weight: .semibold).foregroundStyle(fg)
            }
            .padding(.horizontal, padH).frame(minHeight: minHeight)
            .background(Capsule().fill(pressed ? p.accentSoft : .clear))
            .overlay(Capsule().stroke(pressed ? .clear : tin.ln, lineWidth: 1))
        }
        .buttonStyle(ShellPressStyle(wash: .clear, radius: minHeight / 2)).accessibilityValue(pressed ? "Selected" : "")
    }
}

/// Sounds.svelte .radio: 22 ring (2 px line / accent), 12 dot scaling in.
struct ShellRadio: View {
    let on: Bool
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    var body: some View {
        let p = ShellPalette(tin)
        ZStack {
            Circle().stroke(on ? p.accent : tin.ln, lineWidth: 2).frame(width: 20, height: 20)
            Circle().fill(p.accent).frame(width: 12, height: 12).scaleEffect(on ? 1 : 0.001)
        }
        .frame(width: 22, height: 22)
        .animation(reduced ? nil : TinMotion.easeOut(TinMotion.switchTime), value: on)
        .accessibilityHidden(true)
    }
}

/// `.t-badge`: 18 tall, min 18 wide, padding 0 5, radius 9, accent; muted = a hairline ring in muted.
struct ShellCountBadge: View {
    let n: Int
    var muted = false
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin)
        Text(n > 99 ? "99+" : "\(n)").font(ShellFont.figtree(11, muted ? .semibold : .bold, relativeTo: .caption2)).monospacedDigit()
            .foregroundStyle(muted ? tin.ink2 : p.onAccent)
            .padding(.horizontal, 5).frame(minWidth: 18, minHeight: 18).frame(height: 18)
            .background(Capsule().fill(muted ? .clear : p.accent))
            .overlay { if muted { Capsule().stroke(tin.ln, lineWidth: 1) } }
    }
}

/// Section headers in lists (`.sec`, `h3`): 12, 650, uppercase, .06em, muted.
struct ShellListSection: View {
    let text: String
    @Environment(\.tin) private var tin
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text.uppercased()).shellStyle(.secHead).foregroundStyle(tin.ink2).accessibilityAddTraits(.isHeader)
    }
}

// ---------------------------------------------------------------- dialog and sheet

/// TinDialog: radius 24, optional round icon, title, body, then Cancel / confirm, over a scrim. Shown with `.overlay`.
struct ShellDialog<Content: View>: View {
    let title: String
    var confirm: String
    var dismiss: String? = "Cancel"
    var destructive = false
    var glyph: ShellGlyph? = nil
    var confirmEnabled = true
    let onDismiss: () -> Void
    let onConfirm: () -> Void
    @ViewBuilder var content: () -> Content
    @Environment(\.tin) private var tin
    var body: some View {
        ZStack {
            Color.black.opacity(0.32).ignoresSafeArea().onTapGesture(perform: onDismiss).accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 14) {
                if let glyph {
                    ShellIcon(glyph: glyph, size: 24, color: destructive ? tin.er : tin.onPrc)
                        .frame(width: 44, height: 44).background(Circle().fill(destructive ? tin.erc : tin.prc))
                }
                Text(title).font(ShellFont.figtree(22, .bold, relativeTo: .title2)).foregroundStyle(tin.ink).accessibilityAddTraits(.isHeader)
                content()
                HStack(spacing: 8) {
                    Spacer(minLength: 0)
                    if let dismiss { ShellTinButton(text: dismiss, style: .text, fill: false, action: onDismiss) }
                    ShellTinButton(text: confirm, style: destructive ? .danger : .filled, enabled: confirmEnabled, fill: false, action: onConfirm)
                }
            }
            .padding(24).frame(maxWidth: 420)
            .background(SmoothRoundedRectangle(cornerRadius: 24).fill(tin.sfRaised))
            .tinDepth(.sheet, SmoothRoundedRectangle(cornerRadius: 24))
            .padding(.horizontal, 32).padding(.vertical, 72)
            .accessibilityAddTraits(.isModal)
        }
        .transition(.opacity)
    }
}

/// DialogText: 15/22 muted.
struct ShellDialogText: View {
    let text: String
    @Environment(\.tin) private var tin
    init(_ text: String) { self.text = text }
    var body: some View { Text(text).font(ShellFont.figtree(15)).lineSpacing(4).foregroundStyle(tin.ink2) }
}

/// Lightweight messages at the bottom (android Snackbar `say`): one at a time, replaced by the next.
@MainActor
final class ShellToast: ObservableObject {
    @Published var message: String?
    private var task: Task<Void, Never>?
    func say(_ m: String) {
        message = m
        task?.cancel()
        task = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 4_000_000_000)
            if !Task.isCancelled { self?.message = nil }
        }
    }
}

struct ShellToastView: View {
    @ObservedObject var toast: ShellToast
    @Environment(\.tin) private var tin
    var body: some View {
        VStack {
            Spacer()
            if let m = toast.message {
                Text(m).font(TinType.bodyM).foregroundStyle(tin.bg).padding(.horizontal, 16).padding(.vertical, 14)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 8).fill(tin.ink))
                    .padding(16).transition(.opacity).accessibilityAddTraits(.updatesFrequently)
                    .onTapGesture { toast.message = nil }
            }
        }
        .animation(TinMotion.easeOut(TinMotion.crossfade), value: toast.message)
        .allowsHitTesting(toast.message != nil)
    }
}
