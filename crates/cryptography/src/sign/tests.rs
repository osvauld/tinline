use super::*;
use crate::domains;

#[test]
fn part_boundaries_and_domain_are_framed() {
    let d = domains::GROUP_RECORD;
    assert_ne!(input(d, &[b"ab", b"c"]), input(d, &[b"a", b"bc"]));
    assert_ne!(input(d, &[b"ab"]), input(d, &[b"ab", b""]));
    assert_ne!(input(b"ab", &[b"c"]), input(b"a", &[b"bc"]));
    assert_eq!(input(b"d", &[b"xy"]), [&[0, 0, 0, 0, 0, 0, 0, 1][..], b"d", &[0, 0, 0, 0, 0, 0, 0, 2], b"xy"].concat());
}

#[test]
fn sign_verify_round_trip_and_domain_separation() {
    let sk = [7u8; 32];
    let pk = signature::public_key(&sk);
    let sig = sign(&sk, domains::RECEIPT, &[b"body"]);
    assert!(verify(&pk, domains::RECEIPT, &[b"body"], &sig));
    assert!(!verify(&pk, domains::STORAGE_ATTESTATION, &[b"body"], &sig));
    assert!(!verify(&pk, domains::RECEIPT, &[b"bodx"], &sig));
}

#[test]
fn every_domain_is_unique() {
    let mut all = domains::ALL.to_vec();
    all.sort_unstable();
    let n = all.len();
    all.dedup();
    assert_eq!(all.len(), n, "a domain is listed twice or two share a value");
}
