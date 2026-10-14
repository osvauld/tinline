//! Local search over the selected account's 1:1 chats, groups and names: a sealed, derived index
//! in `search.redb` next to `chat.redb`, written after every accepted change and reconciled on
//! open. Nothing leaves the device. See `docs/search.md`.

pub mod api;
pub mod hot;
pub mod index;
pub mod seg;
pub mod text;
pub mod find;
