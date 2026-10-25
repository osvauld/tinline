//! Desktop strings. `strings.rs` is generated from `i18n/<tag>/desktop.toml` by
//! `scripts/i18n_gen.py`; this module picks the language and fills in placeholders.
//!
//! `tr("chat_send")`, `tr_args("call_ringing", &[("name", n)])`,
//! `tr_count("chat_unread", 3, &[])` (the count is also available as `{count}`).

use std::sync::RwLock;

use intl_pluralrules::{PluralCategory, PluralRuleType, PluralRules};

pub mod date;
mod strings;
pub use strings::{LANGUAGES, Msg};

#[cfg(test)]
mod tests;

const SOURCE: &str = "en";

/// Numbers that appear in sentences, passed as placeholders so the translator can
/// place (and, in some scripts, write) them: recovery words, device-name length.
pub const WORDS: &str = "24";
pub const NAME_MIN: &str = "1";
pub const NAME_MAX: &str = "64";

static CURRENT: RwLock<Option<String>> = RwLock::new(None);

/// The language tag we ship that best matches the OS locale (exact tag, then its
/// primary language), else English.
pub fn detect() -> String {
    sys_locale::get_locale().map(|l| match_tag(&l)).unwrap_or_else(|| SOURCE.into())
}

pub(crate) fn match_tag(locale: &str) -> String {
    let norm = locale.replace('_', "-");
    let norm = norm.split(['.', '@']).next().unwrap_or("");
    let has = |t: &str| LANGUAGES.iter().any(|(tag, _, _)| tag.eq_ignore_ascii_case(t));
    if let Some((tag, _, _)) = LANGUAGES.iter().find(|(t, _, _)| t.eq_ignore_ascii_case(norm)) {
        return (*tag).into();
    }
    let primary = norm.split('-').next().unwrap_or("");
    if has(primary) {
        return primary.to_ascii_lowercase();
    }
    SOURCE.into()
}

/// The language in use (detected from the OS on first use).
pub fn language() -> String {
    if let Some(l) = CURRENT.read().ok().and_then(|g| g.clone()) {
        return l;
    }
    let l = detect();
    set_language(&l);
    l
}

/// Switch language; unknown tags fall back to English.
pub fn set_language(tag: &str) {
    let tag = if LANGUAGES.iter().any(|(t, _, _)| *t == tag) { tag } else { SOURCE };
    if let Ok(mut g) = CURRENT.write() {
        *g = Some(tag.to_string());
    }
}

pub fn is_rtl() -> bool {
    let lang = language();
    LANGUAGES.iter().any(|(t, _, rtl)| *t == lang && *rtl)
}

fn lookup(lang: &str, key: &str) -> Option<Msg> {
    strings::get(lang, key).or_else(|| if lang == SOURCE { None } else { strings::get(SOURCE, key) })
}

fn fill(text: &str, args: &[(&str, &str)]) -> String {
    let mut out = text.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

pub fn tr(key: &str) -> String {
    tr_args(key, &[])
}

/// A message without placeholders as `&'static str`: a drop-in for a string literal
/// (widgets, match arms, struct fields). Plural messages give their `other` form.
pub fn tr_str(key: &str) -> &'static str {
    match lookup(&language(), key) {
        Some(Msg::Text(t)) => t,
        Some(Msg::Plural(forms)) => other(forms),
        None => {
            missing(key);
            // Rare and tiny: the key itself, kept alive for the program's life.
            Box::leak(key.to_string().into_boxed_str())
        }
    }
}

pub fn tr_args(key: &str, args: &[(&str, &str)]) -> String {
    match lookup(&language(), key) {
        Some(Msg::Text(t)) => fill(t, args),
        Some(Msg::Plural(forms)) => fill(other(forms), args),
        None => missing(key),
    }
}

pub fn tr_count(key: &str, count: u64, args: &[(&str, &str)]) -> String {
    let lang = language();
    let Some(msg) = lookup(&lang, key) else { return missing(key) };
    let text = match msg {
        Msg::Text(t) => t,
        Msg::Plural(forms) => pick(&lang, forms, count),
    };
    let n = count.to_string();
    let mut all: Vec<(&str, &str)> = vec![("count", n.as_str())];
    all.extend_from_slice(args);
    fill(text, &all)
}

/// A piece of a message with inline markup: plain text, `**bold**` text, or a
/// `{slot}` the caller fills with its own widget (a link, a name chip).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    Bold(String),
    Slot(String),
}

/// Split a template into pieces. `**x**` is bold; `{name}` found in `args` is
/// replaced by its value (user data), any other `{name}` stays a `Slot`.
fn split_pieces(template: &str, args: &[(&str, &str)]) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let mut bold = false;
    for (i, chunk) in template.split("**").enumerate() {
        if i > 0 {
            bold = !bold;
        }
        let mut rest = chunk;
        while !rest.is_empty() {
            let Some(open) = rest.find('{') else {
                push_text(&mut out, rest, bold);
                break;
            };
            let Some(close) = rest[open..].find('}').map(|c| open + c) else {
                push_text(&mut out, rest, bold);
                break;
            };
            push_text(&mut out, &rest[..open], bold);
            let name = &rest[open + 1..close];
            match args.iter().find(|(n, _)| *n == name) {
                Some((_, v)) => push_text(&mut out, v, bold),
                None => out.push(Piece::Slot(name.to_string())),
            }
            rest = &rest[close + 1..];
        }
    }
    out
}

fn push_text(out: &mut Vec<Piece>, text: &str, bold: bool) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(Piece::Text(t)) if !bold => t.push_str(text),
        Some(Piece::Bold(t)) if bold => t.push_str(text),
        _ => out.push(if bold { Piece::Bold(text.into()) } else { Piece::Text(text.into()) }),
    }
}

/// A message with inline emphasis or widget slots, as pieces for a rich-text row.
/// Never splits a sentence across keys: the translator orders the pieces.
pub fn tr_rich(key: &str, args: &[(&str, &str)]) -> Vec<Piece> {
    match lookup(&language(), key) {
        Some(Msg::Text(t)) => split_pieces(t, args),
        Some(Msg::Plural(forms)) => split_pieces(other(forms), args),
        None => vec![Piece::Text(missing(key))],
    }
}

/// Join names or labels as a list in the language's own way: a separator key
/// for 3+ items and a final "{list} and {last}" pattern (`common_list_separator`,
/// `common_list_and`; the two-item form is `common_list_two`).
pub fn tr_list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_string(),
        [a, b] => tr_args("common_list_two", &[("first", a), ("last", b)]),
        _ => {
            let sep = tr_str("common_list_separator");
            let head = items[..items.len() - 1].join(sep);
            tr_args("common_list_and", &[("list", &head), ("last", items[items.len() - 1])])
        }
    }
}

/// Join whole phrases (each already a complete label) with the language's own
/// separator, usually " · " (`common_dot_separator`).
pub fn tr_dots(parts: &[&str]) -> String {
    parts.iter().copied().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(tr_str("common_dot_separator"))
}

fn other(forms: &'static [(&'static str, &'static str)]) -> &'static str {
    forms.iter().find(|(f, _)| *f == "other").map(|(_, t)| *t).unwrap_or("")
}

fn category_name(c: PluralCategory) -> &'static str {
    match c {
        PluralCategory::ZERO => "zero",
        PluralCategory::ONE => "one",
        PluralCategory::TWO => "two",
        PluralCategory::FEW => "few",
        PluralCategory::MANY => "many",
        PluralCategory::OTHER => "other",
    }
}

fn category(lang: &str, count: u64) -> &'static str {
    let id: Option<unic_langid::LanguageIdentifier> = lang.parse().ok();
    id.and_then(|id| PluralRules::create(id, PluralRuleType::CARDINAL).ok())
        .and_then(|r| r.select(count as usize).ok())
        .map(category_name)
        .unwrap_or("other")
}

fn pick(lang: &str, forms: &'static [(&'static str, &'static str)], count: u64) -> &'static str {
    let form = category(lang, count);
    let find = |f: &str| forms.iter().find(|(k, _)| *k == f).map(|(_, t)| *t);
    if let Some(t) = find(form) {
        return t;
    }
    // This language lacks the form: English's rule, then "other".
    let en = if lang == SOURCE {
        "other"
    } else {
        category(SOURCE, count)
    };
    find(en).or_else(|| find("other")).unwrap_or("")
}

fn missing(key: &str) -> String {
    #[cfg(test)]
    debug_assert!(false, "missing i18n key: {key}");
    key.to_string()
}
