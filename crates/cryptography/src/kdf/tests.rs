use super::{argon2id, hkdf_sha256};

#[test]
fn hkdf_is_deterministic() {
    assert_eq!(
        hkdf_sha256(b"ikm", None, b"info"),
        hkdf_sha256(b"ikm", None, b"info")
    );
}

#[test]
fn hkdf_context_separates() {
    assert_ne!(
        hkdf_sha256(b"ikm", None, b"info-a"),
        hkdf_sha256(b"ikm", None, b"info-b")
    );
}

#[test]
fn argon2_is_deterministic() {
    let salt = [7u8; 16];
    assert_eq!(
        argon2id(b"pw", &salt, 8, 1, 1).unwrap(),
        argon2id(b"pw", &salt, 8, 1, 1).unwrap()
    );
}

#[test]
fn argon2_passphrase_separates() {
    let salt = [7u8; 16];
    assert_ne!(
        argon2id(b"pw", &salt, 8, 1, 1).unwrap(),
        argon2id(b"other", &salt, 8, 1, 1).unwrap()
    );
}
