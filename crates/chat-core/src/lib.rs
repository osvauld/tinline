//! The 1:1 chat model, free of the node, iroh connections and UniFFI: one conversation day as a
//! Loro doc with its batch rules (`doc`), the sealed chat vault rows (`store`), the signed
//! frames of `tinline/chat/7` and the batch signature (`wire`), and photo previews (`photo`).
//! The stream cipher of attachment blobs lives in the `transfer` crate; p2pcore owns the chat
//! engine, the iroh framing and the app API.

pub mod doc;
pub mod photo;
pub mod store;
pub mod wire;

mod error;
pub use error::Error;

#[cfg(test)]
mod doc_tests;
