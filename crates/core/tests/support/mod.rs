//! Shared harness for the core integration tests (`mod support;` in each test file).
//!
//! Every file under `tests/` is its own test binary, so each binary compiles the parts of this
//! module it names and ignores the rest (hence `dead_code` is allowed here). The group call
//! capacity module in `group_call_capacity.rs` is a test module of `group_call.rs`, not a helper;
//! it is not part of this module.
#![allow(dead_code, unused_imports)]

pub mod calls;
pub mod chat;
pub mod codec;
pub mod events;
pub mod fs;
pub mod group;
pub mod peer;
pub mod tempdir;
pub mod wait;

pub use calls::{ended, ended_after, expect, incoming, saw_incoming};
pub use chat::{all_msgs, all_msgs_oldest_first, delivered, has, holds, ticks};
pub use events::{Ev, LEv, LTap, NoLink, Quiet, Tap};
pub use group::{Member, WAIT};
pub use peer::{LinkAs, Peer, Suite};
pub use tempdir::TempDir;
pub use wait::{eventually, eventually_200ms, now_ms, now_ms_i64, now_secs, online, within};

/// The passphrase every test identity uses.
pub const PASS: &str = "test-passphrase";
