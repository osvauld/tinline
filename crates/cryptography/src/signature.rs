use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

pub fn public_key(secret: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(secret).verifying_key().to_bytes()
}

pub fn sign(secret: &[u8; 32], message: &[u8]) -> [u8; 64] {
    SigningKey::from_bytes(secret).sign(message).to_bytes()
}

pub fn verify(public: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
    let Ok(verifying_key) = VerifyingKey::from_bytes(public) else {
        return false;
    };
    verifying_key
        .verify(message, &Signature::from_bytes(signature))
        .is_ok()
}

#[cfg(test)]
mod tests;
