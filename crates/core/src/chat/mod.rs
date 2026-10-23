//! 1:1 chat: sealed vault store, Loro conversation docs, signed batches over `tinline/chat/3`,
//! files and voice as encrypted iroh-blobs. See `docs/chat.md`. The runtime is
//! `chat_runtime::chat`; this module keeps its paths and the UniFFI surface (`api`, `store`).

// The day doc is `chat_core::doc`.
pub use chat_core::doc;

pub mod api;
#[cfg(any(test, feature = "dev-tools"))]
mod hooks;
pub mod store;
