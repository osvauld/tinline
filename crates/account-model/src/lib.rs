//! The account data model with no IO runtime, transport or UniFFI: the persisted `State` and
//! its records, the clear `account.json` and the account directories, the passphrase vault, the
//! 1:1 chat day doc (`chat::doc`), and the own-device
//! Loro docs (`sync`: account, calls, profile, profile cache) with their merge rules, plus the
//! pure helpers those rules need (token slots, device lists, ticket liveness, block deletion
//! requests). p2pcore re-exports these modules under their old paths and owns the node, the
//! sealed account store, sessions, wire and the app API.

#![deny(clippy::string_slice)]

pub mod accounts;
pub mod block;
pub mod caps;
pub mod chat;
pub mod devices;
mod error;
pub mod profile;
pub mod store;
pub mod sync;
pub mod tickets;
pub mod vault;

pub use error::Error;
