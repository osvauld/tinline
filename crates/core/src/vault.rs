//! The identity's secrets at rest: envelope encryption.
//!
//! A random 32-byte data key (DEK) seals `{mnemonic, device_secret}` with AES-256-GCM
//! (`sealed`). The DEK is itself wrapped under a key-encryption key (KEK) derived from the
//! passphrase with Argon2id (`wrapped_dek`). Changing the passphrase only rewraps the DEK, so a
//! DEK the platform remembered (e.g. wrapped by the Android Keystore) stays valid.
//! Parameters and primitives are the ones osvauld's keystore uses.
//!
//! The passphrase is optional: a vault without one has no `salt`/`m`/`t`/`p`/`wrapped_dek`, only
//! `sealed`, and the DEK lives solely in the platform's keystore (or the desktop's keyring).

use base64::{Engine, engine::general_purpose::STANDARD};
use cryptography::{Secret32, aead, kdf};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::Error;

/// AEAD associated data: a wrapped DEK and sealed secrets can never be swapped for each other.
const AAD_WRAPPED_DEK: &[u8] = b"tinline/vault/wrapped-dek/v1";
const AAD_SECRETS: &[u8] = b"tinline/vault/secrets/v1";

pub type Dek = Zeroizing<[u8; 32]>;

#[derive(Serialize, Deserialize, ZeroizeOnDrop)]
pub struct Secrets {
    pub mnemonic: String,
    pub device_secret: [u8; 32],
}

/// The `vault` object of `profile.json`; byte fields are standard base64.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vault {
    // The passphrase slot: all five are absent (empty / 0) when no passphrase is set.
    #[serde(with = "b64", default, skip_serializing_if = "Vec::is_empty")]
    pub salt: Vec<u8>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub m: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub t: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub p: u32,
    #[serde(with = "b64", default, skip_serializing_if = "Vec::is_empty")]
    pub wrapped_dek: Vec<u8>,
    #[serde(with = "b64")]
    pub sealed: Vec<u8>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl Vault {
    /// Whether a passphrase wraps the DEK. Without one only the platform's copy opens it.
    pub fn has_passphrase(&self) -> bool {
        !self.wrapped_dek.is_empty()
    }
}

mod b64 {
    use super::*;
    pub fn serialize<S: serde::Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(v))
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD.decode(s).map_err(serde::de::Error::custom)
    }
}

pub fn check_passphrase(pass: &str) -> Result<(), Error> {
    if pass.is_empty() {
        return Err(Error::WeakPassphrase);
    }
    Ok(())
}

/// The KEK. `kdf::passphrase_key` refuses parameters outside a band around `kdf::VAULT`, so a
/// tampered profile.json can neither make us allocate gigabytes nor downgrade the KDF.
fn kek(pass: &str, salt: &[u8], params: kdf::ArgonParams) -> Result<Secret32, Error> {
    kdf::passphrase_key(pass.as_bytes(), salt, params).map_err(|e| Error::Io(format!("vault: {e}")))
}

fn wrap(dek: &Dek, pass: &str) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let mut salt = vec![0u8; 16];
    cryptography::rng::fill(&mut salt);
    let k = kek(pass, &salt, kdf::VAULT)?;
    let wrapped = aead::seal(&k, AAD_WRAPPED_DEK, &dek[..]).map_err(|e| Error::Io(e.to_string()))?;
    Ok((salt, wrapped))
}

/// Seals `secrets` under a fresh DEK, wrapped by `pass` if given. Slow with a passphrase (one Argon2id).
pub fn seal(secrets: &Secrets, pass: Option<&str>) -> Result<(Vault, Dek), Error> {
    let mut dek = Zeroizing::new([0u8; 32]);
    cryptography::rng::fill(&mut dek[..]);
    let mut plain = serde_json::to_vec(secrets)?;
    let sealed = aead::seal(&dek, AAD_SECRETS, &plain).map_err(|e| Error::Io(e.to_string()));
    plain.zeroize();
    let sealed = sealed?;
    let vault = match pass {
        Some(pass) => rewrap_sealed(sealed, &dek, pass)?,
        None => Vault { salt: vec![], m: 0, t: 0, p: 0, wrapped_dek: vec![], sealed },
    };
    Ok((vault, dek))
}

/// The same DEK and passphrase slot, sealing other secrets (e.g. a new device secret). Fast.
pub fn reseal(v: &Vault, dek: &Dek, secrets: &Secrets) -> Result<Vault, Error> {
    let mut plain = serde_json::to_vec(secrets)?;
    let sealed = aead::seal(dek, AAD_SECRETS, &plain).map_err(|e| Error::Io(e.to_string()));
    plain.zeroize();
    Ok(Vault { sealed: sealed?, ..v.clone() })
}

/// Opens with the passphrase. Slow. A failed tag means a wrong passphrase.
pub fn open(v: &Vault, pass: &str) -> Result<(Secrets, Dek), Error> {
    if !v.has_passphrase() {
        return Err(Error::Protocol("no passphrase is set".into()));
    }
    let k = kek(pass, &v.salt, kdf::ArgonParams { m: v.m, t: v.t, p: v.p })?;
    let mut raw = aead::open(&k, AAD_WRAPPED_DEK, &v.wrapped_dek).map_err(|_| Error::WrongPassphrase)?;
    let dek = <[u8; 32]>::try_from(raw.as_slice()).map(Zeroizing::new);
    raw.zeroize();
    let dek = dek.map_err(|_| Error::WrongPassphrase)?;
    let secrets = open_with_key(v, &dek[..])?;
    Ok((secrets, dek))
}

/// Opens with a remembered DEK; fast.
pub fn open_with_key(v: &Vault, key: &[u8]) -> Result<Secrets, Error> {
    let key = <[u8; 32]>::try_from(key).map(Zeroizing::new).map_err(|_| Error::WrongPassphrase)?;
    let mut plain = aead::open(&key, AAD_SECRETS, &v.sealed).map_err(|_| Error::WrongPassphrase)?;
    let secrets = serde_json::from_slice(&plain);
    plain.zeroize();
    Ok(secrets?)
}

/// The same sealed secrets and DEK, wrapped under a new passphrase with a fresh salt. Slow.
pub fn rewrap(v: &Vault, dek: &Dek, new_pass: &str) -> Result<Vault, Error> {
    rewrap_sealed(v.sealed.clone(), dek, new_pass)
}

fn rewrap_sealed(sealed: Vec<u8>, dek: &Dek, pass: &str) -> Result<Vault, Error> {
    let (salt, wrapped_dek) = wrap(dek, pass)?;
    let kdf::ArgonParams { m, t, p } = kdf::VAULT;
    Ok(Vault { salt, m, t, p, wrapped_dek, sealed })
}

#[cfg(test)]
mod tests;
