use std::path::Path;
use std::sync::Arc;

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

#[derive(Clone)]
pub struct Store {
    db: Arc<Database>,
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
        Ok(Self { db: Arc::new(db) })
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let txn = self.db.begin_read()?;
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
        let txn = self.db.begin_write()?;
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
        let txn = self.db.begin_read()?;
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
        let txn = self.db.begin_read()?;
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
