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
