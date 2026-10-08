//! Embedded key-value store: one ordered keyspace over redb, with sealed (encrypted) records
//! on top.
//!
//! Copied from osvauld2 (`storage` @ 90d22ec, plus the sealed-record idea of `vault`'s
//! `put_entry` / `put_doc`); keep in sync by hand. Changes against the original: atomic
//! multi-key writes (`apply`), prefix deletes and prefix scans that return values, and
//! [`Sealed`], which seals every value with a caller-supplied 32-byte key (AES-256-GCM, via
//! `cryptography`) and binds each record to its own path so a record cannot be moved to
//! another key undetected.
//!
//! Keys are sortable path strings (`"dm/<pair>/<day>/snap"`); the namespace is a prefix
//! convention. No serialisation here: values are bytes.

#![allow(clippy::result_large_err)]

mod error;
mod sealed;
mod store;

pub use error::StorageError;
pub use sealed::Sealed;
pub use store::{Op, Store};
