import SwiftUI

/// One message found in this chat; `day` is the core's UTC day to load it with (`chat_day`).
struct FindHit: Equatable { let id: String; let day: String }

/// Find in this chat (playground screens/search/FindBar.svelte, docs/search.md "Find in chat"). Hits come newest first, so
/// `cur` = 0 is the newest and the count reads 1/m for it. Older = cur + 1, newer = cur - 1, both wrap.
@MainActor
final class FindState: ObservableObject {
    @Published var open = false
    @Published var q = ""
    @Published var hits: [FindHit] = []
    /// The core's cursor for the next (older) page; non-nil shows "m+".
    @Published var next: String?
    @Published var cur = 0
    @Published var focusReq = 0
    @Published var error: String?

    func show() { open = true; focusReq += 1 }
    func close() { open = false; q = ""; hits = []; next = nil; cur = 0; error = nil }
    var current: FindHit? { hits.indices.contains(cur) ? hits[cur] : nil }
    /// The count the bar shows: "3/12", "3/12+" while the core has older pages, "—" after an error.
    var countLabel: String {
        if error != nil { return "—" }
        let shown = hits.isEmpty ? 0 : cur + 1
        return "\(shown)/\(hits.count)" + (next != nil ? "+" : "")
    }
    /// Steps to the next older hit; false when the caller should fetch the next page (`next` is set and we are at the end).
    func olderOrNeedsPage() -> Bool {
        if cur + 1 < hits.count { cur += 1; return false }
        if next == nil { cur = 0; return false }
        return true
    }
    func newer() { if !hits.isEmpty { cur = (cur - 1 + hits.count) % hits.count } }
}

enum ChatFind {
    // Highlighting only: the real core decides which messages match. Keep Indic vowel marks; fold Latin accents only.
    // Android decides "Latin" with Character.UnicodeScript; Swift has no script API, so the Latin blocks are listed.
    private static func isLatin(_ s: Unicode.Scalar) -> Bool {
        let v = s.value
        return v < 0x250 || (0x1E00...0x1EFF).contains(v) || (0x2C60...0x2C7F).contains(v) || (0xA720...0xA7FF).contains(v)
    }

    private static func fold(_ s: String) -> [Unicode.Scalar] {
        let compat = s.lowercased().precomposedStringWithCompatibilityMapping
        var out: [Unicode.Scalar] = []
        for sc in compat.unicodeScalars {
            if isLatin(sc) {
                for d in String(sc).decomposedStringWithCanonicalMapping.unicodeScalars where d.properties.generalCategory != .nonspacingMark { out.append(d) }
            } else { out.append(sc) }
        }
        return out
    }

    private static func isSubstringScript(_ s: Unicode.Scalar) -> Bool {
        let v = s.value
        // Han, Hiragana, Katakana, Thai, Lao, Khmer, Myanmar: no spaces between words, so match anywhere.
        return (0x4E00...0x9FFF).contains(v) || (0x3400...0x4DBF).contains(v) || (0x3040...0x30FF).contains(v)
            || (0x0E00...0x0EFF).contains(v) || (0x1780...0x17FF).contains(v) || (0x1000...0x109F).contains(v)
    }

    private static func isWordChar(_ s: Unicode.Scalar) -> Bool {
        s.properties.isAlphabetic || s.properties.numericType != nil || s.properties.generalCategory == .nonspacingMark
    }

    /// Where each word of `q` matches the start of a word in `text` (the core's rule), as ranges of Unicode scalar offsets
    /// into `text`.
    static func ranges(_ text: String, _ q: String) -> [Range<Int>] {
        let words = q.split(whereSeparator: { $0.isWhitespace }).map { fold(String($0)) }.filter { !$0.isEmpty }
        if words.isEmpty || text.isEmpty { return [] }
        // Folding can change lengths (é -> e + mark -> e); map folded indices back per scalar.
        var folded: [Unicode.Scalar] = []
        var back: [Int] = []
        for (i, sc) in text.unicodeScalars.enumerated() {
            for f in fold(String(sc)) { folded.append(f); back.append(i) }
        }
        var out: [Range<Int>] = []
        for w in words {
            var from = 0
            while from + w.count <= folded.count {
                guard let at = (from...(folded.count - w.count)).first(where: { Array(folded[$0..<$0 + w.count]) == w }) else { break }
                let start = isSubstringScript(w[0]) || at == 0 || !isWordChar(folded[at - 1])
                if start { out.append(back[at]..<(back[at + w.count - 1] + 1)) }
                from = at + 1
            }
        }
        return out.sorted { $0.lowerBound < $1.lowerBound }
    }

    /// Local fallback (previews): newest first over what is loaded.
    static func local(_ msgs: [Message], _ q: String) -> [FindHit] {
        msgs.filter { !$0.deleted && !ranges($0.text.isEmpty ? ($0.attachment?.name ?? "") : $0.text, q).isEmpty }
            .sorted { ($0.at, $0.id) > ($1.at, $1.id) }.map { FindHit(id: $0.id, day: "") }
    }

    /// `text` with each match washed in gold, the current message's stronger (Marked.svelte variant "mark").
    static func marked(_ text: AttributedString, q: String, current: Bool, gold: Color) -> AttributedString {
        let plain = String(text.characters)
        let rs = ranges(plain, q)
        if rs.isEmpty { return text }
        var out = text
        let scalars = plain.unicodeScalars
        for r in rs {
            let lo = scalars.index(scalars.startIndex, offsetBy: r.lowerBound), hi = scalars.index(scalars.startIndex, offsetBy: r.upperBound)
            if let a = AttributedString.Index(lo, within: out), let b = AttributedString.Index(hi, within: out) {
                out[a..<b].backgroundColor = gold.opacity(current ? 0.70 : 0.38)
            }
        }
        return out
    }
}

/// Covers the conversation header in the same box: field (magnifier, query, n/m), older, newer, close.
struct ChatFindBar: View {
    @ObservedObject var state: FindState
    let onQuery: (String) -> Void
    let onOlder: () -> Void
    let onNewer: () -> Void
    let onClose: () -> Void
    @Environment(\.tin) private var tin
    @FocusState private var focused: Bool

    var body: some View {
        let p = ChatPalette(tin)
        let count = state.hits.count
        let shown = count == 0 ? 0 : state.cur + 1
        HStack(spacing: 2) {
            HStack(spacing: 0) {
                ChatIcon(glyph: .search, size: 18, color: tin.ink2).frame(width: 44)
                ZStack(alignment: .leading) {
                    if state.q.isEmpty { Text(tr("chat_find_placeholder")).font(TinType.bodyL).foregroundStyle(tin.ink2).lineLimit(1) }
                    TextField("", text: $state.q)
                        .focused($focused).font(TinType.bodyL).foregroundStyle(tin.ink).tint(tin.pr)
                        .submitLabel(.search).onSubmit(onOlder).textInputAutocapitalization(.never).autocorrectionDisabled()
                        .accessibilityLabel(tr("chat_find_placeholder"))
                        .onChange(of: state.q) { _, v in onQuery(v) }
                }
                Text(state.countLabel).font(TinType.bodyM.monospacedDigit()).foregroundStyle(tin.ink2).lineLimit(1)
                    .frame(width: 64, alignment: .trailing).padding(.trailing, 14)
                    .accessibilityLabel(state.error ?? (count > 0 ? tr("chat_find_match_of", String(shown), String(count)) : tr("chat_find_none")))
            }
            .frame(height: 44).background(tin.sf2, in: Capsule())
            .overlay(Capsule().strokeBorder(focused ? p.accentFill : tin.sf2, lineWidth: 2))
            .animation(ChatMotion.animation(ChatMotion.switch), value: focused)
            ChatIconButton(glyph: .chevUp, label: tr("chat_find_older"), enabled: count > 0 || state.next != nil, iconSize: 20, action: onOlder)
            ChatIconButton(glyph: .chev, label: tr("chat_find_newer"), enabled: count > 0, iconSize: 20, action: onNewer)
            ChatIconButton(glyph: .x, label: tr("chat_find_close"), iconSize: 20, action: onClose)
        }
        .padding(.horizontal, 8).frame(maxWidth: .infinity, maxHeight: .infinity).background(tin.sf)
        .onChange(of: state.focusReq) { _, _ in focused = true }
        .onAppear { focused = true }
    }
}
