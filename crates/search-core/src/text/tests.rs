use super::*;

fn ws(s: &str) -> Vec<String> {
    words(&normalize(s)).into_iter().map(|w| w.chars.into_iter().collect()).collect()
}

// KIND: unit (S1) — English words, case, punctuation inside words.
#[test]
fn english_words() {
    assert_eq!(ws("Hello, World! Can't stop—3.14"), ["hello", "world", "cant", "stop", "314"]);
    assert_eq!(ws("  "), Vec::<String>::new());
}

// KIND: unit (S1) — Latin diacritics fold (precomposed and decomposed); NFKC compat forms.
#[test]
fn accented_latin_folds() {
    assert_eq!(ws("Café Ünïcödé"), ["cafe", "unicode"]);
    assert_eq!(ws("Cafe\u{301}"), ["cafe"]);
    assert_eq!(ws("ﬁle ＡＢＣ"), ["file", "abc"]);
    assert_eq!(ws("Straße Øre"), ["straße", "øre"]);
}

// KIND: unit (S1) — Indic vowel signs and viramas are marks of the word, never dropped or
// split off.
#[test]
fn indic_keeps_vowel_signs() {
    assert_eq!(ws("नमस्ते दुनिया"), ["नमस्ते", "दुनिया"]);
    assert_eq!(ws("മലയാളം ഭാഷ"), ["മലയാളം", "ഭാഷ"]);
    let t = query_terms("नम");
    let w = words(&normalize("नमस्ते"));
    assert_eq!(t[0].matches(&w), [(0, 2)]);
    assert!(!query_terms("മല")[0].matches(&words(&normalize("മലയാളം"))).is_empty());
}

// KIND: unit (S1) — CJK runs index as bigrams; a query run matches as a substring.
#[test]
fn cjk_bigrams() {
    let terms = index_terms("東京都に行く");
    for b in ["東京", "京都", "都に", "に行", "行く"] {
        assert!(terms.contains(b), "{b}");
    }
    let q = &query_terms("京都")[0];
    assert!(q.bigram);
    assert_eq!(q.lookup(), "京都");
    assert_eq!(q.matches(&words(&normalize("東京都に行く"))), [(1, 3)]);
    assert!(query_terms("大阪")[0].matches(&words(&normalize("東京都"))).is_empty());
    // Mixed with Latin.
    assert_eq!(ws("iPhone買った"), ["iphone", "買った"]);
}

// KIND: unit (S1) — emoji and symbols are not words.
#[test]
fn emoji_ignored() {
    assert_eq!(ws("🎉 party 👍🏽 ok ❤️"), ["party", "ok"]);
    assert!(index_terms("🎉👍").is_empty());
}

// KIND: unit (v2) — prefixes 2..=8 chars, a 1-char word as itself.
#[test]
fn prefixes() {
    let t = index_terms("a hello");
    assert_eq!(t.iter().cloned().collect::<Vec<_>>(), ["a", "he", "hel", "hell", "hello"]);
    let long = index_terms("internationalization");
    assert_eq!(long.len(), 7);
    assert!(long.contains("internat"));
    assert!(!long.contains("internati"));
    let q = &query_terms("internationalization")[0];
    assert_eq!(q.lookup().chars().count(), MAX_PREFIX);
    assert!(long.contains(&q.lookup()));
}

// KIND: unit (S1) — every query word must match (AND), each as a word prefix, in any order.
#[test]
fn multi_term_and() {
    let n = normalize("Dinner at Luigi's tomorrow?");
    assert!(match_all(&query_terms("tom din"), &n).is_some());
    assert!(match_all(&query_terms("luigis"), &n).is_some());
    assert!(match_all(&query_terms("dinner lunch"), &n).is_none());
    assert!(match_all(&query_terms("inner"), &n).is_none(), "prefix, not substring");
    assert_eq!(match_all(&query_terms("DIN tomorrow"), &n).unwrap().len(), 2);
}

// KIND: unit (S1) — snippet window and highlight offsets in chars (not bytes).
#[test]
fn snippet_and_highlights() {
    let text = "Ünïcode test";
    let n = normalize(text);
    let w = words(&n);
    let hits = query_terms("unic")[0].matches(&w);
    let (s, hl) = snippet(text, &n, &hits);
    assert_eq!(s, text);
    assert_eq!(hl, [(0, 4)]);
    let long = format!("{} needle {}", "word ".repeat(30), "tail ".repeat(30));
    let n = normalize(&long);
    let hits = query_terms("needle")[0].matches(&words(&n));
    let (s, hl) = snippet(&long, &n, &hits);
    let cs: Vec<char> = s.chars().collect();
    assert!(s.starts_with('…') && s.ends_with('…'));
    let (a, b) = hl[0];
    assert_eq!(cs[a as usize..b as usize].iter().collect::<String>(), "needle");
    assert!(cs.len() <= SNIPPET_CHARS + 2);
}
