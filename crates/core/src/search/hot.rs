//! The hot tier of the search index: the docs of the recent days (and of the cold days that
//! changed since their segment was written), in memory only, with their postings. Nothing here
//! touches the file (`docs/search.md` "Index v2").

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Bound;
use std::time::Instant;

use zeroize::Zeroize;

use super::index::{DocRow, Keyer};
use super::seg::{DayKey, Digest, rev_at};
use super::text::index_terms;

pub struct HotDay {
    pub day: String,
    pub docs: HashSet<u64>,
    /// The watermark: the version vector this day was indexed at.
    pub digest: Option<Digest>,
    /// Since when a cold day has been waiting to go into a segment.
    pub cold_since: Option<Instant>,
    /// Bumped by every change (a segment write notices a day that changed under it).
    pub ver: u64,
}

#[derive(Default)]
pub struct Hot {
    pub days: HashMap<DayKey, HotDay>,
    /// The doc, its day and the term hashes it is posted under.
    pub docs: HashMap<u64, (DayKey, DocRow, Vec<u64>)>,
    post: HashMap<u64, BTreeSet<(u64, u64)>>,
}

impl Hot {
    pub fn day_mut(&mut self, key: DayKey, day: &str) -> &mut HotDay {
        self.days.entry(key).or_insert_with(|| HotDay { day: day.to_string(), docs: HashSet::new(), digest: None, cold_since: None, ver: 0 })
    }

    fn unpost(&mut self, h: u64, rev: u64, toks: &[u64]) {
        for tok in toks {
            if let Some(set) = self.post.get_mut(tok) {
                set.remove(&(rev, h));
                if set.is_empty() {
                    self.post.remove(tok);
                }
            }
        }
    }

    /// Removes a doc; whether it was there.
    pub fn remove(&mut self, h: u64) -> bool {
        let Some((key, mut row, toks)) = self.docs.remove(&h) else { return false };
        self.unpost(h, rev_at(row.at), &toks);
        if let Some(d) = self.days.get_mut(&key) {
            d.docs.remove(&h);
            d.ver += 1;
        }
        row.text.zeroize();
        true
    }

    /// Indexes a doc under a day (a row without words removes it); whether anything changed.
    pub fn put(&mut self, k: &Keyer, key: DayKey, h: u64, row: DocRow) -> bool {
        let terms = index_terms(&row.text);
        let had = self.docs.contains_key(&h);
        if let Some((_, old, _)) = self.docs.get(&h) {
            if *old == row && !terms.is_empty() {
                return false;
            }
            self.remove(h);
        }
        if terms.is_empty() {
            return had;
        }
        let rev = rev_at(row.at);
        let toks: Vec<u64> = terms.iter().map(|t| k.tok(t)).collect();
        for t in &toks {
            self.post.entry(*t).or_default().insert((rev, h));
        }
        let d = self.day_mut(key, &row.day);
        d.docs.insert(h);
        d.ver += 1;
        self.docs.insert(h, (key, row, toks));
        true
    }

    /// Up to `n` postings of `tok` after `after` (newest first) that are also posted under every
    /// token of `also` (and, with `only`, belong to that conversation's src hash), with their rows.
    pub fn range(&self, tok: u64, also: &[u64], after: Option<(u64, u64)>, only: Option<u64>, n: usize) -> Vec<(u64, u64, DocRow)> {
        let Some(set) = self.post.get(&tok) else { return Vec::new() };
        let from = after.map_or(Bound::Unbounded, Bound::Excluded);
        let sets: Vec<Option<&BTreeSet<(u64, u64)>>> = also.iter().map(|t| self.post.get(t)).collect();
        set.range((from, Bound::Unbounded))
            .filter(|k| sets.iter().all(|s| s.is_some_and(|s| s.contains(*k))))
            .filter(|(_, h)| only.is_none_or(|s| self.docs.get(h).is_some_and(|d| (d.0).0 == s)))
            .take(n)
            .filter_map(|(rev, h)| self.docs.get(h).map(|(_, r, _)| (*rev, *h, r.clone())))
            .collect()
    }

    /// Drops a whole day (flushed into a segment).
    pub fn drop_day(&mut self, key: &DayKey) {
        if let Some(d) = self.days.remove(key) {
            for h in d.docs {
                if let Some((_, mut row, toks)) = self.docs.remove(&h) {
                    self.unpost(h, rev_at(row.at), &toks);
                    row.text.zeroize();
                }
            }
        }
    }

    /// Forgets everything, zeroizing the texts.
    pub fn wipe(&mut self) {
        for (_, (_, row, _)) in self.docs.iter_mut() {
            row.text.zeroize();
            row.author.zeroize();
        }
        self.docs.clear();
        self.days.clear();
        self.post.clear();
    }
}

impl Drop for Hot {
    fn drop(&mut self) {
        self.wipe();
    }
}
