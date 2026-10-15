use super::{decrypt, encrypt};
use crate::error::CryptoError;

fn generate_key() -> [u8; 32] {
    crate::rng::bytes()
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

#[test]
fn aad_must_match_and_empty_aad_is_encrypt() {
    let key = generate_key();
    let ct = super::seal(&key, b"purpose-a", b"secret").unwrap();
    assert_eq!(super::open(&key, b"purpose-a", &ct).unwrap(), b"secret");
    assert!(super::open(&key, b"purpose-b", &ct).is_err());
    assert!(decrypt(&key, &ct).is_err());
    let plain = encrypt(&key, b"x").unwrap();
    assert_eq!(super::open(&key, b"", &plain).unwrap(), b"x");
    assert_ne!(super::seal(&key, b"", b"x").unwrap(), super::seal(&key, b"", b"x").unwrap());
}

#[test]
fn stream_chunks_open_in_order_with_the_last_flag() {
    use super::stream::Stream;
    let key = generate_key();
    let mut s = Stream::new(&key);
    let a = s.seal_next(b"one", false).unwrap();
    let b = s.seal_next(b"two", true).unwrap();
    let mut o = Stream::new(&key);
    assert_eq!(o.open_next(&a, false).unwrap(), b"one");
    assert_eq!(o.open_next(&b, true).unwrap(), b"two");
    // Swapped, or the last flag dropped: fails.
    assert!(Stream::new(&key).open_next(&b, true).is_err());
    assert!(Stream::new(&key).open_next(&a, true).is_err());
}
