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

#[test]
fn passphrase_key_accepts_only_the_vault_band() {
    use super::{ArgonParams, VAULT, passphrase_key};
    let salt = [9u8; 16];
    assert!(passphrase_key(b"pw", &salt, ArgonParams { m: 8, t: 1, p: 1 }).is_err());
    assert!(passphrase_key(b"pw", &salt, ArgonParams { m: VAULT.m * 8, ..VAULT }).is_err());
    assert!(passphrase_key(b"pw", &salt[..8], VAULT).is_err());
    assert_eq!(
        passphrase_key(b"pw", &salt, VAULT).unwrap().expose(),
        &argon2id(b"pw", &salt, VAULT.m, VAULT.t, VAULT.p).unwrap()
    );
}

#[test]
fn derive_is_blake3_derive_key() {
    assert_eq!(super::derive("ctx", b"ikm").expose(), &blake3::derive_key("ctx", b"ikm"));
}
