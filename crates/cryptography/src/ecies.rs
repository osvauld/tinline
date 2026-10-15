use x25519_dalek::{PublicKey, StaticSecret};

use crate::aead;
use crate::error::CryptoError;
use crate::kdf;
use crate::secret::Secret32;

const EPHEMERAL_LEN: usize = 32;
const SEAL_INFO: &[u8] = b"osv/ecies/v1";

pub fn public_key(secret: &[u8; 32]) -> [u8; 32] {
    PublicKey::from(&StaticSecret::from(*secret)).to_bytes()
}

pub fn ecdh(our_secret: &[u8; 32], their_public: &[u8; 32]) -> Secret32 {
    Secret32::new(StaticSecret::from(*our_secret).diffie_hellman(&PublicKey::from(*their_public)).to_bytes())
}

/// The AEAD key for a shared secret (HKDF info `osv/ecies/v1`: pinned, do not change).
fn seal_key(shared: &Secret32) -> Secret32 {
    Secret32::new(kdf::hkdf_sha256(shared.expose(), None, SEAL_INFO))
}

/// ECIES seal to a recipient's X25519 public key.
/// Output is `ephemeral_public(32) || aead(nonce | ciphertext | tag)`.
pub fn seal(recipient_public: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let ephemeral = StaticSecret::from(crate::rng::bytes());
    let ephemeral_public = PublicKey::from(&ephemeral);

    let shared = Secret32::new(ephemeral.diffie_hellman(&PublicKey::from(*recipient_public)).to_bytes());
    let ciphertext = aead::seal(&seal_key(&shared), &[], plaintext)?;

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

    aead::open(&seal_key(&ecdh(recipient_secret, &eph)), &[], ciphertext)
}

#[cfg(test)]
mod tests;
