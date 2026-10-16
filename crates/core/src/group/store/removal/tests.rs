use super::*;

// KIND: unit (D12) — the first time a removal is derived is kept (later derivations do not move
// it); a DID that is no longer removed loses its row, so a later removal starts a fresh window.
#[test]
fn the_first_removal_time_is_kept_and_cleared_on_readd() {
    let s = GroupStore::in_memory("rm seen");
    let (m, n) = ("did:key:m".to_string(), "did:key:n".to_string());
    s.sync_removals_seen("g", std::slice::from_ref(&m), 1_000)
        .unwrap();
    s.sync_removals_seen("g", &[m.clone(), n.clone()], 5_000)
        .unwrap();
    assert_eq!(s.removal_seen("g", &m).unwrap(), Some(1_000));
    assert_eq!(s.removal_seen("g", &n).unwrap(), Some(5_000));
    assert_eq!(s.removal_seen("other", &m).unwrap(), None, "per group");
    s.sync_removals_seen("g", std::slice::from_ref(&n), 9_000)
        .unwrap();
    assert_eq!(s.removal_seen("g", &m).unwrap(), None, "m was re-added");
    s.sync_removals_seen("g", &[m.clone(), n.clone()], 12_000)
        .unwrap();
    assert_eq!(s.removal_seen("g", &m).unwrap(), Some(12_000));
    assert_eq!(s.removal_seen("g", &n).unwrap(), Some(5_000));
    s.db.put("rm_seen/g/x", &[1, 2]).unwrap();
    assert!(
        s.removal_seen("g", "x").is_err(),
        "a malformed row is an error"
    );
}
