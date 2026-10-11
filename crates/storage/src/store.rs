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
        Self::init(Database::create(path)?)
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
        Ok(Self { db: Arc::new(RwLock::new(Some(db))) })
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
