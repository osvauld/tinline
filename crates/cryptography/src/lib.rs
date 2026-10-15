//! Low-level cryptographic primitives over raw key bytes: AEAD, KDFs, ECIES, signatures, and the
//! one binary codec (`codec`) that signed and stored bytes use.
//! No notion of identity, mnemonics, or DIDs — that lives in the `identity` crate.

pub mod aead;
pub mod codec;
pub mod domains;
pub mod ecies;
pub mod error;
pub mod hash;
pub mod kdf;
pub mod rng;
pub mod secret;
pub mod sign;
pub mod signature;

pub use error::CryptoError;
pub use secret::Secret32;
