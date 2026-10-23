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
pub const V2: &[u8] = b"2";

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
pub fn phrase_key(domain: &str, phrase: &str) -> Secret32 {
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
