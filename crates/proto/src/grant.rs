//! Capability tokens (`Grant`), their issue and verify paths, and the token policy.

use super::*;

// ---- grant (capability tokens) ------------------------------------------------------

/// What a token lets its holder do with the issuer (U13, U15: separate tokens, one capability
/// each). Only the issuer ever verifies its own tokens, so taking one back is local: the issuer
/// refuses it (per-DID cut-off, see `verify_token`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Grant {
    pub v: u8,
    pub id: String,
    pub iss: String,
    pub holder: String,
    pub cap: String,
    pub iat: u64,
    pub exp: u64,
}

impl Grant {
    /// A legacy (version 1) `call` grant from before separate tokens.
    pub fn is_legacy(&self) -> bool {
        self.v == LEGACY_GRANT_VERSION
    }

    /// Whether this token carries `cap`. A legacy `call` grant carries both (U13 migration).
    pub fn allows(&self, cap: Cap) -> bool {
        self.is_legacy() || Cap::parse(&self.cap) == Some(cap)
    }

    /// The capabilities it carries.
    pub fn caps(&self) -> Vec<Cap> {
        [Cap::Message, Cap::Call]
            .into_iter()
            .filter(|c| self.allows(*c))
            .collect()
    }
}

/// A single-capability token from `id` to `holder_did`, issued at `iat` (the caller picks it, so
/// it can keep tokens above a cut-off), valid for `ttl` seconds.
pub fn issue_token(id: &Identity, holder_did: &str, cap: Cap, iat: u64, ttl: u64) -> SignedGrant {
    let claim = Grant {
        v: GRANT_VERSION,
        id: random16(),
        iss: id.did().to_string(),
        holder: holder_did.to_string(),
        cap: cap.as_str().to_string(),
        iat,
        exp: iat.saturating_add(ttl),
    };
    sign_blob(id, GRANT_DOMAIN, &claim)
}

/// A call token (`issue_token` with `Cap::Call`); kept for the call paths and tests.
pub fn issue_grant(id: &Identity, holder_did: &str, now: u64, ttl: u64) -> SignedGrant {
    issue_token(id, holder_did, Cap::Call, now, ttl)
}

/// For tests only: a legacy version-1 `call` grant as apps before separate tokens issued it.
#[doc(hidden)]
pub fn issue_legacy_grant_for_test(
    id: &Identity,
    holder_did: &str,
    now: u64,
    ttl: u64,
) -> SignedGrant {
    let claim = Grant {
        v: LEGACY_GRANT_VERSION,
        id: random16(),
        iss: id.did().to_string(),
        holder: holder_did.to_string(),
        cap: CAP_CALL.to_string(),
        iat: now,
        exp: now.saturating_add(ttl),
    };
    sign_blob(id, GRANT_DOMAIN, &claim)
}

/// The claims of a token without checking anything (for bookkeeping such as picking which slot a
/// stored token goes in; never for admission).
pub fn peek_grant(g: &SignedGrant) -> Option<Grant> {
    serde_json::from_slice(&dec(&g.payload).ok()?).ok()
}

/// Checks a token of any capability: signed by the DID it names as issuer, issuer and holder as
/// expected, a known version/capability, unexpired, id not revoked. Which capability it carries
/// is the caller's check (`Grant::allows`, or `verify_token`).
pub fn verify_grant(
    g: &SignedGrant,
    expected_iss: &str,
    expected_holder: &str,
    now: u64,
    revoked: &HashSet<String>,
) -> Result<Grant> {
    let grant: Grant = open_blob(g, GRANT_DOMAIN, |c: &Grant| &c.iss)?;
    match grant.v {
        GRANT_VERSION if Cap::parse(&grant.cap).is_some() => {}
        LEGACY_GRANT_VERSION if grant.cap == CAP_CALL => {}
        GRANT_VERSION | LEGACY_GRANT_VERSION => return Err(Error::BadCapability),
        _ => return Err(Error::UnknownVersion),
    }
    if grant.iss != expected_iss {
        return Err(Error::WrongIssuer);
    }
    if grant.holder != expected_holder {
        return Err(Error::WrongHolder);
    }
    if now >= grant.exp {
        return Err(Error::Expired);
    }
    if revoked.contains(&grant.id) {
        return Err(Error::Revoked);
    }
    Ok(grant)
}

/// `verify_grant`, plus: the token carries `cap`, and it was issued after `cut_off` (unix
/// seconds; tokens with `iat <= cut_off` were taken back by the issuer: a block, or "disallow
/// calls" for `Cap::Call`). The cut-off is per holder DID, so it also covers tokens the issuer
/// never recorded (every renewal mints a new id).
pub fn verify_token(
    g: &SignedGrant,
    expected_iss: &str,
    expected_holder: &str,
    cap: Cap,
    cut_off: u64,
    now: u64,
    revoked: &HashSet<String>,
) -> Result<Grant> {
    let grant = verify_grant(g, expected_iss, expected_holder, now, revoked)?;
    if !grant.allows(cap) {
        return Err(Error::BadCapability);
    }
    if grant.iat <= cut_off {
        return Err(Error::Revoked);
    }
    Ok(grant)
}

/// The tokens one side gives the other when they become contacts (U13 defaults): a ticket/QR
/// contact gets message + call, a contact from a group chat request message only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenPolicy {
    /// Include a call token.
    pub call: bool,
    /// Issue above this (unix seconds): the holder's cut-off, so fresh tokens after an unblock
    /// are not refused with the old ones.
    pub not_before: u64,
}

impl TokenPolicy {
    /// Message + call, no cut-off.
    pub const TICKET: TokenPolicy = TokenPolicy {
        call: true,
        not_before: 0,
    };
    /// Message only, no cut-off.
    pub const MESSAGE_ONLY: TokenPolicy = TokenPolicy {
        call: false,
        not_before: 0,
    };

    pub(crate) fn issue(
        self,
        id: &Identity,
        holder: &str,
        now: u64,
        ttl: u64,
    ) -> (SignedGrant, Option<SignedGrant>) {
        let iat = now.max(self.not_before.saturating_add(1));
        (
            issue_token(id, holder, Cap::Message, iat, ttl),
            self.call
                .then(|| issue_token(id, holder, Cap::Call, iat, ttl)),
        )
    }
}

/// Checks the tokens a new contact gave us: a message token, and a call token if any. A legacy
/// grant in the message slot counts for calls too.
pub(crate) fn verify_contact_tokens(
    message: &SignedGrant,
    call: Option<&SignedGrant>,
    iss: &str,
    holder: &str,
    now: u64,
) -> Result<()> {
    let none = HashSet::new();
    verify_token(message, iss, holder, Cap::Message, 0, now, &none)?;
    if let Some(c) = call {
        verify_token(c, iss, holder, Cap::Call, 0, now, &none)?;
    }
    Ok(())
}
