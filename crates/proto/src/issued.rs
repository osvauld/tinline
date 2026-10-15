//! Issued statements: "issuer lets subject do C until exp", one shape for capability tokens,
//! group invites, picture deletion permits and anything else a DID hands out. Every time is unix
//! milliseconds. `Signed<Issued<C>>::redeem` is the one admission check (signature, issuer,
//! subject, validity window, cut-off, spent); callers check membership, verified-DID and other
//! policy after it.
//!
//! Where an issuer remembers what it handed out (spent nonces, per-holder cut-offs) is the
//! caller's store, behind [`IssuanceStore`]; the stores stay where their data lives.

use cryptography::domains::Domain;
use identity::Identity;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{Error, Result, Signable, Signed, Signer};

/// How far an issuer's clock may run ahead of ours: a statement issued up to this far in our
/// future is still accepted.
pub const ISSUE_SKEW_MS: u64 = 5 * 60 * 1000;

/// Who may redeem a statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Subject {
    /// Only this DID.
    Did(String),
    /// Whoever holds it (a bearer statement, e.g. a group invite).
    Anyone,
}

/// The claim inside an [`Issued`] statement: its domain, how long one may live, its size cap.
pub trait IssuedClaim: Serialize + DeserializeOwned {
    const DOMAIN: Domain;
    const MAX_TTL_MS: u64;
    const MAX_BODY: usize;
    /// Shape of the claim itself, never policy.
    fn check(&self) -> Result<()> {
        Ok(())
    }
}

/// `iss` lets `sub` do `claim` from `iat_ms` until `exp_ms` (exclusive).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issued<C> {
    pub iss: String,
    pub sub: Subject,
    pub iat_ms: u64,
    pub exp_ms: u64,
    pub nonce: [u8; 16],
    pub claim: C,
}

impl<C: IssuedClaim> Signable for Issued<C> {
    const DOMAIN: Domain = C::DOMAIN;
    const MAX_BODY: usize = C::MAX_BODY;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.iss)
    }
    fn check(&self) -> Result<()> {
        if self.exp_ms <= self.iat_ms || self.exp_ms - self.iat_ms > C::MAX_TTL_MS {
            return Err(Error::Decode);
        }
        self.claim.check()
    }
}

/// What an issuer remembers about what it handed out.
pub trait IssuanceStore {
    /// Whether `holder` may no longer redeem the statement with this nonce (used up, or not one
    /// the issuer still honours). Contact tickets carry a shorter nonce than `Issued`, so any
    /// length is accepted.
    fn spent(&self, iss: &str, nonce: &[u8], holder: &str) -> bool;
    /// Statements of `domain` for `holder` issued at or before this are taken back.
    fn cut_off_ms(&self, holder: &str, domain: Domain) -> u64;
}

/// A store that remembers nothing: nothing spent, no cut-offs. For statements checked by
/// someone other than their issuer, or before the issuer's own policy runs.
pub struct Unrecorded;

impl IssuanceStore for Unrecorded {
    fn spent(&self, _: &str, _: &[u8], _: &str) -> bool {
        false
    }
    fn cut_off_ms(&self, _: &str, _: Domain) -> u64 {
        0
    }
}

/// What the redeemer expects: the issuer (`None`: any), who is redeeming, and now.
#[derive(Debug, Clone, Copy)]
pub struct Expect<'a> {
    pub iss: Option<&'a str>,
    pub holder: &'a str,
    pub now_ms: u64,
}

impl<C: IssuedClaim> Issued<C> {
    /// Everything `redeem` checks after the signature: issuer, subject, validity window,
    /// cut-off, spent. For a statement already opened (checked again under another store).
    pub fn admit(&self, e: &Expect<'_>, store: &dyn IssuanceStore) -> Result<()> {
        if e.iss.is_some_and(|iss| iss != self.iss) {
            return Err(Error::WrongIssuer);
        }
        if let Subject::Did(d) = &self.sub
            && d != e.holder
        {
            return Err(Error::WrongHolder);
        }
        if self.iat_ms > e.now_ms.saturating_add(ISSUE_SKEW_MS) {
            return Err(Error::NotYetValid);
        }
        if e.now_ms >= self.exp_ms {
            return Err(Error::Expired);
        }
        if self.iat_ms <= store.cut_off_ms(e.holder, C::DOMAIN) {
            return Err(Error::Revoked);
        }
        if store.spent(&self.iss, &self.nonce, e.holder) {
            return Err(Error::Replayed);
        }
        Ok(())
    }
}

impl<C: IssuedClaim> Signed<Issued<C>> {
    /// The one admission check: signature, then [`Issued::admit`].
    pub fn redeem(&self, e: &Expect<'_>, store: &dyn IssuanceStore) -> Result<Issued<C>> {
        let st = self.open()?;
        st.admit(e, store)?;
        Ok(st)
    }
}

/// Signs a fresh statement from `id` with a random nonce, valid `ttl_ms` from `iat_ms`.
pub fn issue<C: IssuedClaim>(id: &Identity, sub: Subject, iat_ms: u64, ttl_ms: u64, claim: C) -> Result<Signed<Issued<C>>> {
    let st = Issued {
        iss: id.did().to_string(),
        sub,
        iat_ms,
        exp_ms: iat_ms.saturating_add(ttl_ms),
        nonce: cryptography::rng::bytes(),
        claim,
    };
    Signed::sign(&st, id)
}

#[cfg(test)]
mod tests;
