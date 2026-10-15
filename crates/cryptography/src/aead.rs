use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};

use crate::error::CryptoError;

const NONCE_LEN: usize = 12;

/// AES-256-GCM seal. Output is `nonce(12) || ciphertext || tag(16)`.
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::Aead)?;
    let nonce: [u8; NONCE_LEN] = crate::rng::bytes();
    let ciphertext = cipher
        .encrypt(&Nonce::from(nonce), plaintext)
        .map_err(|_| CryptoError::Aead)?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if data.len() < NONCE_LEN {
        return Err(CryptoError::CiphertextTooShort);
    }
    let (nonce, ciphertext) = data.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::Aead)?;
    cipher
        .decrypt(&Nonce::try_from(nonce).map_err(|_| CryptoError::Aead)?, ciphertext)
        .map_err(|_| CryptoError::Aead)
}

#[cfg(test)]
mod tests;
