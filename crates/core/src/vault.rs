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
use cryptography::{aead, kdf};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::Error;

pub const ARGON2_M: u32 = 65536; // KiB
pub const ARGON2_T: u32 = 3;
pub const ARGON2_P: u32 = 4;

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

fn kek(pass: &str, salt: &[u8], m: u32, t: u32, p: u32) -> Result<Zeroizing<[u8; 32]>, Error> {
    // A tampered profile.json must not make us allocate gigabytes, nor downgrade the KDF to
    // something fast: only a band around our own parameters is accepted.
    if !(65536..=262144).contains(&m)
        || !(3..=10).contains(&t)
        || !(1..=8).contains(&p)
        || !(16..=64).contains(&salt.len())
    {
        return Err(Error::Io("vault parameters out of range".into()));
    }
    let k = kdf::argon2id(pass.as_bytes(), salt, m, t, p).map_err(|e| Error::Io(e.to_string()))?;
    Ok(Zeroizing::new(k))
}

fn wrap(dek: &Dek, pass: &str) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let mut salt = vec![0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    let k = kek(pass, &salt, ARGON2_M, ARGON2_T, ARGON2_P)?;
    let wrapped = aead::encrypt(&k, &dek[..]).map_err(|e| Error::Io(e.to_string()))?;
    Ok((salt, wrapped))
}

/// Seals `secrets` under a fresh DEK, wrapped by `pass` if given. Slow with a passphrase (one Argon2id).
pub fn seal(secrets: &Secrets, pass: Option<&str>) -> Result<(Vault, Dek), Error> {
    let mut dek = Zeroizing::new([0u8; 32]);
    rand::rngs::OsRng.fill_bytes(&mut dek[..]);
    let mut plain = serde_json::to_vec(secrets)?;
    let sealed = aead::encrypt(&dek, &plain).map_err(|e| Error::Io(e.to_string()));
    plain.zeroize();
    let sealed = sealed?;
    let vault = match pass {
        Some(pass) => rewrap_sealed(sealed, &dek, pass)?,
        None => Vault { salt: vec![], m: 0, t: 0, p: 0, wrapped_dek: vec![], sealed },
    };
    Ok((vault, dek))
}

/// Opens with the passphrase. Slow. A failed tag means a wrong passphrase.
pub fn open(v: &Vault, pass: &str) -> Result<(Secrets, Dek), Error> {
    if !v.has_passphrase() {
        return Err(Error::Protocol("no passphrase is set".into()));
    }
    let k = kek(pass, &v.salt, v.m, v.t, v.p)?;
    let mut raw = aead::decrypt(&k, &v.wrapped_dek).map_err(|_| Error::WrongPassphrase)?;
    let dek = <[u8; 32]>::try_from(raw.as_slice()).map(Zeroizing::new);
    raw.zeroize();
    let dek = dek.map_err(|_| Error::WrongPassphrase)?;
    let secrets = open_with_key(v, &dek[..])?;
    Ok((secrets, dek))
}

/// Opens with a remembered DEK; fast.
pub fn open_with_key(v: &Vault, key: &[u8]) -> Result<Secrets, Error> {
    let key = <[u8; 32]>::try_from(key).map(Zeroizing::new).map_err(|_| Error::WrongPassphrase)?;
    let mut plain = aead::decrypt(&key, &v.sealed).map_err(|_| Error::WrongPassphrase)?;
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
    Ok(Vault { salt, m: ARGON2_M, t: ARGON2_T, p: ARGON2_P, wrapped_dek, sealed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_passphrase_slot_round_trip() {
        let secrets = Secrets { mnemonic: "m".into(), device_secret: [5; 32] };
        let (v, dek) = seal(&secrets, None).unwrap();
        assert!(!v.has_passphrase());
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("sealed") && !json.contains("salt") && !json.contains("wrapped_dek"));
        let v: Vault = serde_json::from_str(&json).unwrap();
        assert_eq!(open_with_key(&v, &dek[..]).unwrap().device_secret, [5; 32]);
        assert!(open(&v, "anything").is_err());
        assert!(!rewrap(&v, &dek, "later").unwrap().wrapped_dek.is_empty());
    }

    #[test]
    fn parameter_bounds() {
        // Rejected before any hashing happens.
        for (m, t, p, s) in [
            (1024, 3, 4, 16),
            (1 << 20, 3, 4, 16),
            (65536, 1, 4, 16),
            (65536, 11, 4, 16),
            (65536, 3, 0, 16),
            (65536, 3, 9, 16),
            (65536, 3, 4, 8),
            (65536, 3, 4, 65),
        ] {
            assert!(kek("pass", &vec![7u8; s], m, t, p).is_err(), "{m} {t} {p} {s}");
        }
        assert!(kek("pass", &[7u8; 16], 65536, 3, 4).is_ok());
        assert!(kek("pass", &[7u8; 64], 65536, 10, 8).is_ok());
    }
}
