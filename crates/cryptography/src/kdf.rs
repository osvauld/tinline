use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::error::CryptoError;

/// HKDF-SHA256 to a 32-byte key. Infallible at this output length.
pub fn hkdf_sha256(ikm: &[u8], salt: Option<&[u8]>, info: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(salt, ikm);
    let mut okm = [0u8; 32];
    hk.expand(info, &mut okm)
        .expect("32-byte HKDF output is always valid");
    okm
}

/// Argon2id password hashing to a 32-byte key. Params and salt are the caller's
/// responsibility to store so the same key can be re-derived.
pub fn argon2id(
    passphrase: &[u8],
    salt: &[u8],
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
) -> Result<[u8; 32], CryptoError> {
    let params = Params::new(m_cost, t_cost, p_cost, Some(32))
        .map_err(|e| CryptoError::Argon2(e.to_string()))?;
    let mut key = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase, salt, &mut key)
        .map_err(|e| CryptoError::Argon2(e.to_string()))?;
    Ok(key)
}

#[cfg(test)]
mod tests;
