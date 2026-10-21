//! Writer IDs, device IDs and DM pair IDs.

/// `pair` for two DIDs: hex BLAKE3 of the sorted, NUL-joined DIDs.
pub fn pair_id(a: &str, b: &str) -> String {
    let (x, y) = if a <= b { (a, b) } else { (b, a) };
    let mut h = blake3::Hasher::new();
    h.update(x.as_bytes());
    h.update(&[0]);
    h.update(y.as_bytes());
    h.finalize().to_hex().to_string()
}

/// The writer ID (U16) of the account `did` on the install whose device ID is `device`: the
/// first 64 bits (little-endian) of `hash::hasher(WRITER_ID)` over the DID and the device ID. The app writes every chat doc with it as the Loro peer id, and every receiver recomputes
/// it from the author and the `dev` of each version. It depends on no device or transport key.
/// `u64::MAX` is not a valid Loro peer id; that one output maps to `u64::MAX - 1`.
pub fn writer_id(did: &str, device: &[u8; 32]) -> u64 {
    let id = cryptography::hash::hasher(cryptography::domains::WRITER_ID).part(did.as_bytes()).part(device).u64();
    if id == u64::MAX { u64::MAX - 1 } else { id }
}

/// A fresh random device ID: generated once per account per install, never derived.
pub fn new_device_value() -> [u8; 32] {
    let mut v = [0u8; 32];
    cryptography::rng::fill(&mut v);
    v
}
