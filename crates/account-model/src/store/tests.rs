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
