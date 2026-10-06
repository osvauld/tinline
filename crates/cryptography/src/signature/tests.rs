use ed25519_dalek::SigningKey;

use super::{public_key, sign, verify};

fn keypair(seed: u8) -> ([u8; 32], [u8; 32]) {
    let secret = [seed; 32];
    let public = SigningKey::from_bytes(&secret).verifying_key().to_bytes();
    (secret, public)
}

#[test]
fn public_key_matches_keypair() {
    let (secret, public) = keypair(5);
    assert_eq!(public_key(&secret), public);
}

#[test]
fn sign_and_verify() {
    let (secret, public) = keypair(3);
    let sig = sign(&secret, b"message");
    assert!(verify(&public, b"message", &sig));
}

#[test]
fn wrong_message_fails() {
    let (secret, public) = keypair(3);
    let sig = sign(&secret, b"message");
    assert!(!verify(&public, b"tampered", &sig));
}

#[test]
fn wrong_key_fails() {
    let (secret, _) = keypair(3);
    let (_, other_public) = keypair(9);
    let sig = sign(&secret, b"message");
    assert!(!verify(&other_public, b"message", &sig));
}
