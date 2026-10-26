import SwiftUI

/// Restore: 24 words (Restore.svelte; android Onboarding.kt RestoreStep). 24 real fields in two columns. Typing a space or Return
/// moves on (completing a word when the prefix has one answer); pasting a run of words spreads them from that field; Backspace in
/// an empty field goes back. A word turns red once it is left (or after Continue) if it isn't on the list, or at once when no
/// word starts that way. The message and suggestions sit in reserved space. The rules are `ShellRestoreWords` (ShellFormat.swift).
struct ShellRestoreStep: View {
    @Binding var error: String?
    let warning: String
    let onSubmit: (String) -> Void
    @Environment(\.tin) private var tin
    @State private var model = ShellRestoreWords(list: ShellWordlist.words)
    @FocusState private var focus: Int?
    @State private var active = -1

    private var sugg: [String] {
        guard active >= 0, !model.set.contains(model.words[active]) else { return [] }
        return model.suggest(model.words[active], 4)
    }

    private func setWord(_ i: Int, _ raw: String) {
        if let next = model.setWord(i, raw, isKey: { isRecoveryKey(input: $0) }) { focus = next }
        if !model.recKey.isEmpty { focus = nil }
        error = nil
    }

    private func submit() {
        if !model.recKey.isEmpty {
            // The core checks the key's checksum: a typo is caught before anything is restored.
            if let phrase = try? recoveryPhraseFromInput(input: model.recKey) { onSubmit(phrase) }
            else { error = tr("restore_key_typo") }
            return
        }
        let r = model.submitWords()
        if let phrase = r.phrase { onSubmit(phrase) } else if let f = r.focus { focus = f }
    }

    private func advance(_ i: Int) {
        model.complete(i)
        if i < 23 { focus = i + 1 } else { submit() }
    }

    var body: some View {
        let p = ShellPalette(tin)
        let msg = model.message(error: error)
        VStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text(tr("restore_title")).shellStyle(.h1).foregroundStyle(tin.ink).accessibilityAddTraits(.isHeader)
                    Text(model.recKey.isEmpty
                         ? tr("restore_lead_words", "24")
                         : tr("restore_lead_key"))
                        .shellStyle(.lead).foregroundStyle(tin.ink2)
                    ShellNotice(warning, kind: .warn, glyph: .info)
                    if !model.recKey.isEmpty {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(tr("restore_key_label")).font(TinType.caption).foregroundStyle(tin.ink2)
                            Text(model.recKey).font(ShellFont.mono(15)).lineSpacing(6).foregroundStyle(tin.ink).privacySensitive()
                            ShellGBtn(text: tr("restore_use_words"), kind: .text, fill: false) { model.recKey = ""; error = nil }.offset(x: -20)
                        }
                        .padding(.horizontal, 16).padding(.vertical, 14).frame(maxWidth: .infinity, alignment: .leading)
                        .background(SmoothRoundedRectangle(cornerRadius: 10).fill(tin.sf))
                        .overlay(SmoothRoundedRectangle(cornerRadius: 10).stroke(p.accentSoft, lineWidth: 1))
                    } else {
                        HStack {
                            ShellGBtn(text: tr("restore_paste"), kind: .text, fill: false) {
                                if let t = ShellClipboard.text() { setWord(0, t) } else { error = tr("restore_paste_failed") }
                            }
                            .offset(x: -20)
                            Spacer(minLength: 0)
                            Text(tr("restore_progress", model.filled, "24")).shellStyle(.sub).monospacedDigit().foregroundStyle(tin.ink2)
                        }
                        .offset(y: -4)
                        VStack(spacing: 8) {
                            ForEach(0..<12, id: \.self) { r in
                                HStack(spacing: 8) {
                                    ForEach(0..<2, id: \.self) { c in
                                        let i = r * 2 + c
                                        ShellWordCell(i: i, value: Binding(get: { model.words[i] }, set: { v in
                                            // A space after one word moves on; anything else is typing or a paste.
                                            if v.hasSuffix(" ") && v.split(separator: " ").count <= 1 {
                                                setWord(i, v.trimmingCharacters(in: .whitespaces)); advance(i)
                                            } else { setWord(i, v) }
                                        }), bad: model.bad(i), ok: !model.words[i].isEmpty && model.set.contains(model.words[i]), focus: $focus,
                                        onNext: { advance(i) }, onBackEmpty: { if i > 0 { focus = i - 1 } })
                                    }
                                }
                            }
                        }
                        .accessibilityElement(children: .contain).accessibilityLabel(tr("restore_words_label"))
                    }
                }
                .padding(.horizontal, 24).padding(.vertical, 8)
            }
            // The dock: suggestions (44 tall, always there) and the message line, above the footer.
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 8) {
                    ForEach(sugg, id: \.self) { s in
                        Button {
                            guard active >= 0 else { return }
                            let n = active
                            model.words[n] = s
                            if n < 23 { focus = n + 1 }
                        } label: {
                            Text(s).font(ShellFont.mono(14, .medium)).fontWeight(.semibold).foregroundStyle(p.accentInk)
                                .padding(.horizontal, 16).frame(minHeight: 44).background(Capsule().fill(p.accentSoft))
                        }
                        .buttonStyle(ShellPressStyle(wash: .clear, radius: 22))
                    }
                    Spacer(minLength: 0)
                }
                .frame(minHeight: 44)
                Text(msg).shellStyle(.sub, weight: .semibold).foregroundStyle(tin.er).frame(maxWidth: .infinity, minHeight: 18, alignment: .leading)
                    .accessibilityAddTraits(.updatesFrequently)
            }
            .padding(.horizontal, 24).padding(.top, 8).background(tin.bg)
            ShellGBtn(text: tr("restore_continue"), enabled: model.filled > 0 || !model.recKey.isEmpty, action: submit)
                .padding(.horizontal, 24).padding(.top, 12).padding(.bottom, 24)
        }
        .onChange(of: focus) { old, new in
            if let old { model.seen[old] = true; if active == old { active = -1 } }
            if let new { active = new }
        }
        .task { await Task.yield(); focus = 0 }
    }
}

private struct ShellWordCell: View {
    let i: Int
    @Binding var value: String
    let bad: Bool
    let ok: Bool
    var focus: FocusState<Int?>.Binding
    let onNext: () -> Void
    let onBackEmpty: () -> Void
    @Environment(\.tin) private var tin

    var body: some View {
        let p = ShellPalette(tin)
        let focused = focus.wrappedValue == i
        HStack(spacing: 6) {
            Text("\(i + 1)").font(ShellFont.figtree(11)).monospacedDigit().foregroundStyle(tin.ink2).frame(width: 20, alignment: .trailing)
            ShellBackspaceField(text: $value, onBackEmpty: onBackEmpty)
                .font(ShellFont.mono(15)).foregroundStyle(tin.ink).tint(p.accentInk)
                .textInputAutocapitalization(.never).autocorrectionDisabled(true).keyboardType(.asciiCapable)
                .submitLabel(i == 23 ? .done : .next).onSubmit(onNext)
                .focused(focus, equals: i)
                .accessibilityLabel(tr("restore_word_n", i + 1))
        }
        .padding(.leading, 10).padding(.trailing, 6).frame(maxWidth: .infinity, minHeight: 44)
        .background(SmoothRoundedRectangle(cornerRadius: 10).fill(bad ? tin.erc : tin.sf))
        .overlay(SmoothRoundedRectangle(cornerRadius: 10).stroke(focused ? .clear : bad ? tin.er : ok ? p.accentSoft : tin.ln, lineWidth: 1))
        .overlay(SmoothRoundedRectangle(cornerRadius: 11).stroke(focused ? p.accentInk : .clear, lineWidth: 2).padding(-1))
    }
}

/// A plain text field; Backspace in an empty field calls `onBackEmpty`. SwiftUI has no key hook for that, so an empty field holds
/// one zero-width space: deleting it is the Backspace.
/// TODO(ios-device): check VoiceOver reads the field as empty and that paste over the marker spreads words as intended.
private struct ShellBackspaceField: View {
    @Binding var text: String
    let onBackEmpty: () -> Void
    private static let marker = "\u{200B}"
    var body: some View {
        TextField("", text: Binding(
            get: { text.isEmpty ? Self.marker : text },
            set: { v in
                if v.isEmpty && text.isEmpty { onBackEmpty(); return }
                text = v.replacingOccurrences(of: Self.marker, with: "")
            }))
    }
}
