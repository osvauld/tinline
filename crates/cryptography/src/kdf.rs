use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::error::CryptoError;
use crate::secret::Secret32;

/// Argon2id cost parameters (memory in KiB, iterations, lanes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgonParams {
    pub m: u32,
    pub t: u32,
    pub p: u32,
}

/// The one parameter set for passphrase-derived keys (the account vault).
pub const VAULT: ArgonParams = ArgonParams { m: 65536, t: 3, p: 4 };

/// BLAKE3 `derive_key`: a 32-byte subkey of `ikm` for the fixed context `label`.
pub fn derive(label: &str, ikm: &[u8]) -> Secret32 {
    Secret32::new(blake3::derive_key(label, ikm))
}

/// Argon2id of a passphrase with stored parameters, accepted only in a band around [`VAULT`]
/// (a tampered file must not make us allocate gigabytes, nor downgrade to something fast).
pub fn passphrase_key(passphrase: &[u8], salt: &[u8], params: ArgonParams) -> Result<Secret32, CryptoError> {
    if !(VAULT.m..=4 * VAULT.m).contains(&params.m)
        || !(VAULT.t..=10).contains(&params.t)
        || !(1..=8).contains(&params.p)
        || !(16..=64).contains(&salt.len())
    {
        return Err(CryptoError::Argon2("parameters out of range".into()));
    }
    let mut k = argon2id(passphrase, salt, params.m, params.t, params.p)?;
    let s = Secret32::new(k);
    zeroize::Zeroize::zeroize(&mut k);
    Ok(s)
}

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
