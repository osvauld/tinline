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

use super::record::{Body, Record, RecordError, SignedRecord, record_id};
use crate::chat::doc::{DAY_MS, HOUR_MS, day_of, day_start, writer_id};

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
        SignedRecord::open(&bytes, &self.group).map_err(Reject::Record)?;
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
fn ops_between(doc: &LoroDoc, from: &VersionVector, to: &VersionVector, group: &str, id: &DocId, verify: bool) -> Result<Vec<Entry>, Reject> {
    let root = loro::ContainerID::new_root(ROOT, loro::ContainerType::Map);
    let mut out = Vec::new();
    for c in doc.export_json_updates_without_peer_compression(from, to).changes {
        for op in c.ops {
            if op.container != root {
                return Err(Reject::Structure);
            }
            let loro::JsonOpContent::Map(loro::JsonMapOp::Insert { key, value }) = &op.content else { return Err(Reject::Structure) };
            let LoroValue::Binary(bytes) = value else { return Err(Reject::Structure) };
            let rec = if verify {
                if record_id(bytes) != *key {
                    return Err(Reject::WrongKey);
                }
                let rec = SignedRecord::open(bytes, group).map_err(Reject::Record)?;
                if writer_id(&rec.author, &rec.dev) != c.id.peer {
                    return Err(Reject::ForeignPeer);
                }
                rec
            } else {
                SignedRecord::decode_trusted(bytes, group).map_err(Reject::Record)?
            };
            fits(id, &rec)?;
            out.push(Entry { id: key.clone(), rec, peer: c.id.peer, counter: op.counter });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::record::*;
    use super::*;

    fn acct() -> identity::Identity {
        identity::generate().0
    }

    /// The day doc the tests' posts (`at: 1`) belong in.
    fn day() -> DocId {
        DocId::Day("1970-01-01".into())
    }

    fn dd(group: &str, peer: u64) -> GroupDoc {
        GroupDoc::new_in(group, day(), peer)
    }

    fn post(a: &identity::Identity, dev: [u8; 32], doc: &GroupDoc, text: &str) -> Record {
        Record { v: 1, group: doc.group.clone(), author: a.did().into(), dev, at: 1, seen: doc.seen(), body: Body::Post { text: text.into(), reply_to: None } }
    }

    fn ok(_: &[Entry]) -> Result<(), String> {
        Ok(())
    }

    // KIND: unit (G1 stage 1)
    #[test]
    fn a_signed_record_round_trips_between_docs() {
        let a = acct();
        let dev = [1; 32];
        let da = dd("g", writer_id(a.did(), &dev));
        let (e, up) = da.write(&a, &post(&a, dev, &da, "hello")).unwrap();
        let b = acct();
        let mut db = dd("g", writer_id(b.did(), &[2; 32]));
        let got = db.apply_remote(&up, ok).unwrap();
        assert_eq!(got, vec![e.clone()]);
        assert_eq!(db.entries().unwrap(), vec![e]);
        // Applying again: nothing new.
        assert!(db.apply_remote(&up, ok).unwrap().is_empty());
        // Reload from snapshot.
        let re = GroupDoc::load_in("g", day(), 9, Some(&db.snapshot()), &[]).unwrap();
        assert_eq!(re.entries().unwrap().len(), 1);
    }

    // KIND: unit (G1 stage 1) — U10/U16: forged, tampered and foreign-peer batches are refused whole.
    #[test]
    fn forged_tampered_and_foreign_batches_are_refused() {
        let a = acct();
        let m = acct();
        let dev = [1; 32];
        let mut target = dd("g", 77);

        // Signed by a, claims to be m: bad signature.
        let mallory = dd("g", writer_id(a.did(), &dev));
        let mut r = post(&a, dev, &mallory, "x");
        r.author = m.did().into();
        let body = bincode::serialize(&r).unwrap();
        let fake = SignedRecord { body, sig: a.sign(b"whatever").to_vec() }.to_bytes();
        let up = mallory.raw_insert(&record_id(&fake), fake);
        assert!(matches!(target.apply_remote(&up, ok), Err(Reject::Record(RecordError::BadSignature))));

        // A genuine record of m's replayed under another peer id: foreign peer.
        let dm = dd("g", writer_id(m.did(), &dev));
        let (_, _) = dm.write(&m, &post(&m, dev, &dm, "mine")).unwrap();
        let genuine = dm.entries().unwrap().remove(0);
        let bytes = genuine.rec.sign(&m).to_bytes();
        let copier = dd("g", writer_id(a.did(), &dev));
        let up = copier.raw_insert(&record_id(&bytes), bytes.clone());
        assert_eq!(target.apply_remote(&up, ok), Err(Reject::ForeignPeer));

        // Key that is not the id.
        let liar = dd("g", writer_id(m.did(), &dev));
        let up = liar.raw_insert("not-the-id", bytes.clone());
        assert_eq!(target.apply_remote(&up, ok), Err(Reject::WrongKey));

        // Another group's record.
        let other = dd("h", writer_id(m.did(), &dev));
        let (_, up) = other.write(&m, &post(&m, dev, &other, "elsewhere")).unwrap();
        assert!(matches!(target.apply_remote(&up, ok), Err(Reject::Record(RecordError::WrongGroup))));

        // Deletes and other containers.
        let mut t2 = dd("g", 78);
        t2.apply_remote(&dm.export_since(&VersionVector::default()).unwrap(), ok).unwrap();
        let del = dm.raw_delete(&genuine.id);
        assert_eq!(t2.apply_remote(&del, ok), Err(Reject::Structure));
        assert_eq!(t2.entries().unwrap().len(), 1);
        let side = dd("g", writer_id(m.did(), &[3; 32]));
        let up = side.raw_other();
        assert_eq!(target.apply_remote(&up, ok), Err(Reject::Structure));

        // A good record and a bad one in one batch: nothing is imported.
        let mixed = dd("g", writer_id(m.did(), &[4; 32]));
        mixed.write(&m, &post(&m, [4; 32], &mixed, "good")).unwrap();
        let before = target.vv();
        mixed.raw_insert("junk", vec![1, 2, 3]);
        assert!(target.apply_remote(&mixed.export_since(&VersionVector::default()).unwrap(), ok).is_err());
        assert_eq!(target.vv(), before);
        assert!(target.entries().unwrap().is_empty());
    }

    // KIND: unit (G1 stage 1) — the policy hook refuses a batch without importing it.
    #[test]
    fn a_policy_refusal_imports_nothing() {
        let m = acct();
        let dm = dd("g", writer_id(m.did(), &[1; 32]));
        let (_, up) = dm.write(&m, &post(&m, [1; 32], &dm, "x")).unwrap();
        let mut t = dd("g", 5);
        assert!(matches!(t.apply_remote(&up, |_| Err("removed".into())), Err(Reject::Policy(_))));
        assert!(t.entries().unwrap().is_empty());
    }

    // KIND: unit (G1 stage 1) — a writer cannot write under a writer ID that is not its own.
    #[test]
    fn writing_under_another_writer_id_is_refused_locally() {
        let a = acct();
        let d = dd("g", writer_id(a.did(), &[1; 32]));
        let r = post(&a, [2; 32], &d, "x");
        assert_eq!(d.write(&a, &r).unwrap_err(), Reject::ForeignPeer);
        d.set_peer(writer_id(a.did(), &[2; 32]));
        assert!(d.write(&a, &r).is_ok());
    }

    // KIND: unit (group shards) — a post belongs only in the day doc of its day (+- 1 h); every
    // other record only in meta; a wrong doc refuses the whole batch.
    #[test]
    fn records_are_accepted_only_in_their_doc() {
        let a = acct();
        let dev = [1; 32];
        let peer = writer_id(a.did(), &dev);
        let meta = GroupDoc::new("g", peer);
        let rec = |at: i64, body: Body| Record { v: 1, group: "g".into(), author: a.did().into(), dev, at, seen: vec![], body };
        let p = |at| rec(at, Body::Post { text: "x".into(), reply_to: None });
        let leave = rec(1, Body::Leave);
        // Local writes refuse the wrong doc too.
        assert_eq!(meta.write(&a, &p(1)).unwrap_err(), Reject::WrongDoc);
        let d0 = DocId::Day("2024-05-10".into());
        let t0 = day_start("2024-05-10").unwrap();
        let day0 = GroupDoc::new_in("g", d0.clone(), peer);
        assert_eq!(day0.write(&a, &leave).unwrap_err(), Reject::WrongDoc);
        assert_eq!(day0.write(&a, &p(t0 - 2 * HOUR_MS)).unwrap_err(), Reject::OutOfDay);
        assert_eq!(day0.write(&a, &p(t0 + DAY_MS + HOUR_MS)).unwrap_err(), Reject::OutOfDay);
        // Within the day +- 1 h.
        day0.write(&a, &p(t0 - HOUR_MS)).unwrap();
        day0.write(&a, &p(t0 + 5)).unwrap();
        day0.write(&a, &p(t0 + DAY_MS + HOUR_MS - 1)).unwrap();
        meta.write(&a, &leave).unwrap();

        // On arrival the same verdicts, whole batch.
        let forged_post = GroupDoc::new("g", peer);
        let up = forged_post.raw_insert(&record_id(&p(1).sign(&a).to_bytes()), p(1).sign(&a).to_bytes());
        let mut m2 = GroupDoc::new("g", 90);
        assert_eq!(m2.apply_remote(&up, ok), Err(Reject::WrongDoc));
        let forged_leave = GroupDoc::new_in("g", day(), peer);
        let up = forged_leave.raw_insert(&record_id(&leave.sign(&a).to_bytes()), leave.sign(&a).to_bytes());
        let mut d2 = dd("g", 91);
        assert_eq!(d2.apply_remote(&up, ok), Err(Reject::WrongDoc));
        // A good post for another day in a day doc: out of day.
        let wrong_day = GroupDoc::new_in("g", DocId::Day("2024-05-11".into()), peer);
        let good = p(t0 + DAY_MS + 2 * HOUR_MS).sign(&a).to_bytes();
        let up = wrong_day.raw_insert(&record_id(&good), good);
        let mut d3 = GroupDoc::new_in("g", d0, 92);
        assert_eq!(d3.apply_remote(&up, ok), Err(Reject::OutOfDay));
        assert!(d3.entries().unwrap().is_empty());
        // Day names are canonical calendar days.
        assert!(DocId::Day("2024-02-29".into()).is_valid() && DocId::Meta.is_valid());
        assert!(!DocId::Day("2023-02-29".into()).is_valid() && !DocId::Day("2024-5-1".into()).is_valid() && !DocId::Day("meta".into()).is_valid());
    }

    // KIND: unit (group shards) — our own sealed rows load without checking signatures again.
    #[test]
    fn trusted_entries_read_without_verifying() {
        let a = acct();
        let dev = [1; 32];
        let d = dd("g", writer_id(a.did(), &dev));
        let (e, _) = d.write(&a, &post(&a, dev, &d, "hello")).unwrap();
        let re = GroupDoc::load_in("g", day(), 5, Some(&d.snapshot()), &[]).unwrap();
        assert_eq!(re.entries_trusted().unwrap(), vec![e]);
        // A forged signature slips through the trusted read, never the verifying one.
        let mut r = post(&a, dev, &d, "forged");
        r.at = 2;
        let fake = SignedRecord { body: bincode::serialize(&r).unwrap(), sig: vec![0; 64] }.to_bytes();
        let forger = dd("g", writer_id(a.did(), &dev));
        forger.raw_insert(&record_id(&fake), fake);
        assert_eq!(forger.entries_trusted().unwrap().len(), 1);
        assert!(forger.entries().is_err());
    }

    // KIND: unit (group shards) — the in-place apply of big docs: same verdicts as `apply_remote`;
    // a refused batch leaves the doc tainted (the caller reloads it), a good one does not.
    #[test]
    fn in_place_apply_agrees_and_taints_on_refusal() {
        let a = acct();
        let dev = [1; 32];
        let src = dd("g", writer_id(a.did(), &dev));
        let (e, up) = src.write(&a, &post(&a, dev, &src, "hello")).unwrap();
        let mut t = dd("g", 5);
        assert_eq!(t.apply_remote_inplace(&up, ok).unwrap(), vec![e]);
        assert!(!t.tainted());
        assert!(t.apply_remote_inplace(&up, ok).unwrap().is_empty() && !t.tainted(), "nothing new");
        // Refused by policy, by structure and by record: tainted each time.
        let (_, up2) = src.write(&a, &post(&a, dev, &src, "two")).unwrap();
        assert!(matches!(t.apply_remote_inplace(&up2, |_| Err("no".into())), Err(Reject::Policy(_))));
        assert!(t.tainted());
        let mut u = dd("g", 6);
        let forged = dd("g", writer_id(a.did(), &[9; 32]));
        let up3 = forged.raw_insert("not-the-id", vec![1, 2, 3]);
        assert!(u.apply_remote_inplace(&up3, ok).is_err() && u.tainted());
        let mut m = GroupDoc::new("g", 7);
        assert_eq!(m.apply_remote_inplace(&up, ok), Err(Reject::WrongDoc));
        assert!(m.tainted());
    }
}
