//! Loro building blocks under p2pcore, with no IO, no transport and no UniFFI:
//!
//! - [`doc`]: a Loro doc that own devices sync whole (account doc, call log, profiles).
//! - [`batching`]: bounded export of native operations (group docs, receipts).
//! - [`signed_map`]: inspection of grow-only binary-map inserts.
//! - [`time`]: UTC days of unix-millisecond times; [`id`]: writer IDs and device IDs.

pub mod batching;
pub mod doc;
mod error;
pub mod id;
pub mod signed_map;
pub mod time;

pub use error::Error;
pub use id::{new_device_value, writer_id};
pub use time::{DAY_MS, HOUR_MS, day_of, day_start};
