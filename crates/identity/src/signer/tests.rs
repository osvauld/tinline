use super::*;
use crate::{generate, public_key_from_did, verify};

/// A holder that signs but never hands the identity out — the shape `vault` will take, and
/// the reason the trait exists.
struct Sealed {
    inner: Identity,
}

impl Signer for Sealed {
    fn did(&self) -> &str {
        self.inner.did()
    }

    fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.inner.sign(message)
    }

    fn encryption_public_key(&self) -> [u8; 32] {
        self.inner.encryption_public_key()
    }

    fn device_public_key(&self) -> [u8; 32] {
        self.inner.device_public_key()
    }
}

fn signed_by(signer: &impl Signer, message: &[u8]) -> bool {
    let key = public_key_from_did(signer.did()).unwrap();
    verify(&key, message, &signer.sign(message))
}

#[test]
fn an_identity_signs_as_itself_through_the_trait() {
    let (identity, _) = generate();

    assert!(signed_by(&identity, b"hello"));
    assert_eq!(Signer::did(&identity), identity.did());
    assert_eq!(
        Signer::encryption_public_key(&identity),
        identity.encryption_public_key()
    );
    assert_eq!(
        Signer::device_public_key(&identity),
        identity.device_public_key()
    );
}

#[test]
fn a_wrapper_signs_without_surrendering_the_identity() {
    let (identity, _) = generate();
    let did = identity.did().to_string();
    let sealed = Sealed { inner: identity };

    // The caller only ever sees `&impl Signer`, and the signature still verifies against the
    // DID — which is the whole point: keys stay where they are sealed.
    assert!(signed_by(&sealed, b"hello"));
    assert_eq!(sealed.did(), did);
}

#[test]
fn a_reference_signs_as_what_it_points_at() {
    let (identity, _) = generate();
    let by_ref: &dyn Signer = &identity;

    assert!(signed_by(&by_ref, b"hello"));
}
