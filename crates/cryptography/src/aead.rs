//! AES-256-GCM, the app's one AEAD. `seal`/`open`: a random 96-bit nonce per message, output
//! `nonce(12) || ciphertext || tag(16)`, and associated data that must match on open (bind a
//! ciphertext to its purpose or transcript). `stream`: chunked blobs under a one-use key.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};

use crate::error::CryptoError;

pub mod stream;

const NONCE_LEN: usize = 12;

/// Seals `plaintext` under `key`, authenticating `aad` too.
pub fn seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::Aead)?;
    let nonce: [u8; NONCE_LEN] = crate::rng::bytes();
    let ciphertext = cipher
        .encrypt(&Nonce::from(nonce), Payload { msg: plaintext, aad })
        .map_err(|_| CryptoError::Aead)?;
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Opens what [`seal`] made with the same `key` and `aad`.
pub fn open(key: &[u8; 32], aad: &[u8], data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if data.len() < NONCE_LEN {
        return Err(CryptoError::CiphertextTooShort);
    }
    let (nonce, ciphertext) = data.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::Aead)?;
    cipher
        .decrypt(&Nonce::try_from(nonce).map_err(|_| CryptoError::Aead)?, Payload { msg: ciphertext, aad })
        .map_err(|_| CryptoError::Aead)
}

/// [`seal`] with no associated data (the layout the golden vectors pin).
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    seal(key, &[], plaintext)
}

/// [`open`] with no associated data.
pub fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    open(key, &[], data)
}

#[cfg(test)]
mod tests;
