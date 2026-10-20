use super::*;

#[test]
fn keys_after_pages_a_prefix() {
    let s = Store::open_in_memory().unwrap();
    for k in ["a/1", "a/2", "a/3", "b/1"] {
        s.put(k, b"").unwrap();
    }
    assert_eq!(s.keys_after("a/", None, 2).unwrap(), ["a/1", "a/2"]);
    assert_eq!(s.keys_after("a/", Some("a/2"), 5).unwrap(), ["a/3"]);
    assert!(s.keys_after("a/", Some("a/3"), 5).unwrap().is_empty());
    assert_eq!(s.keys_after("b/", Some("a/9"), 5).unwrap(), ["b/1"]);
}

// KIND: invariant (GL) — `close` lets the file go for every clone at once: the next owner opens
// it without waiting, and the old clones fail cleanly instead of holding it.
#[test]
fn close_releases_the_file_for_every_clone() {
    let dir = std::env::temp_dir().join(format!("storage-close-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.redb");
    let a = Store::open(&path).unwrap();
    let b = a.clone();
    a.put("k", b"v").unwrap();
    assert!(Store::open(&path).is_err(), "held");
    b.close();
    assert!(a.is_closed());
    assert!(matches!(a.get("k"), Err(StorageError::Closed)));
    assert!(matches!(a.put("k", b"w"), Err(StorageError::Closed)));
    let c = Store::open(&path).expect("opens at once after close");
    assert_eq!(c.get("k").unwrap().as_deref(), Some(&b"v"[..]));
    a.close();
    drop(c);
    let _ = std::fs::remove_dir_all(&dir);
}
