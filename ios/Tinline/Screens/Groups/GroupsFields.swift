import SwiftUI

/*
 * Form pieces the group flows use (android: GroupsLive.kt GField / CheckBox, and Components.kt TinField, TinButton, TopBar, Hint,
 * InfoCard, CardBox, which the other areas also use). Kept here under the Groups prefix so this folder stands alone.
 */

/// `.fld` (sheets: 44 pt, radius 14) and TinField (flows: 56 pt, radius 8, 2 pt border on focus or error), a 13 pt label above.
struct GroupsField: View {
    enum Look { case sheet, form }
    @Environment(\.tin) private var tin
    @FocusState private var focused: Bool
    let label: String
    @Binding var text: String
    var placeholder = ""
    var error: String?
    var look: Look = .sheet
    var mono = false
    var multiline = false
    var minHeight: CGFloat?
    var hint: String?
    var limit: Int?

    var body: some View {
        let radius: CGFloat = look == .sheet ? 14 : 8
        let shape = SmoothRoundedRectangle(cornerRadius: radius)
        let err = error != nil
        let border: Color = err ? tin.er : (focused ? (look == .sheet ? tin.grpAccent : tin.pr) : (look == .sheet ? tin.ln : tin.ln2))
        let bw: CGFloat = look == .form && (err || focused) ? 2 : 1
        let font: Font = mono ? TinType.mono : (look == .sheet ? GroupsType.body : TinType.bodyL)
        VStack(alignment: .leading, spacing: 6) {
            Text(label).font(look == .sheet ? GroupsType.subStrong : TinType.caption.weight(.semibold)).foregroundStyle(err && look == .form ? tin.er : tin.ink2)
            ZStack(alignment: multiline ? .topLeading : .leading) {
                if text.isEmpty && !placeholder.isEmpty { Text(placeholder).font(font).foregroundStyle(tin.ink2.opacity(0.7)).allowsHitTesting(false) }
                Group {
                    if multiline { TextField("", text: $text, axis: .vertical).lineLimit(2...6) } else { TextField("", text: $text) }
                }
                .font(font).foregroundStyle(tin.ink).tint(look == .sheet ? tin.grpAccentInk : tin.pr)
                .textInputAutocapitalization(mono ? .never : .sentences).autocorrectionDisabled(mono)
                .focused($focused).accessibilityLabel(label)
            }
            .padding(.horizontal, look == .sheet ? 14 : 16).padding(.vertical, multiline ? 12 : 10)
            .frame(minHeight: minHeight ?? (look == .sheet ? 44 : 56), alignment: multiline ? .topLeading : .leading)
            .background(tin.sf, in: shape)
            .overlay { shape.stroke(border, lineWidth: bw) }
            .background { if focused && look == .sheet { shape.stroke(tin.grpAccentInk.opacity(0.14), lineWidth: 6) } }
            if let m = error ?? hint { Text(m).font(GroupsType.sub).foregroundStyle(err ? tin.er : tin.ink2) }
        }
        .onChange(of: text) { _, v in if let limit, v.count > limit { text = String(v.prefix(limit)) } }
    }
}

/// `.box`: 22 pt, radius 7, a 2 pt line ring; checked fills accent with a 16 pt check (120 ms).
struct GroupsCheckBox: View {
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    let on: Bool
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 7).fill(on ? tin.grpAccent : .clear)
            if !on { RoundedRectangle(cornerRadius: 7).strokeBorder(tin.ln, lineWidth: 2) }
            if on { GroupsIconView(icon: GroupsIcons.check, size: 16, color: tin.grpOnAccent) }
        }
        .frame(width: 22, height: 22)
        .animation(reduced ? nil : GroupsMotion.easeOut(GroupsMotion.switchT), value: on)
    }
}

/// The 60 pt surface bar with a hairline under it (info pages) and the 64 pt flow bar (`TopBar`): back, then the title.
struct GroupsBar: View {
    @Environment(\.tin) private var tin
    let title: String
    var onBack: (() -> Void)?
    var surface = true
    var body: some View {
        HStack(spacing: 4) {
            if let onBack {
                Button(action: onBack) { GroupsIconView(icon: GroupsIcons.back, size: 22, color: tin.ink).frame(width: 44, height: 44).contentShape(Circle()) }
                    .buttonStyle(GroupsPressStyle()).accessibilityLabel(tr("group_back"))
            } else { Spacer().frame(width: 12) }
            Text(title).font(surface ? GroupsType.title : TinType.titleM).foregroundStyle(tin.ink).lineLimit(1).accessibilityAddTraits(.isHeader)
            Spacer(minLength: 0)
        }
        .padding(.leading, 8).padding(.trailing, 12).padding(.vertical, 8).frame(minHeight: surface ? 60 : 64)
        .background(surface ? tin.sf : .clear)
    }
}

/// A flow screen: page colour, own bar (the system bar is hidden), content below.
struct GroupsPage<Content: View>: View {
    @Environment(\.tin) private var tin
    @Environment(\.dismiss) private var dismiss
    let title: String
    var hairline = false
    var onBack: (() -> Void)?
    @ViewBuilder var content: () -> Content
    var body: some View {
        VStack(spacing: 0) {
            GroupsBar(title: title, onBack: onBack ?? { dismiss() }, surface: hairline)
            if hairline { GroupsRule() }
            content()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .background(tin.bg.ignoresSafeArea())
        .toolbar(.hidden, for: .navigationBar)
    }
}

enum GroupsFlowStyle { case filled, tonal, outlined, text }

/// TinButton: a pill, 52 pt by default.
struct GroupsFlowButton: View {
    @Environment(\.tin) private var tin
    let text: String
    var style: GroupsFlowStyle = .filled
    var icon: GroupsIcon?
    var enabled = true
    var height: CGFloat = 52
    var fill = true
    let action: () -> Void
    var body: some View {
        let (bg, fg): (Color, Color) = {
            switch style {
            case .filled: return enabled ? (tin.pr, tin.onPr) : (tin.sf3, tin.ink2)
            case .tonal: return enabled ? (tin.prc, tin.onPrc) : (tin.sf3, tin.ink2)
            case .outlined, .text: return (.clear, enabled ? tin.pr : tin.ink2)
            }
        }()
        Button(action: action) {
            HStack(spacing: 8) {
                if let icon { GroupsIconView(icon: icon, size: 20, color: fg) }
                Text(text).font(TinType.button).foregroundStyle(fg).multilineTextAlignment(.center)
            }
            .padding(.horizontal, style == .text && !fill ? 12 : 24)
            .frame(maxWidth: fill ? .infinity : nil, minHeight: height)
            .background(bg, in: Capsule())
            .overlay { if style == .outlined { Capsule().strokeBorder(tin.ln2, lineWidth: 1) } }
            .contentShape(Capsule())
        }
        .buttonStyle(GroupsPressStyle()).disabled(!enabled)
    }
}

/// Muted 13/18 line (Components Hint).
struct GroupsHint: View {
    @Environment(\.tin) private var tin
    let text: String
    var color: Color?
    var align: TextAlignment = .leading
    init(_ text: String, color: Color? = nil, align: TextAlignment = .leading) { self.text = text; self.color = color; self.align = align }
    var body: some View {
        Text(text).font(TinType.bodyM.weight(.regular)).foregroundStyle(color ?? tin.ink2).multilineTextAlignment(align)
            .frame(maxWidth: .infinity, alignment: align == .center ? .center : .leading).fixedSize(horizontal: false, vertical: true)
    }
}

enum GroupsBannerKind { case neutral, warn, error }

/// Rounded info strip: text with an optional leading icon (Components InfoCard).
struct GroupsInfoCard: View {
    @Environment(\.tin) private var tin
    let text: String
    var icon: GroupsIcon?
    var kind: GroupsBannerKind = .neutral
    var body: some View {
        let (bg, fg): (Color, Color) = {
            switch kind {
            case .warn: return (tin.warn, tin.onWarn)
            case .error: return (tin.erc, tin.onErc)
            case .neutral: return (tin.sf2, tin.ink2)
            }
        }()
        HStack(alignment: .top, spacing: 10) {
            if let icon { GroupsIconView(icon: icon, size: 22, color: fg) }
            Text(text).font(TinType.bodyM).foregroundStyle(fg).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
        .padding(14).background(bg, in: SmoothRoundedRectangle(cornerRadius: 14))
    }
}

/// Rounded content card: white surface with a hairline (Components CardBox).
struct GroupsCardBox<Content: View>: View {
    @Environment(\.tin) private var tin
    var radius: CGFloat = 14
    @ViewBuilder var content: () -> Content
    var body: some View {
        VStack(alignment: .leading, spacing: 0, content: content)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(tin.sf, in: SmoothRoundedRectangle(cornerRadius: radius))
            .overlay { SmoothRoundedRectangle(cornerRadius: radius).stroke(tin.ln, lineWidth: 1) }
    }
}

/// Section label over a list (Components SectionLabel).
struct GroupsSectionLabel: View {
    @Environment(\.tin) private var tin
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text).font(TinType.caption).tracking(0.8).foregroundStyle(tin.ink2).frame(maxWidth: .infinity, alignment: .leading).accessibilityAddTraits(.isHeader)
    }
}
