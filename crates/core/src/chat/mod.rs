//! 1:1 chat: sealed vault store, Loro conversation docs, signed batches over `tinline/chat/1`,
//! files and voice as encrypted iroh-blobs. See `docs/chat.md`.

pub mod blobs;
pub mod crypt;

#[cfg(test)]
mod blobs_tests;
pub mod api;
pub mod engine;
pub mod own;
pub mod store;
pub mod wire;
pub mod doc;
#[cfg(test)]
mod doc_tests;
