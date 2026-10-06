use std::sync::Arc;

use bip39::Mnemonic;
use cryptography::{CryptoError, ecies, signature};
use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::ZeroizeOnDrop;

use crate::did;

// Domain-separated HKDF contexts. These are a forever-contract: changing one
// changes every key (and DID) derived from a seed.
const CTX_SIGNING: &[u8] = b"osv/identity/signing/v1";
const CTX_ENCRYPTION: &[u8] = b"osv/identity/encryption/v1";
const CTX_DEVICE: &[u8] = b"osv/identity/device/v1";

#[derive(ZeroizeOnDrop)]
struct Inner {
    signing_secret: [u8; 32],
    encryption_secret: [u8; 32],
    device_secret: [u8; 32],
    #[zeroize(skip)]
    signing_public: [u8; 32],
    #[zeroize(skip)]
    encryption_public: [u8; 32],
    #[zeroize(skip)]
    device_public: [u8; 32],
    #[zeroize(skip)]
    did: String,
}

#[derive(Clone)]
pub struct Identity {
    inner: Arc<Inner>,
}

impl Identity {
    pub(crate) fn generate() -> (Self, Mnemonic) {
        let mut entropy = [0u8; 32];
        OsRng.fill_bytes(&mut entropy);
        let mnemonic = Mnemonic::from_entropy(&entropy).expect("32 bytes is valid entropy");
        let identity = Self::from_mnemonic(&mnemonic);
        (identity, mnemonic)
    }

    pub(crate) fn from_mnemonic(mnemonic: &Mnemonic) -> Self {
        Self::from_seed(&mnemonic.to_seed(""))
    }

    pub(crate) fn from_seed(seed: &[u8; 64]) -> Self {
        Self::from_secret_keys(
            cryptography::kdf::hkdf_sha256(seed, None, CTX_SIGNING),
            cryptography::kdf::hkdf_sha256(seed, None, CTX_ENCRYPTION),
            cryptography::kdf::hkdf_sha256(seed, None, CTX_DEVICE),
        )
    }

    pub(crate) fn from_secret_keys(
        signing_secret: [u8; 32],
        encryption_secret: [u8; 32],
        device_secret: [u8; 32],
    ) -> Self {
        let signing_public = signature::public_key(&signing_secret);
        let encryption_public = ecies::public_key(&encryption_secret);
        let device_public = signature::public_key(&device_secret);
        let did = did::did_from_public_key(&signing_public);
        Self {
            inner: Arc::new(Inner {
                signing_secret,
                encryption_secret,
                device_secret,
                signing_public,
                encryption_public,
                device_public,
                did,
            }),
        }
    }

    pub fn did(&self) -> &str {
        &self.inner.did
    }

    pub fn signing_public_key(&self) -> [u8; 32] {
        self.inner.signing_public
    }

    pub fn encryption_public_key(&self) -> [u8; 32] {
        self.inner.encryption_public
    }

    pub fn device_public_key(&self) -> [u8; 32] {
        self.inner.device_public
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        signature::sign(&self.inner.signing_secret, message)
    }

    pub fn decrypt_sealed(&self, data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        ecies::open(&self.inner.encryption_secret, data)
    }

    pub(crate) fn signing_secret(&self) -> &[u8; 32] {
        &self.inner.signing_secret
    }

    pub(crate) fn encryption_secret(&self) -> &[u8; 32] {
        &self.inner.encryption_secret
    }

    pub(crate) fn device_secret(&self) -> &[u8; 32] {
        &self.inner.device_secret
    }
}

#[cfg(test)]
mod tests;
