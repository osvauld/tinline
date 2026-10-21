//! The group vault (`group.redb`). Its rows and accessors live in `group_core::store`,
//! re-exported here; this module opens the file and derives its key from the recovery phrase.

use std::path::PathBuf;

pub use group_core::store::*;
use storage::Store;

use crate::Error;

/// The store key, from the recovery phrase (see `rekey::phrase_key`).
pub fn key_from_phrase(phrase: &str) -> cryptography::Secret32 {
    crate::rekey::phrase_key(STORE_DOMAIN, phrase)
}

/// Opens (creating) the group vault at `path`, sealed under the phrase's key.
pub fn open(path: PathBuf, phrase: &str) -> Result<GroupStore, Error> {
    // A store of the previous session may still be closing (its last tasks hold it).
    let mut tries = 0;
    let store = loop {
        match Store::open(&path) {
            Ok(s) => {
                crate::store::note_open(&path, tries > 0);
                break s;
            }
            Err(e) if tries >= 50 => return Err(e.into()),
            Err(_) => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    };
    Ok(GroupStore::new(store, key_from_phrase(phrase)))
}
