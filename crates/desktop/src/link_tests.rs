use super::*;

#[test]
fn every_core_reason_has_its_own_words() {
    let generic = failure_text("something else");
    for r in ["expired", "cancelled", "rejected", "bad_proof", "mismatch", "timeout", "net", "locked", "busy", "bad_grant", "account_exists"] {
        assert_ne!(failure_text(r), generic, "{r}");
    }
}

#[test]
fn the_qr_countdown_stops_at_zero() {
    let mut l = LinkState::new(Role::New);
    assert!(l.remaining() <= l.ttl && l.remaining() > 0);
    l.started = Instant::now() - Duration::from_secs(l.ttl + 30);
    assert_eq!(l.remaining(), 0);
}
