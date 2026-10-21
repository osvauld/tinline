import SwiftUI

/*
 * Profile furniture ported from playground src/screens/profile (android ProfileUi.kt): FieldRow.svelte (one piece of information:
 * view / edit / remove), Value.svelte (any stored value, readably), and the shared input (FieldRow / AddInfo `input, textarea`).
 * Page furniture (pf-bar, pf-sec, pf-card, pf-act, pf-note, pf-strip, pf-err) is PeopleKit's; buttons are PeopleButton (groups/Btn).
 */

/// "Saved" stays on a row this long (FieldRow / Availability SAVED_MS: a UI constant, not a motion timing).
let savedMs: UInt64 = 1800

/// The "check Saved" tag: check 13 + 600 accent-ink, fading in over --dur-enter (ease-out).
struct PeopleSavedTag: View {
    @Environment(\.tin) private var c
    @Environment(\.accessibilityReduceMotion) private var reduced
    let offline: Bool
    @State private var shown = false
    var body: some View {
        HStack(spacing: 3) {
            PeopleIconView(icon: .check, size: 13, color: c.peopleAccentInk)
            Text(offline ? "Saved \u{00B7} syncs when connected" : "Saved").peopleStyle(.sub, weight: .semibold)
                .foregroundStyle(c.peopleAccentInk).lineLimit(1).fixedSize()
        }
        .opacity(shown || reduced ? 1 : 0)
        .onAppear { withAnimation(PeopleMotion.easeOut(PeopleMotion.enter)) { shown = true } }
    }
}

/// FieldRow / AddInfo input: padding 10 14, r-ctl 14 (CSS radius), 1 px line border on surface, 15/21 ink, at least 44 tall.
/// Focus: accent border and the soft focus ring (`--ring-focus`: 3 px of accent-ink at 14 %, outside the box). `rows` > 1 is the
/// textarea (rows x 21 + padding). The platform text field gives no select-all on focus (FieldRow focusIn), so the caret lands at the end.
struct PeopleInput: View {
    @Environment(\.tin) private var c
    @Binding var text: String
    let label: String
    var placeholder = ""
    var rows = 1
    var link = false
    var focus = false
    var plain = false
    var onSubmit: (() -> Void)? = nil
    var onEscape: (() -> Void)? = nil
    @FocusState private var focused: Bool

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 14, style: .circular)
        let multi = rows > 1
        TextField("", text: $text, prompt: Text(placeholder).foregroundColor(c.ink2.opacity(0.75)), axis: multi ? .vertical : .horizontal)
            .lineLimit(multi ? rows...rows : 1...1)
            .focused($focused)
            .peopleStyle(.body).foregroundStyle(c.ink).tint(c.ink)
            .textInputAutocapitalization(plain || link ? .never : .sentences).autocorrectionDisabled(plain || link)
            .keyboardType(link ? .URL : .default).submitLabel(multi ? .return : .done)
            .onSubmit { if !multi { onSubmit?() } }
            .onKeyPress(.escape) { if let onEscape { onEscape(); return .handled }; return .ignored }
            .padding(.horizontal, 14).padding(.vertical, 10)
            .frame(maxWidth: .infinity, minHeight: multi ? CGFloat(rows) * 21 + 20 : 44, alignment: multi ? .topLeading : .leading)
            .background(shape.fill(c.sf))
            .overlay(shape.stroke(focused ? c.peopleAccent : c.ln, lineWidth: 1))
            .background { if focused { shape.stroke(c.peopleAccentInk.opacity(0.14), lineWidth: 3).padding(-1.5) } }
            .accessibilityLabel(label)
            .task(id: focus) { if focus { focused = true } }
    }
}

enum FieldKind { case text, multiline, link }

/// FieldRow.svelte. View: label over value, the whole row edits (the pencil is a pointer convenience). Edit: label, field, the
/// always-present hint / error line (36 min, so a refusal never moves the buttons), the byte count once past 80 % of the limit,
/// then [Remove] ... Cancel / Save. A failed save keeps the draft. Read-only rows show the value; values this version can't edit
/// (structured data from another app version) can only be removed, with an inline confirm.
/// `onSave` / `onRemove` return nil when accepted, else the message.
struct PeopleFieldRow: View {
    @Environment(\.tin) private var c
    let label: String
    let value: PeopleJSON?
    var kind: FieldKind = .text
    var hint = ""
    var placeholder = ""
    var optional = false
    var readonly = false
    var limit = 0
    var offline = false
    var empty = "Not set"
    var note = ""
    var onSave: ((String) async -> String?)? = nil
    var onRemove: (() async -> String?)? = nil
    var onOpen: ((String) -> Void)? = nil
    var startEditing = false

    private enum Mode { case view, edit, remove }
    @State private var mode = Mode.view
    @State private var draft = ""
    @State private var error = ""
    @State private var saved = 0
    @State private var busy = false

    private var isNull: Bool { value == nil || value?.isNull == true }
    private var text: String {
        switch value {
        case .string(let s)?: return s
        case .number(let n)?: return n
        case .bool(let b)?: return b ? "true" : "false"
        default: return ""
        }
    }
    private var editable: Bool { !readonly && onSave != nil && (isNull || (value?.isScalar ?? false)) }
    private var removable: Bool { !readonly && onRemove != nil && optional && !editable }
    private var url: String? { kind == .link ? ProfileInfo.safeUrl(text) : nil }

    var body: some View {
        Group {
            switch mode {
            case .edit: editor.transition(.opacity)
            case .remove: remover.transition(.opacity)
            case .view: viewer
            }
        }
        .frame(maxWidth: .infinity)
        .peopleAnimation(PeopleMotion.crossfade, value: mode == .view)
        .task(id: saved) { if saved > 0 { try? await Task.sleep(nanoseconds: savedMs * 1_000_000); saved = 0 } }
        .task { if startEditing && editable { draft = text; mode = .edit } }
    }

    private func edit() { draft = text; error = ""; mode = .edit }
    private func cancel() { mode = .view; error = "" }
    private func save() {
        guard !busy, let run = onSave else { return }
        busy = true
        Task {
            let r = await run(draft)
            busy = false
            if r == nil { mode = .view; error = ""; saved += 1 } else { error = r! }
        }
    }
    private func drop() {
        guard !busy, let run = onRemove else { return }
        busy = true
        Task {
            let r = await run()
            busy = false
            if let r { error = r } else { mode = .view; error = "" }
        }
    }

    private var editor: some View {
        let bytes = ProfileRules.utf8(draft)
        let near = limit > 0 && Double(bytes) > Double(limit) * 0.8
        return ZStack(alignment: .topTrailing) {
            VStack(alignment: .leading, spacing: 6) {
                Text(label).peopleStyle(.sub, weight: .semibold).foregroundStyle(c.ink2)
                PeopleInput(text: $draft, label: label, placeholder: placeholder, rows: kind == .multiline ? 4 : 1, link: kind == .link,
                            focus: true, plain: kind != .multiline, onSubmit: save, onEscape: cancel)
                VStack(alignment: .leading, spacing: 2) {
                    if !error.isEmpty { Text(error).peopleStyle(.sub).foregroundStyle(c.peopleDanger) }
                    else if !hint.isEmpty { Text(hint).peopleStyle(.sub).foregroundStyle(c.ink2) }
                }
                .frame(maxWidth: .infinity, minHeight: 36, alignment: .topLeading).accessibilityAddTraits(.updatesFrequently)
                HStack(spacing: 8) {
                    if optional && !isNull { PeopleButton("Remove", variant: .dangerQuiet, action: drop) }
                    Spacer(minLength: 0)
                    PeopleButton("Cancel", variant: .quiet, action: cancel)
                    PeopleButton("Save", variant: .primary, action: save)
                }
            }
            .padding(.leading, 16).padding(.trailing, 16).padding(.top, 14).padding(.bottom, 12)
            if near {
                Text("\(bytes) of \(limit) bytes").peopleStyle(.sub, tnum: true).foregroundStyle(bytes > limit ? c.peopleDanger : c.ink2)
                    .lineLimit(1).fixedSize().padding(.top, 14).padding(.trailing, 16)
            }
        }
    }

    private var remover: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Remove \u{201C}\(label)\u{201D} from your profile?").peopleStyle(.body, weight: .semibold).foregroundStyle(c.ink)
            Text("It disappears from your profile. Earlier versions can remain in synced history.").peopleStyle(.sub19).foregroundStyle(c.ink2)
            if !error.isEmpty { Text(error).peopleStyle(.sub).foregroundStyle(c.peopleDanger).accessibilityAddTraits(.updatesFrequently) }
            HStack(spacing: 8) {
                Spacer(minLength: 0)
                PeopleButton("Cancel", variant: .quiet, action: cancel)
                PeopleButton("Remove", variant: .danger, action: drop)
            }
        }
        .padding(.horizontal, 16).padding(.top, 14).padding(.bottom, 12)
    }

    private var viewer: some View {
        HStack(spacing: 0) {
            let content = VStack(alignment: .leading, spacing: 1) {
                if !label.isEmpty || saved > 0 {
                    HStack(spacing: 8) {
                        Text(label).peopleStyle(.sub).foregroundStyle(c.ink2)
                        if saved > 0 { PeopleSavedTag(offline: offline && editable) }
                    }
                }
                if editable { Text(text.isEmpty ? empty : text).peopleStyle(.body).foregroundStyle(text.isEmpty ? c.ink2 : c.ink) }
                else if isNull || text.isEmpty && value?.asString != nil { Text(empty).peopleStyle(.body).foregroundStyle(c.ink2) }
                else if let s = value?.asString { Text(s).peopleStyle(.body).foregroundStyle(c.ink) }
                else if let value { PeopleProfileValue(value: value) }
                if !note.isEmpty { Text(note).peopleStyle(.sub).foregroundStyle(c.ink2).padding(.top, 2) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16).padding(.vertical, editable ? 10 : label.isEmpty ? 14 : 12)
            .frame(minHeight: !editable && label.isEmpty ? 0 : 64, alignment: editable ? .leading : .topLeading)
            if editable {
                Button(action: edit) { content.contentShape(Rectangle()) }.buttonStyle(.plain)
                    .accessibilityElement(children: .ignore).accessibilityLabel("\(label): \(text.isEmpty ? empty : text). Edit")
                    .accessibilityAddTraits(.isButton)
            } else { content }
            if let url, let onOpen { toolButton(.link, 20, "Open \(label): \(ProfileInfo.shortUrl(url))") { onOpen(url) } }
            if editable {
                toolButton(.edit, 18, nil, alpha: 0.8) { edit() }.padding(.trailing, 6).padding(.leading, url != nil && onOpen != nil ? -8 : 0)
            }
            if removable { toolButton(.trash, 20, "Remove \(label)") { error = ""; mode = .remove } }
        }
        .fixedSize(horizontal: false, vertical: true)
    }

    /// FieldRow `.tool`: a full-height 44 wide icon button, muted; `label` nil = decorative for assistive tech (the row is the control).
    private func toolButton(_ icon: PeopleIcon, _ size: CGFloat, _ label: String?, alpha: Double = 1, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            PeopleIconView(icon: icon, size: size, color: c.ink2).opacity(alpha).frame(width: 44).frame(maxHeight: .infinity).contentShape(Rectangle())
        }
        .buttonStyle(PeoplePressStyle())
        .accessibilityHidden(label == nil).accessibilityLabel(label ?? "")
    }
}

/// Value.svelte: text and numbers as text, lists as lists, nested objects as indented label/value pairs. Text only, never fetched.
/// Deeper than 4 levels stops at an ellipsis, so a hostile value can't grow the page.
struct PeopleProfileValue: View {
    @Environment(\.tin) private var c
    let value: PeopleJSON
    var depth = 0

    private func prim(_ x: PeopleJSON) -> Bool {
        switch x { case .array, .object: return false; default: return true }
    }
    private func show(_ x: PeopleJSON) -> String {
        switch x {
        case .null: return "\u{2014}"
        case .bool(let b): return b ? "Yes" : "No"
        case .number(let n): return n
        case .string(let s): return s
        default: return x.text
        }
    }
    private func nest<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        content().padding(.leading, 12).padding(.top, 6)
            .overlay(alignment: .leading) { Rectangle().fill(c.peopleHairline).frame(width: 2).padding(.top, 6) }
    }

    var body: some View {
        if depth > 4 {
            Text("\u{2026}").peopleStyle(.body).foregroundStyle(c.ink2)
        } else {
            switch value {
            case .array(let a) where a.allSatisfy(prim):
                Text(a.isEmpty ? "\u{2014}" : a.map(show).joined(separator: ", ")).peopleStyle(.body).foregroundStyle(c.ink)
            case .array(let a):
                nest { VStack(alignment: .leading, spacing: 8) { ForEach(Array(a.enumerated()), id: \.offset) { PeopleProfileValue(value: $0.element, depth: depth + 1) } } }
            case .object(let o):
                nest {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(o.keys, id: \.self) { k in
                            VStack(alignment: .leading, spacing: 1) {
                                Text(ProfileInfo.humanize(k)).peopleStyle(.sub).foregroundStyle(c.ink2)
                                PeopleProfileValue(value: o[k] ?? .null, depth: depth + 1)
                            }
                        }
                    }
                }
            default:
                Text(show(value)).peopleStyle(.body).foregroundStyle(c.ink)
            }
        }
    }
}
