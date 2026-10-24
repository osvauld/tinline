import SwiftUI
import UIKit

/// Generic building blocks (android Components.kt), prefixed `Tin`. Same sizes, radii, colours and copy as Kotlin.

private func figtree(_ size: CGFloat, _ weight: Font.Weight, _ style: Font.TextStyle) -> Font {
    .custom("Figtree", size: size, relativeTo: style).weight(weight)
}

// MARK: layout

/// Full-screen surface in the page colour, clear of the system bars and the keyboard (SwiftUI's safe area does both).
struct TinPage<Content: View>: View {
    @Environment(\.tin) private var tin
    @ViewBuilder var content: () -> Content

    var body: some View {
        VStack(spacing: 0, content: content)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            .background(tin.bg.ignoresSafeArea())
    }
}

struct TinTopBar<Actions: View>: View {
    @Environment(\.tin) private var tin
    var title: String?
    var onBack: (() -> Void)?
    @ViewBuilder var actions: () -> Actions

    init(title: String? = nil, onBack: (() -> Void)? = nil, @ViewBuilder actions: @escaping () -> Actions) {
        self.title = title; self.onBack = onBack; self.actions = actions
    }

    var body: some View {
        HStack(spacing: 0) {
            if let onBack { TinIconButton(TinIcons.back, "Back", onBack) } else { Spacer().frame(width: 16) }
            Text(title ?? "").font(figtree(20, .semibold, .title3)).foregroundStyle(tin.ink).lineLimit(1).truncationMode(.tail)
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityAddTraits(.isHeader)
            actions()
        }
        .frame(minHeight: 64)
        .padding(.leading, 4).padding(.trailing, 8)
    }
}

extension TinTopBar where Actions == EmptyView {
    init(title: String? = nil, onBack: (() -> Void)? = nil) { self.init(title: title, onBack: onBack) { EmptyView() } }
}

/// 48 pt round tap target around a 24 pt icon.
struct TinIconButton: View {
    @Environment(\.tin) private var tin
    var icon: TinIconData
    var desc: String
    var action: () -> Void
    var tint: Color?
    var enabled: Bool

    init(_ icon: TinIconData, _ desc: String, _ action: @escaping () -> Void, tint: Color? = nil, enabled: Bool = true) {
        self.icon = icon; self.desc = desc; self.action = action; self.tint = tint; self.enabled = enabled
    }

    var body: some View {
        Button(action: action) {
            TinIcon(icon, size: 24, tint: tint ?? tin.ink).frame(width: 48, height: 48).contentShape(Circle())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .accessibilityLabel(desc)
    }
}

// MARK: buttons

enum TinButtonKind { case filled, tonal, outlined, text, danger, dangerFilled, outlinedDanger }

/// app.css .press: scale .97 while pressed (100 ms in, 140 ms back, ease-out); none under reduced motion.
struct TinPressStyle: ButtonStyle {
    @Environment(\.accessibilityReduceMotion) private var reduced
    func makeBody(configuration: Configuration) -> some View {
        let on = configuration.isPressed && !reduced
        configuration.label
            .scaleEffect(on ? 0.97 : 1)
            .animation(TinMotion.easeOut(on ? TinMotion.press : TinMotion.release), value: on)
    }
}

/// Pill button, 52 pt tall by default.
struct TinButton: View {
    @Environment(\.tin) private var tin
    var text: String
    var kind: TinButtonKind = .filled
    var icon: TinIconData?
    var enabled = true
    var height: CGFloat = 52
    var fill = true
    var font: Font = TinType.button
    var action: () -> Void

    init(_ text: String, kind: TinButtonKind = .filled, icon: TinIconData? = nil, enabled: Bool = true, height: CGFloat = 52,
         fill: Bool = true, font: Font = TinType.button, action: @escaping () -> Void) {
        self.text = text; self.kind = kind; self.icon = icon; self.enabled = enabled; self.height = height; self.fill = fill
        self.font = font; self.action = action
    }

    private var colours: (bg: Color, fg: Color) {
        let c = tin
        switch kind {
        case .filled: return enabled ? (c.pr, c.onPr) : (c.sf3, c.ink2)
        case .tonal: return enabled ? (c.prc, c.onPrc) : (c.sf3, c.ink2)
        case .outlined, .text: return (.clear, enabled ? c.pr : c.ink2)
        case .danger: return (c.erc, c.er)
        case .dangerFilled: return (c.er, c.onEr)
        case .outlinedDanger: return (.clear, c.er)
        }
    }

    var body: some View {
        let (bg, fg) = colours
        Button(action: action) {
            HStack(spacing: 8) {
                if let icon { TinIcon(icon, size: 20, tint: fg) }
                Text(text).font(font).foregroundStyle(fg).multilineTextAlignment(.center)
            }
            .padding(.horizontal, kind == .text && !fill ? 12 : 24)
            .frame(maxWidth: fill ? .infinity : nil, minHeight: height)
            .background(Capsule().fill(bg))
            .overlay { if kind == .outlined || kind == .outlinedDanger { Capsule().strokeBorder(tin.ln2, lineWidth: 1) } }
            .contentShape(Capsule())
        }
        .buttonStyle(TinPressStyle())
        .disabled(!enabled)
    }
}

/// Round call-style button: accept / end (72 pt) and the 64 pt mute / route controls with a label.
struct TinRoundButton: View {
    @Environment(\.tin) private var tin
    var icon: TinIconData
    var desc: String
    var bg: Color, fg: Color
    var size: CGFloat = 72
    var iconSize: CGFloat = 30
    var rotate: Double = 0
    var action: () -> Void

    var body: some View {
        Button(action: action) {
            TinIcon(icon, size: iconSize, tint: fg).rotationEffect(.degrees(rotate))
                .frame(width: size, height: size)
                .background(Circle().fill(bg).tinDepth(.lift, Circle()))
                .contentShape(Circle())
        }
        .buttonStyle(TinPressStyle())
        .accessibilityLabel(desc)
    }
}

struct TinLabeledRound<Content: View>: View {
    @Environment(\.tin) private var tin
    var label: String
    @ViewBuilder var content: () -> Content

    var body: some View {
        VStack(spacing: 8) {
            content()
            Text(label).font(figtree(13, .medium, .caption)).foregroundStyle(tin.ink).multilineTextAlignment(.center)
        }
        .frame(width: 96)
    }
}

// MARK: fields

enum TinFieldState { case normal, error }

/// 56 pt text field, radius 8, label above. Border: 1 pt outline, 2 pt primary when focused, 2 pt error.
struct TinField<Trailing: View>: View {
    @Environment(\.tin) private var tin
    @Binding var text: String
    var label: String
    var mono = false
    var password = false
    var state: TinFieldState = .normal
    var enabled = true
    var singleLine = true
    var minHeight: CGFloat = 56
    var hint: String?
    var placeholder: String?
    var keyboard: UIKeyboardType = .default
    var submitLabel: SubmitLabel = .done
    var onSubmit: (() -> Void)?
    @ViewBuilder var trailing: () -> Trailing

    @FocusState private var focused: Bool
    @State private var shown = false

    init(_ label: String, text: Binding<String>, mono: Bool = false, password: Bool = false, state: TinFieldState = .normal,
         enabled: Bool = true, singleLine: Bool = true, minHeight: CGFloat = 56, hint: String? = nil, placeholder: String? = nil,
         keyboard: UIKeyboardType = .default, submitLabel: SubmitLabel = .done, onSubmit: (() -> Void)? = nil,
         @ViewBuilder trailing: @escaping () -> Trailing) {
        self.label = label; _text = text; self.mono = mono; self.password = password; self.state = state; self.enabled = enabled
        self.singleLine = singleLine; self.minHeight = minHeight; self.hint = hint; self.placeholder = placeholder
        self.keyboard = keyboard; self.submitLabel = submitLabel; self.onSubmit = onSubmit; self.trailing = trailing
    }

    private var font: Font { mono ? .custom("IBMPlexMono-Medium", size: 16, relativeTo: .body) : TinType.bodyL }

    var body: some View {
        let err = state == .error
        let border = err ? tin.er : focused ? tin.pr : tin.ln2
        let shape = SmoothRoundedRectangle(cornerRadius: 8)
        VStack(alignment: .leading, spacing: 6) {
            Text(label).font(figtree(13, .semibold, .caption)).foregroundStyle(err ? tin.er : tin.ink2)
            HStack(alignment: singleLine ? .center : .top, spacing: 0) {
                entry
                    .font(font).foregroundStyle(tin.ink).tint(tin.pr)
                    .keyboardType(password ? .asciiCapable : keyboard)
                    .autocorrectionDisabled(password)
                    .textInputAutocapitalization(password ? .never : nil)
                    .submitLabel(submitLabel)
                    .onSubmit { onSubmit?() }
                    .focused($focused)
                    .disabled(!enabled)
                    .padding(.horizontal, 16).padding(.vertical, singleLine ? 0 : 12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if password {
                    TinIconButton(shown ? TinIcons.eyeoff : TinIcons.eye, shown ? "Hide passphrase" : "Show passphrase", { shown.toggle() }, tint: tin.ink2)
                } else {
                    trailing()
                }
            }
            .frame(minHeight: minHeight)
            .background(tin.sf, in: shape)
            .overlay(shape.strokeBorder(border, lineWidth: err || focused ? 2 : 1))
            .clipShape(shape)
            if let hint { Text(hint).font(figtree(13, .regular, .footnote)).foregroundStyle(err ? tin.er : tin.ink2) }
        }
    }

    @ViewBuilder private var entry: some View {
        let prompt = placeholder.map { Text($0).foregroundStyle(tin.ink2.opacity(0.7)) }
        if password && !shown {
            SecureField("", text: $text, prompt: prompt)
        } else if singleLine {
            TextField("", text: $text, prompt: prompt)
        } else {
            TextField("", text: $text, prompt: prompt, axis: .vertical)
        }
    }
}

extension TinField where Trailing == EmptyView {
    init(_ label: String, text: Binding<String>, mono: Bool = false, password: Bool = false, state: TinFieldState = .normal,
         enabled: Bool = true, singleLine: Bool = true, minHeight: CGFloat = 56, hint: String? = nil, placeholder: String? = nil,
         keyboard: UIKeyboardType = .default, submitLabel: SubmitLabel = .done, onSubmit: (() -> Void)? = nil) {
        self.init(label, text: text, mono: mono, password: password, state: state, enabled: enabled, singleLine: singleLine,
                  minHeight: minHeight, hint: hint, placeholder: placeholder, keyboard: keyboard, submitLabel: submitLabel,
                  onSubmit: onSubmit) { EmptyView() }
    }
}

// MARK: avatars

/// Me - string - them, used on Adding / Added / Calling screens. `meDID` is my own DID (nil while it is not known).
struct TinPairAvatars: View {
    @Environment(\.tin) private var tin
    var me: String
    var meDID: String
    var other: String
    var otherDID: String?
    var mine: CGFloat = 64
    var theirs: CGFloat = 96
    var unknown = false
    var otherAlpha: Double = 1

    var body: some View {
        HStack(spacing: 0) {
            TinAvatar(did: meDID, size: mine, name: me)
            RoundedRectangle(cornerRadius: 2).fill(tin.thread).frame(width: 36, height: 3)
            if unknown || otherDID == nil {
                Text("?").font(TinType.titleL).foregroundStyle(tin.ink2).frame(width: theirs, height: theirs).background(Circle().fill(tin.sf3))
            } else if let otherDID {
                TinAvatar(did: otherDID, size: theirs, name: other, alpha: otherAlpha)
            }
        }
    }
}

// MARK: banners, pills, rows

enum TinBannerKind { case warn, error, neutral }

struct TinBanner: View {
    @Environment(\.tin) private var tin
    var kind: TinBannerKind
    var icon: TinIconData
    var title: String
    var message: String
    var action: String?
    var onAction: () -> Void = {}

    var body: some View {
        let (bg, fg): (Color, Color) = switch kind {
        case .warn: (tin.warn, tin.onWarn)
        case .error: (tin.erc, tin.onErc)
        case .neutral: (tin.sf2, tin.ink)
        }
        HStack(alignment: .top, spacing: 12) {
            TinIcon(icon, size: 24, tint: fg)
            VStack(alignment: .leading, spacing: 0) {
                Text(title).font(figtree(15, .bold, .callout)).foregroundStyle(fg)
                Text(message).font(TinType.bodyM).foregroundStyle(fg)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if let action {
                Button(action: onAction) {
                    Text(action).font(TinType.label.weight(.bold)).underline().foregroundStyle(kind == .neutral ? tin.pr : fg)
                        .padding(.horizontal, 10).frame(minHeight: 40).contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .frame(maxHeight: .infinity, alignment: .center)
            }
        }
        .padding(.leading, 14).padding(.vertical, 14).padding(.trailing, 8)
        .background(bg, in: SmoothRoundedRectangle(cornerRadius: 14))
        .padding(.horizontal, 16)
    }
}

enum TinBadgeKind { case direct, relayed, warn, neutral, mono }

struct TinBadge<Leading: View>: View {
    @Environment(\.tin) private var tin
    var text: String
    var kind: TinBadgeKind
    var icon: TinIconData?
    @ViewBuilder var leading: () -> Leading

    init(_ text: String, kind: TinBadgeKind, icon: TinIconData? = nil, @ViewBuilder leading: @escaping () -> Leading) {
        self.text = text; self.kind = kind; self.icon = icon; self.leading = leading
    }

    var body: some View {
        let (bg, fg): (Color, Color) = switch kind {
        case .direct: (tin.prc, tin.onPrc)
        case .relayed: (tin.relayC, tin.onRelayC)
        case .warn: (tin.warn, tin.onWarn)
        case .neutral, .mono: (tin.sf2, tin.ink)
        }
        HStack(spacing: 6) {
            if let icon { TinIcon(icon, size: 20, tint: fg) }
            leading()
            Text(text).font(kind == .mono ? .custom("IBMPlexMono-Medium", size: 14, relativeTo: .callout) : figtree(13, .semibold, .callout)).foregroundStyle(fg)
        }
        .padding(.leading, icon != nil ? 8 : 12).padding(.trailing, 12)
        .frame(minHeight: 30)
        .background(Capsule().fill(bg))
    }
}

extension TinBadge where Leading == EmptyView {
    init(_ text: String, kind: TinBadgeKind, icon: TinIconData? = nil) { self.init(text, kind: kind, icon: icon) { EmptyView() } }
}

struct TinSectionLabel<Trailing: View>: View {
    @Environment(\.tin) private var tin
    var text: String
    @ViewBuilder var trailing: () -> Trailing

    init(_ text: String, @ViewBuilder trailing: @escaping () -> Trailing) { self.text = text; self.trailing = trailing }

    var body: some View {
        HStack {
            Text(text).font(figtree(13, .semibold, .caption)).tracking(0.04).foregroundStyle(tin.ink2)
                .frame(maxWidth: .infinity, alignment: .leading).accessibilityAddTraits(.isHeader)
            trailing()
        }
        .padding(.leading, 20).padding(.trailing, 20).padding(.top, 18).padding(.bottom, 6)
    }
}

extension TinSectionLabel where Trailing == EmptyView {
    init(_ text: String) { self.init(text) { EmptyView() } }
}

/// Settings-style row: icon, title, optional sub-line, trailing slot (chevron by default).
struct TinListItem<Trailing: View>: View {
    @Environment(\.tin) private var tin
    var title: String
    var sub: String?
    var icon: TinIconData?
    var titleColor: Color?
    var onTap: (() -> Void)?
    @ViewBuilder var trailing: () -> Trailing

    init(_ title: String, sub: String? = nil, icon: TinIconData? = nil, titleColor: Color? = nil, onTap: (() -> Void)? = nil,
         @ViewBuilder trailing: @escaping () -> Trailing) {
        self.title = title; self.sub = sub; self.icon = icon; self.titleColor = titleColor; self.onTap = onTap; self.trailing = trailing
    }

    var body: some View {
        let row = HStack(spacing: 16) {
            if let icon { TinIcon(icon, size: 24, tint: tin.ink2) }
            VStack(alignment: .leading, spacing: 0) {
                Text(title).font(TinType.bodyL).foregroundStyle(titleColor ?? tin.ink)
                if let sub { Text(sub).font(figtree(13, .regular, .footnote)).foregroundStyle(tin.ink2) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            trailing()
        }
        .padding(.leading, 20).padding(.trailing, 16).padding(.vertical, 6)
        .frame(maxWidth: .infinity, minHeight: 60)
        .contentShape(Rectangle())
        if let onTap { Button(action: onTap) { row }.buttonStyle(.plain) } else { row }
    }
}

extension TinListItem where Trailing == TinChevron {
    /// The default trailing slot is the chevron.
    init(_ title: String, sub: String? = nil, icon: TinIconData? = nil, titleColor: Color? = nil, onTap: (() -> Void)? = nil) {
        self.init(title, sub: sub, icon: icon, titleColor: titleColor, onTap: onTap) { TinChevron() }
    }
}

struct TinChevron: View {
    @Environment(\.tin) private var tin
    var body: some View { TinIcon(TinIcons.chevr, size: 24, tint: tin.ink2).tinMirrorRTL() }
}

/// Rounded info strip: hint text with an optional leading icon.
struct TinInfoCard: View {
    @Environment(\.tin) private var tin
    var text: String
    var icon: TinIconData?
    var kind: TinBannerKind = .neutral
    var bold: String?

    var body: some View {
        let (bg, fg): (Color, Color) = switch kind {
        case .warn: (tin.warn, tin.onWarn)
        case .error: (tin.erc, tin.onErc)
        case .neutral: (tin.sf2, tin.ink2)
        }
        HStack(alignment: .top, spacing: 10) {
            if let icon { TinIcon(icon, size: 22, tint: fg) }
            (bold.map { Text($0).bold() + Text(" ") } ?? Text("")) + Text(text)
        }
        .font(TinType.bodyM).foregroundStyle(fg)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(bg, in: SmoothRoundedRectangle(cornerRadius: 14))
    }
}

struct TinHint: View {
    @Environment(\.tin) private var tin
    var text: String
    var color: Color?
    var align: TextAlignment = .leading

    var body: some View {
        Text(text).font(figtree(13, .regular, .footnote)).foregroundStyle(color ?? tin.ink2).multilineTextAlignment(align)
    }
}

// MARK: dialogs

/// Dialog: radius 24, optional round icon, title, body, then Cancel / confirm (android `TinDialog`). Shown over the whole app on a
/// scrim; a tap on the scrim dismisses. `confirm` stays disabled while `confirmEnabled` is false.
extension View {
    func tinDialog<Inner: View>(
        isPresented: Binding<Bool>, title: String, confirm: String, onConfirm: @escaping () -> Void, dismiss: String? = "Cancel",
        destructive: Bool = false, icon: TinIconData? = nil, confirmEnabled: Bool = true, @ViewBuilder body: @escaping () -> Inner
    ) -> some View {
        fullScreenCover(isPresented: isPresented) {
            TinDialogCard(title: title, confirm: confirm, onConfirm: onConfirm, dismissLabel: dismiss, destructive: destructive, icon: icon,
                          confirmEnabled: confirmEnabled, onDismiss: { isPresented.wrappedValue = false }, content: body)
                .presentationBackground(tinScrim)
        }
    }
}

private struct TinDialogCard<Inner: View>: View {
    @Environment(\.tin) private var tin
    let title: String, confirm: String
    let onConfirm: () -> Void
    let dismissLabel: String?
    let destructive: Bool
    let icon: TinIconData?
    let confirmEnabled: Bool
    let onDismiss: () -> Void
    @ViewBuilder let content: () -> Inner

    var body: some View {
        let shape = SmoothRoundedRectangle(cornerRadius: 24)
        ZStack {
            Color.clear.contentShape(Rectangle()).onTapGesture(perform: onDismiss).accessibilityHidden(true)
            GeometryReader { geo in
                VStack(alignment: .leading, spacing: 14) {
                    if let icon {
                        TinIcon(icon, size: 24, tint: destructive ? tin.er : tin.onPrc).frame(width: 44, height: 44)
                            .background(Circle().fill(destructive ? tin.erc : tin.prc))
                    }
                    Text(title).font(figtree(22, .bold, .title2)).foregroundStyle(tin.ink).accessibilityAddTraits(.isHeader)
                    content()
                    HStack(spacing: 8) {
                        Spacer(minLength: 0)
                        if let dismissLabel { TinButton(dismissLabel, kind: .text, fill: false, action: onDismiss) }
                        TinButton(confirm, kind: destructive ? .dangerFilled : .filled, enabled: confirmEnabled, fill: false, action: onConfirm)
                    }
                }
                .padding(24)
                .frame(width: geo.size.width * 0.78)
                .background(shape.fill(tin.sfRaised).tinDepth(.sheet, shape))
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .accessibilityAddTraits(.isModal)
    }
}

struct TinDialogText: View {
    @Environment(\.tin) private var tin
    var text: String
    var body: some View { Text(text).font(figtree(15, .regular, .callout)).foregroundStyle(tin.ink2).lineSpacing(3) }
}

/// Rounded content card: white surface with a hairline.
struct TinCardBox<Content: View>: View {
    @Environment(\.tin) private var tin
    var radius: CGFloat = 14
    @ViewBuilder var content: () -> Content

    var body: some View {
        let shape = SmoothRoundedRectangle(cornerRadius: radius)
        VStack(alignment: .leading, spacing: 0, content: content)
            .background(tin.sf, in: shape)
            .overlay(shape.strokeBorder(tin.ln, lineWidth: 1))
            .clipShape(shape)
    }
}

#Preview("Kit") {
    struct Demo: View {
        @State private var text = ""
        @State private var dialog = false
        var body: some View {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    TinTopBar(title: "Settings", onBack: {}) { TinIconButton(TinIcons.search, "Search", {}) }
                    TinMark(size: 48, state: .taut, live: true)
                    HStack { TinAvatar(did: "did:key:z6MkSynthetic", size: 44, name: "Maya"); ReceiptTickView(tick: .delivered, size: 12) }
                    TinField("Name", text: $text, hint: "Shown beside your avatar")
                    TinButton("Continue") {}
                    TinButton("Not now", kind: .text, fill: false) { dialog = true }
                    TinBanner(kind: .warn, icon: TinIcons.warning, title: "Heads up", message: "You are offline.", action: "Retry")
                    TinBadge("Direct", kind: .direct, icon: TinIcons.direct)
                    TinSectionLabel("Account")
                    TinListItem("Devices", sub: "2 linked", icon: TinIcons.devices) {}
                    TinInfoCard(text: "Only you can read this.", icon: TinIcons.lock, bold: "Private.")
                }
            }
            .tinTheme()
            .tinDialog(isPresented: $dialog, title: "Leave?", confirm: "Leave", onConfirm: { dialog = false }) { TinDialogText(text: "You can come back any time.") }
        }
    }
    return Demo()
}
