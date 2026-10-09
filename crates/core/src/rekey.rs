//! The keys of the two sealed stores (`account.redb`, `chat.redb`) and the one-time move from the
//! old key (derived from the vault's data key) to the new one (derived from the recovery phrase).
//!
//! Why: with the data key as the root, forgetting the passphrase (and having no remembered key)
//! lost contacts, history and chats even though the user still had the 24 words. The new keys are
//! `BLAKE3-derive-key(domain, normalised phrase)`, so the phrase alone reopens both files. They
//! stay protected by the passphrase because the phrase itself is only on disk inside the vault.
//!
//! Format marker: a store keyed by the phrase carries the unsealed row [`KEYVER`] = `"2"`. A store
//! without it is either empty (fresh: the marker is written at once) or still under the old key.
//! Migration reads every row, opens it with the old key, seals it with the new key and writes all
//! of them plus the marker in ONE redb transaction, so a crash leaves the old file or the new one,
//! never a mix; running again after an interruption simply starts over from the old file. The
//! cost is that the whole store passes through memory once (chat history of several MB; fine).

use storage::{Op, Sealed, Store};

use crate::Error;

/// Raw (unsealed) row marking a store sealed under the phrase-derived key.
pub const KEYVER: &str = "~keyver";
const V2: &[u8] = b"2";

const ACCOUNT_DOMAIN_V1: &str = "tinline/account-store/v1";
const CHAT_DOMAIN_V1: &str = "tinline chat store v1";
const ACCOUNT_DOMAIN_V2: &str = "tinline/account-store/v2";
const CHAT_DOMAIN_V2: &str = "tinline/chat-store/v2";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Account,
    Chat,
}

/// The new key of a store, from the recovery phrase (words lower-cased, single-spaced).
pub fn key_from_phrase(which: Which, phrase: &str) -> [u8; 32] {
    let norm = phrase.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>().join(" ");
    let domain = match which {
        Which::Account => ACCOUNT_DOMAIN_V2,
        Which::Chat => CHAT_DOMAIN_V2,
    };
    blake3::derive_key(domain, norm.as_bytes())
}

/// The pre-34f key, from the vault's data key.
pub fn old_key(which: Which, dek: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(
        match which {
            Which::Account => ACCOUNT_DOMAIN_V1,
            Which::Chat => CHAT_DOMAIN_V1,
        },
        dek,
    )
}

fn io(e: storage::StorageError) -> Error {
    Error::Io(e.to_string())
}

/// Whether the file holds data that is not yet under the phrase key.
pub fn needs_migration(store: &Store) -> Result<bool, Error> {
    if store.get(KEYVER).map_err(io)?.is_some() {
        return Ok(false);
    }
    Ok(!store.list_prefixed("").map_err(io)?.is_empty())
}

/// The rows re-sealed from `old` to `new`, plus the marker; nothing is written.
fn plan(store: &Store, old: &Sealed, new: &Sealed) -> Result<Vec<Op>, Error> {
    let mut ops = Vec::new();
    for (path, raw) in store.scan("").map_err(io)? {
        let plain = zeroize::Zeroizing::new(old.unseal(&path, &raw).map_err(io)?);
        ops.push(new.put_op(&path, &plain).map_err(io)?);
    }
    ops.push(Op::Put(KEYVER.into(), V2.to_vec()));
    Ok(ops)
}

/// Opens a store under the phrase key, migrating first when it still has the old one. With
/// `old = None` an old-keyed store is an error (`Io`), never touched.
pub fn open(store: Store, new_key: [u8; 32], old: Option<[u8; 32]>) -> Result<Sealed, Error> {
    match store.get(KEYVER).map_err(io)?.as_deref() {
        Some(v) if v == V2 => return Ok(Sealed::new(store, new_key)),
        Some(_) => return Err(Error::Io("store written by a newer version".into())),
        None => {}
    }
    if store.list_prefixed("").map_err(io)?.is_empty() {
        store.put(KEYVER, V2).map_err(io)?;
        return Ok(Sealed::new(store, new_key));
    }
    let Some(old_key) = old else {
        return Err(Error::Io("store is sealed under the previous key".into()));
    };
    let ops = plan(&store, &Sealed::new(store.clone(), old_key), &Sealed::new(store.clone(), new_key))?;
    store.apply(&ops).map_err(io)?;
    Ok(Sealed::new(store, new_key))
}

/// For a restore: whether the file at `path` is unreadable without the old key.
pub fn file_needs_migration(path: &std::path::Path) -> Result<bool, Error> {
    if !path.exists() {
        return Ok(false);
    }
    let mut tries = 0;
    let store = loop {
        match Store::open(path) {
            Ok(s) => break s,
            Err(e) if tries >= 20 => return Err(io(e)),
            Err(_) => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    };
    needs_migration(&store)
}

/// Sets a file that cannot be migrated aside (renamed, never deleted).
pub fn quarantine(path: &std::path::Path) -> Result<(), Error> {
    let mut to = path.as_os_str().to_owned();
    to.push(".unreadable");
    std::fs::rename(path, std::path::PathBuf::from(to))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn old_store(rows: &[(&str, &[u8])], dek: &[u8; 32]) -> Store {
        let s = Store::open_in_memory().unwrap();
        let sealed = Sealed::new(s.clone(), old_key(Which::Chat, dek));
        for (k, v) in rows {
            sealed.put(k, v).unwrap();
        }
        s
    }

    #[test]
    fn migrates_atomically_and_only_once() {
        let dek = [7u8; 32];
        let store = old_store(&[("a/1", b"one"), ("b", b"two")], &dek);
        let new = key_from_phrase(Which::Chat, "Word  other");
        assert_eq!(new, key_from_phrase(Which::Chat, "word other"));
        assert!(needs_migration(&store).unwrap());
        assert!(open(store.clone(), new, None).is_err(), "no old key: untouched");
        assert!(needs_migration(&store).unwrap());
        // Wrong old key: nothing changes.
        assert!(open(store.clone(), new, Some([1; 32])).is_err());
        assert!(needs_migration(&store).unwrap());
        // "Interrupted": a plan that was computed but never applied leaves the old file.
        let _ = plan(&store, &Sealed::new(store.clone(), old_key(Which::Chat, &dek)), &Sealed::new(store.clone(), new)).unwrap();
        assert!(needs_migration(&store).unwrap());
        let sealed = open(store.clone(), new, Some(old_key(Which::Chat, &dek))).unwrap();
        assert_eq!(sealed.get("a/1").unwrap().unwrap(), b"one");
        assert_eq!(sealed.get("b").unwrap().unwrap(), b"two");
        assert!(!needs_migration(&store).unwrap());
        // Done: opens without the old key; the old key no longer opens anything.
        let again = open(store.clone(), new, None).unwrap();
        assert_eq!(again.get("b").unwrap().unwrap(), b"two");
        assert!(Sealed::new(store, old_key(Which::Chat, &dek)).get("b").is_err());
    }

    #[test]
    fn a_fresh_store_is_marked_and_a_corrupt_row_blocks_migration() {
        let s = Store::open_in_memory().unwrap();
        let k = key_from_phrase(Which::Account, "x y");
        open(s.clone(), k, None).unwrap().put("state", b"s").unwrap();
        assert!(!needs_migration(&s).unwrap());

        let dek = [3u8; 32];
        let bad = old_store(&[("r", b"1")], &dek);
        let mut raw = bad.get("r").unwrap().unwrap();
        *raw.last_mut().unwrap() ^= 1;
        bad.put("r", &raw).unwrap();
        assert!(open(bad.clone(), k, Some(old_key(Which::Chat, &dek))).is_err());
        assert!(needs_migration(&bad).unwrap());
    }
}
