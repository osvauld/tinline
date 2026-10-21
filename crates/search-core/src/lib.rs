//! The local search index under p2pcore, with no transport, no async runtime and no UniFFI
//! (`docs/search.md`):
//!
//! - [`text`]: normalizing, tokenizing, matching and snippets.
//! - [`index`]: the sealed index (hot tier in [`hot`], write-once time segments in [`seg`]).
//! - [`find`]: the per-conversation filter behind find in chat.
//!
//! p2pcore owns the keys (derived from the recovery phrase), opening the file and the UniFFI
//! search API.

mod error;
pub mod find;
pub mod hot;
pub mod index;
pub mod seg;
pub mod text;
#[cfg(test)]
mod testkit;

pub use error::Error;
pub use index::{DocRow, INDEX_DOMAIN, Keys, Mark, Mode, PHASES, STORE_DOMAIN, SearchIndex};
