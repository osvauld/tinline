//! Shared setup of the unit tests: the keys of a phrase and synthetic rows.

use std::path::Path;

use crate::Error;
use crate::index::{DocRow, INDEX_DOMAIN, Keys, STORE_DOMAIN, SearchIndex};

/// 2026-01-01 00:00 UTC.
pub const T0: i64 = 1_767_225_600_000;

/// The keys p2pcore derives from an already normalised phrase.
pub fn keys(phrase: &str) -> Keys {
    Keys { store: cryptography::kdf::derive(STORE_DOMAIN, phrase.as_bytes()), index: cryptography::kdf::derive(INDEX_DOMAIN, phrase.as_bytes()) }
}

/// A message row of `src` by a fixed author in a fixed conversation.
pub fn row(src: &str, id: &str, at: i64, text: &str) -> DocRow {
    DocRow { src: src.into(), conv: "did:key:z".into(), day: crdt::day_of(at), id: id.into(), at, author: "did:key:a".into(), text: text.into() }
}

/// Opens (creating) an index file with the keys of `phrase`, waiting up to about 2 s for a
/// previous owner of the file (a killed child process) like p2pcore's opener.
pub fn open(path: &Path, phrase: &str) -> Result<SearchIndex, Error> {
    SearchIndex::open(path, keys(phrase), &open_store)
}

fn open_store(path: &Path) -> Result<storage::Store, Error> {
    let mut tries = 0;
    loop {
        match storage::Store::open(path) {
            Ok(db) => return Ok(db),
            Err(e) if tries >= 20 => return Err(e.into()),
            Err(_) => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}
