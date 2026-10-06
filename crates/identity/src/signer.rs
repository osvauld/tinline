use crate::Identity;

/// Acting as an identity without holding one. Everything here is public material or a
/// signature over it — never a secret — so a caller that needs a peer's signature can take
/// this instead of an [`Identity`] and leave the private keys wherever they are sealed.
///
/// `Identity` is the direct implementation; `vault` implements it over the account it holds
/// unlocked, which is what keeps signing keys out of the layers above it.
pub trait Signer {
    fn did(&self) -> &str;
    fn sign(&self, message: &[u8]) -> [u8; 64];
    fn encryption_public_key(&self) -> [u8; 32];
    fn device_public_key(&self) -> [u8; 32];
}

// Qualified calls, not `self.did()`: inherent methods win that lookup, but writing it out
// says so rather than relying on the reader knowing.
impl Signer for Identity {
    fn did(&self) -> &str {
        Identity::did(self)
    }

    fn sign(&self, message: &[u8]) -> [u8; 64] {
        Identity::sign(self, message)
    }

    fn encryption_public_key(&self) -> [u8; 32] {
        Identity::encryption_public_key(self)
    }

    fn device_public_key(&self) -> [u8; 32] {
        Identity::device_public_key(self)
    }
}

/// A `&T` signs exactly as the `T` behind it does, so `&impl Signer` arguments compose
/// without every caller re-borrowing.
impl<T: Signer + ?Sized> Signer for &T {
    fn did(&self) -> &str {
        (**self).did()
    }

    fn sign(&self, message: &[u8]) -> [u8; 64] {
        (**self).sign(message)
    }

    fn encryption_public_key(&self) -> [u8; 32] {
        (**self).encryption_public_key()
    }

    fn device_public_key(&self) -> [u8; 32] {
        (**self).device_public_key()
    }
}

#[cfg(test)]
mod tests;
