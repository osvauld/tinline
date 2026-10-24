use super::*;

const T0: u64 = 1_800_000_000;

#[test]
fn renewed_grant_must_be_valid_and_later() {
    let (them, _) = identity::generate();
    let (me, _) = identity::generate();
    let (other, _) = identity::generate();
    let old = proto::issue_grant(&them, me.did(), T0, 1000);
    let newer = proto::issue_grant(&them, me.did(), T0 + 500, 1000);
    let shorter = proto::issue_grant(&them, me.did(), T0, 500);
    let ok = |new: &proto::SignedGrant| grant_outlives(&old, new, them.did(), me.did(), T0 + 10);
    assert!(ok(&newer));
    assert!(!ok(&shorter), "must not shorten");
    assert!(!ok(&old), "same expiry is not an extension");
    // Wrong issuer, wrong holder.
    assert!(!ok(&proto::issue_grant(&other, me.did(), T0 + 500, 1000)));
    assert!(!ok(&proto::issue_grant(&them, other.did(), T0 + 500, 1000)));
    // Bad signature: a longer-lived payload under another grant's signature.
    let long = proto::issue_grant(&them, me.did(), T0 + 500, 100_000);
    let forged = proto::SignedGrant::from_parts(long.body().to_vec(), *newer.sig());
    assert!(!ok(&forged));
    // An expired stored grant is beaten by any valid one.
    assert!(grant_outlives(&old, &newer, them.did(), me.did(), T0 + 1200));
}

#[test]
fn safety_numbers_match_on_both_sides_and_differ_between_pairs() {
    let (a, _) = identity::generate();
    let (b, _) = identity::generate();
    let (c, _) = identity::generate();
    let ab = safety_number(a.did(), b.did()).unwrap();
    assert_eq!(ab, safety_number(b.did(), a.did()).unwrap());
    assert_ne!(ab, safety_number(a.did(), c.did()).unwrap());
    assert_ne!(ab, safety_number(b.did(), c.did()).unwrap());
    let groups: Vec<&str> = ab.split(' ').collect();
    assert_eq!(groups.len(), 12);
    assert!(groups.iter().all(|g| g.len() == 5 && g.bytes().all(|b| b.is_ascii_digit())));
    assert!(safety_number(a.did(), "did:key:zNope").is_err());
}

#[test]
fn only_the_current_ticket_has_a_nonce() {
    let (id, _) = identity::generate();
    let dev = cryptography::signature::public_key(&proto::new_device_secret());
    let mut st = State::default();
    assert_eq!(current_nonce(&st, T0), None);
    let t = proto::issue_contact_ticket(&id, dev, "x", None, T0, 600);
    st.ticket = Some(t.to_text());
    let n = current_nonce(&st, T0 + 1).unwrap();
    assert_eq!(n, t.verify(T0 + 1).unwrap().nonce);
    assert_eq!(current_nonce(&st, T0 + 601), None, "expired");
    st.ticket = None;
    assert_eq!(current_nonce(&st, T0 + 1), None);
}

#[test]
fn reasons_and_relays_are_bounded() {
    assert_eq!(clip_reason("a\u{0}b\n").as_str(), "ab");
    assert_eq!(clip_reason(&"é".repeat(500)).chars().count(), MAX_REASON_CHARS);
    assert!(relay_hint(&Some("http://relay.example/".into())).is_none());
    assert!(relay_hint(&Some(format!("https://{}.example/", "a".repeat(300)))).is_none());
    assert!(relay_hint(&Some("https://use1-1.relay.n0.iroh.link./".into())).is_some());
    assert!(relay_hint(&None).is_none());
}

#[cfg(test)]
mod find_card_tests;
mod clip_tests;
mod or_log_tests;

