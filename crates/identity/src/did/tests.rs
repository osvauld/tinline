use super::{did_from_public_key, public_key_from_did};

#[test]
fn round_trip() {
    let pubkey = [7u8; 32];
    let did = did_from_public_key(&pubkey);
    assert_eq!(public_key_from_did(&did), Some(pubkey));
}

#[test]
fn has_did_key_prefix() {
    assert!(did_from_public_key(&[1u8; 32]).starts_with("did:key:z"));
}

#[test]
fn rejects_garbage() {
    assert_eq!(public_key_from_did("not-a-did"), None);
    assert_eq!(public_key_from_did("did:key:z!!not-base58!!"), None);
}
