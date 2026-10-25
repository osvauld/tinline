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
