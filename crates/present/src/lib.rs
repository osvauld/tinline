//! Shared presentation logic: what both the iced desktop app and the Compose Android app used to
//! implement twice. Everything here is pure and deterministic. There is no clock and no time zone
//! read: callers pass `now`, timestamps (unix seconds) and a UTC offset (seconds east of UTC).
//! Functions return semantic enums where a screen needs to branch, plus a default English string
//! in the voice of `docs/design/VOICE.md`.
//!
//! The crate has no iroh/core dependency. It carries its own `uniffi::setup_scaffolding!()`; core
//! re-exports it (`pub use present;`) so the Android cdylib's bindings include these functions.
//! Where the two apps disagreed, the choice made is recorded in `DECISIONS.md`.

uniffi::setup_scaffolding!();

pub mod avatar;
pub mod block;
pub mod call;
pub mod emoji;
pub mod link;
pub mod names;
pub mod time;
