//! Token slots (U13): which of two tokens a contact's slot keeps. p2pcore owns issuing,
//! delivering and checking tokens on sessions.

use proto::{Cap, SignedGrant, Unrecorded};

/// Whether `new` (from `issuer` to `holder`, carrying `cap`) should replace `old` in that slot:
/// valid now and expiring later than `old` (an `old` that no longer verifies loses).
pub fn outlives(old: Option<&SignedGrant>, new: &SignedGrant, cap: Cap, issuer: &str, holder: &str, now_ms: u64) -> bool {
    let Ok(new) = proto::verify_token(new, issuer, holder, cap, now_ms, &Unrecorded) else { return false };
    let old_exp = old.and_then(|o| proto::verify_token(o, issuer, holder, cap, now_ms, &Unrecorded).ok()).map_or(0, |g| g.exp_ms);
    new.exp_ms > old_exp
}
