use super::{decrypt, encrypt};
use crate::error::CryptoError;
use rand::{RngCore, rngs::OsRng};

fn generate_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    key
}

#[test]
fn round_trip() {
    let key = generate_key();
    let ct = encrypt(&key, b"hello world").unwrap();
    assert_eq!(decrypt(&key, &ct).unwrap(), b"hello world");
}

#[test]
fn wrong_key_fails() {
    let ct = encrypt(&generate_key(), b"secret").unwrap();
    assert!(decrypt(&generate_key(), &ct).is_err());
}

#[test]
fn tamper_fails() {
    let key = generate_key();
    let mut ct = encrypt(&key, b"secret").unwrap();
    let last = ct.len() - 1;
    ct[last] ^= 0x01;
    assert!(decrypt(&key, &ct).is_err());
}

#[test]
fn short_ciphertext_rejected() {
    let key = generate_key();
    assert!(matches!(
        decrypt(&key, &[0u8; 4]),
        Err(CryptoError::CiphertextTooShort)
    ));
}
