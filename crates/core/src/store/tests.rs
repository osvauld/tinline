use super::*;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("p2pcore-store-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    p
}

#[cfg(unix)]
#[test]
fn files_are_private_and_old_ones_are_fixed() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir = tmp("modes");
    // An install from before: loose dir and file modes.
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(dir.join("state.json"), b"{}").unwrap();
    fs::set_permissions(dir.join("state.json"), fs::Permissions::from_mode(0o644)).unwrap();
    Store::open(&dir).unwrap();
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&dir.join("state.json")), 0o600);
    let top = tmp("fresh");
    let fresh = top.join("a");
    Store::open(&fresh).unwrap();
    assert_eq!(mode(&fresh), 0o700);
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&top);
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
fn stale_temp_files_are_removed_on_open() {
    let dir = tmp("stale");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("profile.tmp"), b"secret").unwrap();
    fs::write(dir.join("state.tmp"), b"x").unwrap();
    fs::write(dir.join("state.json"), b"{}").unwrap();
    Store::open(&dir).unwrap();
    assert!(!dir.join("profile.tmp").exists() && !dir.join("state.tmp").exists());
    assert!(dir.join("state.json").exists());
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

#[test]
fn old_contact_records_load_with_the_relay_on_the_first_device() {
    let (them, _) = identity::generate();
    let grant = serde_json::to_value(proto::issue_token(
        &them,
        "did:key:me",
        proto::Cap::Message,
        1,
        100,
    ))
    .unwrap();
    let old = serde_json::json!({
        "did": "did:key:zX", "name": "N",
        "devices": [vec![1u8; 32], vec![2u8; 32]],
        "relay": "https://r.example/",
        "grant_from_them": grant,
        "added_at": 5
    });
    let c: StoredContact = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(c.devices.len(), 2);
    assert_eq!((c.devices[0].device, c.devices[0].relay.as_deref()), ([1u8; 32], Some("https://r.example/")));
    assert_eq!((c.devices[1].device, c.devices[1].relay.clone()), ([2u8; 32], None));
    assert!(c.device_list.is_none() && !c.verified && c.alias.is_none());
    // Before P6: calls allowed by default; a message token carries no call slot, so nothing to migrate.
    assert!(c.calls_allowed && c.call_grant_from_them.is_none());
    // Round trip in the new shape, and as part of a whole old State.
    let again: StoredContact = serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
    assert_eq!(again.devices, c.devices);
    let st: State = serde_json::from_value(serde_json::json!({"contacts": [old], "blocked": ["b"]})).unwrap();
    assert_eq!(st.contacts[0].devices.len(), 2);
    assert!(st.registry.is_empty() && st.own_list.is_none());
    // No relay, and a contact with no devices at all, still load.
    let bare = serde_json::json!({"did":"d","name":"n","devices":[],"relay":null,"grant_from_them":grant,"added_at":1});
    assert!(
        serde_json::from_value::<StoredContact>(bare)
            .unwrap()
            .devices
            .is_empty()
    );
}
