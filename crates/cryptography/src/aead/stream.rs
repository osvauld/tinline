//! Chunked AES-256-GCM for blobs under a key used for exactly one blob:
//! `chunk i = AES-256-GCM(key, nonce = 0u32 || i as u64 BE, aad = [last as u8], plaintext_i)`.
//! The index in the nonce stops reordering and duplication; the `last` flag stops truncation
//! and extension. A key must never seal two blobs (the nonces would repeat).

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};

use crate::error::CryptoError;

/// GCM tag bytes added to every chunk.
pub const TAG: usize = 16;

/// One blob's chunk sequence, sealing or opening in order from chunk 0.
pub struct Stream {
    cipher: Aes256Gcm,
    index: u64,
}

impl Stream {
    pub fn new(key: &[u8; 32]) -> Self {
        Self { cipher: Aes256Gcm::new_from_slice(key).expect("32-byte key"), index: 0 }
    }

    fn nonce(&mut self) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[4..].copy_from_slice(&self.index.to_be_bytes());
        self.index += 1;
        n
    }

    /// Seals the next chunk; `last` marks the final one.
    pub fn seal_next(&mut self, plaintext: &[u8], last: bool) -> Result<Vec<u8>, CryptoError> {
        let nonce = self.nonce();
        self.cipher.encrypt(&Nonce::from(nonce), Payload { msg: plaintext, aad: &[last as u8] }).map_err(|_| CryptoError::Aead)
    }

    /// Opens the next chunk; `last` must say whether it is the final one.
    pub fn open_next(&mut self, chunk: &[u8], last: bool) -> Result<Vec<u8>, CryptoError> {
        let nonce = self.nonce();
        self.cipher.decrypt(&Nonce::from(nonce), Payload { msg: chunk, aad: &[last as u8] }).map_err(|_| CryptoError::Aead)
    }
}
