use std::sync::Arc;

use cryptography::Secret32;
use zeroize::Zeroizing;

use crate::{Op, StorageError, Store};

/// A [`Store`] whose values are sealed under one key (Tinline's data key).
///
/// A sealed value is `AES-256-GCM(key, path_len(u16 BE) ‖ path ‖ value)`; opening checks the
/// embedded path against the one asked for, so swapping two records on disk is detected.
/// Keys (paths) are not hidden; they carry no message content (pair hash, day, ids).
#[derive(Clone)]
pub struct Sealed {
    store: Store,
    key: Arc<Secret32>,
}

impl Sealed {
    pub fn new(store: Store, key: impl Into<Secret32>) -> Self {
        Self { store, key: Arc::new(key.into()) }
    }

    pub fn raw(&self) -> &Store {
        &self.store
    }

    /// [`Store::close`] of the store underneath (for every clone).
    pub fn close(&self) {
        self.store.close();
    }

    pub fn seal(&self, path: &str, value: &[u8]) -> Result<Vec<u8>, StorageError> {
        let mut plain = Zeroizing::new(Vec::with_capacity(2 + path.len() + value.len()));
        plain.extend_from_slice(&(path.len() as u16).to_be_bytes());
        plain.extend_from_slice(path.as_bytes());
        plain.extend_from_slice(value);
        cryptography::aead::seal(&self.key, &[], &plain).map_err(|_| StorageError::Unseal(path.into()))
    }

    pub fn unseal(&self, path: &str, sealed: &[u8]) -> Result<Vec<u8>, StorageError> {
        let bad = || StorageError::Unseal(path.to_string());
        let plain = Zeroizing::new(cryptography::aead::open(&self.key, &[], sealed).map_err(|_| bad())?);
        let n = u16::from_be_bytes(plain.get(..2).ok_or_else(bad)?.try_into().unwrap()) as usize;
        if plain.get(2..2 + n) != Some(path.as_bytes()) {
            return Err(bad());
        }
        Ok(plain[2 + n..].to_vec())
    }

    pub fn get(&self, path: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.store.get(path)?.map(|s| self.unseal(path, &s)).transpose()
    }

    pub fn put(&self, path: &str, value: &[u8]) -> Result<(), StorageError> {
        self.store.put(path, &self.seal(path, value)?)
    }

    /// A put for use inside [`apply`](Self::apply).
    pub fn put_op(&self, path: &str, value: &[u8]) -> Result<Op, StorageError> {
        Ok(Op::Put(path.to_string(), self.seal(path, value)?))
    }

    pub fn apply(&self, ops: &[Op]) -> Result<(), StorageError> {
        self.store.apply(ops)
    }

    pub fn delete(&self, path: &str) -> Result<(), StorageError> {
        self.store.delete(path)
    }

    pub fn delete_prefix(&self, prefix: &str) -> Result<(), StorageError> {
        self.store.delete_prefix(prefix)
    }

    pub fn list_prefixed(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
        self.store.list_prefixed(prefix)
    }

    /// [`Store::keys_after`]: one page of the keys under `prefix`.
    pub fn keys_after(&self, prefix: &str, after: Option<&str>, limit: usize) -> Result<Vec<String>, StorageError> {
        self.store.keys_after(prefix, after, limit)
    }

    /// Opened values under `prefix`, sorted by path. A record that does not open fails the scan.
    pub fn scan(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>, StorageError> {
        self.store
            .scan(prefix)?
            .into_iter()
            .map(|(k, v)| self.unseal(&k, &v).map(|v| (k, v)))
            .collect()
    }
}

#[cfg(test)]
mod tests;
