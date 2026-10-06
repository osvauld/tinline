use super::{Keystore, seal, unlock};
use crate::error::IdentityError;
use crate::identity::Identity;

#[test]
fn seal_unlock_round_trip() {
    let (identity, _) = Identity::generate();
    let keystore = seal(&identity, "pw").unwrap();
    let restored = unlock(&keystore, "pw").unwrap();
    assert_eq!(identity.did(), restored.did());
    assert_eq!(identity.signing_public_key(), restored.signing_public_key());
    assert_eq!(
        identity.encryption_public_key(),
        restored.encryption_public_key()
    );
}

#[test]
fn wrong_passphrase_fails() {
    let (identity, _) = Identity::generate();
    let keystore = seal(&identity, "pw").unwrap();
    assert!(matches!(
        unlock(&keystore, "wrong"),
        Err(IdentityError::WrongPassphrase)
    ));
}

#[test]
fn did_readable_before_unlock() {
    let (identity, _) = Identity::generate();
    let keystore = seal(&identity, "pw").unwrap();
    assert_eq!(keystore.did(), identity.did());
}

#[test]
fn bytes_round_trip() {
    let (identity, _) = Identity::generate();
    let bytes = seal(&identity, "pw").unwrap().to_bytes();
    let keystore = Keystore::from_bytes(&bytes).unwrap();
    let restored = unlock(&keystore, "pw").unwrap();
    assert_eq!(identity.did(), restored.did());
}

#[test]
fn from_bytes_rejects_garbage() {
    assert!(matches!(
        Keystore::from_bytes(b"not a keystore"),
        Err(IdentityError::Decode)
    ));
}
