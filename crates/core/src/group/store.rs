//! The group records in their own sealed vault file (`group.redb`, encrypted at rest like chats:
//! every value sealed under a key derived from the recovery phrase).
//!
//! ```text
//! grp/{gid}                     GroupMeta  per-group local state (not replicated): read cursor,
//!                                          last post and unread count (the chat list never loads
//!                                          day docs), newest day, days with pending posts
//! meta/{gid}/snap               bytes      Loro snapshot of the meta doc (every record but posts)
//! meta/{gid}/u/{seq}            bytes      validated update batches since the snapshot
//! shard/{gid}/{day}/meta        ShardMeta  vv, update counters, compacted flag of one UTC day's posts
//! shard/{gid}/{day}/snap        bytes      Loro snapshot of that day doc (a closed day: all there is)
//! shard/{gid}/{day}/u/{seq}     bytes      validated update batches since the snapshot
//! days/{gid}/{day}              empty      index of the days we hold (paging, history lists)
//! hosts/{gid}/{did}/{dev}       bytes      bincode SignedHostAddr: where a host is (U7: hosts only)
//! dir/{gid}/{did}/{dev}         DirEntry   host side only: a member device's group endpoint
//! ack/{gid}/{did}/{dev}         bytes      the meta version vector that host acknowledged holding
//! ackd/{gid}/{day}/{did}/{dev}  bytes      the same for one day doc (delivery state of our posts)
//! ep/secret                     32 bytes   this account's group endpoint key on this install
//! ep/made                       u64 BE     when that key was made (unix ms; rotation, U12)
//! dev/id                        32 bytes   this store's device ID (U16 device value, G2 stage 1)
//! req/{id}                      ReqRow     a 1:1 chat request we sent or got (U3)
//! out/{gid}/{mail id}           bytes      sealed mail of ours not yet taken by a host
//! mail/{gid}/{to}/{mail id}     bytes      host side: (expiry ms, sealed mail) to deliver
//! seen/{mail id}                u64        mail we already handled (recipient side)
//! ```

use serde::{Deserialize, Serialize};
use storage::{Op, Sealed, StorageError, Store};

use super::record::SignedHostAddr;
use crate::Error;

const STORE_DOMAIN: &str = "tinline/group-store/v2";
const UPDATES_BEFORE_SNAPSHOT: u64 = 64;

/// The post a chat-list row shows, kept in `GroupMeta` so the list never loads a day doc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostBrief {
    pub id: String,
    pub author: String,
    pub at: i64,
    pub text: String,
    pub reply_to: Option<String>,
    /// Where the record sits in its day doc (delivery state of our own posts).
    pub peer: u64,
    pub counter: i32,
}

/// What we hold of one day doc besides its rows.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShardMeta {
    /// Encoded version vector of the doc (what `Hello` and history lists report).
    pub vv: Vec<u8>,
    pub next_seq: u64,
    pub n_updates: u64,
    /// Compacted into one snapshot row (a day older than `CLOSE_AFTER_DAYS`); an update that
    /// arrives later opens it again until the next compaction.
    pub closed: bool,
}

/// `(snapshot, updates)` rows of a doc.
pub type DocRows = (Option<Vec<u8>>, Vec<Vec<u8>>);

pub(crate) fn io(e: StorageError) -> Error {
    Error::Io(e.to_string())
}

/// The store key, from the recovery phrase (words lower-cased, single-spaced), like `rekey.rs`.
pub fn key_from_phrase(phrase: &str) -> [u8; 32] {
    let norm = phrase.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>().join(" ");
    blake3::derive_key(STORE_DOMAIN, norm.as_bytes())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GroupMeta {
    pub next_seq: u64,
    pub n_updates: u64,
    /// We are joining with this invite (bincode `SignedInvite`) and have not written our join yet.
    #[serde(default)]
    pub joining: Option<Vec<u8>>,
    /// The signed records a host showed us to prove we were removed (U9 visible notice).
    #[serde(default)]
    pub removal_proof: Vec<Vec<u8>>,
    /// Items read up to this `(at)`.
    #[serde(default)]
    pub read_upto: i64,
    /// Learned from one of our own devices (stage 2): forgotten again if a host refuses us
    /// before we ever held the group.
    #[serde(default)]
    pub from_own: bool,
    /// The newest visible post, for the chat list.
    #[serde(default)]
    pub last: Option<PostBrief>,
    /// Visible posts of others newer than `read_upto`.
    #[serde(default)]
    pub unread: u32,
    /// The newest day we hold posts of.
    #[serde(default)]
    pub newest_day: Option<String>,
    /// Days holding posts whose `seen` names meta records we lack: judged again when the meta
    /// doc advances.
    #[serde(default)]
    pub pending_days: std::collections::BTreeSet<String>,
    /// Days with posts of ours that no host has acknowledged yet.
    #[serde(default)]
    pub owed_days: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirEntry {
    pub endpoint: String,
    pub seen_ms: u64,
    /// The member's home relay as it said in its `Hello` (to reach it after a rotation).
    #[serde(default)]
    pub relay: Option<String>,
}

#[derive(Clone)]
pub struct GroupStore {
    pub(crate) db: Sealed,
}

pub(crate) fn json<T: Serialize>(v: &T) -> Vec<u8> {
    serde_json::to_vec(v).expect("serialises")
}

impl GroupStore {
    pub fn open(path: std::path::PathBuf, phrase: &str) -> Result<Self, Error> {
        // A store of the previous session may still be closing (its last tasks hold it).
        let mut tries = 0;
        let store = loop {
            match Store::open(&path) {
                Ok(s) => {
                    crate::store::note_open(&path, tries > 0);
                    break s;
                }
                Err(e) if tries >= 50 => return Err(io(e)),
                Err(_) => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        };
        Ok(Self { db: Sealed::new(store, key_from_phrase(phrase)) })
    }

    #[cfg(test)]
    pub fn in_memory(phrase: &str) -> Self {
        Self { db: Sealed::new(Store::open_in_memory().unwrap(), key_from_phrase(phrase)) }
    }

    pub fn groups(&self) -> Result<Vec<(String, GroupMeta)>, Error> {
        let mut out = Vec::new();
        for (k, v) in self.db.scan("grp/").map_err(io)? {
            if let (Some(g), Ok(m)) = (k.strip_prefix("grp/"), serde_json::from_slice::<GroupMeta>(&v)) {
                out.push((g.to_string(), m));
            }
        }
        Ok(out)
    }

    pub fn put_meta(&self, gid: &str, m: &GroupMeta) -> Result<(), Error> {
        self.db.put(&format!("grp/{gid}"), &json(m)).map_err(io)
    }

    /// `(snapshot, updates)` of a group's meta doc.
    pub fn meta_rows(&self, gid: &str) -> Result<DocRows, Error> {
        self.rows(&format!("meta/{gid}"))
    }

    fn rows(&self, base: &str) -> Result<DocRows, Error> {
        let snap = self.db.get(&format!("{base}/snap")).map_err(io)?;
        let ups = self.db.scan(&format!("{base}/u/")).map_err(io)?.into_iter().map(|(_, v)| v).collect();
        Ok((snap, ups))
    }

    /// Appends an accepted meta update (or, every so often, replaces everything by
    /// `snapshot()`), and writes the group meta in the same transaction.
    pub fn append_meta(&self, gid: &str, meta: &mut GroupMeta, update: &[u8], snapshot: impl FnOnce() -> Vec<u8>) -> Result<(), Error> {
        let mut ops: Vec<Op> = Vec::new();
        if meta.n_updates + 1 >= UPDATES_BEFORE_SNAPSHOT {
            ops.push(Op::DeletePrefix(format!("meta/{gid}/u/")));
            ops.push(self.db.put_op(&format!("meta/{gid}/snap"), &snapshot()).map_err(io)?);
            meta.n_updates = 0;
        } else {
            ops.push(self.db.put_op(&format!("meta/{gid}/u/{:010}", meta.next_seq), update).map_err(io)?);
            meta.next_seq += 1;
            meta.n_updates += 1;
        }
        ops.push(self.db.put_op(&format!("grp/{gid}"), &json(meta)).map_err(io)?);
        self.db.apply(&ops).map_err(io)
    }

    // ---- day docs ------------------------------------------------------------------------------

    pub fn shard_meta(&self, gid: &str, day: &str) -> Result<Option<ShardMeta>, Error> {
        Ok(self.db.get(&format!("shard/{gid}/{day}/meta")).map_err(io)?.and_then(|b| serde_json::from_slice(&b).ok()))
    }

    pub fn shard_rows(&self, gid: &str, day: &str) -> Result<DocRows, Error> {
        self.rows(&format!("shard/{gid}/{day}"))
    }

    /// The days we hold, oldest first.
    pub fn days(&self, gid: &str) -> Result<Vec<String>, Error> {
        let p = format!("days/{gid}/");
        Ok(self.db.list_prefixed(&p).map_err(io)?.into_iter().filter_map(|k| k.strip_prefix(&p).map(str::to_string)).collect())
    }

    /// Appends an accepted update of a day doc whose version vector is now `vv` (or, every so
    /// often, replaces the rows by `snapshot()`), with the day index and, if given, the group
    /// meta, in one transaction.
    pub fn append_shard(&self, gid: &str, day: &str, sm: &mut ShardMeta, vv: Vec<u8>, update: &[u8], snapshot: impl FnOnce() -> Vec<u8>, group: Option<&GroupMeta>) -> Result<(), Error> {
        let mut ops: Vec<Op> = Vec::new();
        if sm.n_updates + 1 >= UPDATES_BEFORE_SNAPSHOT {
            ops.push(Op::DeletePrefix(format!("shard/{gid}/{day}/u/")));
            ops.push(self.db.put_op(&format!("shard/{gid}/{day}/snap"), &snapshot()).map_err(io)?);
            sm.n_updates = 0;
        } else {
            ops.push(self.db.put_op(&format!("shard/{gid}/{day}/u/{:010}", sm.next_seq), update).map_err(io)?);
            sm.next_seq += 1;
            sm.n_updates += 1;
        }
        sm.vv = vv;
        sm.closed = false;
        ops.push(self.db.put_op(&format!("shard/{gid}/{day}/meta"), &json(sm)).map_err(io)?);
        ops.push(self.db.put_op(&format!("days/{gid}/{day}"), &[]).map_err(io)?);
        if let Some(g) = group {
            ops.push(self.db.put_op(&format!("grp/{gid}"), &json(g)).map_err(io)?);
        }
        self.db.apply(&ops).map_err(io)
    }

    /// Closes a day: its rows become the one snapshot `snap`.
    pub fn compact_shard(&self, gid: &str, day: &str, sm: &mut ShardMeta, snap: &[u8]) -> Result<(), Error> {
        sm.n_updates = 0;
        sm.closed = true;
        let ops = [
            Op::DeletePrefix(format!("shard/{gid}/{day}/u/")),
            self.db.put_op(&format!("shard/{gid}/{day}/snap"), snap).map_err(io)?,
            self.db.put_op(&format!("shard/{gid}/{day}/meta"), &json(sm)).map_err(io)?,
        ];
        self.db.apply(&ops).map_err(io)
    }

    /// The version vectors hosts acknowledged for one day doc, per host device.
    pub fn acks_day(&self, gid: &str, day: &str) -> Result<Vec<(String, Vec<u8>)>, Error> {
        let p = format!("ackd/{gid}/{day}/");
        Ok(self.db.scan(&p).map_err(io)?.into_iter().filter_map(|(k, v)| Some((k.strip_prefix(&p)?.to_string(), v))).collect())
    }

    pub fn put_ack_day(&self, gid: &str, day: &str, did: &str, dev: &[u8; 32], vv: &[u8]) -> Result<(), Error> {
        self.db.put(&format!("ackd/{gid}/{day}/{did}/{}", hex::encode(dev)), vv).map_err(io)
    }

    pub fn hosts(&self, gid: &str) -> Result<Vec<SignedHostAddr>, Error> {
        Ok(self.db.scan(&format!("hosts/{gid}/")).map_err(io)?.into_iter().filter_map(|(_, v)| bincode::deserialize(&v).ok()).collect())
    }

    pub fn put_host(&self, gid: &str, did: &str, dev: &[u8; 32], a: &SignedHostAddr) -> Result<(), Error> {
        self.db.put(&format!("hosts/{gid}/{did}/{}", hex::encode(dev)), &bincode::serialize(a).expect("serialises")).map_err(io)
    }

    pub fn dir(&self, gid: &str) -> Result<Vec<(String, DirEntry)>, Error> {
        let p = format!("dir/{gid}/");
        Ok(self
            .db
            .scan(&p)
            .map_err(io)?
            .into_iter()
            .filter_map(|(k, v)| Some((k.strip_prefix(&p)?.to_string(), serde_json::from_slice(&v).ok()?)))
            .collect())
    }

    pub fn put_dir(&self, gid: &str, did: &str, dev: &[u8; 32], e: &DirEntry) -> Result<(), Error> {
        self.db.put(&format!("dir/{gid}/{did}/{}", hex::encode(dev)), &json(e)).map_err(io)
    }

    /// Drops every directory entry of `did` (a removed member, U9).
    pub fn drop_dir(&self, gid: &str, did: &str) -> Result<(), Error> {
        self.db.apply(&[Op::DeletePrefix(format!("dir/{gid}/{did}/"))]).map_err(io)
    }

    pub fn acks(&self, gid: &str) -> Result<Vec<(String, Vec<u8>)>, Error> {
        let p = format!("ack/{gid}/");
        Ok(self.db.scan(&p).map_err(io)?.into_iter().filter_map(|(k, v)| Some((k.strip_prefix(&p)?.to_string(), v))).collect())
    }

    pub fn put_ack(&self, gid: &str, did: &str, dev: &[u8; 32], vv: &[u8]) -> Result<(), Error> {
        self.db.put(&format!("ack/{gid}/{did}/{}", hex::encode(dev)), vv).map_err(io)
    }

    /// Everything of a group that never completed its join.
    pub fn forget(&self, gid: &str) -> Result<(), Error> {
        self.db
            .apply(&[
                Op::Delete(format!("grp/{gid}")),
                Op::DeletePrefix(format!("meta/{gid}/")),
                Op::DeletePrefix(format!("shard/{gid}/")),
                Op::DeletePrefix(format!("days/{gid}/")),
                Op::DeletePrefix(format!("ackd/{gid}/")),
                Op::DeletePrefix(format!("hosts/{gid}/")),
                Op::DeletePrefix(format!("dir/{gid}/")),
                Op::DeletePrefix(format!("ack/{gid}/")),
                Op::DeletePrefix(format!("out/{gid}/")),
                Op::DeletePrefix(format!("mail/{gid}/")),
            ])
            .map_err(io)
    }

    /// This account's group endpoint key on this install (made once).
    pub fn endpoint_secret(&self) -> Result<[u8; 32], Error> {
        if let Some(b) = self.db.get("ep/secret").map_err(io)?
            && let Ok(k) = <[u8; 32]>::try_from(b.as_slice())
        {
            return Ok(k);
        }
        self.rotate_endpoint_secret(crate::node::now_ms())
    }

    /// This store's device ID: the group writer ID is `writer_id(DID, this)` (U16). A store
    /// made from nothing gets a fresh random one (G2 stage 1, like P10 for chat): whatever this
    /// install wrote before under an older ID (a lost or deleted `group.redb`) may still sit at
    /// some host, and writing again from counter 0 under the same writer ID would collide with
    /// it. A store from before this row existed keeps `adopt` (the profile's device ID it used).
    pub fn device_id(&self, adopt: Option<[u8; 32]>) -> Result<[u8; 32], Error> {
        if let Some(b) = self.db.get("dev/id").map_err(io)?
            && let Ok(k) = <[u8; 32]>::try_from(b.as_slice())
        {
            return Ok(k);
        }
        let k = adopt.unwrap_or_else(crate::chat::doc::new_device_value);
        self.db.put("dev/id", &k).map_err(io)?;
        Ok(k)
    }

    /// When the endpoint key was made (unix ms; 0 = unknown, before rotation existed).
    pub fn endpoint_made(&self) -> u64 {
        self.db.get("ep/made").ok().flatten().and_then(|b| <[u8; 8]>::try_from(b.as_slice()).ok()).map(u64::from_be_bytes).unwrap_or(0)
    }

    /// Replaces the endpoint key (U12 rotation); returns the new one.
    pub fn rotate_endpoint_secret(&self, made_ms: u64) -> Result<[u8; 32], Error> {
        let k = proto::new_device_secret();
        self.db.apply(&[self.db.put_op("ep/secret", &k).map_err(io)?, self.db.put_op("ep/made", &made_ms.to_be_bytes()).map_err(io)?]).map_err(io)?;
        Ok(k)
    }

    /// Raw (sealed) bytes of every row, for the at-rest test.
    #[cfg(test)]
    pub fn raw_rows(&self) -> Vec<Vec<u8>> {
        self.db.raw().scan("").unwrap().into_iter().map(|(_, v)| v).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::doc::{DocId, GroupDoc};
    use super::super::record::*;
    use super::*;
    use loro::VersionVector;

    // KIND: unit (G1 stage 1) — the meta doc is stored sealed and reloads intact.
    #[test]
    fn the_doc_is_sealed_at_rest_and_reloads() {
        let a = identity::generate().0;
        let s = GroupStore::in_memory("alpha beta");
        let dev = [1; 32];
        let peer = crate::chat::doc::writer_id(a.did(), &dev);
        let doc = GroupDoc::new("g", peer);
        let mut meta = GroupMeta::default();
        for i in 0..70 {
            let name = format!("secret text {i}");
            let rec = Record { v: 1, group: "g".into(), author: a.did().into(), dev, at: i, seen: doc.seen(), body: Body::Genesis { name, nonce: [0; 16], owner_name: String::new() } };
            let (_, up) = doc.write(&a, &rec).unwrap();
            s.append_meta("g", &mut meta, &up, || doc.snapshot()).unwrap();
        }
        for row in s.raw_rows() {
            assert!(!row.windows(11).any(|w| w == b"secret text"), "plaintext at rest");
        }
        let (snap, ups) = s.meta_rows("g").unwrap();
        assert!(snap.is_some(), "compacted once");
        let back = GroupDoc::load("g", peer, snap.as_deref(), &ups).unwrap();
        assert_eq!(back.entries().unwrap().len(), 70);
        assert_eq!(s.groups().unwrap().len(), 1);
        // Another phrase cannot read it.
        let other = GroupStore { db: Sealed::new(s.db.raw().clone(), key_from_phrase("gamma")) };
        assert!(other.meta_rows("g").is_err());
    }

    // KIND: unit (group shards) — day docs are stored sealed per day, listed in the day index,
    // compacted to one snapshot row and reload intact; forgetting the group drops them.
    #[test]
    fn day_docs_are_sealed_indexed_and_compact() {
        let a = identity::generate().0;
        let s = GroupStore::in_memory("alpha beta");
        let dev = [1; 32];
        let peer = crate::chat::doc::writer_id(a.did(), &dev);
        let day = || DocId::Day("1970-01-01".into());
        let doc = GroupDoc::new_in("g", day(), peer);
        let mut sm = ShardMeta::default();
        for i in 0..70 {
            let text = format!("secret text {i}");
            let rec = Record { v: 1, group: "g".into(), author: a.did().into(), dev, at: i, seen: vec![], body: Body::Post { text, reply_to: None } };
            let (_, up) = doc.write(&a, &rec).unwrap();
            s.append_shard("g", "1970-01-01", &mut sm, doc.vv().encode(), &up, || doc.snapshot(), None).unwrap();
        }
        assert_eq!(s.days("g").unwrap(), vec!["1970-01-01".to_string()]);
        assert!(s.days("h").unwrap().is_empty());
        for row in s.raw_rows() {
            assert!(!row.windows(11).any(|w| w == b"secret text"), "plaintext at rest");
        }
        let stored = s.shard_meta("g", "1970-01-01").unwrap().unwrap();
        assert!(!stored.closed && VersionVector::decode(&stored.vv).unwrap() == doc.vv());
        let (snap, ups) = s.shard_rows("g", "1970-01-01").unwrap();
        assert!(snap.is_some() && !ups.is_empty(), "compacted once, then updates again");
        // Closing: one snapshot row, no updates.
        s.compact_shard("g", "1970-01-01", &mut sm, &doc.snapshot()).unwrap();
        let (snap, ups) = s.shard_rows("g", "1970-01-01").unwrap();
        assert!(ups.is_empty() && s.shard_meta("g", "1970-01-01").unwrap().unwrap().closed);
        let back = GroupDoc::load_in("g", day(), peer, snap.as_deref(), &ups).unwrap();
        assert_eq!(back.entries_trusted().unwrap().len(), 70);
        // A later update opens it again.
        let rec = Record { v: 1, group: "g".into(), author: a.did().into(), dev, at: 99, seen: vec![], body: Body::Post { text: "late".into(), reply_to: None } };
        let (_, up) = doc.write(&a, &rec).unwrap();
        s.append_shard("g", "1970-01-01", &mut sm, doc.vv().encode(), &up, || doc.snapshot(), None).unwrap();
        assert!(!s.shard_meta("g", "1970-01-01").unwrap().unwrap().closed);
        s.forget("g").unwrap();
        assert!(s.days("g").unwrap().is_empty() && s.shard_meta("g", "1970-01-01").unwrap().is_none());
    }
}
