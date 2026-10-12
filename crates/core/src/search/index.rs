//! The index file (`search.redb`), as typed accessors over `Sealed` (layout in `docs/search.md`).
//!
//! ```text
//! d/{h}                         DocRow  sealed: src, conv, day, id, at, author, text
//! s/{src}/{h}                   empty   the docs of a conversation (for removal, group diffs)
//! t/{tok}/{rev_at}/{h}          empty   a posting: doc {h} contains the indexed term {tok}
//! w/{src}/{day}                 bytes   sealed: the version vector indexed (watermark)
//! v                             u32 BE  sealed: index format version (mismatch -> rebuild)
//! ```
//!
//! `h` = 16 hex of a keyed hash of `src/id`; `tok` = 32 hex of a keyed hash of the term. Both use
//! `index_key`, not the sealing key, and neither can be reversed without it. Writes go through
//! one lock, so a doc's old postings are always the ones its stored row implies.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use storage::{Op, Sealed, StorageError, Store};
use zeroize::Zeroizing;

use super::text::index_terms;
use crate::Error;

/// Bump when the tokenizer or the layout changes: the index is rebuilt from the chat and group
/// stores.
pub const VERSION: u32 = 1;
const STORE_DOMAIN: &str = "tinline/search-store/v1";
const INDEX_DOMAIN: &str = "tinline/search-index/v1";

pub(crate) fn io(e: StorageError) -> Error {
    Error::Io(e.to_string())
}

fn norm_phrase(phrase: &str) -> Zeroizing<String> {
    Zeroizing::new(phrase.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>().join(" "))
}

/// One indexed message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocRow {
    /// `dm.{pair}` or `g.{gid}`.
    pub src: String,
    /// The contact's DID (DMs) or the group id.
    pub conv: String,
    /// The DM shard day; `-` for groups.
    pub day: String,
    pub id: String,
    pub at: i64,
    pub author: String,
    pub text: String,
}

/// "Set the watermark of `src/day` to `vv`" — if the stored one equals `prev` (`None`: always).
/// A hook that only indexed one change must not mark a shard whose older changes are missing.
pub struct Mark {
    pub src: String,
    pub day: String,
    pub prev: Option<Vec<u8>>,
    pub vv: Vec<u8>,
}

fn vv(b: &[u8]) -> Option<loro::VersionVector> {
    if b.is_empty() {
        return Some(loro::VersionVector::default());
    }
    loro::VersionVector::decode(b).ok()
}

/// Whether two encoded version vectors are the same (encoding order is not stable).
pub fn vv_eq(a: &[u8], b: &[u8]) -> bool {
    matches!((vv(a), vv(b)), (Some(x), Some(y)) if x == y)
}

/// One page of postings: each with its cursor and its row (`None`: stale), and whether there
/// may be more.
pub type Page = (Vec<(String, Option<DocRow>)>, bool);

pub struct SearchIndex {
    db: Sealed,
    index_key: Zeroizing<[u8; 32]>,
    write: Mutex<()>,
}

impl SearchIndex {
    /// Opens (creating) the index; a file of another format version is emptied (a rebuild).
    pub fn open(path: &Path, phrase: &str) -> Result<Self, Error> {
        let store = crate::store::open_waiting(path)?;
        crate::store::set_private(path);
        Self::with_store(store, phrase)
    }

    #[cfg(test)]
    pub fn in_memory(phrase: &str) -> Self {
        Self::with_store(Store::open_in_memory().unwrap(), phrase).unwrap()
    }

    fn with_store(store: Store, phrase: &str) -> Result<Self, Error> {
        let p = norm_phrase(phrase);
        let me = Self {
            db: Sealed::new(store, blake3::derive_key(STORE_DOMAIN, p.as_bytes())),
            index_key: Zeroizing::new(blake3::derive_key(INDEX_DOMAIN, p.as_bytes())),
            write: Mutex::new(()),
        };
        let current = matches!(me.db.get("v"), Ok(Some(v)) if v == VERSION.to_be_bytes());
        if !current {
            me.db.apply(&[Op::DeletePrefix(String::new()), me.db.put_op("v", &VERSION.to_be_bytes()).map_err(io)?]).map_err(io)?;
        }
        Ok(me)
    }

    /// Lets the file go for every holder (stop of the account's stores).
    pub fn close(&self) {
        self.db.close();
    }

    pub fn is_closed(&self) -> bool {
        self.db.raw().is_closed()
    }

    pub fn doc_key(&self, src: &str, id: &str) -> String {
        let mut h = blake3::Hasher::new_keyed(&self.index_key);
        h.update(b"doc\0");
        h.update(src.as_bytes());
        h.update(b"/");
        h.update(id.as_bytes());
        hex::encode(&h.finalize().as_bytes()[..8])
    }

    /// A keyed tag of a day, kept with each doc key so the docs of one day can be listed
    /// (`day_keys`) without opening any row.
    fn day_tag(&self, day: &str) -> Vec<u8> {
        let mut h = blake3::Hasher::new_keyed(&self.index_key);
        h.update(b"day\0");
        h.update(day.as_bytes());
        h.finalize().as_bytes()[..8].to_vec()
    }

    pub fn tok(&self, term: &str) -> String {
        let mut h = blake3::Hasher::new_keyed(&self.index_key);
        h.update(b"term\0");
        h.update(term.as_bytes());
        hex::encode(&h.finalize().as_bytes()[..16])
    }

    fn rev_at(at: i64) -> String {
        format!("{:016x}", u64::MAX - at.max(0) as u64)
    }

    pub fn row(&self, h: &str) -> Result<Option<DocRow>, Error> {
        match self.db.get(&format!("d/{h}")).map_err(io)? {
            Some(b) => Ok(bincode::deserialize(&b).ok()),
            None => Ok(None),
        }
    }

    fn postings(&self, row: &DocRow, h: &str) -> Vec<String> {
        let rev = Self::rev_at(row.at);
        index_terms(&row.text).iter().map(|t| format!("t/{}/{rev}/{h}", self.tok(t))).collect()
    }

    fn unindex(&self, old: &DocRow, h: &str, ops: &mut Vec<Op>) {
        ops.extend(self.postings(old, h).into_iter().map(Op::Delete));
        ops.push(Op::Delete(format!("d/{h}")));
        ops.push(Op::Delete(format!("s/{}/{h}", old.src)));
    }

    /// Indexes `put` (a row without words is removed instead), removes the docs `drop` (keys
    /// from `doc_key`) and applies the watermarks, in one transaction. Idempotent.
    pub fn apply(&self, put: &[DocRow], drop: &[String], marks: &[Mark]) -> Result<(), Error> {
        let _w = self.write.lock();
        let mut ops = Vec::new();
        let mut rows: BTreeMap<String, &DocRow> = BTreeMap::new();
        for r in put {
            rows.insert(self.doc_key(&r.src, &r.id), r);
        }
        for (h, r) in &rows {
            // An unreadable old row cannot tell its postings: they stay, and queries skip them.
            let old = self.row(h).unwrap_or(None);
            if old.as_ref() == Some(*r) {
                continue;
            }
            if let Some(o) = &old {
                self.unindex(o, h, &mut ops);
            }
            let posts = self.postings(r, h);
            if posts.is_empty() {
                continue;
            }
            ops.push(self.db.put_op(&format!("d/{h}"), &bincode::serialize(*r).expect("serialises")).map_err(io)?);
            ops.push(Op::Put(format!("s/{}/{h}", r.src), self.day_tag(&r.day)));
            ops.extend(posts.into_iter().map(|k| Op::Put(k, Vec::new())));
        }
        for h in drop.iter().filter(|h| !rows.contains_key(*h)) {
            if let Some(o) = self.row(h).unwrap_or(None) {
                self.unindex(&o, h, &mut ops);
            }
        }
        for m in marks {
            let key = format!("w/{}/{}", m.src, m.day);
            let ok = match &m.prev {
                None => true,
                Some(p) => vv_eq(&self.db.get(&key).ok().flatten().unwrap_or_default(), p),
            };
            if ok {
                ops.push(self.db.put_op(&key, &m.vv).map_err(io)?);
            }
        }
        if ops.is_empty() {
            return Ok(());
        }
        self.db.apply(&ops).map_err(io)
    }

    /// Removes everything of a conversation: its docs, their postings, its watermarks.
    pub fn forget(&self, src: &str) -> Result<(), Error> {
        let hs = self.doc_keys(src)?;
        let _w = self.write.lock();
        let mut ops = Vec::new();
        for h in &hs {
            if let Some(o) = self.row(h).unwrap_or(None) {
                self.unindex(&o, h, &mut ops);
            }
        }
        ops.push(Op::DeletePrefix(format!("s/{src}/")));
        ops.push(Op::DeletePrefix(format!("w/{src}/")));
        self.db.apply(&ops).map_err(io)
    }

    /// The doc keys of a conversation.
    pub fn doc_keys(&self, src: &str) -> Result<HashSet<String>, Error> {
        let p = format!("s/{src}/");
        Ok(self.db.list_prefixed(&p).map_err(io)?.into_iter().filter_map(|k| k.strip_prefix(&p).map(str::to_string)).collect())
    }

    /// The doc keys of a conversation indexed under `day`.
    pub fn day_keys(&self, src: &str, day: &str) -> Result<HashSet<String>, Error> {
        let p = format!("s/{src}/");
        let tag = self.day_tag(day);
        Ok(self.db.raw().scan(&p).map_err(io)?.into_iter().filter(|(_, v)| *v == tag).filter_map(|(k, _)| k.strip_prefix(&p).map(str::to_string)).collect())
    }

    pub fn watermark(&self, src: &str, day: &str) -> Option<Vec<u8>> {
        self.db.get(&format!("w/{src}/{day}")).ok().flatten()
    }

    /// Up to `n` postings of `term`, newest first, after the cursor `after` (`rev_at/h`), each
    /// with its cursor and its row (`None` for a stale posting: the row is gone or moved), and
    /// whether there may be more.
    pub fn page(&self, term: &str, after: Option<&str>, n: usize) -> Result<Page, Error> {
        let prefix = format!("t/{}/", self.tok(term));
        let from = after.map(|a| format!("{prefix}{a}"));
        let keys = self.db.keys_after(&prefix, from.as_deref(), n).map_err(io)?;
        let more = keys.len() == n;
        let out = keys
            .into_iter()
            .map(|k| {
                let cur = k[prefix.len()..].to_string();
                let row = cur.split_once('/').and_then(|(rev, h)| self.row(h).ok().flatten().filter(|r| Self::rev_at(r.at) == rev));
                (cur, row)
            })
            .collect();
        Ok((out, more))
    }

    #[cfg(test)]
    pub fn raw_rows(&self) -> Vec<(String, Vec<u8>)> {
        self.db.raw().scan("").unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(src: &str, id: &str, at: i64, text: &str) -> DocRow {
        DocRow { src: src.into(), conv: "did:key:z".into(), day: "2026-10-11".into(), id: id.into(), at, author: "did:key:a".into(), text: text.into() }
    }

    fn ids(ix: &SearchIndex, term: &str) -> Vec<String> {
        ix.page(term, None, 100).unwrap().0.into_iter().filter_map(|(_, r)| r).map(|r| r.id).collect()
    }

    // KIND: unit (S1) — postings are newest first; an edit moves them, a delete removes them.
    #[test]
    fn edit_and_delete_reindex() {
        let ix = SearchIndex::in_memory("alpha beta");
        ix.apply(&[row("dm.p", "1", 10, "hello there"), row("dm.p", "2", 20, "help me")], &[], &[]).unwrap();
        assert_eq!(ids(&ix, "hel"), ["2", "1"]);
        ix.apply(&[row("dm.p", "1", 10, "goodbye")], &[], &[]).unwrap();
        assert_eq!(ids(&ix, "hel"), ["2"]);
        assert_eq!(ids(&ix, "go"), ["1"]);
        ix.apply(&[row("dm.p", "2", 20, "")], &[], &[]).unwrap();
        assert!(ids(&ix, "hel").is_empty());
        ix.apply(&[], &[ix.doc_key("dm.p", "1")], &[]).unwrap();
        assert!(ids(&ix, "go").is_empty());
        let left: Vec<String> = ix.raw_rows().into_iter().map(|(k, _)| k).filter(|k| k != "v").collect();
        assert!(left.is_empty(), "nothing left behind: {left:?}");
    }

    // KIND: unit (S1) — forgetting a conversation removes its docs, postings and watermarks only.
    #[test]
    fn forget_removes_one_conversation() {
        let ix = SearchIndex::in_memory("alpha beta");
        let mark = |src: &str| Mark { src: src.into(), day: "-".into(), prev: None, vv: Vec::new() };
        ix.apply(&[row("g.a", "1", 1, "picnic plans"), row("g.b", "2", 2, "picnic too")], &[], &[mark("g.a"), mark("g.b")]).unwrap();
        ix.forget("g.a").unwrap();
        assert_eq!(ids(&ix, "pic"), ["2"]);
        assert!(ix.watermark("g.a", "-").is_none() && ix.watermark("g.b", "-").is_some());
        assert!(ix.doc_keys("g.a").unwrap().is_empty());
    }

    // KIND: unit (S1) — a conditional watermark only moves from the version it expects.
    #[test]
    fn conditional_marks() {
        let ix = SearchIndex::in_memory("alpha beta");
        let mut v1 = loro::VersionVector::default();
        v1.insert(7, 3);
        let mut v2 = v1.clone();
        v2.insert(7, 5);
        let m = |prev: Option<&loro::VersionVector>, vv: &loro::VersionVector| Mark { src: "dm.p".into(), day: "d".into(), prev: prev.map(|p| p.encode()), vv: vv.encode() };
        ix.apply(&[], &[], &[m(Some(&loro::VersionVector::default()), &v1)]).unwrap();
        assert!(vv_eq(&ix.watermark("dm.p", "d").unwrap(), &v1.encode()));
        // A change that assumed some other version leaves it alone.
        ix.apply(&[], &[], &[m(Some(&v2), &v2)]).unwrap();
        assert!(vv_eq(&ix.watermark("dm.p", "d").unwrap(), &v1.encode()));
        ix.apply(&[], &[], &[m(Some(&v1), &v2)]).unwrap();
        assert!(vv_eq(&ix.watermark("dm.p", "d").unwrap(), &v2.encode()));
    }

    // KIND: unit (S1) — paging by cursor walks every posting once, newest first.
    #[test]
    fn paging_newest_first() {
        let ix = SearchIndex::in_memory("alpha beta");
        let rows: Vec<DocRow> = (0..25).map(|i| row("dm.p", &format!("m{i:02}"), 1000 + i, "lunch today")).collect();
        ix.apply(&rows, &[], &[]).unwrap();
        let mut seen = Vec::new();
        let mut cur: Option<String> = None;
        loop {
            let (page, more) = ix.page("lunch", cur.as_deref(), 10).unwrap();
            seen.extend(page.iter().map(|(_, r)| r.as_ref().unwrap().at));
            cur = page.last().map(|(c, _)| c.clone());
            if !more {
                break;
            }
        }
        let want: Vec<i64> = (0..25).rev().map(|i| 1000 + i).collect();
        assert_eq!(seen, want);
    }

    // KIND: unit (S1) — at rest the file holds no words, names or authors: values are sealed and
    // keys carry only keyed hashes. Another phrase reads nothing.
    #[test]
    fn sealed_at_rest() {
        let ix = SearchIndex::in_memory("alpha beta");
        let mut r = row("dm.p", "1", 10, "confidential rendezvous");
        r.author = "did:key:zAuthorSecret".into();
        ix.apply(&[r], &[], &[]).unwrap();
        for (k, v) in ix.raw_rows() {
            for word in ["confidential", "rendezvous", "conf", "AuthorSecret"] {
                assert!(!k.contains(word), "key {k} leaks {word}");
                assert!(!v.windows(word.len()).any(|w| w == word.as_bytes()), "value of {k} leaks {word}");
            }
        }
        let other = SearchIndex { db: Sealed::new(ix.db.raw().clone(), [9; 32]), index_key: Zeroizing::new([9; 32]), write: Mutex::new(()) };
        assert!(other.page("conf", None, 10).unwrap().0.iter().all(|(_, r)| r.is_none()));
        assert!(ix.db.raw().get("v").unwrap().is_some());
    }

    // KIND: unit (S1) — a file of another format version is emptied on open (rebuild).
    #[test]
    fn version_mismatch_empties() {
        let ix = SearchIndex::in_memory("alpha beta");
        ix.apply(&[row("dm.p", "1", 10, "hello")], &[], &[]).unwrap();
        let raw = ix.db.raw().clone();
        ix.db.put("v", &(VERSION + 1).to_be_bytes()).unwrap();
        let again = SearchIndex::with_store(raw, "alpha beta").unwrap();
        assert!(ids(&again, "hel").is_empty());
        assert_eq!(again.raw_rows().len(), 1, "only the version row");
    }
}
