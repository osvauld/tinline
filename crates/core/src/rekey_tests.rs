use cryptography::Secret32;
use storage::Store;

use super::*;

/// Store keys must never drift: these bytes open existing account/chat/group/search files.
/// Expected values come from the reference BLAKE3 (Python `blake3`), not from this code.
#[test]
fn phrase_keys_are_byte_identical() {
    let phrase = " Alpha  beta\tGAMMA\n";
    let hex = |k: Secret32| hex::encode(*k);
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
