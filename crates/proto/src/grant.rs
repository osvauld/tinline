//! Capability tokens (`Grant` = `Issued<Cap>`), their issue and verify paths, and the token
//! policy.

use super::*;

// ---- grant (capability tokens) ------------------------------------------------------

/// Longest a token may live (`GRANT_TTL` in core is a year).
pub const MAX_GRANT_TTL_MS: u64 = 366 * 24 * 3600 * 1000;

/// What a token lets its holder do with the issuer (U13, U15: separate tokens, one capability
/// each). Only the issuer ever verifies its own tokens, so taking one back is local: the issuer
/// refuses it (per-DID cut-off, see `IssuanceStore::cut_off_ms`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Cap {
    /// 1:1 chat (`tinline/chat`), history and delivery.
    Message,
    /// Ringing the issuer.
    Call,
}

impl Cap {
    pub fn as_str(self) -> &'static str {
        match self {
            Cap::Message => CAP_MESSAGE,
            Cap::Call => CAP_CALL,
        }
    }

    pub fn parse(s: &str) -> Option<Cap> {
        match s {
            CAP_MESSAGE => Some(Cap::Message),
            CAP_CALL => Some(Cap::Call),
            _ => None,
        }
    }
}

impl IssuedClaim for Cap {
    const DOMAIN: domains::Domain = domains::GRANT;
    const MAX_TTL_MS: u64 = MAX_GRANT_TTL_MS;
    const MAX_BODY: usize = 512;
}

/// A token: `iss` lets `sub` (always one DID) use `claim`.
pub type Grant = Issued<Cap>;
pub type SignedGrant = Signed<Grant>;

impl Grant {
    /// Whether this token carries `cap`.
    pub fn allows(&self, cap: Cap) -> bool {
        self.claim == cap
    }

    /// The DID it was issued to.
    pub fn holder(&self) -> Option<&str> {
        match &self.sub {
            Subject::Did(d) => Some(d),
            Subject::Anyone => None,
        }
    }
}

/// A single-capability token from `id` to `holder_did`, issued at `iat_ms` (the caller picks it,
/// so it can keep tokens above a cut-off), valid for `ttl_ms`.
pub fn issue_token(id: &Identity, holder_did: &str, cap: Cap, iat_ms: u64, ttl_ms: u64) -> SignedGrant {
    issued::issue(id, Subject::Did(holder_did.to_string()), iat_ms, ttl_ms, cap).expect("own grant signs")
}

/// A call token (`issue_token` with `Cap::Call`); kept for the call paths and tests.
pub fn issue_grant(id: &Identity, holder_did: &str, now_ms: u64, ttl_ms: u64) -> SignedGrant {
    issue_token(id, holder_did, Cap::Call, now_ms, ttl_ms)
}

/// `redeem` for a token, plus: it carries `cap`. `store` is the issuer's (cut-offs); a holder
/// checking a token it was given passes `Unrecorded`.
pub fn verify_token(
    g: &SignedGrant,
    expected_iss: &str,
    expected_holder: &str,
    cap: Cap,
    now_ms: u64,
    store: &dyn IssuanceStore,
) -> Result<Grant> {
    let grant = g.redeem(
        &Expect { iss: Some(expected_iss), holder: expected_holder, now_ms },
        store,
    )?;
    if !grant.allows(cap) {
        return Err(Error::BadCapability);
    }
    Ok(grant)
}

/// The tokens one side gives the other when they become contacts (U13 defaults): a ticket/QR
/// contact gets message + call, a contact from a group chat request message only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenPolicy {
    /// Include a call token.
    pub call: bool,
    /// Issue above this (unix ms): the holder's cut-off, so fresh tokens after an unblock are not
    /// refused with the old ones.
    pub not_before_ms: u64,
}

impl TokenPolicy {
    /// Message + call, no cut-off.
    pub const TICKET: TokenPolicy = TokenPolicy {
        call: true,
        not_before_ms: 0,
    };
    /// Message only, no cut-off.
    pub const MESSAGE_ONLY: TokenPolicy = TokenPolicy {
        call: false,
        not_before_ms: 0,
    };

    pub(crate) fn issue(
        self,
        id: &Identity,
        holder: &str,
        now_ms: u64,
        ttl_ms: u64,
    ) -> (SignedGrant, Option<SignedGrant>) {
        let iat = now_ms.max(self.not_before_ms.saturating_add(1));
        (
            issue_token(id, holder, Cap::Message, iat, ttl_ms),
            self.call
                .then(|| issue_token(id, holder, Cap::Call, iat, ttl_ms)),
        )
    }
}

/// Checks the tokens a new contact gave us: a message token, and a call token if any.
pub(crate) fn verify_contact_tokens(
    message: &SignedGrant,
    call: Option<&SignedGrant>,
    iss: &str,
    holder: &str,
    now_ms: u64,
) -> Result<()> {
    verify_token(message, iss, holder, Cap::Message, now_ms, &Unrecorded)?;
    if let Some(c) = call {
        verify_token(c, iss, holder, Cap::Call, now_ms, &Unrecorded)?;
    }
    Ok(())
}
