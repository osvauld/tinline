use super::Identity;

#[test]
fn generate_produces_well_formed_did() {
    let (identity, _mnemonic) = Identity::generate();
    assert!(identity.did().starts_with("did:key:z"));
}

#[test]
fn from_mnemonic_is_deterministic() {
    let (identity, mnemonic) = Identity::generate();
    let again = Identity::from_mnemonic(&mnemonic);
    assert_eq!(identity.did(), again.did());
    assert_eq!(identity.signing_public_key(), again.signing_public_key());
    assert_eq!(
        identity.encryption_public_key(),
        again.encryption_public_key()
    );
    assert_eq!(identity.device_public_key(), again.device_public_key());
}

#[test]
fn sign_and_verify_round_trip() {
    let (identity, _) = Identity::generate();
    let sig = identity.sign(b"hello");
    assert!(crate::verify(
        &identity.signing_public_key(),
        b"hello",
        &sig
    ));
    assert!(!crate::verify(
        &identity.signing_public_key(),
        b"tampered",
        &sig
    ));
}

#[test]
fn encrypt_for_and_decrypt_round_trip() {
    let (recipient, _) = Identity::generate();
    let sealed = crate::encrypt_for(&recipient.encryption_public_key(), b"secret").unwrap();
    assert_eq!(recipient.decrypt_sealed(&sealed).unwrap(), b"secret");
}
