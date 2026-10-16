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
//! hosts/{gid}/{did}/{dev}       bytes      codec SignedHostAddr: where a host is (U7: hosts only)
//! dir/{gid}/{did}/{dev}         DirEntry   host side only: a member device's group endpoint
//! ack/{gid}/{did}/{dev}         bytes      the meta version vector that host acknowledged holding
//! ackd/{gid}/{day}/{did}/{dev}  bytes      the same for one day doc (delivery state of our posts)
//! ep/secret                     32 bytes   this account's group endpoint key on this install
//! ep/made                       u64 BE     when that key became current (unix ms; rotation)
//! ep/announce                   u64 BE     until when a rotated host tells members it moved
//! ep/jitter                     u64 BE     the rotation jitter drawn for this key, ms
//! ep/pending                    32 bytes   a staged next key, not current yet (endpoint.rs)
//! ep/policy                     json       this device's RotationPolicy (default if absent)
//! dev/id                        32 bytes   this store's device ID (U16 device value, G2 stage 1)
//! req/{id}                      ReqRow     a 1:1 chat request we sent or got (U3)
//! out/{gid}/{mail id}           bytes      sealed mail of ours not yet taken by a host
//! mail/{gid}/{to}/{mail id}     bytes      host side: (expiry ms, sealed mail) to deliver
//! seen/{mail id}                u64        mail we already handled (recipient side)
//! rm_seen/{gid}/{did}           u64 BE     when we first derived that member's removal (local ms)
//! clog/{gid}/{call id}          GroupCallRecord  the group call log (calllog.rs), local to this device
//! ```

use serde::{Deserialize, Serialize};
use storage::{Op, Sealed, StorageError, Store};

use super::record::SignedHostAddr;
use crate::Error;

mod endpoint;
mod receipts;
mod removal;
pub(crate) use endpoint::EpState;
pub(crate) use receipts::StoredAttestation;

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
    /// Size of the rows, bytes (what a history list reports).
    #[serde(default)]
    pub bytes: u64,
    /// Compacted into one snapshot row (a day older than `CLOSE_AFTER_DAYS`); an update that
    /// arrives later opens it again until the next compaction.
    pub closed: bool,
}

/// `(snapshot, updates)` rows of a doc.
pub type DocRows = (Option<Vec<u8>>, Vec<Vec<u8>>);

pub(crate) fn io(e: StorageError) -> Error {
    Error::Io(e.to_string())
}

/// The store key, from the recovery phrase (see `rekey::phrase_key`).
pub fn key_from_phrase(phrase: &str) -> cryptography::Secret32 {
    crate::rekey::phrase_key(STORE_DOMAIN, phrase)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GroupMeta {
    pub next_seq: u64,
    pub n_updates: u64,
    /// We are joining with this invite (`codec` bytes of a `SignedInvite`) and have not written our join yet.
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
    /// Validated metadata frontier received through an own-device contents link
    /// that still needs storage at a host. Empty means no such obligation.
    /// This is local bookkeeping, not a membership or destination receipt.
    #[serde(default)]
    pub own_owed_meta: Vec<u8>,
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
    #[cfg(test)]
    fail_next_append: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(test)]
    fail_next_meta_write: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
        Ok(Self {
            db: Sealed::new(store, key_from_phrase(phrase)),
            #[cfg(test)]
            fail_next_append: Default::default(),
            #[cfg(test)]
            fail_next_meta_write: Default::default(),
        })
    }

    #[cfg(test)]
    pub fn in_memory(phrase: &str) -> Self {
        Self {
            db: Sealed::new(Store::open_in_memory().unwrap(), key_from_phrase(phrase)),
            fail_next_append: Default::default(),
            fail_next_meta_write: Default::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn fail_next_meta_write_for_test(&self) {
        self.fail_next_meta_write.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn fail_next_append_for_test(&self) {
        self.fail_next_append.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn apply_append(&self, ops: &[Op]) -> Result<(), Error> {
        #[cfg(test)]
        if self.fail_next_append.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(Error::Io("injected group append failure".into()));
        }
        self.db.apply(ops).map_err(io)
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
        #[cfg(test)]
        if self.fail_next_meta_write.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(Error::Io("injected group metadata write failure".into()));
        }
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
        self.apply_append(&ops)
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

    /// One bounded day-index page, without materializing the complete archive.
    pub(crate) fn days_after(&self, gid: &str, after: Option<&str>, limit: usize) -> Result<Vec<String>, Error> {
        let prefix = format!("days/{gid}/");
        let cursor = after.map(|day| format!("{prefix}{day}"));
        Ok(self.db.keys_after(&prefix, cursor.as_deref(), limit).map_err(io)?
            .into_iter().filter_map(|key| key.strip_prefix(&prefix).map(str::to_string)).collect())
    }

    /// Appends an accepted update of a day doc whose version vector is now `vv` (or, every so
    /// often, replaces the rows by `snapshot()`), with the day index and, if given, the group
    /// meta, in one transaction.
    #[allow(clippy::too_many_arguments)]
    pub fn append_shard(&self, gid: &str, day: &str, sm: &mut ShardMeta, vv: Vec<u8>, update: &[u8], snapshot: impl FnOnce() -> Vec<u8>, group: Option<&GroupMeta>) -> Result<(), Error> {
        let mut ops: Vec<Op> = Vec::new();
        if sm.n_updates + 1 >= UPDATES_BEFORE_SNAPSHOT {
            ops.push(Op::DeletePrefix(format!("shard/{gid}/{day}/u/")));
            let snap = snapshot();
            sm.bytes = snap.len() as u64;
            ops.push(self.db.put_op(&format!("shard/{gid}/{day}/snap"), &snap).map_err(io)?);
            sm.n_updates = 0;
        } else {
            ops.push(self.db.put_op(&format!("shard/{gid}/{day}/u/{:010}", sm.next_seq), update).map_err(io)?);
            sm.next_seq += 1;
            sm.n_updates += 1;
            sm.bytes += update.len() as u64;
        }
        sm.vv = vv;
        sm.closed = false;
        ops.push(self.db.put_op(&format!("shard/{gid}/{day}/meta"), &json(sm)).map_err(io)?);
        ops.push(self.db.put_op(&format!("days/{gid}/{day}"), &[]).map_err(io)?);
        if let Some(g) = group {
            ops.push(self.db.put_op(&format!("grp/{gid}"), &json(g)).map_err(io)?);
        }
        self.apply_append(&ops)
    }

    /// Forgets every day doc of a group (test hook: a wiped host).
    pub fn drop_days(&self, gid: &str) -> Result<(), Error> {
        self.db.apply(&[Op::DeletePrefix(format!("shard/{gid}/")), Op::DeletePrefix(format!("days/{gid}/")), Op::DeletePrefix(format!("ackd/{gid}/"))]).map_err(io)
    }

    /// Closes a day: its rows become the one snapshot `snap`.
    pub fn compact_shard(&self, gid: &str, day: &str, sm: &mut ShardMeta, snap: &[u8]) -> Result<(), Error> {
        sm.n_updates = 0;
        sm.bytes = snap.len() as u64;
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
        Ok(self.db.scan(&format!("hosts/{gid}/")).map_err(io)?.into_iter().filter_map(|(_, v)| cryptography::codec::from_bytes(&v, crate::store::MAX_ROW).ok()).collect())
    }

    pub fn put_host(&self, gid: &str, did: &str, dev: &[u8; 32], a: &SignedHostAddr) -> Result<(), Error> {
        self.db.put(&format!("hosts/{gid}/{did}/{}", hex::encode(dev)), &cryptography::codec::to_bytes(a).expect("serialises")).map_err(io)
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
                Op::DeletePrefix(format!("proof/{gid}/")),
                Op::DeletePrefix(format!("out/{gid}/")),
                Op::DeletePrefix(format!("mail/{gid}/")),
                Op::DeletePrefix(format!("clog/{gid}/")),
                Op::DeletePrefix(format!("rm_seen/{gid}/")),
            ])
            .map_err(io)
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

    /// Raw (sealed) bytes of every row, for the at-rest test.
    #[cfg(test)]
    pub fn raw_rows(&self) -> Vec<Vec<u8>> {
        self.db.raw().scan("").unwrap().into_iter().map(|(_, v)| v).collect()
    }
}

#[cfg(test)]
mod tests;
