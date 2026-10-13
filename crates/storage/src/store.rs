use std::path::Path;
use std::sync::{Arc, RwLock, RwLockReadGuard};

use redb::{Database, ReadableTable, TableDefinition};

use crate::error::StorageError;

// One keyspace for everything. The hierarchical path lives entirely in the key; redb keeps
// keys sorted, so prefix and range scans are cheap.
const TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("store");

/// One step of an atomic write.
#[derive(Debug, Clone)]
pub enum Op {
    Put(String, Vec<u8>),
    Delete(String),
    /// Every key that begins with the prefix.
    DeletePrefix(String),
}

/// Clones share one database. [`close`](Store::close) lets the file go for every clone at
/// once (after the transactions in flight); every later use fails with
/// [`StorageError::Closed`]. Without it the file is held until the last clone is dropped.
#[derive(Clone)]
pub struct Store {
    db: Arc<RwLock<Option<Database>>>,
    /// The file (none for the in-memory backend), for [`usage`](Store::usage).
    path: Option<Arc<std::path::PathBuf>>,
}

/// How much of the file is in use, see [`Store::usage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    /// Length of the file on disk.
    pub file_bytes: u64,
    /// Bytes of the file that hold live pages (data, tree metadata, bookkeeping).
    pub used_bytes: u64,
    /// redb's own breakdown: (leaf pages, branch pages, stored leaf bytes, tree metadata
    /// bytes, fragmented bytes inside pages and free pages).
    pub detail: (u64, u64, u64, u64, u64),
}

impl Usage {
    /// Bytes of the file that hold nothing live (free pages, growth steps).
    pub fn wasted(&self) -> u64 {
        self.file_bytes.saturating_sub(self.used_bytes)
    }
}

/// The open database, for the length of one operation.
struct Open<'a>(RwLockReadGuard<'a, Option<Database>>);

impl std::ops::Deref for Open<'_> {
    type Target = Database;
    fn deref(&self) -> &Database {
        self.0.as_ref().expect("checked open")
    }
}

impl Store {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let path = path.as_ref();
        let mut s = Self::init(Database::create(path)?)?;
        s.path = Some(Arc::new(path.to_path_buf()));
        Ok(s)
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        let db = Database::builder().create_with_backend(redb::backends::InMemoryBackend::new())?;
        Self::init(db)
    }

    // Create the table eagerly so a read before the first write doesn't hit a missing table.
    fn init(db: Database) -> Result<Self, StorageError> {
        let txn = db.begin_write()?;
        txn.open_table(TABLE)?;
        txn.commit()?;
        Ok(Self { db: Arc::new(RwLock::new(Some(db))), path: None })
    }

    fn db(&self) -> Result<Open<'_>, StorageError> {
        let g = self.db.read().unwrap_or_else(|e| e.into_inner());
        if g.is_none() {
            return Err(StorageError::Closed);
        }
        Ok(Open(g))
    }

    /// Closes the database for every clone: waits for the operations in flight, then lets the
    /// file go, so it can be opened again at once. Idempotent.
    pub fn close(&self) {
        let db = self.db.write().unwrap_or_else(|e| e.into_inner()).take();
        drop(db);
    }

    /// File length and the bytes of it in use; `None` for the in-memory backend. Reads the
    /// whole tree's statistics, so it is not for a hot path.
    pub fn usage(&self) -> Result<Option<Usage>, StorageError> {
        let Some(path) = &self.path else { return Ok(None) };
        let db = self.db()?;
        // Pages freed by earlier commits are only handed back to the allocator by later ones:
        // two empty commits first, or the count includes them.
        for _ in 0..2 {
            let txn = db.begin_write()?;
            txn.commit()?;
        }
        let txn = db.begin_write()?;
        let st = txn.stats()?;
        txn.abort()?;
        let file_bytes = std::fs::metadata(path.as_path()).map(|m| m.len()).unwrap_or(0);
        Ok(Some(Usage { file_bytes, used_bytes: st.allocated_pages() * st.page_size() as u64, detail: (st.leaf_pages(), st.branch_pages(), st.stored_bytes(), st.metadata_bytes(), st.fragmented_bytes()) }))
    }

    /// Rewrites the file without its free pages and gives the space back. Waits for the
    /// operations in flight and holds every other one off until it is done (it is slow:
    /// seconds for hundreds of MB), so run it in the background, off any latency path.
    /// Every step is an ordinary two-phase commit: a crash in the middle leaves a database
    /// that opens (after redb's own repair) with all its data. Returns whether pages moved.
    pub fn compact(&self) -> Result<bool, StorageError> {
        let mut g = self.db.write().unwrap_or_else(|e| e.into_inner());
        let Some(db) = g.as_mut() else { return Err(StorageError::Closed) };
        Ok(db.compact()?)
    }

    /// Runs `f` while the store is held exclusively, as a compaction holds it (tests of what
    /// must not wait for the file).
    #[doc(hidden)]
    pub fn while_exclusive<R>(&self, f: impl FnOnce() -> R) -> R {
        let _g = self.db.write().unwrap_or_else(|e| e.into_inner());
        f()
    }

    pub fn is_closed(&self) -> bool {
        self.db.read().unwrap_or_else(|e| e.into_inner()).is_none()
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let db = self.db()?;
        let txn = db.begin_read()?;
        let table = txn.open_table(TABLE)?;
        Ok(table.get(key)?.map(|value| value.value().to_vec()))
    }

    pub fn put(&self, key: &str, value: &[u8]) -> Result<(), StorageError> {
        self.apply(&[Op::Put(key.to_string(), value.to_vec())])
    }

    pub fn delete(&self, key: &str) -> Result<(), StorageError> {
        self.apply(&[Op::Delete(key.to_string())])
    }

    pub fn delete_prefix(&self, prefix: &str) -> Result<(), StorageError> {
        self.apply(&[Op::DeletePrefix(prefix.to_string())])
    }

    /// All the ops in one transaction: after a crash either every one happened or none did.
    pub fn apply(&self, ops: &[Op]) -> Result<(), StorageError> {
        let db = self.db()?;
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(TABLE)?;
            for op in ops {
                match op {
                    Op::Put(k, v) => {
                        table.insert(k.as_str(), v.as_slice())?;
                    }
                    Op::Delete(k) => {
                        table.remove(k.as_str())?;
                    }
                    Op::DeletePrefix(p) => {
                        let mut keys = Vec::new();
                        for item in table.range(p.as_str()..)? {
                            let (key, _) = item?;
                            if !key.value().starts_with(p.as_str()) {
                                break;
                            }
                            keys.push(key.value().to_string());
                        }
                        for k in keys {
                            table.remove(k.as_str())?;
                        }
                    }
                }
            }
        }
        txn.commit()?;
        Ok(())
    }

    /// Every key that begins with `prefix`, in sorted order.
    pub fn list_prefixed(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
        let db = self.db()?;
        let txn = db.begin_read()?;
        let table = txn.open_table(TABLE)?;
        let mut keys = Vec::new();
        for item in table.range(prefix..)? {
            let (key, _) = item?;
            let key = key.value();
            if !key.starts_with(prefix) {
                break;
            }
            keys.push(key.to_string());
        }
        Ok(keys)
    }

    /// At most `limit` keys under `prefix` that sort after `after` (all of them from the start
    /// with `None`), in sorted order: one page of a prefix scan.
    pub fn keys_after(&self, prefix: &str, after: Option<&str>, limit: usize) -> Result<Vec<String>, StorageError> {
        let db = self.db()?;
        let txn = db.begin_read()?;
        let table = txn.open_table(TABLE)?;
        let start = after.filter(|a| *a >= prefix).unwrap_or(prefix);
        let mut keys = Vec::new();
        for item in table.range(start..)? {
            let (key, _) = item?;
            let key = key.value();
            if !key.starts_with(prefix) || keys.len() >= limit {
                break;
            }
            if after == Some(key) {
                continue;
            }
            keys.push(key.to_string());
        }
        Ok(keys)
    }

    /// Keys and values under `prefix`, sorted.
    pub fn scan(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>, StorageError> {
        let db = self.db()?;
        let txn = db.begin_read()?;
        let table = txn.open_table(TABLE)?;
        let mut out = Vec::new();
        for item in table.range(prefix..)? {
            let (key, value) = item?;
            let key = key.value();
            if !key.starts_with(prefix) {
                break;
            }
            out.push((key.to_string(), value.value().to_vec()));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_after_pages_a_prefix() {
        let s = Store::open_in_memory().unwrap();
        for k in ["a/1", "a/2", "a/3", "b/1"] {
            s.put(k, b"").unwrap();
        }
        assert_eq!(s.keys_after("a/", None, 2).unwrap(), ["a/1", "a/2"]);
        assert_eq!(s.keys_after("a/", Some("a/2"), 5).unwrap(), ["a/3"]);
        assert!(s.keys_after("a/", Some("a/3"), 5).unwrap().is_empty());
        assert_eq!(s.keys_after("b/", Some("a/9"), 5).unwrap(), ["b/1"]);
    }

    // KIND: invariant (GL) — `close` lets the file go for every clone at once: the next owner opens
    // it without waiting, and the old clones fail cleanly instead of holding it.
    #[test]
    fn close_releases_the_file_for_every_clone() {
        let dir = std::env::temp_dir().join(format!("storage-close-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.redb");
        let a = Store::open(&path).unwrap();
        let b = a.clone();
        a.put("k", b"v").unwrap();
        assert!(Store::open(&path).is_err(), "held");
        b.close();
        assert!(a.is_closed());
        assert!(matches!(a.get("k"), Err(StorageError::Closed)));
        assert!(matches!(a.put("k", b"w"), Err(StorageError::Closed)));
        let c = Store::open(&path).expect("opens at once after close");
        assert_eq!(c.get("k").unwrap().as_deref(), Some(&b"v"[..]));
        a.close();
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod size_probe {
    use super::*;

    // Probe (run by hand): bytes on disk for sorted small records, one transaction or many.
    #[test]
    #[ignore]
    fn probe() {
        for (name, per_txn) in [("one txn", 1_000_000usize), ("1000/txn", 1000)] {
            let dir = std::env::temp_dir().join(format!("probe-{}-{per_txn}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let s = Store::open(dir.join("p.redb")).unwrap();
            let n = 100_000usize;
            let mut raw = 0usize;
            let mut ops = Vec::new();
            for i in 0..n {
                let k = format!("x/{:08x}/{:016x}/000", i / 5000, (i as u64).wrapping_mul(0x9e3779b97f4a7c15));
                let v = vec![7u8; 60];
                raw += k.len() + v.len();
                ops.push(Op::Put(k, v));
                if ops.len() == per_txn {
                    s.apply(&ops).unwrap();
                    ops.clear();
                }
            }
            s.apply(&ops).unwrap();
            let u = s.usage().unwrap().unwrap();
            println!("{name}: raw {raw} file {} used {}", u.file_bytes, u.used_bytes);
            s.compact().unwrap();
            let u = s.usage().unwrap().unwrap();
            println!("{name} compacted: file {} used {}", u.file_bytes, u.used_bytes);
        }
    }
}
