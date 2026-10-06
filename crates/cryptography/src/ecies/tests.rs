use rand::rngs::OsRng;
use x25519_dalek::{PublicKey, StaticSecret};

use super::{open, public_key, seal};

fn keypair() -> ([u8; 32], [u8; 32]) {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    (secret.to_bytes(), public.to_bytes())
}

#[test]
fn public_key_matches_keypair() {
    let (secret, public) = keypair();
    assert_eq!(public_key(&secret), public);
}

#[test]
fn seal_open_round_trip() {
    let (secret, public) = keypair();
    let ct = seal(&public, b"for your eyes only").unwrap();
    assert_eq!(open(&secret, &ct).unwrap(), b"for your eyes only");
}

#[test]
fn wrong_recipient_fails() {
    let (_, public) = keypair();
    let (other_secret, _) = keypair();
    let ct = seal(&public, b"secret").unwrap();
    assert!(open(&other_secret, &ct).is_err());
}

#[test]
fn short_ciphertext_rejected() {
    let (secret, _) = keypair();
    assert!(open(&secret, &[0u8; 8]).is_err());
}
