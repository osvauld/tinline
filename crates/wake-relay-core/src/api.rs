//! The relay's JSON API (`POST /v1/register`, `/v1/wake`, `/v1/revoke`). Byte fields are
//! base64url without padding.
//!
//! Status codes the sender acts on: 202 sent; 410 the token is dead (expired, revoked, push
//! token gone, relay key retired): drop it and wait for a fresh one; 429 limited and 503 platform
//! trouble: fall back to normal retries; 403 kind not allowed; 400 malformed.

use serde::{Deserialize, Serialize};

use crate::b64;
use crate::token::{Kind, Kinds, Target};

/// Largest request body the relay reads.
pub const MAX_BODY_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub target: Target,
    #[serde(with = "b64::array")]
    pub hint_key: [u8; 32],
    pub tags: Vec<TagRequest>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagRequest {
    #[serde(with = "b64::array")]
    pub tag: [u8; 16],
    pub kinds: Kinds,
    pub exp_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub tokens: Vec<IssuedToken>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssuedToken {
    #[serde(with = "b64::array")]
    pub tag: [u8; 16],
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WakeRequest {
    pub token: String,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevokeRequest {
    pub token: String,
}
