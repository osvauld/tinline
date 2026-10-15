//! `Secret32`: a 32-byte secret that is wiped when dropped and never printed.

use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Secret32(Zeroizing<[u8; 32]>);

impl Secret32 {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Fresh random bytes.
    pub fn random() -> Self {
        let mut s = Self::new([0; 32]);
        crate::rng::fill(&mut s.0[..]);
        s
    }

    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<[u8; 32]> for Secret32 {
    fn from(bytes: [u8; 32]) -> Self {
        Self::new(bytes)
    }
}

impl std::ops::Deref for Secret32 {
    type Target = [u8; 32];
    fn deref(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for Secret32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret32(..)")
    }
}

#[cfg(test)]
mod tests;
