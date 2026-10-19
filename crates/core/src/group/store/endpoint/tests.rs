use super::*;

fn current(s: &GroupStore) -> [u8; 32] {
    *s.ep_state().unwrap().unwrap().secret
}

/// A restart: a second handle on the same rows, discarding what was staged (what `group_core`
/// does on open).
fn restart(s: &GroupStore) -> GroupStore {
    let s = s.clone();
    s.ep_discard_pending().unwrap();
    s
}

// KIND: unit (rotation, G-12) — the first key is made once and read back the same.
#[test]
fn the_first_key_is_made_once() {
    let s = GroupStore::in_memory("ep first");
    assert!(s.ep_state().unwrap().is_none());
    let a = s.ep_state_or_init(1_000, 7).unwrap();
    let b = s.ep_state_or_init(9_000, 9).unwrap();
    assert_eq!(*a.secret, *b.secret);
    assert_eq!((b.made, b.jitter, b.announce_until), (1_000, 7, 0));
}

// KIND: unit (rotation, G-12) — a fault at every step leaves either the old key or the new one
// current, never anything else, and a staged key that was never promoted is gone after restart.
#[test]
fn a_fault_at_any_step_leaves_the_old_or_the_new_key() {
    let s = GroupStore::in_memory("ep faults");
    let old = *s.ep_state_or_init(1_000, 0).unwrap().secret;

    // Stage fails.
    s.fail_next_append_for_test();
    assert!(s.ep_stage().is_err());
    assert_eq!(current(&s), old);
    assert!(s.ep_pending_for_test().is_none());

    // Staged, then a crash before promote (the rebind failed or the process died).
    let staged = *s.ep_stage().unwrap();
    assert_ne!(staged, old);
    assert_eq!(current(&s), old, "staging does not change the current key");
    let s = restart(&s);
    assert_eq!(current(&s), old);
    assert!(
        s.ep_pending_for_test().is_none(),
        "a never-promoted key is discarded on start"
    );
    assert!(
        s.ep_promote(2_000, 3_000, 5).is_err(),
        "nothing to promote after the discard"
    );
    assert_eq!(current(&s), old);

    // Staged, then the promote fails: still the old key; the staged one is still only staged.
    s.ep_stage().unwrap();
    s.fail_next_append_for_test();
    assert!(s.ep_promote(2_000, 3_000, 5).is_err());
    assert_eq!(current(&s), old);
    let s = restart(&s);
    assert_eq!(current(&s), old);

    // A full rotation: the new key, with its schedule, all at once.
    let next = *s.ep_stage().unwrap();
    s.ep_promote(2_000, 3_000, 5).unwrap();
    let st = s.ep_state().unwrap().unwrap();
    assert_eq!(*st.secret, next);
    assert_eq!((st.made, st.announce_until, st.jitter), (2_000, 3_000, 5));
    assert!(s.ep_pending_for_test().is_none());
    // A crash after the promote: the new key stays.
    let s = restart(&s);
    assert_eq!(current(&s), next);
}

// KIND: unit (rotation, G-32) — a malformed row is an error, never a silent new key or `made = 0`.
#[test]
fn a_malformed_row_is_an_error() {
    for (row, value) in [
        ("ep/secret", &[1u8; 31][..]),
        ("ep/made", &[1u8; 7][..]),
        ("ep/announce", &[1u8; 9][..]),
        ("ep/jitter", &[][..]),
    ] {
        let s = GroupStore::in_memory("ep malformed");
        let old = *s.ep_state_or_init(1_000, 0).unwrap().secret;
        s.ep_put_raw_for_test(row, value);
        assert!(s.ep_state().is_err(), "{row}");
        assert!(
            s.ep_state_or_init(2_000, 0).is_err(),
            "{row}: no silent new key"
        );
        if row != "ep/secret" {
            s.ep_put_raw_for_test(row, &5u64.to_be_bytes());
            assert_eq!(current(&s), old, "{row}: the key never changed");
        }
    }
    // A key without its `made` row.
    let s = GroupStore::in_memory("ep partial");
    s.ep_put_raw_for_test("ep/secret", &[3; 32]);
    assert!(s.ep_state().is_err());
}

// KIND: unit (rotation) — the policy defaults until set; an out-of-bounds one is refused and a
// malformed row is an error.
#[test]
fn the_rotation_policy_row() {
    let s = GroupStore::in_memory("ep policy");
    assert_eq!(s.rotation_policy().unwrap(), RotationPolicy::DEFAULT);
    let p = RotationPolicy {
        interval_ms: RotationPolicy::MIN_INTERVAL_MS * 3,
        ..RotationPolicy::DEFAULT
    };
    s.set_rotation_policy(&p).unwrap();
    assert_eq!(s.rotation_policy().unwrap(), p);
    let short = RotationPolicy {
        enabled: true,
        interval_ms: 1_000,
        jitter_ms: 0,
        announce_ms: 1_000,
    };
    assert!(matches!(
        s.set_rotation_policy(&short),
        Err(Error::Rejected(_))
    ));
    assert_eq!(s.rotation_policy().unwrap(), p);
    // A row written before `enabled` existed reads as off.
    s.ep_put_raw_for_test("ep/policy", br#"{"interval_ms":172800000,"jitter_ms":0,"announce_ms":86400000}"#);
    assert!(!s.rotation_policy().unwrap().enabled);
    s.ep_put_raw_for_test("ep/policy", b"{");
    assert!(s.rotation_policy().is_err());
}
