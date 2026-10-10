//! The group records in their own sealed vault file (`group.redb`, encrypted at rest like chats:
//! every value sealed under a key derived from the recovery phrase).
//!
//! ```text
//! grp/{gid}                     GroupMeta  per-group local state (not replicated)
//! doc/{gid}/snap                bytes      Loro snapshot
//! doc/{gid}/u/{seq}             bytes      validated update batches since the snapshot
//! hosts/{gid}/{did}/{dev}       bytes      bincode SignedHostAddr: where a host is (U7: hosts only)
//! dir/{gid}/{did}/{dev}         DirEntry   host side only: a member device's group endpoint
//! ack/{gid}/{did}/{dev}         bytes      the version vector that host acknowledged holding
//! ep/secret                     32 bytes   this account's group endpoint key on this install
//! ```

use serde::{Deserialize, Serialize};
use storage::{Op, Sealed, StorageError, Store};

use super::record::SignedHostAddr;
use crate::Error;

const STORE_DOMAIN: &str = "tinline/group-store/v1";
const UPDATES_BEFORE_SNAPSHOT: u64 = 64;

fn io(e: StorageError) -> Error {
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirEntry {
    pub endpoint: String,
    pub seen_ms: u64,
}

#[derive(Clone)]
pub struct GroupStore {
    db: Sealed,
}

fn json<T: Serialize>(v: &T) -> Vec<u8> {
    serde_json::to_vec(v).expect("serialises")
}

impl GroupStore {
    pub fn open(path: std::path::PathBuf, phrase: &str) -> Result<Self, Error> {
        // A store of the previous session may still be closing (its last tasks hold it).
        let mut tries = 0;
        let store = loop {
            match Store::open(&path) {
                Ok(s) => break s,
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

    /// `(snapshot, updates)` of a group's doc.
    pub fn doc_rows(&self, gid: &str) -> Result<(Option<Vec<u8>>, Vec<Vec<u8>>), Error> {
        let snap = self.db.get(&format!("doc/{gid}/snap")).map_err(io)?;
        let ups = self.db.scan(&format!("doc/{gid}/u/")).map_err(io)?.into_iter().map(|(_, v)| v).collect();
        Ok((snap, ups))
    }

    /// Appends an accepted update (or, every so often, replaces everything by `snapshot()`), and
    /// writes the meta in the same transaction.
    pub fn append(&self, gid: &str, meta: &mut GroupMeta, update: &[u8], snapshot: impl FnOnce() -> Vec<u8>) -> Result<(), Error> {
        let mut ops: Vec<Op> = Vec::new();
        if meta.n_updates + 1 >= UPDATES_BEFORE_SNAPSHOT {
            ops.push(Op::DeletePrefix(format!("doc/{gid}/u/")));
            ops.push(self.db.put_op(&format!("doc/{gid}/snap"), &snapshot()).map_err(io)?);
            meta.n_updates = 0;
        } else {
            ops.push(self.db.put_op(&format!("doc/{gid}/u/{:010}", meta.next_seq), update).map_err(io)?);
            meta.next_seq += 1;
            meta.n_updates += 1;
        }
        ops.push(self.db.put_op(&format!("grp/{gid}"), &json(meta)).map_err(io)?);
        self.db.apply(&ops).map_err(io)
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
                Op::DeletePrefix(format!("doc/{gid}/")),
                Op::DeletePrefix(format!("hosts/{gid}/")),
                Op::DeletePrefix(format!("dir/{gid}/")),
                Op::DeletePrefix(format!("ack/{gid}/")),
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
        let k = proto::new_device_secret();
        self.db.put("ep/secret", &k).map_err(io)?;
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
    use super::super::doc::GroupDoc;
    use super::super::record::*;
    use super::*;

    // KIND: unit (G1 stage 1) — the group doc is stored sealed and reloads intact.
    #[test]
    fn the_doc_is_sealed_at_rest_and_reloads() {
        let a = identity::generate().0;
        let s = GroupStore::in_memory("alpha beta");
        let dev = [1; 32];
        let peer = crate::chat::doc::writer_id(a.did(), &dev);
        let doc = GroupDoc::new("g", peer);
        let mut meta = GroupMeta::default();
        for i in 0..70 {
            let text = format!("secret text {i}");
            let rec = Record { v: 1, group: "g".into(), author: a.did().into(), dev, at: i, seen: doc.seen(), body: Body::Post { text, reply_to: None } };
            let (_, up) = doc.write(&a, &rec).unwrap();
            s.append("g", &mut meta, &up, || doc.snapshot()).unwrap();
        }
        for row in s.raw_rows() {
            assert!(!row.windows(11).any(|w| w == b"secret text"), "plaintext at rest");
        }
        let (snap, ups) = s.doc_rows("g").unwrap();
        assert!(snap.is_some(), "compacted once");
        let back = GroupDoc::load("g", peer, snap.as_deref(), &ups).unwrap();
        assert_eq!(back.entries().unwrap().len(), 70);
        assert_eq!(s.groups().unwrap().len(), 1);
        // Another phrase cannot read it.
        let other = GroupStore { db: Sealed::new(s.db.raw().clone(), key_from_phrase("gamma")) };
        assert!(other.doc_rows("g").is_err());
    }
}
