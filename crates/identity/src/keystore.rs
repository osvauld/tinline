use cryptography::{aead, kdf};
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::error::IdentityError;
use crate::identity::Identity;

const VERSION: u8 = 1;
const ARGON2_M: u32 = 65536;
const ARGON2_T: u32 = 3;
const ARGON2_P: u32 = 4;

// Encrypted form of an identity: public keys + DID in the clear (so the DID can be
// shown before unlock), the three secret keys sealed under a passphrase-derived key.
// Self-contained and versioned; `to_bytes` serializes it, the caller persists it wherever.
#[derive(Serialize, Deserialize)]
pub struct Keystore {
    version: u8,
    did: String,
    public_signing_key: [u8; 32],
    public_encryption_key: [u8; 32],
    public_device_key: [u8; 32],
    sealed_signing: Vec<u8>,
    sealed_encryption: Vec<u8>,
    sealed_device: Vec<u8>,
    kdf_salt: [u8; 16],
    argon2_m: u32,
    argon2_t: u32,
    argon2_p: u32,
}

impl Keystore {
    pub fn did(&self) -> &str {
        &self.did
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("keystore serialization is infallible")
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, IdentityError> {
        let keystore: Self = bincode::deserialize(bytes).map_err(|_| IdentityError::Decode)?;
        if keystore.version != VERSION {
            return Err(IdentityError::Decode);
        }
        Ok(keystore)
    }
}

pub fn seal(identity: &Identity, passphrase: &str) -> Result<Keystore, IdentityError> {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let mut kek = kdf::argon2id(passphrase.as_bytes(), &salt, ARGON2_M, ARGON2_T, ARGON2_P)?;
    let result = seal_with(identity, &kek, salt);
    kek.zeroize();
    result
}

fn seal_with(
    identity: &Identity,
    kek: &[u8; 32],
    salt: [u8; 16],
) -> Result<Keystore, IdentityError> {
    Ok(Keystore {
        version: VERSION,
        did: identity.did().to_string(),
        public_signing_key: identity.signing_public_key(),
        public_encryption_key: identity.encryption_public_key(),
        public_device_key: identity.device_public_key(),
        sealed_signing: aead::encrypt(kek, identity.signing_secret())?,
        sealed_encryption: aead::encrypt(kek, identity.encryption_secret())?,
        sealed_device: aead::encrypt(kek, identity.device_secret())?,
        kdf_salt: salt,
        argon2_m: ARGON2_M,
        argon2_t: ARGON2_T,
        argon2_p: ARGON2_P,
    })
}

pub fn unlock(keystore: &Keystore, passphrase: &str) -> Result<Identity, IdentityError> {
    let mut kek = kdf::argon2id(
        passphrase.as_bytes(),
        &keystore.kdf_salt,
        keystore.argon2_m,
        keystore.argon2_t,
        keystore.argon2_p,
    )
    .map_err(|_| IdentityError::Decode)?;
    let result = unlock_with(keystore, &kek);
    kek.zeroize();
    result
}

fn unlock_with(keystore: &Keystore, kek: &[u8; 32]) -> Result<Identity, IdentityError> {
    Ok(Identity::from_secret_keys(
        decrypt_secret(kek, &keystore.sealed_signing)?,
        decrypt_secret(kek, &keystore.sealed_encryption)?,
        decrypt_secret(kek, &keystore.sealed_device)?,
    ))
}

// A failed AEAD tag during unlock almost always means a wrong passphrase, so we
// collapse decrypt failures to that rather than leaking the underlying cause.
fn decrypt_secret(kek: &[u8; 32], sealed: &[u8]) -> Result<[u8; 32], IdentityError> {
    let mut plain = aead::decrypt(kek, sealed).map_err(|_| IdentityError::WrongPassphrase)?;
    let secret = plain
        .as_slice()
        .try_into()
        .map_err(|_| IdentityError::Decode);
    plain.zeroize();
    secret
}

#[cfg(test)]
mod tests;
