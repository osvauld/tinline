use std::sync::Arc;

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
    key: Arc<Zeroizing<[u8; 32]>>,
}

impl Sealed {
    pub fn new(store: Store, key: [u8; 32]) -> Self {
        Self { store, key: Arc::new(Zeroizing::new(key)) }
    }

    pub fn raw(&self) -> &Store {
        &self.store
    }

    pub fn seal(&self, path: &str, value: &[u8]) -> Result<Vec<u8>, StorageError> {
        let mut plain = Zeroizing::new(Vec::with_capacity(2 + path.len() + value.len()));
        plain.extend_from_slice(&(path.len() as u16).to_be_bytes());
        plain.extend_from_slice(path.as_bytes());
        plain.extend_from_slice(value);
        cryptography::aead::encrypt(&self.key, &plain).map_err(|_| StorageError::Unseal(path.into()))
    }

    pub fn unseal(&self, path: &str, sealed: &[u8]) -> Result<Vec<u8>, StorageError> {
        let bad = || StorageError::Unseal(path.to_string());
        let plain = Zeroizing::new(cryptography::aead::decrypt(&self.key, sealed).map_err(|_| bad())?);
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
mod tests {
    use super::*;

    fn sealed(key: u8) -> Sealed {
        Sealed::new(Store::open_in_memory().unwrap(), [key; 32])
    }

    #[test]
    fn round_trip_and_ciphertext_is_opaque() {
        let s = sealed(1);
        s.put("dm/a/2026-10-07/snap", b"hello world").unwrap();
        assert_eq!(s.get("dm/a/2026-10-07/snap").unwrap().unwrap(), b"hello world");
        let raw = s.raw().get("dm/a/2026-10-07/snap").unwrap().unwrap();
        assert!(!raw.windows(5).any(|w| w == b"hello"));
        assert_eq!(s.get("missing").unwrap(), None);
    }

    #[test]
    fn wrong_key_and_moved_records_fail() {
        let s = sealed(1);
        s.put("a", b"1").unwrap();
        s.put("b", b"2").unwrap();
        let other = Sealed::new(s.raw().clone(), [2; 32]);
        assert!(matches!(other.get("a"), Err(StorageError::Unseal(_))));
        let a = s.raw().get("a").unwrap().unwrap();
        s.raw().put("b", &a).unwrap();
        assert!(matches!(s.get("b"), Err(StorageError::Unseal(_))));
        let mut x = s.raw().get("a").unwrap().unwrap();
        *x.last_mut().unwrap() ^= 1;
        s.raw().put("a", &x).unwrap();
        assert!(s.get("a").is_err());
    }

    #[test]
    fn batch_prefix_scan_and_delete() {
        let s = sealed(3);
        let ops = vec![
            s.put_op("x/1", b"one").unwrap(),
            s.put_op("x/2", b"two").unwrap(),
            s.put_op("y/1", b"other").unwrap(),
        ];
        s.apply(&ops).unwrap();
        let got = s.scan("x/").unwrap();
        assert_eq!(got, vec![("x/1".into(), b"one".to_vec()), ("x/2".into(), b"two".to_vec())]);
        assert_eq!(s.list_prefixed("").unwrap().len(), 3);
        s.delete_prefix("x/").unwrap();
        assert_eq!(s.list_prefixed("").unwrap(), vec!["y/1".to_string()]);
        s.apply(&[Op::Delete("y/1".into())]).unwrap();
        assert!(s.list_prefixed("").unwrap().is_empty());
    }

    #[test]
    fn survives_reopen() {
        let dir = std::env::temp_dir().join(format!("storage-reopen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("db.redb");
        {
            let s = Sealed::new(Store::open(&path).unwrap(), [9; 32]);
            s.put("k", b"v").unwrap();
        }
        let s = Sealed::new(Store::open(&path).unwrap(), [9; 32]);
        assert_eq!(s.get("k").unwrap().unwrap(), b"v");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
