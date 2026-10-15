//! A group's Loro docs: per group one `meta` doc (every record except posts) and one doc per UTC
//! day of posts (`docs/group-shards.md`). Each has one root map `r`, `record id -> SignedRecord
//! bytes` (a Loro binary value): a grow-only set of signed records; Loro gives us version
//! vectors, incremental export and snapshots.
//!
//! Acceptance rule (every device, hosts included, U10): a batch is imported only if EVERY new op
//! is an insert into `r` whose key is the id of its value, whose value is a record of this group
//! that verifies under its author's DID, whose Loro peer id is `writer_id(author, dev)` of that
//! record (U16), and that belongs in this doc: a `Post` only in the day doc of its UTC day
//! (`at` within that day +- 1 h, as in 1:1 chat), every other body only in `meta`. Deletes,
//! other containers, nested containers, a mismatched key, peer or doc: the whole batch is
//! refused (U22: no per-op hiding, every device reaches the same verdict). The verdict depends
//! only on the batch and the doc, never on arrival order or on who relays.
//!
//! What a record *means* (membership, roles, removal, which posts show) is not decided here:
//! `state::derive_meta` reads the meta set and `state::post_visible` judges each post, so the
//! outcome is the same in every delivery order.
//!
//! Signatures are verified when records enter a device (`apply_remote`, `write`); rebuilding a
//! doc from our own sealed store rows (`load`, `entries_trusted`) does not verify them again.

use std::collections::BTreeMap;

use loro::{ExportMode, LoroDoc, LoroValue, VersionVector};
use serde::{Deserialize, Serialize};

use super::record::{Body, Record, RecordError, decode_trusted, open_record, record_id};
use crate::chat::doc::{DAY_MS, HOUR_MS, day_of, day_start, writer_id};

pub(crate) mod batching;

const ROOT: &str = "r";

/// Which of a group's docs: the meta doc, or the posts of one UTC day (`YYYY-MM-DD`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DocId {
    Meta,
    Day(String),
}

impl DocId {
    /// A day doc named by a canonical `YYYY-MM-DD` that exists on the calendar.
    pub fn is_valid(&self) -> bool {
        match self {
            DocId::Meta => true,
            DocId::Day(d) => day_valid(d),
        }
    }

    pub fn day(&self) -> Option<&str> {
        match self {
            DocId::Meta => None,
            DocId::Day(d) => Some(d),
        }
    }
}

pub fn day_valid(day: &str) -> bool {
    day_start(day).is_some_and(|ms| day_of(ms) == day)
}


#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Reject {
    #[error("batch does not decode")]
    Undecodable,
    #[error("batch is not self-contained (pending ops)")]
    Pending,
    #[error("an op that is not a record insert")]
    Structure,
    #[error("a record's key is not its id")]
    WrongKey,
    #[error("record refused: {0}")]
    Record(RecordError),
    #[error("ops under a peer id that is not their author's writer ID")]
    ForeignPeer,
    #[error("a record in a doc it does not belong in")]
    WrongDoc,
    #[error("a post outside the day of its doc")]
    OutOfDay,
    #[error("refused by policy: {0}")]
    Policy(String),
    #[error("group persistence failed: {0}")]
    Persistence(String),
}

/// A verified record and where it sits in the oplog (its writer's peer id and op counter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub rec: Record,
    pub peer: u64,
    pub counter: i32,
}

impl Entry {
    /// Whether `seen` (a signed frontier) includes this record.
    pub fn seen_by(&self, seen: &[(u64, i32)]) -> bool {
        seen_has(seen, self.peer, self.counter)
    }
}

/// Whether `rec` belongs in doc `id` (the rule of the module doc).
fn fits(id: &DocId, rec: &Record) -> Result<(), Reject> {
    let post = matches!(rec.body, Body::Post { .. });
    match id {
        DocId::Meta if post => Err(Reject::WrongDoc),
        DocId::Meta => Ok(()),
        DocId::Day(_) if !post => Err(Reject::WrongDoc),
        DocId::Day(d) => {
            let lo = day_start(d).ok_or(Reject::OutOfDay)? - HOUR_MS;
            if rec.at >= lo && rec.at < lo + DAY_MS + 2 * HOUR_MS { Ok(()) } else { Err(Reject::OutOfDay) }
        }
    }
}

/// Whether the frontier `seen` (sorted `(peer, counter)`) includes the op `(peer, counter)`.
pub fn seen_has(seen: &[(u64, i32)], peer: u64, counter: i32) -> bool {
    seen.binary_search_by_key(&peer, |(p, _)| *p).map(|i| seen[i].1 > counter).unwrap_or(false)
}

pub struct GroupDoc {
    pub group: String,
    id: DocId,
    doc: LoroDoc,
    /// An `apply_remote_inplace` failed after importing: the doc holds ops nobody accepted and
    /// must be dropped and reloaded from the store.
    tainted: bool,
}

impl GroupDoc {
    /// An empty meta doc that writes under `peer` (our writer ID).
    pub fn new(group: &str, peer: u64) -> Self {
        Self::new_in(group, DocId::Meta, peer)
    }

    pub fn new_in(group: &str, id: DocId, peer: u64) -> Self {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer).expect("peer id");
        Self { group: group.to_string(), id, doc, tainted: false }
    }

    /// Rebuilds the meta doc from stored, already-validated bytes.
    pub fn load(group: &str, peer: u64, snapshot: Option<&[u8]>, updates: &[Vec<u8>]) -> Result<Self, Reject> {
        Self::load_in(group, DocId::Meta, peer, snapshot, updates)
    }

    pub fn load_in(group: &str, id: DocId, peer: u64, snapshot: Option<&[u8]>, updates: &[Vec<u8>]) -> Result<Self, Reject> {
        let s = Self::new_in(group, id, peer);
        if let Some(b) = snapshot {
            s.doc.import(b).map_err(|_| Reject::Undecodable)?;
        }
        for u in updates {
            s.doc.import(u).map_err(|_| Reject::Undecodable)?;
        }
        s.doc.set_peer_id(peer).expect("peer id");
        Ok(s)
    }

    pub fn peer(&self) -> u64 {
        self.doc.peer_id()
    }

    pub fn vv(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    /// Our frontier as a record's `seen`: sorted `(peer, counter)`.
    pub fn seen(&self) -> Vec<(u64, i32)> {
        let mut v: Vec<(u64, i32)> = self.vv().iter().map(|(p, c)| (*p, *c)).filter(|(_, c)| *c > 0).collect();
        v.sort();
        v
    }

    pub fn snapshot(&self) -> Vec<u8> {
        self.doc.export(ExportMode::Snapshot).expect("snapshot export")
    }

    /// Independent rollback checkpoint for the small meta doc. Unlike cloning a
    /// Loro handle, a fork does not advance when the original imports new ops.
    /// Preserve this install's writer ID; the fork's generated ID must not escape.
    pub(crate) fn checkpoint(&self) -> Self {
        let doc = self.doc.fork();
        doc.set_peer_id(self.peer()).expect("peer id");
        Self { group: self.group.clone(), id: self.id.clone(), doc, tainted: self.tainted }
    }

    /// Every op beyond `theirs`; `None` if they hold all we do.
    pub fn export_since(&self, theirs: &VersionVector) -> Option<Vec<u8>> {
        if theirs.partial_cmp(&self.vv()).is_some_and(|o| o != std::cmp::Ordering::Less) {
            return None;
        }
        self.doc.export(ExportMode::updates(theirs)).ok()
    }

    /// Every op in the causal past of the ops at `ids` (each `(peer, counter)`) that `theirs`
    /// lacks: what a removed member may still get (everything up to its removal, nothing after).
    pub fn export_until(&self, ids: &[(u64, i32)], theirs: &VersionVector) -> Option<Vec<u8>> {
        let mut vv = VersionVector::default();
        for (p, c) in ids {
            vv.merge(&self.doc.frontiers_to_vv(&loro::Frontiers::from_id(loro::ID::new(*p, *c)))?);
        }
        let fork = self.doc.fork_at(&self.doc.vv_to_frontiers(&vv)).ok()?;
        if theirs.partial_cmp(&fork.oplog_vv()).is_some_and(|o| o != std::cmp::Ordering::Less) {
            return None;
        }
        fork.export(ExportMode::updates(theirs)).ok()
    }

    /// Only ops written under `peer` (our own device) beyond `theirs`.
    pub fn export_peer_since(&self, peer: u64, theirs: &VersionVector) -> Option<Vec<u8>> {
        let mine = self.vv();
        if theirs.get(&peer).copied().unwrap_or(0) >= mine.get(&peer).copied().unwrap_or(0) {
            return None;
        }
        let mut from = theirs.clone();
        for (p, c) in mine.iter() {
            if *p != peer {
                from.insert(*p, *c);
            }
        }
        self.doc.export(ExportMode::updates(&from)).ok()
    }

    /// Writes `rec` signed by `account` as one insert under our writer ID (which must be
    /// `writer_id(rec.author, rec.dev)`). Returns the entry and the update to persist and push.
    pub fn write(&self, account: &identity::Identity, rec: &Record) -> Result<(Entry, Vec<u8>), Reject> {
        if writer_id(&rec.author, &rec.dev) != self.peer() || rec.group != self.group {
            return Err(Reject::ForeignPeer);
        }
        fits(&self.id, rec)?;
        let bytes = rec.sign(account).to_bytes();
        // What we write must pass what everyone else checks.
        open_record(&bytes, &self.group).map_err(Reject::Record)?;
        let id = record_id(&bytes);
        let before = self.vv();
        let counter = before.get(&self.peer()).copied().unwrap_or(0);
        self.doc.get_map(ROOT).insert(&id, LoroValue::Binary(bytes.into())).map_err(|_| Reject::Structure)?;
        self.doc.commit();
        let update = self.doc.export(ExportMode::updates(&before)).map_err(|_| Reject::Undecodable)?;
        Ok((Entry { id, rec: rec.clone(), peer: self.peer(), counter }, update))
    }

    /// Validates `update` (see the module doc) and, if every new op is acceptable, imports it.
    /// `admit` may refuse the batch on its new entries (a host's per-connection policy). Returns
    /// the new entries (records seen for the first time). On any `Err` nothing was imported.
    pub fn apply_remote(&mut self, update: &[u8], admit: impl FnOnce(&[Entry]) -> Result<(), String>) -> Result<Vec<Entry>, Reject> {
        LoroDoc::decode_import_blob_meta(update, true).map_err(|_| Reject::Undecodable)?;
        let old_vv = self.vv();
        let scratch = self.doc.fork();
        let status = scratch.import(update).map_err(|_| Reject::Undecodable)?;
        if status.pending.is_some() {
            return Err(Reject::Pending);
        }
        let new_vv = scratch.oplog_vv();
        if !new_vv.iter().any(|(p, c)| *c > old_vv.get(p).copied().unwrap_or(0)) {
            return Ok(Vec::new());
        }
        let new = ops_between(&scratch, &old_vv, &new_vv, &self.group, &self.id, true)?;
        let root = self.doc.get_map(ROOT);
        let mut fresh: Vec<Entry> = Vec::new();
        for e in new {
            if root.get(&e.id).is_none() && !fresh.iter().any(|f| f.id == e.id) {
                fresh.push(e);
            }
        }
        admit(&fresh).map_err(Reject::Policy)?;
        self.doc.import(update).map_err(|_| Reject::Undecodable)?;
        Ok(fresh)
    }

    /// `apply_remote` for the big docs (a day of posts): imports into the doc itself instead of a
    /// scratch fork (a fork copies the whole doc, milliseconds a post in a busy day) and checks the
    /// new ops afterwards. A refused batch leaves the doc `tainted`: the caller must drop it and
    /// reload it from the store, which holds only what was accepted. Returns the new records
    /// (a record already held under another op may be among them: the caller skips those).
    pub fn apply_remote_inplace(&mut self, update: &[u8], admit: impl FnOnce(&[Entry]) -> Result<(), String>) -> Result<Vec<Entry>, Reject> {
        LoroDoc::decode_import_blob_meta(update, true).map_err(|_| Reject::Undecodable)?;
        let old_vv = self.vv();
        self.tainted = true;
        let status = self.doc.import(update).map_err(|_| Reject::Undecodable)?;
        if status.pending.is_some() {
            return Err(Reject::Pending);
        }
        let new_vv = self.vv();
        if !new_vv.iter().any(|(p, c)| *c > old_vv.get(p).copied().unwrap_or(0)) {
            self.tainted = false;
            return Ok(Vec::new());
        }
        let mut fresh: Vec<Entry> = Vec::new();
        for e in ops_between(&self.doc, &old_vv, &new_vv, &self.group, &self.id, true)? {
            if !fresh.iter().any(|f| f.id == e.id) {
                fresh.push(e);
            }
        }
        admit(&fresh).map_err(Reject::Policy)?;
        self.tainted = false;
        Ok(fresh)
    }

    pub fn tainted(&self) -> bool {
        self.tainted
    }

    /// Test hook (`synth`): imports `update` unchecked, to merge batches written by several
    /// member docs into one before it goes through `apply_remote`.
    #[cfg(any(test, feature = "dev-tools"))]
    #[doc(hidden)]
    pub(crate) fn import_for_test(&self, update: &[u8]) {
        self.doc.import(update).expect("import");
    }

    /// The stored bytes of record `id`.
    pub fn raw_value(&self, id: &str) -> Option<Vec<u8>> {
        match self.doc.get_map(ROOT).get(id) {
            Some(loro::ValueOrContainer::Value(LoroValue::Binary(b))) => Some(b.to_vec()),
            _ => None,
        }
    }

    /// Every record in the doc with its first position (all were validated on the way in).
    #[cfg(test)]
    pub fn entries(&self) -> Result<Vec<Entry>, Reject> {
        self.read_entries(true)
    }

    /// `entries` for a doc rebuilt from our own sealed store rows: the records were verified
    /// when they entered the store and the row is bound to its path by the vault's AEAD, so
    /// signatures are not checked again (verify once, on arrival).
    pub fn entries_trusted(&self) -> Result<Vec<Entry>, Reject> {
        self.read_entries(false)
    }

    fn read_entries(&self, verify: bool) -> Result<Vec<Entry>, Reject> {
        let all = ops_between(&self.doc, &VersionVector::default(), &self.vv(), &self.group, &self.id, verify)?;
        let mut first: BTreeMap<String, Entry> = BTreeMap::new();
        for e in all {
            first.entry(e.id.clone()).or_insert(e);
        }
        Ok(first.into_values().collect())
    }

    /// Test hook: inserts raw `value` under `key` with our peer id, unchecked (a forgery).
    #[cfg(test)]
    pub(crate) fn raw_insert(&self, key: &str, value: Vec<u8>) -> Vec<u8> {
        let before = self.vv();
        self.doc.get_map(ROOT).insert(key, LoroValue::Binary(value.into())).unwrap();
        self.doc.commit();
        self.doc.export(ExportMode::updates(&before)).unwrap()
    }

    #[cfg(test)]
    pub(crate) fn raw_delete(&self, key: &str) -> Vec<u8> {
        let before = self.vv();
        self.doc.get_map(ROOT).delete(key).unwrap();
        self.doc.commit();
        self.doc.export(ExportMode::updates(&before)).unwrap()
    }

    #[cfg(test)]
    pub(crate) fn raw_other(&self) -> Vec<u8> {
        let before = self.vv();
        self.doc.get_map("other").insert("x", 1).unwrap();
        self.doc.commit();
        self.doc.export(ExportMode::updates(&before)).unwrap()
    }

    #[cfg(test)]
    pub(crate) fn set_peer(&self, peer: u64) {
        self.doc.set_peer_id(peer).unwrap();
    }
}

/// Every op of `doc` between `from` and `to`, each checked to be a valid record insert for doc
/// `id` (`verify`: including the key, the writer ID and the author's signature).
fn ops_between(
    doc: &LoroDoc,
    from: &VersionVector,
    to: &VersionVector,
    group: &str,
    id: &DocId,
    verify: bool,
) -> Result<Vec<Entry>, Reject> {
    crate::signed_map::inserts(
        doc,
        from,
        to,
        ROOT,
        Reject::Structure,
        |key, bytes, peer, counter| {
            let rec = if verify {
                if record_id(bytes) != key {
                    return Err(Reject::WrongKey);
                }
                let rec = open_record(bytes, group).map_err(Reject::Record)?;
                if writer_id(&rec.author, &rec.dev) != peer {
                    return Err(Reject::ForeignPeer);
                }
                rec
            } else {
                decode_trusted(bytes, group).map_err(Reject::Record)?
            };
            fits(id, &rec)?;
            Ok(Entry {
                id: key.to_string(),
                rec,
                peer,
                counter,
            })
        },
    )
}

#[cfg(test)]
mod tests;
