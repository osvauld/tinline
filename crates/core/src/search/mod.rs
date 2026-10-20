//! Local search over the selected account's 1:1 chats, groups and names: a sealed, derived index
//! in `search.redb` next to `chat.redb`, written after every accepted change and reconciled on
//! open. Nothing leaves the device. See `docs/search.md`.

pub mod api;
// The index itself lives in `search-core`; p2pcore keeps the paths.
pub use search_core::{find, index, text};
