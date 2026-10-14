//! Avatar tint: which of N tinted containers a contact gets.
//!
//! The slot comes from FNV-1a (32 bit) over the key's UTF-8 bytes. The algorithm is fixed by the
//! constants below, so it gives the same slot on every device, platform and Rust version (unlike
//! `std`'s `DefaultHasher`).

const FNV_OFFSET: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;

/// FNV-1a, 32 bit, over `key`'s UTF-8 bytes.
#[uniffi::export]
pub fn stable_hash(key: String) -> u32 {
    key.bytes()
        .fold(FNV_OFFSET, |h, b| (h ^ b as u32).wrapping_mul(FNV_PRIME))
}

/// A stable index in `0..slots` for `key` (a contact DID). `slots` of 0 gives 0.
#[uniffi::export]
pub fn avatar_slot(key: String, slots: u32) -> u32 {
    if slots == 0 {
        0
    } else {
        stable_hash(key) % slots
    }
}

#[cfg(test)]
mod tests;
