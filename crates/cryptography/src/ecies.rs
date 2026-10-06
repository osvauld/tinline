use rand::rngs::OsRng;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

use crate::aead;
use crate::error::CryptoError;
use crate::kdf;

const EPHEMERAL_LEN: usize = 32;
const SEAL_INFO: &[u8] = b"osv/ecies/v1";

pub fn public_key(secret: &[u8; 32]) -> [u8; 32] {
    PublicKey::from(&StaticSecret::from(*secret)).to_bytes()
}

pub fn ecdh(our_secret: &[u8; 32], their_public: &[u8; 32]) -> [u8; 32] {
    StaticSecret::from(*our_secret)
        .diffie_hellman(&PublicKey::from(*their_public))
        .to_bytes()
}

/// ECIES seal to a recipient's X25519 public key.
/// Output is `ephemeral_public(32) || aead(nonce | ciphertext | tag)`.
pub fn seal(recipient_public: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let ephemeral = StaticSecret::random_from_rng(OsRng);
    let ephemeral_public = PublicKey::from(&ephemeral);

    let mut shared = ephemeral
        .diffie_hellman(&PublicKey::from(*recipient_public))
        .to_bytes();
    let mut key = kdf::hkdf_sha256(&shared, None, SEAL_INFO);
    let ciphertext = aead::encrypt(&key, plaintext);
    shared.zeroize();
    key.zeroize();
    let ciphertext = ciphertext?;

    let mut out = Vec::with_capacity(EPHEMERAL_LEN + ciphertext.len());
    out.extend_from_slice(ephemeral_public.as_bytes());
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn open(recipient_secret: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if data.len() < EPHEMERAL_LEN {
        return Err(CryptoError::CiphertextTooShort);
    }
    let (ephemeral_public, ciphertext) = data.split_at(EPHEMERAL_LEN);
    let mut eph = [0u8; 32];
    eph.copy_from_slice(ephemeral_public);

    let mut shared = ecdh(recipient_secret, &eph);
    let mut key = kdf::hkdf_sha256(&shared, None, SEAL_INFO);
    let result = aead::decrypt(&key, ciphertext);
    shared.zeroize();
    key.zeroize();
    result
}

#[cfg(test)]
mod tests;
