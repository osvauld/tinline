//! Receipts. The claim codec, grow-only Loro receipt map and transactional ledger live in the
//! `receipts-core` crate and are re-exported here, so `crate::receipts::*` paths keep working.
//! This module keeps what needs p2pcore: the UniFFI surface ([`api`], [`list`]), the tick
//! visuals the apps draw ([`ticks`]) and the DM version digest over chat records ([`messages`]).
pub(crate) use receipts_core::*;

pub(crate) mod api;
pub(crate) mod list;
pub(crate) mod messages;
pub(crate) mod ticks;
