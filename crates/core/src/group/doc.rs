//! The group's Loro doc: one root map `r`, `record id -> SignedRecord bytes` (a Loro binary
//! value). It is a grow-only set of signed records; Loro gives us version vectors, incremental
//! export and snapshots.
//!
//! Acceptance rule (every device, hosts included, U10): a batch is imported only if EVERY new op
//! is an insert into `r` whose key is the id of its value, whose value is a record of this group
//! that verifies under its author's DID, and whose Loro peer id is `writer_id(author, dev)` of
//! that record (U16). Deletes, other containers, nested containers, a mismatched key or peer:
//! the whole batch is refused (U22: no per-op hiding, every device reaches the same verdict).
//! The verdict depends only on the batch and the doc, never on arrival order or on who relays.
//!
//! What a record *means* (membership, roles, removal) is not decided here: `state::derive` reads
//! the whole set, so the outcome is the same in every delivery order.

use std::collections::BTreeMap;

use loro::{ExportMode, LoroDoc, LoroValue, VersionVector};

use super::record::{Record, RecordError, SignedRecord, record_id};
use crate::chat::doc::writer_id;

const ROOT: &str = "r";

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
        seen.binary_search_by_key(&self.peer, |(p, _)| *p).map(|i| seen[i].1 > self.counter).unwrap_or(false)
    }
}

pub struct GroupDoc {
    pub group: String,
    doc: LoroDoc,
}

impl GroupDoc {
    /// An empty doc that writes under `peer` (our writer ID).
    pub fn new(group: &str, peer: u64) -> Self {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer).expect("peer id");
        Self { group: group.to_string(), doc }
    }

    /// Rebuilds from stored, already-validated bytes.
    pub fn load(group: &str, peer: u64, snapshot: Option<&[u8]>, updates: &[Vec<u8>]) -> Result<Self, Reject> {
        let s = Self::new(group, peer);
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
        let new = ops_between(&scratch, &old_vv, &new_vv, &self.group)?;
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

    /// The stored bytes of record `id`.
    pub fn raw_value(&self, id: &str) -> Option<Vec<u8>> {
        match self.doc.get_map(ROOT).get(id) {
            Some(loro::ValueOrContainer::Value(LoroValue::Binary(b))) => Some(b.to_vec()),
            _ => None,
        }
    }

    /// Every record in the doc with its first position (all were validated on the way in).
    pub fn entries(&self) -> Result<Vec<Entry>, Reject> {
        let all = ops_between(&self.doc, &VersionVector::default(), &self.vv(), &self.group)?;
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

/// Every op of `doc` between `from` and `to`, each checked to be a valid record insert.
fn ops_between(doc: &LoroDoc, from: &VersionVector, to: &VersionVector, group: &str) -> Result<Vec<Entry>, Reject> {
    let root = loro::ContainerID::new_root(ROOT, loro::ContainerType::Map);
    let mut out = Vec::new();
    for c in doc.export_json_updates_without_peer_compression(from, to).changes {
        for op in c.ops {
            if op.container != root {
                return Err(Reject::Structure);
            }
            let loro::JsonOpContent::Map(loro::JsonMapOp::Insert { key, value }) = &op.content else { return Err(Reject::Structure) };
            let LoroValue::Binary(bytes) = value else { return Err(Reject::Structure) };
            if record_id(bytes) != *key {
                return Err(Reject::WrongKey);
            }
            let rec = SignedRecord::open(bytes, group).map_err(Reject::Record)?;
            if writer_id(&rec.author, &rec.dev) != c.id.peer {
                return Err(Reject::ForeignPeer);
            }
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
        let da = GroupDoc::new("g", writer_id(a.did(), &dev));
        let (e, up) = da.write(&a, &post(&a, dev, &da, "hello")).unwrap();
        let b = acct();
        let mut db = GroupDoc::new("g", writer_id(b.did(), &[2; 32]));
        let got = db.apply_remote(&up, ok).unwrap();
        assert_eq!(got, vec![e.clone()]);
        assert_eq!(db.entries().unwrap(), vec![e]);
        // Applying again: nothing new.
        assert!(db.apply_remote(&up, ok).unwrap().is_empty());
        // Reload from snapshot.
        let re = GroupDoc::load("g", 9, Some(&db.snapshot()), &[]).unwrap();
        assert_eq!(re.entries().unwrap().len(), 1);
    }

    // KIND: unit (G1 stage 1) — U10/U16: forged, tampered and foreign-peer batches are refused whole.
    #[test]
    fn forged_tampered_and_foreign_batches_are_refused() {
        let a = acct();
        let m = acct();
        let dev = [1; 32];
        let mut target = GroupDoc::new("g", 77);

        // Signed by a, claims to be m: bad signature.
        let mallory = GroupDoc::new("g", writer_id(a.did(), &dev));
        let mut r = post(&a, dev, &mallory, "x");
        r.author = m.did().into();
        let body = bincode::serialize(&r).unwrap();
        let fake = SignedRecord { body, sig: a.sign(b"whatever").to_vec() }.to_bytes();
        let up = mallory.raw_insert(&record_id(&fake), fake);
        assert!(matches!(target.apply_remote(&up, ok), Err(Reject::Record(RecordError::BadSignature))));

        // A genuine record of m's replayed under another peer id: foreign peer.
        let dm = GroupDoc::new("g", writer_id(m.did(), &dev));
        let (_, _) = dm.write(&m, &post(&m, dev, &dm, "mine")).unwrap();
        let genuine = dm.entries().unwrap().remove(0);
        let bytes = genuine.rec.sign(&m).to_bytes();
        let copier = GroupDoc::new("g", writer_id(a.did(), &dev));
        let up = copier.raw_insert(&record_id(&bytes), bytes.clone());
        assert_eq!(target.apply_remote(&up, ok), Err(Reject::ForeignPeer));

        // Key that is not the id.
        let liar = GroupDoc::new("g", writer_id(m.did(), &dev));
        let up = liar.raw_insert("not-the-id", bytes.clone());
        assert_eq!(target.apply_remote(&up, ok), Err(Reject::WrongKey));

        // Another group's record.
        let other = GroupDoc::new("h", writer_id(m.did(), &dev));
        let (_, up) = other.write(&m, &post(&m, dev, &other, "elsewhere")).unwrap();
        assert!(matches!(target.apply_remote(&up, ok), Err(Reject::Record(RecordError::WrongGroup))));

        // Deletes and other containers.
        let mut t2 = GroupDoc::new("g", 78);
        t2.apply_remote(&dm.export_since(&VersionVector::default()).unwrap(), ok).unwrap();
        let del = dm.raw_delete(&genuine.id);
        assert_eq!(t2.apply_remote(&del, ok), Err(Reject::Structure));
        assert_eq!(t2.entries().unwrap().len(), 1);
        let side = GroupDoc::new("g", writer_id(m.did(), &[3; 32]));
        let up = side.raw_other();
        assert_eq!(target.apply_remote(&up, ok), Err(Reject::Structure));

        // A good record and a bad one in one batch: nothing is imported.
        let mixed = GroupDoc::new("g", writer_id(m.did(), &[4; 32]));
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
        let dm = GroupDoc::new("g", writer_id(m.did(), &[1; 32]));
        let (_, up) = dm.write(&m, &post(&m, [1; 32], &dm, "x")).unwrap();
        let mut t = GroupDoc::new("g", 5);
        assert!(matches!(t.apply_remote(&up, |_| Err("removed".into())), Err(Reject::Policy(_))));
        assert!(t.entries().unwrap().is_empty());
    }

    // KIND: unit (G1 stage 1) — a writer cannot write under a writer ID that is not its own.
    #[test]
    fn writing_under_another_writer_id_is_refused_locally() {
        let a = acct();
        let d = GroupDoc::new("g", writer_id(a.did(), &[1; 32]));
        let r = post(&a, [2; 32], &d, "x");
        assert_eq!(d.write(&a, &r).unwrap_err(), Reject::ForeignPeer);
        d.set_peer(writer_id(a.did(), &[2; 32]));
        assert!(d.write(&a, &r).is_ok());
    }
}
