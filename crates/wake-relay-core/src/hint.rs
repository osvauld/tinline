//! The hint: the only data a push carries.
//!
//! `base64url(nonce[12] || AES-256-GCM(hint_key, postcard(Hint)))` under [`HINT_AAD`]. The relay
//! seals it with the receiving device's `hint_key` (from the wake token) and a fresh nonce, so
//! Google and Apple see opaque bytes that differ every time, and cannot link two pushes from the
//! same contact. The receiver opens it to learn which contact woke it, for what, and when.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use serde::{Deserialize, Serialize};

use crate::token::Kind;
use crate::{WakeError, b64};

pub const HINT_AAD: &[u8] = b"tinline/wake/hint/v1";
/// A call wake older than this is not rung (PROVISIONAL, docs/push.md "Calls").
pub const STALE_CALL_MS: u64 = 30_000;
const MAX_HINT_CHARS: usize = 512;
const NONCE_LEN: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hint {
    pub tag: [u8; 16],
    pub kind: Kind,
    /// The relay's clock when it sent the push.
    pub sent_at_ms: u64,
}

impl Hint {
    /// A call wake this old is not rung (iOS still reports it to CallKit and ends it at once).
    pub fn is_stale_call(&self, now_ms: u64) -> bool {
        self.kind == Kind::Call && now_ms.saturating_sub(self.sent_at_ms) > STALE_CALL_MS
    }
}

pub fn seal(hint_key: &[u8; 32], hint: &Hint, nonce: [u8; NONCE_LEN]) -> Result<String, WakeError> {
    let body = postcard::to_allocvec(hint).map_err(|_| WakeError::Malformed)?;
    let cipher = Aes256Gcm::new_from_slice(hint_key).map_err(|_| WakeError::Sealed)?;
    let sealed = cipher
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: &body,
                aad: HINT_AAD,
            },
        )
        .map_err(|_| WakeError::Sealed)?;
    let mut out = Vec::with_capacity(NONCE_LEN + sealed.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(b64::encode(&out))
}

pub fn open(hint_key: &[u8; 32], text: &str) -> Result<Hint, WakeError> {
    if text.len() > MAX_HINT_CHARS {
        return Err(WakeError::TooLong);
    }
    let raw = b64::decode(text).ok_or(WakeError::Malformed)?;
    if raw.len() < NONCE_LEN {
        return Err(WakeError::Malformed);
    }
    let (nonce, sealed) = raw.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(hint_key).map_err(|_| WakeError::Sealed)?;
    let nonce = Nonce::try_from(nonce).map_err(|_| WakeError::Malformed)?;
    let body = cipher
        .decrypt(
            &nonce,
            Payload {
                msg: sealed,
                aad: HINT_AAD,
            },
        )
        .map_err(|_| WakeError::Sealed)?;
    postcard::from_bytes(&body).map_err(|_| WakeError::Malformed)
}
