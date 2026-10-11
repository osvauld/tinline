//! Characterization tests for the existing identity boundary, not a rotation protocol.
//! The remote key stands for the key authenticated by iroh; these tests do not run QUIC.
use std::collections::HashSet;

use proto::{accept_self_hello, attest, Error};

#[test]
fn successive_endpoints_prove_the_same_account_without_relinking() {
    let (owner, _) = identity::generate();
    for generation in 1..=3u8 {
        let endpoint = [generation; 32];
        let proof = attest(&owner, endpoint, u64::from(generation));
        let accepted = accept_self_hello(&proof, owner.did(), endpoint, &HashSet::new()).unwrap();
        assert_eq!(accepted.did, owner.did());
        assert_eq!(accepted.device_key().unwrap(), endpoint);
    }
    // This acceptance does NOT enforce succession or retire earlier endpoints.
}

#[test]
fn attestation_cannot_be_reused_from_a_different_transport_key() {
    let (owner, _) = identity::generate();
    let proof = attest(&owner, [1; 32], 1);
    assert!(matches!(
        accept_self_hello(&proof, owner.did(), [2; 32], &HashSet::new()),
        Err(Error::DeviceMismatch)
    ));
}

#[test]
fn another_account_cannot_claim_own_device_membership() {
    let (owner, _) = identity::generate();
    let (stranger, _) = identity::generate();
    let proof = attest(&stranger, [1; 32], 1);
    assert!(matches!(
        accept_self_hello(&proof, owner.did(), [1; 32], &HashSet::new()),
        Err(Error::WrongIssuer)
    ));
}

#[test]
fn tombstone_blocks_old_key_but_not_a_new_attestation_from_the_same_did() {
    let (owner, _) = identity::generate();
    let removed = [1; 32];
    let fresh = [2; 32];
    let tombstones = HashSet::from([removed]);
    assert!(matches!(
        accept_self_hello(&attest(&owner, removed, 1), owner.did(), removed, &tombstones),
        Err(Error::Tombstoned)
    ));
    // Deliberately records the CURRENT limitation: every mnemonic holder can
    // attest a fresh endpoint. This is not a desired compromised-device defense.
    assert!(accept_self_hello(
        &attest(&owner, fresh, 2), owner.did(), fresh, &tombstones
    ).is_ok());
}

#[test]
fn existing_attestation_has_no_monotonic_generation_policy() {
    let (owner, _) = identity::generate();
    let old = attest(&owner, [1; 32], 1);
    let new = attest(&owner, [2; 32], 2);
    assert!(accept_self_hello(&new, owner.did(), [2; 32], &HashSet::new()).is_ok());
    assert!(accept_self_hello(&old, owner.did(), [1; 32], &HashSet::new()).is_ok());
    // Rotation needs a separate persisted ordering/retirement check.
}
