//! Normalizing and tokenizing text for the search index (`docs/search.md` "Tokenizing").
//!
//! One normalizer for messages, names and queries: per character NFKC, then lowercase, then (for
//! Latin letters only) diacritics dropped. Every normalized char remembers which original char it
//! came from, so matches map back to the text the user sees (snippets and highlights).
//!
//! Words come from Unicode word boundaries, letters, digits and their marks only. Scripts written
//! without spaces (CJK, Thai, Lao, Khmer, Myanmar) are cut into runs and indexed as overlapping
//! character bigrams instead.

use std::collections::BTreeSet;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;
use unicode_segmentation::UnicodeSegmentation;

/// Longest word prefix indexed (chars). A longer query word is looked up by this prefix and
/// checked against the text.
pub const MAX_PREFIX: usize = 8;

/// Normalized text: the chars and, for each, the index of the original char it came from.
#[derive(Debug, Default)]
pub struct Norm {
    pub chars: Vec<char>,
    pub from: Vec<usize>,
}

fn is_latin(c: char) -> bool {
    c.is_alphabetic()
        && (c <= '\u{024F}'
            || ('\u{1E00}'..='\u{1EFF}').contains(&c)
            || ('\u{2C60}'..='\u{2C7F}').contains(&c)
            || ('\u{A720}'..='\u{A7FF}').contains(&c)
            || ('\u{AB30}'..='\u{AB6F}').contains(&c))
}

/// Scripts without spaces between words: indexed as character bigrams.
pub fn is_bigram_script(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FDF | 0x3005..=0x3007 | 0x3021..=0x3029 | 0x3038..=0x303B
        | 0x3040..=0x30FF | 0x31F0..=0x31FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF
        | 0x20000..=0x3FFFF
        | 0x0E00..=0x0EFF // Thai, Lao
        | 0x1780..=0x17FF | 0x19E0..=0x19FF // Khmer
        | 0x1000..=0x109F | 0xA9E0..=0xA9FF | 0xAA60..=0xAA7F) // Myanmar
}

pub fn normalize(s: &str) -> Norm {
    let mut n = Norm::default();
    let mut buf = [0u8; 4];
    for (i, c) in s.chars().enumerate() {
        for k in c.encode_utf8(&mut buf).nfkc() {
            for l in k.to_lowercase() {
                if is_latin(l) {
                    for d in std::iter::once(l).nfd().filter(|d| !is_combining_mark(*d)) {
                        n.chars.push(d);
                        n.from.push(i);
                    }
                } else if is_combining_mark(l) && n.chars.last().is_some_and(|p| is_latin(*p)) {
                    // A mark typed after a Latin base (decomposed input): folded like é -> e.
                } else {
                    n.chars.push(l);
                    n.from.push(i);
                }
            }
        }
    }
    n
}

/// One word (or one run of a bigram script) of normalized text: `start..end` in `Norm::chars`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub chars: Vec<char>,
    pub start: usize,
    pub end: usize,
    pub bigram: bool,
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || is_combining_mark(c)
}

pub fn words(n: &Norm) -> Vec<Word> {
    let mut out = Vec::new();
    let cs = &n.chars;
    let mut i = 0;
    while i < cs.len() {
        let big = is_bigram_script(cs[i]);
        let mut j = i + 1;
        while j < cs.len() && (is_bigram_script(cs[j]) == big || (big && is_combining_mark(cs[j]))) {
            j += 1;
        }
        if big {
            out.push(Word { chars: cs[i..j].to_vec(), start: i, end: j, bigram: true });
        } else {
            other_words(&cs[i..j], i, &mut out);
        }
        i = j;
    }
    out
}

/// Words of a stretch of ordinary text by Unicode word boundaries, letters/digits/marks only
/// ("can't" -> "cant").
fn other_words(cs: &[char], base: usize, out: &mut Vec<Word>) {
    let s: String = cs.iter().collect();
    // Byte offset -> char index within `cs`.
    let mut at = vec![0usize; s.len() + 1];
    for (ci, (bi, _)) in s.char_indices().enumerate() {
        at[bi] = ci;
    }
    for (bi, seg) in s.split_word_bound_indices() {
        let first = at[bi];
        let mut chars = Vec::new();
        let (mut start, mut end) = (None, 0);
        for (k, c) in seg.chars().enumerate() {
            if word_char(c) && (c.is_alphanumeric() || !chars.is_empty()) {
                chars.push(c);
                start.get_or_insert(first + k);
                end = first + k + 1;
            }
        }
        if let Some(st) = start
            && chars.iter().any(|c| c.is_alphanumeric())
        {
            out.push(Word { chars, start: base + st, end: base + end, bigram: false });
        }
    }
}

/// What gets indexed for a text: every word's prefixes from 2 to `MAX_PREFIX` (8) chars (a 1-char
/// word as itself), every bigram of a bigram-script run (a 1-char run as itself).
pub fn index_terms(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for w in words(&normalize(text)) {
        if w.chars.len() == 1 {
            out.insert(w.chars[0].to_string());
        } else if w.bigram {
            for b in w.chars.windows(2) {
                out.insert(b.iter().collect());
            }
        } else {
            for k in 2..=w.chars.len().min(MAX_PREFIX) {
                out.insert(w.chars[..k].iter().collect());
            }
        }
    }
    out
}

/// A query word and how it is looked up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    pub chars: Vec<char>,
    pub bigram: bool,
}

impl Term {
    /// The indexed term this one is found under.
    pub fn lookup(&self) -> String {
        let n = if self.chars.len() == 1 {
            1
        } else if self.bigram {
            2
        } else {
            self.chars.len().min(MAX_PREFIX)
        };
        self.chars[..n].iter().collect()
    }

    /// How narrow its posting list is likely to be (longer is narrower).
    pub fn weight(&self) -> usize {
        self.chars.len().min(MAX_PREFIX)
    }

    /// Where it matches the words of a text, as `start..end` in the normalized chars: a word
    /// it is a prefix of, or (bigram scripts) anywhere inside a run.
    pub fn matches(&self, ws: &[Word]) -> Vec<(usize, usize)> {
        let t = &self.chars;
        let mut out = Vec::new();
        for w in ws.iter().filter(|w| w.bigram == self.bigram) {
            if !self.bigram {
                if w.chars.starts_with(t) {
                    out.push((w.start, w.start + t.len()));
                }
            } else if w.chars.len() >= t.len() {
                for k in 0..=w.chars.len() - t.len() {
                    if w.chars[k..k + t.len()] == t[..] {
                        out.push((w.start + k, w.start + k + t.len()));
                    }
                }
            }
        }
        out
    }
}

/// Where every one of `terms` matches `text` (normalized ranges), or `None` if one does not:
/// the query's words are ANDed.
pub fn match_all(terms: &[Term], n: &Norm) -> Option<Vec<(usize, usize)>> {
    let ws = words(n);
    let mut hits = Vec::new();
    for t in terms {
        let m = t.matches(&ws);
        if m.is_empty() {
            return None;
        }
        hits.extend(m);
    }
    Some(hits)
}

pub fn query_terms(q: &str) -> Vec<Term> {
    let mut out: Vec<Term> = Vec::new();
    for w in words(&normalize(q)) {
        let t = Term { chars: w.chars, bigram: w.bigram };
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// The normalized text as one string with words separated by single spaces (for name matching).
pub fn folded(s: &str) -> String {
    let ws = words(&normalize(s));
    ws.iter().map(|w| w.chars.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

/// Chars of snippet context kept before the first match.
const LEAD: usize = 24;
pub const SNIPPET_CHARS: usize = 80;

/// A snippet of `text` around the first of `hits` (normalized `start..end` ranges) and the hit
/// ranges inside it, as char offsets into the snippet. Line breaks read as spaces; a cut is
/// marked with "…".
pub fn snippet(text: &str, n: &Norm, hits: &[(usize, usize)]) -> (String, Vec<(u32, u32)>) {
    let orig: Vec<char> = text.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).collect();
    let mut ranges: Vec<(usize, usize)> = hits
        .iter()
        .filter(|(s, e)| s < e && *e <= n.from.len())
        .map(|(s, e)| (n.from[*s], n.from[e - 1] + 1))
        .collect();
    ranges.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for r in ranges {
        match merged.last_mut() {
            Some(l) if r.0 <= l.1 => l.1 = l.1.max(r.1),
            _ => merged.push(r),
        }
    }
    let first = merged.first().map_or(0, |r| r.0);
    let (mut start, mut end) = (0, orig.len());
    if orig.len() > SNIPPET_CHARS {
        start = first.saturating_sub(LEAD);
        // Begin at a word if one starts shortly after the cut.
        if start > 0
            && let Some(k) = orig[start..first].iter().position(|c| *c == ' ')
            && k < LEAD / 2
        {
            start += k + 1;
        }
        end = (start + SNIPPET_CHARS).min(orig.len());
        if end == orig.len() {
            start = end.saturating_sub(SNIPPET_CHARS).min(start);
        }
    }
    let lead = usize::from(start > 0);
    let mut s = String::new();
    if start > 0 {
        s.push('…');
    }
    s.extend(&orig[start..end]);
    if end < orig.len() {
        s.push('…');
    }
    let hl = merged
        .into_iter()
        .filter(|(a, b)| *b > start && *a < end)
        .map(|(a, b)| ((a.max(start) - start + lead) as u32, (b.min(end) - start + lead) as u32))
        .collect();
    (s, hl)
}

#[cfg(test)]
mod tests;
