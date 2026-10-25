//! The checks behind each endpoint, in the order the Worker runs them. The Worker adds the two
//! things that need storage: the revoke list (KV, keyed by [`revoke_key`]) and the rate limits
//! (Cloudflare's rate-limit binding, keyed by [`rate_key`]).

use crate::WakeError;
use crate::api::{IssuedToken, RegisterRequest, RegisterResponse, WakeRequest};
use crate::hint::{self, Hint};
use crate::push::{self, HttpRequest, PushConfig};
use crate::token::{self, Kind, MAX_TOKEN_LIFE_MS, RelayKey, WakeClaim};

/// Most tags one register call may carry (one per contact of the device).
pub const MAX_TAGS_PER_REGISTER: usize = 512;
/// Rate limits per token and kind, over 60 s windows (PROVISIONAL, docs/push.md "Relay").
pub const MSG_WAKES_PER_MINUTE: u32 = 2;
pub const CALL_WAKES_PER_MINUTE: u32 = 3;

/// `/v1/register`: seals one wake token per tag. `nonce` yields fresh random bytes.
pub fn register(
    key: &RelayKey,
    req: &RegisterRequest,
    now_ms: u64,
    mut nonce: impl FnMut() -> [u8; 12],
) -> Result<RegisterResponse, WakeError> {
    req.target.validate()?;
    if req.tags.is_empty() || req.tags.len() > MAX_TAGS_PER_REGISTER {
        return Err(WakeError::BadRequest("tag count"));
    }
    let mut tokens = Vec::with_capacity(req.tags.len());
    for t in &req.tags {
        if !t.kinds.is_valid() {
            return Err(WakeError::BadRequest("kinds"));
        }
        if t.exp_ms <= now_ms || t.exp_ms > now_ms + MAX_TOKEN_LIFE_MS {
            return Err(WakeError::BadRequest("expiry"));
        }
        if t.kinds.allows(Kind::Call) && !req.target.supports(Kind::Call) {
            return Err(WakeError::BadRequest("call wakes need a voip token"));
        }
        let claim = WakeClaim {
            target: req.target.clone(),
            hint_key: req.hint_key,
            tag: t.tag,
            kinds: t.kinds,
            exp_ms: t.exp_ms,
        };
        tokens.push(IssuedToken {
            tag: t.tag,
            token: token::seal(key, &claim, nonce())?,
        });
    }
    Ok(RegisterResponse { tokens })
}

/// `/v1/wake`, before the revoke and rate checks: the token opens and allows this kind now.
pub fn open_for_wake(
    keys: &[RelayKey],
    req: &WakeRequest,
    now_ms: u64,
) -> Result<WakeClaim, WakeError> {
    let claim = token::open(keys, &req.token)?;
    claim.check(req.kind, now_ms)?;
    Ok(claim)
}

/// `/v1/wake`, after the checks: the platform request carrying a freshly sealed hint.
pub fn wake_request(
    cfg: &PushConfig,
    claim: &WakeClaim,
    kind: Kind,
    now_ms: u64,
    nonce: [u8; 12],
) -> Result<HttpRequest, WakeError> {
    let sealed = hint::seal(
        &claim.hint_key,
        &Hint {
            tag: claim.tag,
            kind,
            sent_at_ms: now_ms,
        },
        nonce,
    )?;
    push::request(cfg, &claim.target, kind, &sealed, now_ms).ok_or(WakeError::KindNotAllowed)
}

/// KV key of a revoked token; the entry lives until the token would have expired anyway.
pub fn revoke_key(claim: &WakeClaim) -> String {
    format!("rv:{}", claim.tag_id())
}

pub fn rate_key(claim: &WakeClaim, kind: Kind) -> String {
    format!("{}:{}", claim.tag_id(), kind.as_str())
}

impl WakeError {
    /// The HTTP status the relay answers with (see [`crate::api`]).
    pub fn status(&self) -> u16 {
        match self {
            WakeError::Expired | WakeError::UnknownKey => 410,
            WakeError::KindNotAllowed => 403,
            WakeError::Malformed
            | WakeError::TooLong
            | WakeError::Sealed
            | WakeError::BadTarget(_)
            | WakeError::BadRequest(_) => 400,
        }
    }
}
