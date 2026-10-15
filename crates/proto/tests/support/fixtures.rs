//! Fixtures shared by the rotation_* and token contract tests: deterministic identities and
//! endpoint keys, JSON helpers and URL-safe base64 for tampering with signed payloads.
//! TEST-ONLY. Fixture keys come from fixed seeds so runs are reproducible; this is NOT a
//! derivation scheme and receivers never compute them.
//!
//! The rotation design models that kept their own `fixture_key` now live on the
//! `design-models` branch (see `docs/archive/models.md`).
#![allow(dead_code)]
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::SigningKey;
use identity::Identity;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub fn fixture_seed(label: &[u8], seed: u8, generation: u64) -> [u8; 32] {
    Sha256::digest([label, &[seed], &generation.to_be_bytes()].concat()).into()
}

/// Deterministic fixture endpoint key. NOT a derivation scheme: receivers never compute it.
pub fn fixture_key(seed: u8, generation: u64) -> SigningKey {
    SigningKey::from_bytes(&fixture_seed(
        b"test-only/fixture-endpoint\0",
        seed,
        generation,
    ))
}

pub fn fixture_device_id(seed: u8, generation: u64) -> [u8; 32] {
    fixture_seed(b"test-only/fixture-device-id\0", seed, generation)
}

/// Deterministic owner identity from fixed entropy (never a real profile). Cloning it models
/// another holder of the SAME mnemonic (linked device or thief).
pub fn owner(seed: u8) -> Identity {
    let m = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
    identity::recover(&m.to_string()).unwrap()
}

pub fn did(seed: u8) -> String {
    owner(seed).did().to_string()
}

pub fn ser<T: Serialize>(t: &T) -> Vec<u8> {
    serde_json::to_vec(t).unwrap()
}

/// URL-safe, unpadded base64 (the encoding of `proto::SignedBlob` fields).
pub fn base64_encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn base64_decode(text: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(text).unwrap()
}
