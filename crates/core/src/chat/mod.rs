//! 1:1 chat: sealed vault store, Loro conversation docs, signed batches over `tinline/chat/3`,
//! files and voice as encrypted iroh-blobs. See `docs/chat.md`.

// The day doc and the blob cipher are `account_model::chat`.
pub use account_model::chat::doc;
pub use transfer::crypt;

pub mod api;
pub mod engine;
pub mod own;
pub mod presence;
pub mod store;
pub mod wire;
pub mod photo;
