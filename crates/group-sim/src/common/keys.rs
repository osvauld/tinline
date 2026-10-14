use identity::{Identity, public_key_from_did, verify};
use ed25519_dalek::SigningKey;
use serde::Serialize;

pub(crate) fn hex32(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
pub(crate) fn has(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}
pub(crate) fn ident(seed: u8) -> Identity {
    let mnemonic = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
    identity::recover(&mnemonic.to_string()).unwrap()
}
pub(crate) fn ep(prefix: &str, k: &SigningKey) -> String {
    format!("{prefix}-{}", hex32(&k.verifying_key().to_bytes()))
}
pub(crate) fn signed<T: Serialize>(domain: &[u8], body: &T) -> Vec<u8> {
    [domain, &serde_json::to_vec(body).unwrap()].concat()
}
pub(crate) fn valid(did: &str, bytes: &[u8], sig: &[u8]) -> bool {
    let (Some(key), Ok(sig)) = (public_key_from_did(did), <[u8; 64]>::try_from(sig)) else {
        return false;
    };
    verify(&key, bytes, &sig)
}
pub(crate) fn digest<T: Serialize>(x: &T) -> String {
    blake3::hash(&serde_json::to_vec(x).unwrap())
        .to_hex()
        .to_string()
}
pub(crate) fn dir_key(did: &str, slot: u64) -> String {
    format!("{did}#{slot}")
}
pub(crate) fn pair(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}
pub(crate) fn set(xs: &[&str]) -> std::collections::BTreeSet<String> {
    xs.iter().map(|s| s.to_string()).collect()
}
