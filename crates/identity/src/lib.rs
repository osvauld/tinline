//! Identity: BIP39 mnemonic -> three derived keypairs (signing, encryption, device),
//! a self-authenticating DID, and a passphrase-sealed keystore.
//!
//! I/O-free: `seal` produces a `Keystore` (serialize with `to_bytes`), `unlock`
//! consumes one. Where those bytes live — file, db, memory — is the caller's choice.

mod did;
mod error;
mod identity;
mod keystore;
mod signer;

use cryptography::{ecies, signature};

pub use bip39::Mnemonic;
pub use cryptography::CryptoError;
pub use did::{did_from_public_key, public_key_from_did};
pub use error::IdentityError;
pub use identity::Identity;
pub use keystore::{Keystore, seal, unlock};
pub use signer::Signer;

pub fn generate() -> (Identity, Mnemonic) {
    Identity::generate()
}

pub fn recover(phrase: &str) -> Result<Identity, IdentityError> {
    let mnemonic = Mnemonic::parse(phrase).map_err(|_| IdentityError::Bip39)?;
    Ok(Identity::from_mnemonic(&mnemonic))
}

/// Verify a signature against a public signing key — e.g. a peer's, recovered from
/// their DID with `public_key_from_did`.
pub fn verify(public: &[u8; 32], message: &[u8], sig: &[u8; 64]) -> bool {
    signature::verify(public, message, sig)
}

/// ECIES-seal to a recipient's X25519 public key. The ephemeral keypair means the
/// sender's identity isn't involved, so this is a free function, not a method.
pub fn encrypt_for(
    recipient_encryption_key: &[u8; 32],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    ecies::seal(recipient_encryption_key, plaintext)
}

#[cfg(test)]
mod tests;
