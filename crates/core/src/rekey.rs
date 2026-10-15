//! The keys of the two sealed stores (`account.redb`, `chat.redb`).
//!
//! The keys are `BLAKE3-derive-key(domain, normalised phrase)`, so the phrase alone reopens both
//! files (forgetting the passphrase loses nothing while the 24 words are kept). They stay
//! protected by the passphrase because the phrase itself is only on disk inside the vault.
//!
//! Format marker: a store keyed by the phrase carries the unsealed row [`KEYVER`] = `"2"`. A store
//! without it must be empty (fresh: the marker is written at once); anything else is refused.

use storage::{Sealed, Store};
use cryptography::Secret32;
use zeroize::Zeroizing;

use crate::Error;

/// Raw (unsealed) row marking a store sealed under the phrase-derived key.
pub const KEYVER: &str = "~keyver";
const V2: &[u8] = b"2";

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

/// Opens a store under the phrase key. A fresh store gets the marker; a store without it that
/// has rows (sealed under some other key) is an error, never touched.
pub fn open(store: Store, key: impl Into<Secret32>) -> Result<Sealed, Error> {
    let key = key.into();
    match store.get(KEYVER)?.as_deref() {
        Some(v) if v == V2 => return Ok(Sealed::new(store, key)),
        Some(_) => return Err(Error::Io("store written by a newer version".into())),
        None => {}
    }
    if !store.list_prefixed("")?.is_empty() {
        return Err(Error::Io("store is not sealed under the phrase key".into()));
    }
    store.put(KEYVER, V2)?;
    Ok(Sealed::new(store, key))
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

    #[test]
    fn a_fresh_store_is_marked_and_an_unmarked_one_is_refused() {
        let s = Store::open_in_memory().unwrap();
        let k = key_from_phrase(Which::Account, "x y");
        open(s.clone(), k.clone()).unwrap().put("state", b"s").unwrap();
        assert_eq!(s.get(KEYVER).unwrap().as_deref(), Some(V2));
        assert_eq!(open(s, k.clone()).unwrap().get("state").unwrap().unwrap(), b"s");

        let other = Store::open_in_memory().unwrap();
        other.put("r", b"1").unwrap();
        assert!(open(other.clone(), k).is_err());
        assert!(other.get(KEYVER).unwrap().is_none(), "untouched");
    }
}
