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

#[test]
fn small_order_key_is_rejected() {
    // The identity point as a public key with R = identity, S = 0 satisfies the plain
    // verification equation for every message; strict verification refuses the weak key.
    let mut public = [0u8; 32];
    public[0] = 1;
    let mut sig = [0u8; 64];
    sig[0] = 1;
    assert!(!verify(&public, b"anything", &sig));
}
