//! Golden redb file for `storage`, frozen BEFORE the redb 2 -> 4 upgrade.
//!
//! `tests/golden/v2.redb` was written by the pre-upgrade code (redb 2.6.4) via the `generate` test below
//! (`cargo test -p storage --test golden_redb -- --ignored generate`). It must keep opening and reading the
//! same rows after the upgrade (redb 4 may need a file-format upgrade on open; that is the migration this
//! fixture exists to prove). Tests work on a COPY because opening can modify the file. Do not regenerate it
//! with a newer redb.
//!
//! Rows: three plain rows, and two rows sealed under key `[0x42; 32]` (AES-256-GCM, path bound; the
//! nonce is random so the sealed bytes live only in the fixture).

use std::path::{Path, PathBuf};

use storage::{Op, Sealed, Store};

const KEY: [u8; 32] = [0x42; 32];

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/v2.redb")
}

#[test]
#[ignore = "generator: rewrites tests/golden/v2.redb"]
fn generate() {
    let path = fixture();
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).unwrap();
    store
        .apply(&[
            Op::Put("a/1".into(), b"one".to_vec()),
            Op::Put("a/2".into(), b"two".to_vec()),
            Op::Put("b/\u{00e9}".into(), vec![0, 255, 7]),
        ])
        .unwrap();
    let sealed = Sealed::new(store.clone(), KEY);
    sealed.put("s/1", b"secret one").unwrap();
    sealed.put("s/2", b"secret two").unwrap();
    store.close();
}

fn copy_of_fixture(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("storage-golden-{tag}-{}.redb", std::process::id()));
    std::fs::copy(fixture(), &p).unwrap();
    p
}

#[test]
fn v2_file_opens_and_reads_expected_rows() {
    let p = copy_of_fixture("read");
    let store = Store::open(&p).unwrap();
    assert_eq!(store.get("a/1").unwrap().as_deref(), Some(&b"one"[..]));
    assert_eq!(store.get("a/2").unwrap().as_deref(), Some(&b"two"[..]));
    assert_eq!(store.get("b/\u{00e9}").unwrap(), Some(vec![0, 255, 7]));
    assert_eq!(store.get("missing").unwrap(), None);
    assert_eq!(
        store.list_prefixed("a/").unwrap(),
        vec!["a/1".to_string(), "a/2".to_string()]
    );
    assert_eq!(store.list_prefixed("").unwrap().len(), 5);

    let sealed = Sealed::new(store.clone(), KEY);
    assert_eq!(
        sealed.get("s/1").unwrap().as_deref(),
        Some(&b"secret one"[..])
    );
    assert_eq!(
        sealed.get("s/2").unwrap().as_deref(),
        Some(&b"secret two"[..])
    );
    // Path binding: a row moved to another key must not open.
    let raw = store.get("s/1").unwrap().unwrap();
    assert!(sealed.unseal("s/2", &raw).is_err());
    assert!(Sealed::new(store.clone(), [0x43; 32]).get("s/1").is_err());
    store.close();
    let _ = std::fs::remove_file(p);
}

#[test]
fn v2_file_accepts_writes_and_reopens() {
    let p = copy_of_fixture("write");
    let store = Store::open(&p).unwrap();
    store.put("c/1", b"new").unwrap();
    store.delete("a/1").unwrap();
    store.close();
    let store = Store::open(&p).unwrap();
    assert_eq!(store.get("c/1").unwrap().as_deref(), Some(&b"new"[..]));
    assert_eq!(store.get("a/1").unwrap(), None);
    assert_eq!(store.get("a/2").unwrap().as_deref(), Some(&b"two"[..]));
    store.close();
    let _ = std::fs::remove_file(p);
}
