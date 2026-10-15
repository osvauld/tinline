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
use cryptography::Secret32;
use zeroize::Zeroizing;

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

/// The recovery phrase as every store key sees it: words lower-cased, single-spaced.
fn norm_phrase(phrase: &str) -> Zeroizing<String> {
    Zeroizing::new(phrase.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>().join(" "))
}

/// A store key: BLAKE3 derive-key of the normalised phrase under `domain`. The one derivation
/// behind every phrase-keyed store (account, chat, group, search); see the golden test.
pub(crate) fn phrase_key(domain: &str, phrase: &str) -> Secret32 {
    cryptography::kdf::derive(domain, norm_phrase(phrase).as_bytes())
}

/// The new key of a store, from the recovery phrase.
pub fn key_from_phrase(which: Which, phrase: &str) -> Secret32 {
    phrase_key(
        match which {
            Which::Account => ACCOUNT_DOMAIN_V2,
            Which::Chat => CHAT_DOMAIN_V2,
        },
        phrase,
    )
}

/// The pre-34f key, from the vault's data key.
pub fn old_key(which: Which, dek: &[u8; 32]) -> Secret32 {
    cryptography::kdf::derive(
        match which {
            Which::Account => ACCOUNT_DOMAIN_V1,
            Which::Chat => CHAT_DOMAIN_V1,
        },
        dek,
    )
}

/// Whether the file holds data that is not yet under the phrase key.
pub fn needs_migration(store: &Store) -> Result<bool, Error> {
    if store.get(KEYVER)?.is_some() {
        return Ok(false);
    }
    Ok(!store.list_prefixed("")?.is_empty())
}

/// The rows re-sealed from `old` to `new`, plus the marker; nothing is written.
fn plan(store: &Store, old: &Sealed, new: &Sealed) -> Result<Vec<Op>, Error> {
    let mut ops = Vec::new();
    for (path, raw) in store.scan("")? {
        let plain = zeroize::Zeroizing::new(old.unseal(&path, &raw)?);
        ops.push(new.put_op(&path, &plain)?);
    }
    ops.push(Op::Put(KEYVER.into(), V2.to_vec()));
    Ok(ops)
}

/// Opens a store under the phrase key, migrating first when it still has the old one. With
/// `old = None` an old-keyed store is an error (`Io`), never touched.
pub fn open(store: Store, new_key: impl Into<Secret32>, old: Option<Secret32>) -> Result<Sealed, Error> {
    let new_key = new_key.into();
    match store.get(KEYVER)?.as_deref() {
        Some(v) if v == V2 => return Ok(Sealed::new(store, new_key)),
        Some(_) => return Err(Error::Io("store written by a newer version".into())),
        None => {}
    }
    if store.list_prefixed("")?.is_empty() {
        store.put(KEYVER, V2)?;
        return Ok(Sealed::new(store, new_key));
    }
    let Some(old_key) = old else {
        return Err(Error::Io("store is sealed under the previous key".into()));
    };
    let ops = plan(&store, &Sealed::new(store.clone(), old_key), &Sealed::new(store.clone(), new_key.clone()))?;
    store.apply(&ops)?;
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
            Err(e) if tries >= 20 => return Err(Error::from(e)),
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

    /// Store keys must never drift: these bytes open existing account/chat/group/search files.
    /// Expected values come from the reference BLAKE3 (Python `blake3`), not from this code.
    #[test]
    fn phrase_keys_are_byte_identical() {
        let phrase = " Alpha  beta\tGAMMA\n";
        let hex = |k: Secret32| k.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(key_from_phrase(Which::Account, phrase)), "aaede98680b9849ea1cc82ade454ce6a6a160007faf952812676085851f8474a");
        assert_eq!(hex(key_from_phrase(Which::Chat, phrase)), "ff534a58745f63af85c6170d22180e758d0a057add803ebfc4c1113838aef9fd");
        assert_eq!(hex(crate::group::store::key_from_phrase(phrase)), "ecbbddb539af581bebf460ddd0c12c59b7cbd3e48d5b81db4d40d5ceb4938bcc");
        assert_eq!(hex(phrase_key("tinline/search-store/v1", phrase)), "e4bd1ac222796ceafe8b72b613ce4a7f6082d9967c87216dce4def76ba90dd0e");
        assert_eq!(hex(phrase_key("tinline/search-index/v1", phrase)), "6f9b78a5763af28178b56f8763228f8b80e740aa958ae2fff5f9c0f823c51356");
    }

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
        assert_eq!(new.expose(), key_from_phrase(Which::Chat, "word other").expose());
        assert!(needs_migration(&store).unwrap());
        assert!(open(store.clone(), new.clone(), None).is_err(), "no old key: untouched");
        assert!(needs_migration(&store).unwrap());
        // Wrong old key: nothing changes.
        assert!(open(store.clone(), new.clone(), Some([1; 32].into())).is_err());
        assert!(needs_migration(&store).unwrap());
        // "Interrupted": a plan that was computed but never applied leaves the old file.
        let _ = plan(&store, &Sealed::new(store.clone(), old_key(Which::Chat, &dek)), &Sealed::new(store.clone(), new.clone())).unwrap();
        assert!(needs_migration(&store).unwrap());
        let sealed = open(store.clone(), new.clone(), Some(old_key(Which::Chat, &dek))).unwrap();
        assert_eq!(sealed.get("a/1").unwrap().unwrap(), b"one");
        assert_eq!(sealed.get("b").unwrap().unwrap(), b"two");
        assert!(!needs_migration(&store).unwrap());
        // Done: opens without the old key; the old key no longer opens anything.
        let again = open(store.clone(), new.clone(), None).unwrap();
        assert_eq!(again.get("b").unwrap().unwrap(), b"two");
        assert!(Sealed::new(store, old_key(Which::Chat, &dek)).get("b").is_err());
    }

    #[test]
    fn a_fresh_store_is_marked_and_a_corrupt_row_blocks_migration() {
        let s = Store::open_in_memory().unwrap();
        let k = key_from_phrase(Which::Account, "x y");
        open(s.clone(), k.clone(), None).unwrap().put("state", b"s").unwrap();
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
