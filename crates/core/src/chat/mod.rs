//! 1:1 chat: sealed vault store, Loro conversation docs, signed batches over `tinline/chat/1`,
//! files and voice as encrypted iroh-blobs. See `docs/chat.md`.

pub mod blobs;
pub mod crypt;

#[cfg(test)]
mod blobs_tests;
