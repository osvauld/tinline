//! Encrypted file and blob transfer on iroh-blobs, with no chat, account or UniFFI types.
//!
//! - [`crypt`]: the chunked AEAD every stored blob is sealed with (one fresh key per blob).
//! - [`BlobHub`]: the node's blob store and its serving side. Everything in it is ciphertext;
//!   the hash is the ciphertext's BLAKE3. Serving is gated per `(device key, hash)` by a
//!   [`Gate`] the caller supplies, and live upload progress and ends reach the caller through
//!   [`UploadProgress`] and [`UploadEnd`]. Fetching resumes partial downloads and gives up
//!   after [`STALL`] without progress. A hash is protected from the store's GC by a tag: the
//!   shared one ([`tag_of`], via `keep`/`release`) or an owner-scoped one (`*_owned`), so
//!   several users of one hash (a chat attachment, a profile picture) release independently.
//!
//! Who may read which hash, which connection to fetch over and what the bytes mean stay with
//! the caller (p2pcore for 1:1 chat and profile pictures).

#![deny(clippy::string_slice)]

pub mod crypt;
mod error;
mod hub;

pub use error::Error;
pub use hub::{BlobHub, GC_INTERVAL, Gate, STALL, UploadEnd, UploadProgress, tag_of};
