//! Shared test setup: receipt ledgers over in-memory or temporary on-disk sealed stores.
use crate::store::ReceiptStore;
use std::{path::PathBuf, sync::Arc};
use storage::{Sealed, Store};

/// A ledger factory over a fresh in-memory store sealed under `[key; 32]`.
pub fn memory(key: u8) -> Arc<ReceiptStore> {
    ReceiptStore::new(Sealed::new(Store::open_in_memory().unwrap(), [key; 32]))
}

/// A fresh directory `{prefix}-{pid}-{random}` under the temp dir, removed on drop.
pub struct TempDir(pub PathBuf);
impl TempDir {
    pub fn new(prefix: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            hex::encode(proto::new_device_secret())
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
