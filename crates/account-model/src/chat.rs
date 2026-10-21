//! The 1:1 chat model: one conversation day as a Loro doc with its batch rules (`doc`). The
//! stream cipher of attachment blobs lives in the `transfer` crate. p2pcore owns the chat store, engine and wire.

pub mod doc;
#[cfg(test)]
mod doc_tests;
pub mod photo;
