//! Domain-separated BLAKE3 for ids and digests. `hasher(d).part(a).part(b).finish()` is
//! `BLAKE3(sign::input(d, [a, b]))`: the same framing as signatures, so nothing hashed under one
//! domain or split can collide with another.

use crate::domains::Domain;

/// An incremental domain-separated hash.
pub struct Hasher(blake3::Hasher);

/// Starts a hash under `domain`.
pub fn hasher(domain: Domain) -> Hasher {
    let mut h = blake3::Hasher::new();
    absorb(&mut h, domain);
    Hasher(h)
}

fn absorb(h: &mut blake3::Hasher, bytes: &[u8]) {
    h.update(&(bytes.len() as u64).to_be_bytes());
    h.update(bytes);
}

impl Hasher {
    /// Adds one length-framed part.
    pub fn part(mut self, bytes: &[u8]) -> Self {
        absorb(&mut self.0, bytes);
        self
    }

    pub fn finish(self) -> [u8; 32] {
        *self.0.finalize().as_bytes()
    }

    /// Lowercase hex of the full digest (64 characters).
    pub fn hex(self) -> String {
        self.0.finalize().to_hex().to_string()
    }

    /// The first 8 digest bytes, little-endian.
    pub fn u64(self) -> u64 {
        let d = self.finish();
        u64::from_le_bytes(d[..8].try_into().expect("8 bytes"))
    }
}

/// The digest of one byte string under `domain`.
pub fn content(domain: Domain, bytes: &[u8]) -> [u8; 32] {
    hasher(domain).part(bytes).finish()
}

/// Keyed BLAKE3 over `sign::input(domain, parts)`: a MAC under a 32-byte secret key.
pub fn keyed(key: &[u8; 32], domain: Domain, parts: &[&[u8]]) -> [u8; 32] {
    *blake3::keyed_hash(key, &crate::sign::input(domain, parts)).as_bytes()
}

/// Constant-time check of a [`keyed`] tag.
pub fn keyed_verify(key: &[u8; 32], domain: Domain, parts: &[&[u8]], tag: &[u8]) -> bool {
    let Ok(tag) = <[u8; 32]>::try_from(tag) else { return false };
    blake3::keyed_hash(key, &crate::sign::input(domain, parts)) == blake3::Hash::from_bytes(tag)
}

#[cfg(test)]
mod tests;
