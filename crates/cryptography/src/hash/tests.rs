use super::*;
use crate::domains;

#[test]
fn hasher_matches_the_signing_framing() {
    let d = domains::GROUP_ID;
    let h = hasher(d).part(b"a").part(b"bc").finish();
    assert_eq!(h, *blake3::hash(&crate::sign::input(d, &[b"a", b"bc"])).as_bytes());
    assert_ne!(h, hasher(d).part(b"ab").part(b"c").finish());
    assert_ne!(content(domains::GROUP_RECORD_ID, b"x"), content(domains::GROUP_CALLS_FRONTIER, b"x"));
    assert_eq!(hasher(d).part(b"z").hex(), hex(&hasher(d).part(b"z").finish()));
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn keyed_tags_verify_only_under_their_key_domain_and_parts() {
    let k = [3u8; 32];
    let t = keyed(&k, domains::LINK_PROOF, &[b"scanner", b"dev"]);
    assert!(keyed_verify(&k, domains::LINK_PROOF, &[b"scanner", b"dev"], &t));
    assert!(!keyed_verify(&[4; 32], domains::LINK_PROOF, &[b"scanner", b"dev"], &t));
    assert!(!keyed_verify(&k, domains::LINK_CONFIRM, &[b"scanner", b"dev"], &t));
    assert!(!keyed_verify(&k, domains::LINK_PROOF, &[b"scannerdev"], &t));
    assert!(!keyed_verify(&k, domains::LINK_PROOF, &[b"scanner", b"dev"], &t[..31]));
}
