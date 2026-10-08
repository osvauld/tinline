//! The chat records in the sealed vault file (`chat.redb`), as typed accessors over `Sealed`.
//!
//! Layout (all values sealed with a key derived from the DEK; paths are not secret):
//! ```text
//! conv/{pair}                    ConvMeta   list row: peer, unread, last message
//! shard/{pair}/{day}/meta        ShardMeta  vv, counters, closed-snapshot ref
//! shard/{pair}/{day}/snap        bytes      Loro snapshot (open shards)
//! shard/{pair}/{day}/u/{seq}     bytes      validated update batches since the snapshot
//! ack/{pair}/{day}               bytes      the peer's last acknowledged version vector
//! mc/{pair}/{day}/{id}           u32 BE     our op counter after creating that message
//! out/{pair}/{day}               empty      this day has ops of ours the peer has not acked
//! blob/{hash}                    BlobInfo   hash -> key (+ name, size, mime, state)
//! ref/{hash}/{pair}              empty      this conversation may read that blob
//! pb/{pair}/{hash}               empty      the blobs a conversation uses (for deletion)
//! snapcache/{pair}/{day}         SnapRef    a snapshot blob we made for a peer's history request
//! set/auto_download              u64 BE
//! ```

use serde::{Deserialize, Serialize};
use storage::{Op, Sealed, StorageError};

use crate::Error;

pub fn io(e: StorageError) -> Error {
    Error::Io(e.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClosedRef {
    pub hash: String,
    pub key: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShardMeta {
    #[serde(with = "hexbytes")]
    pub vv: Vec<u8>,
    pub next_seq: u64,
    pub n_updates: u32,
    pub n_messages: u32,
    /// Set once the day was compacted into a snapshot blob and its vault rows dropped.
    pub closed: Option<ClosedRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LastMsg {
    pub id: String,
    pub at: i64,
    pub preview: String,
    pub outgoing: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConvMeta {
    pub peer_did: String,
    pub unread: u32,
    pub last: Option<LastMsg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobInfo {
    pub key: String,
    pub name: String,
    pub size: u64,
    pub mime: String,
    pub kind: String,
    pub pair: String,
    /// The ciphertext is complete in the blob store.
    pub ready: bool,
    /// The user asked for it (or it is under the auto-download limit): fetch whenever possible.
    pub wanted: bool,
    pub failed: bool,
    #[serde(default)]
    pub day: String,
    #[serde(default)]
    pub msg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapRef {
    #[serde(with = "hexbytes")]
    pub vv: Vec<u8>,
    pub hash: String,
    pub key: String,
    pub size: u64,
}

mod hexbytes {
    pub fn serialize<S: serde::Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(v))
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s: String = serde::Deserialize::deserialize(d)?;
        hex::decode(s).map_err(serde::de::Error::custom)
    }
}

/// `(snapshot, updates)` rows of an open shard.
pub type ShardRows = (Option<Vec<u8>>, Vec<Vec<u8>>);

pub const DEFAULT_AUTO_DOWNLOAD: u64 = 10 * 1024 * 1024;

#[derive(Clone)]
pub struct ChatStore {
    pub db: Sealed,
}

fn json<T: Serialize>(v: &T) -> Vec<u8> {
    serde_json::to_vec(v).expect("serialises")
}

impl ChatStore {
    pub fn new(db: Sealed) -> Self {
        Self { db }
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<Option<T>, Error> {
        match self.db.get(path).map_err(io)? {
            Some(b) => Ok(Some(serde_json::from_slice(&b)?)),
            None => Ok(None),
        }
    }

    // ---- conversations ---------------------------------------------------------------------

    pub fn conv(&self, pair: &str) -> Result<Option<ConvMeta>, Error> {
        self.get_json(&format!("conv/{pair}"))
    }

    pub fn conv_op(&self, pair: &str, c: &ConvMeta) -> Result<Op, Error> {
        let path = format!("conv/{pair}");
        self.db.put_op(&path, &json(c)).map_err(io)
    }

    // ---- shards ----------------------------------------------------------------------------

    pub fn shard_meta(&self, pair: &str, day: &str) -> Result<Option<ShardMeta>, Error> {
        self.get_json(&format!("shard/{pair}/{day}/meta"))
    }

    /// Days we hold for a conversation, newest first.
    pub fn shard_days(&self, pair: &str) -> Result<Vec<String>, Error> {
        let prefix = format!("shard/{pair}/");
        let mut days: Vec<String> = self
            .db
            .list_prefixed(&prefix)
            .map_err(io)?
            .into_iter()
            .filter_map(|k| k.strip_prefix(&prefix)?.strip_suffix("/meta").map(str::to_string))
            .filter(|d| !d.contains('/'))
            .collect();
        days.sort_by(|a, b| b.cmp(a));
        Ok(days)
    }

    /// `(snapshot, updates)` rows of an open shard.
    pub fn shard_rows(&self, pair: &str, day: &str) -> Result<ShardRows, Error> {
        let snap = self.db.get(&format!("shard/{pair}/{day}/snap")).map_err(io)?;
        let ups = self.db.scan(&format!("shard/{pair}/{day}/u/")).map_err(io)?.into_iter().map(|(_, v)| v).collect();
        Ok((snap, ups))
    }

    pub fn put_meta_op(&self, pair: &str, day: &str, m: &ShardMeta) -> Result<Op, Error> {
        self.db.put_op(&format!("shard/{pair}/{day}/meta"), &json(m)).map_err(io)
    }

    pub fn put_update_op(&self, pair: &str, day: &str, seq: u64, update: &[u8]) -> Result<Op, Error> {
        self.db.put_op(&format!("shard/{pair}/{day}/u/{seq:010}"), update).map_err(io)
    }

    pub fn put_snap_ops(&self, pair: &str, day: &str, snap: &[u8]) -> Result<Vec<Op>, Error> {
        Ok(vec![
            Op::DeletePrefix(format!("shard/{pair}/{day}/u/")),
            self.db.put_op(&format!("shard/{pair}/{day}/snap"), snap).map_err(io)?,
        ])
    }

    pub fn drop_rows_ops(&self, pair: &str, day: &str) -> Vec<Op> {
        vec![
            Op::DeletePrefix(format!("shard/{pair}/{day}/u/")),
            Op::Delete(format!("shard/{pair}/{day}/snap")),
        ]
    }

    // ---- acks, counters, outbox --------------------------------------------------------------

    pub fn ack(&self, pair: &str, day: &str) -> Result<Option<Vec<u8>>, Error> {
        self.db.get(&format!("ack/{pair}/{day}")).map_err(io)
    }

    pub fn put_ack_op(&self, pair: &str, day: &str, vv: &[u8]) -> Result<Op, Error> {
        self.db.put_op(&format!("ack/{pair}/{day}"), vv).map_err(io)
    }

    pub fn mc(&self, pair: &str, day: &str, id: &str) -> Result<Option<i32>, Error> {
        Ok(self
            .db
            .get(&format!("mc/{pair}/{day}/{id}"))
            .map_err(io)?
            .and_then(|b| b.try_into().ok().map(i32::from_be_bytes)))
    }

    /// Counters of all our messages of one day.
    pub fn mcs(&self, pair: &str, day: &str) -> Result<Vec<(String, i32)>, Error> {
        let prefix = format!("mc/{pair}/{day}/");
        Ok(self
            .db
            .scan(&prefix)
            .map_err(io)?
            .into_iter()
            .filter_map(|(k, v)| Some((k.strip_prefix(&prefix)?.to_string(), i32::from_be_bytes(v.try_into().ok()?))))
            .collect())
    }

    pub fn put_mc_op(&self, pair: &str, day: &str, id: &str, c: i32) -> Result<Op, Error> {
        self.db.put_op(&format!("mc/{pair}/{day}/{id}"), &c.to_be_bytes()).map_err(io)
    }

    pub fn out_op(&self, pair: &str, day: &str, pending: bool) -> Result<Op, Error> {
        let path = format!("out/{pair}/{day}");
        if pending { self.db.put_op(&path, b"").map_err(io) } else { Ok(Op::Delete(path)) }
    }

    /// Days with ops the peer has not acknowledged.
    pub fn pending_days(&self, pair: &str) -> Result<Vec<String>, Error> {
        let prefix = format!("out/{pair}/");
        Ok(self
            .db
            .list_prefixed(&prefix)
            .map_err(io)?
            .into_iter()
            .filter_map(|k| k.strip_prefix(&prefix).map(str::to_string))
            .collect())
    }

    // ---- blobs -----------------------------------------------------------------------------

    pub fn blob(&self, hash: &str) -> Result<Option<BlobInfo>, Error> {
        self.get_json(&format!("blob/{hash}"))
    }

    pub fn put_blob_op(&self, hash: &str, b: &BlobInfo) -> Result<Op, Error> {
        self.db.put_op(&format!("blob/{hash}"), &json(b)).map_err(io)
    }

    pub fn ref_ops(&self, hash: &str, pair: &str) -> Result<Vec<Op>, Error> {
        Ok(vec![
            self.db.put_op(&format!("ref/{hash}/{pair}"), b"").map_err(io)?,
            self.db.put_op(&format!("pb/{pair}/{hash}"), b"").map_err(io)?,
        ])
    }

    pub fn may_read(&self, hash: &str, pair: &str) -> bool {
        matches!(self.db.raw().get(&format!("ref/{hash}/{pair}")), Ok(Some(_)))
    }

    /// Blobs the user wants (or auto-download chose) that are not complete yet.
    pub fn wanted_blobs(&self) -> Result<Vec<(String, BlobInfo)>, Error> {
        let mut out = Vec::new();
        for (k, v) in self.db.scan("blob/").map_err(io)? {
            let b: BlobInfo = serde_json::from_slice(&v)?;
            if b.wanted && !b.ready {
                out.push((k.trim_start_matches("blob/").to_string(), b));
            }
        }
        Ok(out)
    }

    pub fn conv_blobs(&self, pair: &str) -> Result<Vec<String>, Error> {
        let prefix = format!("pb/{pair}/");
        Ok(self
            .db
            .list_prefixed(&prefix)
            .map_err(io)?
            .into_iter()
            .filter_map(|k| k.strip_prefix(&prefix).map(str::to_string))
            .collect())
    }

    pub fn snapcache(&self, pair: &str, day: &str) -> Result<Option<SnapRef>, Error> {
        self.get_json(&format!("snapcache/{pair}/{day}"))
    }

    pub fn put_snapcache_op(&self, pair: &str, day: &str, s: &SnapRef) -> Result<Op, Error> {
        self.db.put_op(&format!("snapcache/{pair}/{day}"), &json(s)).map_err(io)
    }

    // ---- settings ----------------------------------------------------------------------------

    pub fn auto_download(&self) -> u64 {
        self.db
            .get("set/auto_download")
            .ok()
            .flatten()
            .and_then(|b| b.try_into().ok().map(u64::from_be_bytes))
            .unwrap_or(DEFAULT_AUTO_DOWNLOAD)
    }

    pub fn set_auto_download(&self, bytes: u64) -> Result<(), Error> {
        self.db.put("set/auto_download", &bytes.to_be_bytes()).map_err(io)
    }

    /// Everything about one conversation, except the blobs' bytes (the caller releases those).
    pub fn delete_conversation(&self, pair: &str, hashes: &[String]) -> Result<(), Error> {
        let mut ops = vec![
            Op::Delete(format!("conv/{pair}")),
            Op::DeletePrefix(format!("shard/{pair}/")),
            Op::DeletePrefix(format!("ack/{pair}/")),
            Op::DeletePrefix(format!("mc/{pair}/")),
            Op::DeletePrefix(format!("out/{pair}/")),
            Op::DeletePrefix(format!("pb/{pair}/")),
            Op::DeletePrefix(format!("snapcache/{pair}/")),
            Op::DeletePrefix(format!("mi/{pair}/")),
        ];
        for h in hashes {
            ops.push(Op::Delete(format!("ref/{h}/{pair}")));
            ops.push(Op::Delete(format!("blob/{h}")));
        }
        self.db.apply(&ops).map_err(io)
    }
}

impl ChatStore {
    /// Message id -> the day it lives in.
    pub fn mi(&self, pair: &str, id: &str) -> Result<Option<String>, Error> {
        Ok(self.db.get(&format!("mi/{pair}/{id}")).map_err(io)?.and_then(|b| String::from_utf8(b).ok()))
    }

    pub fn mi_op(&self, pair: &str, id: &str, day: &str) -> Result<Op, Error> {
        self.db.put_op(&format!("mi/{pair}/{id}"), day.as_bytes()).map_err(io)
    }
}
