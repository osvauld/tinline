//! The one signing-input rule: `input(domain, parts)` is
//! `u64 BE len(domain) || domain || (u64 BE len(part) || part)*`. Every signed structure in the
//! app signs exactly this, so no two (domain, parts) lists share bytes: part boundaries and the
//! domain are both framed.

use crate::domains::Domain;
use crate::signature;

/// Appends `u64 BE len || bytes`.
pub(crate) fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

/// The canonical signing input for `parts` under `domain`.
pub fn input(domain: Domain, parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + domain.len() + parts.iter().map(|p| 8 + p.len()).sum::<usize>());
    put(&mut out, domain);
    for p in parts {
        put(&mut out, p);
    }
    out
}

/// Ed25519 over `input(domain, parts)` with a raw secret key.
pub fn sign(secret: &[u8; 32], domain: Domain, parts: &[&[u8]]) -> [u8; 64] {
    signature::sign(secret, &input(domain, parts))
}

/// Strict Ed25519 verification of `sig` over `input(domain, parts)`.
pub fn verify(public: &[u8; 32], domain: Domain, parts: &[&[u8]], sig: &[u8; 64]) -> bool {
    signature::verify(public, &input(domain, parts), sig)
}

#[cfg(test)]
mod tests;
