//! The 1:1 chat model: one conversation day as a Loro doc with its batch rules (`doc`) and the
//! stream cipher of attachment blobs (`crypt`). p2pcore owns the chat store, engine and wire.

pub mod crypt;
pub mod doc;
#[cfg(test)]
mod doc_tests;
pub mod photo;
