//! Low-level cryptographic primitives over raw key bytes: AEAD, KDFs, ECIES, signatures.
//! No notion of identity, mnemonics, or DIDs — that lives in the `identity` crate.

pub mod aead;
pub mod ecies;
pub mod error;
pub mod kdf;
pub mod signature;

pub use error::CryptoError;
