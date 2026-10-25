import Foundation

/// One lookup for every user-visible string. Keys and English text live in `i18n/en/ios.toml`; the generator writes
/// `Resources/Localizable.xcstrings`. Arguments fill the `{placeholders}` in the order they first appear in the English
/// text (strings for names, `Int` for the `{count}` placeholder). Missing keys fall back to the key itself.
func tr(_ key: String, _ args: CVarArg...) -> String {
    let format = NSLocalizedString(key, comment: "")
    return args.isEmpty ? format : String(format: format, locale: Locale.current, arguments: args)
}

/// Count-dependent text: the entry has `one`/`other` (CLDR) forms and `{count}`. `count` is the plural argument; any
/// further placeholders follow in `args`, in the order they first appear in the English text (count included there).
func trn(_ key: String, _ count: Int, _ args: CVarArg...) -> String {
    let format = NSLocalizedString(key, comment: "")
    return String(format: format, locale: Locale.current, arguments: [count as CVarArg] + args)
}

/// Escapes markdown control characters in user data (names, group names) before it goes into a markdown sentence.
func trEscapeMarkdown(_ s: String) -> String {
    var out = ""
    for ch in s {
        if "\\*_`[]~<>".contains(ch) { out.append("\\") }
        out.append(ch)
    }
    return out
}

/// A sentence with `**bold**` runs written as markdown inside the one translated string, so translators keep word order
/// and the emphasis lands on the right words. `String` arguments are user data and are escaped; `Int` arguments pass
/// through. `style` adjusts each bold run (for example a stronger font); without it the runs use the system bold.
func trRich(_ key: String, _ args: CVarArg..., style: ((inout AttributedString, Range<AttributedString.Index>) -> Void)? = nil) -> AttributedString {
    let safe: [CVarArg] = args.map { ($0 as? String).map(trEscapeMarkdown) ?? $0 }
    return trMarkdown(String(format: NSLocalizedString(key, comment: ""), locale: Locale.current, arguments: safe), style: style)
}

/// Parses inline markdown (bold only is expected) and lets `style` restyle the bold runs.
func trMarkdown(_ markdown: String, style: ((inout AttributedString, Range<AttributedString.Index>) -> Void)? = nil) -> AttributedString {
    let opts = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
    guard var out = try? AttributedString(markdown: markdown, options: opts) else { return AttributedString(markdown) }
    if let style {
        let bold = out.runs.filter { $0.inlinePresentationIntent?.contains(.stronglyEmphasized) == true }.map(\.range)
        for r in bold { style(&out, r) }
    }
    return out
}

/// Names (or any user data) as one list in the user's language: "Ana, Ben and Chi" / "Ana, Ben, and Chi" / "Ana، Ben و Chi".
func trList(_ items: [String]) -> String {
    ListFormatter.localizedString(byJoining: items)
}
