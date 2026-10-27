//! The wake relay's logic with no I/O (design: `docs/push.md`). The Cloudflare Worker
//! (`crates/wake-relay-worker`, wasm32) does the HTTP, storage, randomness and signing; this crate
//! decides what is sent. The app core uses the same token and hint formats.
//!
//! - [`token`]: the wake token a device hands each contact, sealed by the relay.
//! - [`hint`]: the only data in a push, sealed to the woken device.
//! - [`relay`]: the three endpoints' checks (`register`, `wake`, `revoke`).
//! - [`push`]: the FCM / APNs / UnifiedPush requests and how to read their answers.
//! - [`jwt`]: the APNs and FCM auth tokens around a signature made elsewhere (WebCrypto).
//!
//! Nothing here carries content: no DID, name, message text or call detail.

pub mod api;
pub mod b64;
pub mod hint;
pub mod jwt;
pub mod push;
pub mod relay;
pub mod token;

#[cfg(test)]
mod tests;

pub use hint::Hint;
pub use token::{Kind, Kinds, RelayKey, Target, WakeClaim};

/// Why a request to the relay, or a token or hint, was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WakeError {
    #[error("malformed")]
    Malformed,
    #[error("too long")]
    TooLong,
    #[error("unknown relay key")]
    UnknownKey,
    #[error("does not open")]
    Sealed,
    #[error("expired")]
    Expired,
    #[error("kind not allowed")]
    KindNotAllowed,
    #[error("bad target: {0}")]
    BadTarget(&'static str),
    #[error("bad request: {0}")]
    BadRequest(&'static str),
}

pub const DAY_MS: u64 = 86_400_000;
