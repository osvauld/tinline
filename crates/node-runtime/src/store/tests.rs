use std::fs;

use super::*;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("p2pcore-store-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    p
}

// KIND: invariant (P10)
// A store file another owner still holds for a moment (the node before a restart) opens once
// it is let go, instead of failing at once; one held for good still fails, after ~2 s.
#[test]
fn a_store_still_held_opens_once_released() {
    let dir = tmp("held");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("chat.redb");
    let held = storage::Store::open(&path).unwrap();
    assert!(storage::Store::open(&path).is_err(), "redb refuses a second owner");
    let t = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        drop(held);
    });
    open_waiting(&path).unwrap();
    t.join().unwrap();
    let _again = open_waiting(&path).unwrap();
    let start = std::time::Instant::now();
    assert!(open_waiting(&path).is_err());
    assert!(start.elapsed() >= std::time::Duration::from_secs(1));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn history_is_capped_newest_first_and_private() {
    let dir = tmp("calls");
    let store = Store::open(&dir).unwrap();
    let db = open_sealed(store.dir(), "calls phrase").unwrap();
    let h = History::new(Vec::new(), Some(Backing::Sealed(db.clone())));
    let rec = |i: usize, did: &str| CallRecord {
        call_id: i.to_string(),
        peer_did: did.into(),
        peer_name: "x".into(),
        incoming: false,
        started_at: i as u64,
        duration_secs: 0,
        reason: "hangup_local".into(),
        direct: false,
        missed: false,
        video: false,
    };
    for i in 0..MAX_HISTORY + 5 {
        h.push(rec(i, if i % 2 == 0 { "a" } else { "b" }));
    }
    h.save().unwrap();
    let saved: Vec<CallRecord> = serde_json::from_slice(&db.get(CALLS_KEY).unwrap().unwrap()).unwrap();
    let back = History::new(saved, None);
    assert_eq!(back.list(None, 10_000).len(), MAX_HISTORY);
    assert_eq!(back.list(None, 1)[0].call_id, (MAX_HISTORY + 4).to_string());
    assert!(back.list(Some("a"), 10_000).iter().all(|r| r.peer_did == "a"));
    back.remove_peer("a");
    assert!(back.list(Some("a"), 10).is_empty() && !back.list(Some("b"), 10).is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dir.join("account.redb")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    drop(db);
    let _ = fs::remove_dir_all(&dir);
}

fn call(id: &str, at: u64, reason: &str, duration_secs: u32, missed: bool) -> CallRecord {
    CallRecord {
        call_id: id.into(),
        peer_did: "a".into(),
        peer_name: "x".into(),
        incoming: true,
        started_at: at,
        duration_secs,
        reason: reason.into(),
        direct: false,
        missed,
        video: false,
    }
}

#[test]
fn history_keeps_one_record_per_call() {
    let h = History::new(Vec::new(), None);
    h.push(call("c1", 10, "hangup_remote", 42, false));
    // A redial of the same call turned away as busy, logged after the real record.
    h.push(call("c1", 12, "busy", 0, true));
    h.push(call("c2", 20, "hangup_local", 0, false));
    let all = h.list(None, 10);
    assert_eq!(all.iter().map(|r| r.call_id.as_str()).collect::<Vec<_>>(), ["c2", "c1"]);
    assert_eq!(all[1].duration_secs, 42, "the answered record stays");

    // The busy one first, the answered one after: the answered one replaces it.
    let h = History::new(Vec::new(), None);
    h.push(call("c3", 10, "busy", 0, true));
    h.push(call("c3", 10, "hangup_remote", 7, false));
    let all = h.list(None, 10);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].duration_secs, 7);
}

#[test]
fn a_saved_log_with_a_call_twice_loads_once() {
    let saved = vec![
        call("c2", 20, "busy", 0, true),
        call("c1", 15, "busy", 0, true),
        call("c2", 20, "hangup_remote", 9, false),
    ];
    let h = History::new(saved, None);
    let all = h.list(None, 10);
    assert_eq!(all.iter().map(|r| r.call_id.as_str()).collect::<Vec<_>>(), ["c2", "c1"]);
    assert_eq!(all[0].duration_secs, 9);
}
