//! Separate `message` and `call` tokens (P6; decisions U13, U15; `docs/protocol.md` "Tokens").
//!
//! Each side of a contact holds the other's DID-signed tokens: a message token (opens chat) and,
//! when allowed, a call token (rings). Only the issuer verifies its own tokens, so taking one back
//! is local: the issuer refuses tokens issued at or before a per-DID cut-off (`State::cut_offs`).
//! - Ticket/QR contacts get message + call; group chat-request contacts message only
//!   (`ContactOrigin`).
//! - "Allow calls" (`allow_calls(did, true)`) sends a fresh call token; "disallow calls" raises the
//!   call cut-off, so every call token of that DID, legacy ones included, stops ringing; the
//!   contact keeps messaging. A call without a usable token is answered `CALLS_NOT_ALLOWED`
//!   (never rings, never "blocked").
//! - A block raises both cut-offs. Unblocking does not lower them: the other side re-adds us
//!   (U20) and the re-add carries fresh tokens issued above the cut-off.
//! - Renewal: the call token on an answered call (`Accept.renewed_grant`); the message token when
//!   a chat session finds it within `RENEW_WINDOW` of expiry (PROVISIONAL (coordinator)).
//! - Tokens sent outside the contact exchange and the call answer travel as `Msg::Tokens` from the
//!   device that queued them (`State::token_outbox`), retried until taken.
//! - Legacy (pre-P6) `call` grants count as message + call (U13 migration).
//!
//! Own devices share cut-offs and the per-contact `calls_allowed` through the account doc; a
//! sibling that has not synced yet still honours a token taken back elsewhere (sync window,
//! best effort: section 14 Q4 is open).

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use parking_lot::Mutex;
use proto::{Cap, Msg, SignedGrant};

use crate::Error;
use crate::node::{GRANT_TTL, Inner, Me, Node, addr_for, now};
use crate::store::State;
use crate::wire::{Ctrl, RETRY_SECS};

/// What a contact that may not call us is told (`Reject` reason) and how its call ends.
pub const CALLS_NOT_ALLOWED: &str = proto::CALLS_NOT_ALLOWED;

/// PROVISIONAL (coordinator): a chat session renews the peer's message token when it expires
/// within this many seconds.
pub(crate) const RENEW_WINDOW: u64 = 30 * 24 * 3600;

/// How a contact came to us, which decides the tokens we give it (U13 defaults).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactOrigin {
    /// Ticket / QR: message + call.
    Ticket,
    /// A 1:1 chat request through a group host (U3): message only, until "allow calls".
    GroupRequest,
}

/// Why a token was refused after the holder passed `proto::accept_hello` and the block check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Denied {
    /// Taken back by a block, or the wrong kind of token for this path: the generic refusal.
    Refused,
    /// A contact that may not call us: `CALLS_NOT_ALLOWED`.
    CallsNotAllowed,
}

/// The admission rule, pure. `grant` is a token we issued to `did` (already verified).
pub(crate) fn check(st: &State, did: &str, grant: &proto::Grant, cap: Cap) -> Result<(), Denied> {
    let cut = st.cut_offs.get(did).copied().unwrap_or_default();
    // A block takes back every token of that DID, whatever it carries.
    if grant.iat <= cut.message {
        return Err(Denied::Refused);
    }
    match cap {
        Cap::Message if grant.allows(Cap::Message) => Ok(()),
        Cap::Message => Err(Denied::Refused),
        Cap::Call => {
            let allowed = st.contacts.iter().any(|c| c.did == did && c.calls_allowed);
            if allowed && grant.allows(Cap::Call) && grant.iat > cut.call { Ok(()) } else { Err(Denied::CallsNotAllowed) }
        }
    }
}

/// Fresh tokens for `did` go above its cut-offs (both: a block raised both).
pub(crate) fn not_before(st: &State, did: &str) -> u64 {
    st.cut_offs.get(did).map_or(0, |c| c.message.max(c.call))
}

/// The tokens a contact from `origin` gets from us: an existing contact keeps its current call
/// permission (PROVISIONAL (coordinator): re-scanning a ticket does not upgrade a message-only
/// contact), a new one gets the `origin` default; always above the cut-off.
pub(crate) fn policy_for(st: &State, did: &str, origin: ContactOrigin) -> proto::TokenPolicy {
    let call = match st.contacts.iter().find(|c| c.did == did) {
        Some(c) => c.calls_allowed,
        None => origin == ContactOrigin::Ticket,
    };
    proto::TokenPolicy { call, not_before: not_before(st, did) }
}

/// Whether `new` (from `issuer` to `holder`, carrying `cap`) should replace `old` in that slot:
/// valid now and expiring later than `old` (an `old` that no longer verifies loses).
pub(crate) fn outlives(old: Option<&SignedGrant>, new: &SignedGrant, cap: Cap, issuer: &str, holder: &str, now: u64) -> bool {
    let none = HashSet::new();
    let Ok(new) = proto::verify_token(new, issuer, holder, cap, 0, now, &none) else { return false };
    let old_exp = old.and_then(|o| proto::verify_token(o, issuer, holder, cap, 0, now, &none).ok()).map_or(0, |g| g.exp);
    new.exp > old_exp
}

/// Runtime state that is not persisted.
#[derive(Default)]
pub(crate) struct CapsRt {
    /// DIDs with a token delivery task running.
    delivering: Mutex<HashSet<String>>,
}

#[uniffi::export]
impl Node {
    /// "Allow calls" for a contact (U13): `true` sends them a fresh call token (retried until
    /// their app takes it); `false` takes back every call token they hold (they keep messaging,
    /// and a call from them ends with "calls not allowed" without ringing). Synced to our other
    /// devices. A call already in progress is not ended. `NotFound` if `did` is not a contact.
    pub fn allow_calls(&self, did: String, allowed: bool) -> Result<(), Error> {
        self.inner.me()?;
        let deliver = {
            let mut s = self.inner.shared.lock();
            let st = &mut s.state;
            let c = st.contacts.iter_mut().find(|c| c.did == did).ok_or(Error::NotFound)?;
            c.calls_allowed = allowed;
            if allowed {
                // Blocked: the token goes with the re-add after an unblock instead.
                let deliver = !st.blocked.contains(&did);
                if deliver {
                    st.token_outbox.entry(did.clone()).or_default().insert(proto::CAP_CALL.into());
                }
                deliver
            } else {
                let cut = st.cut_offs.entry(did.clone()).or_default();
                cut.call = cut.call.max(now());
                if let Some(q) = st.token_outbox.get_mut(&did) {
                    q.remove(proto::CAP_CALL);
                }
                false
            }
        };
        let saved = self.inner.persist();
        if deliver {
            self.inner.tokens_kick(did);
        }
        self.inner.events.on_contacts_changed();
        saved
    }

    /// Whether we let `did` call us (they hold, or are being sent, our call token). Ticket
    /// contacts: true unless "disallow calls"; group chat-request contacts: false until "allow
    /// calls".
    pub fn calls_allowed(&self, did: String) -> bool {
        self.inner.shared.lock().state.contacts.iter().any(|c| c.did == did && c.calls_allowed)
    }

    /// Whether we can call `did`: we hold a call token from them that has not expired (they may
    /// still have taken it back; the call then ends with "calls not allowed" and this turns
    /// false). For the call button.
    pub fn can_call(&self, did: String) -> bool {
        let Ok(me) = self.inner.me() else { return false };
        let s = self.inner.shared.lock();
        if crate::block::blocked_me(&s.state, &did) {
            return false;
        }
        s.state.contacts.iter().find(|c| c.did == did).and_then(|c| c.call_grant_from_them.as_ref()).is_some_and(|g| {
            proto::verify_token(g, &did, me.id.did(), Cap::Call, 0, now(), &HashSet::new()).is_ok()
        })
    }
}

impl Node {
    /// Adds a contact from a ticket with the tokens `origin` decides (see `ContactOrigin`).
    /// `add_contact` is `ContactOrigin::Ticket`; the group packet will add chat-request contacts
    /// through `Inner::add_contact_from` with `GroupRequest`.
    #[doc(hidden)]
    pub fn add_contact_as_for_test(&self, ticket: String, origin: ContactOrigin) -> Result<crate::Contact, Error> {
        let inner = self.inner.clone();
        self.block_on(inner.add_contact_from(ticket, origin))?
    }

    /// For tests: ids of the (message, call) tokens we hold from `did`.
    #[doc(hidden)]
    pub fn held_token_ids_for_test(&self, did: String) -> (Option<String>, Option<String>) {
        let s = self.inner.shared.lock();
        let c = s.state.contacts.iter().find(|c| c.did == did);
        let id = |g: Option<&SignedGrant>| g.and_then(proto::peek_grant).map(|g| g.id);
        (id(c.map(|c| &c.grant_from_them)), id(c.and_then(|c| c.call_grant_from_them.as_ref())))
    }
}

impl Inner {
    /// A fresh token of ours for `did`, above its cut-offs.
    pub(crate) fn fresh_token(&self, me: &Me, did: &str, cap: Cap) -> SignedGrant {
        let nb = not_before(&self.shared.lock().state, did);
        proto::issue_token(&me.id, did, cap, now().max(nb.saturating_add(1)), GRANT_TTL)
    }

    /// The admission rule against the current state (see `check`).
    pub(crate) fn token_check(&self, did: &str, grant: &proto::Grant, cap: Cap) -> Result<(), Denied> {
        check(&self.shared.lock().state, did, grant, cap)
    }

    /// A chat session with `did` showed us `grant` (a message token of ours): renew it when it
    /// expires soon.
    pub(crate) fn renew_message_if_due(self: &Arc<Self>, did: &str, grant: &proto::Grant) {
        if grant.exp.saturating_sub(now()) >= RENEW_WINDOW {
            return;
        }
        let Ok(me) = self.me() else { return };
        {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return;
            }
            s.state.token_outbox.entry(did.to_string()).or_default().insert(proto::CAP_MESSAGE.into());
        }
        self.persist_in_background(me.epoch);
        self.tokens_kick(did.to_string());
    }

    /// A block takes back every token of `did` (both cut-offs to now) and drops what we still
    /// meant to send them. Called with the state lock held.
    pub(crate) fn revoke_all(st: &mut State, did: &str) {
        let t = now();
        let cut = st.cut_offs.entry(did.to_string()).or_default();
        cut.message = cut.message.max(t);
        cut.call = cut.call.max(t);
        st.token_outbox.remove(did);
    }

    /// We called `did` and they answered "calls not allowed": the call token we hold is of no
    /// use (taken back, or never given).
    pub(crate) fn drop_call_token(self: &Arc<Self>, epoch: u64, did: &str) {
        let changed = {
            let mut s = self.shared.lock();
            if s.epoch != epoch {
                return;
            }
            match s.state.contacts.iter_mut().find(|c| c.did == did) {
                Some(c) => c.call_grant_from_them.take().is_some(),
                None => false,
            }
        };
        if changed {
            self.persist_in_background(epoch);
            self.events.on_contacts_changed();
        }
    }

    /// An incoming connection that opened with `Tokens`: keep each token that is ours (holder =
    /// us), from a contact, and outlives the one held in its slot; say `TokensTaken` either way
    /// so the sender stops retrying.
    pub(crate) async fn tokens_incoming(self: &Arc<Self>, mut ctrl: Ctrl, grants: Vec<SignedGrant>) -> Result<(), Error> {
        let me = self.me()?;
        let my_did = me.id.did().to_string();
        let t = now();
        let mut changed = false;
        {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return Err(Error::Locked);
            }
            for g in grants.iter().take(8) {
                let Some(claim) = proto::peek_grant(g) else { continue };
                let Ok(v) = proto::verify_grant(g, &claim.iss, &my_did, t, &HashSet::new()) else { continue };
                let Some(c) = s.state.contacts.iter_mut().find(|c| c.did == v.iss) else { continue };
                if v.allows(Cap::Message) && !v.is_legacy() && outlives(Some(&c.grant_from_them), g, Cap::Message, &v.iss, &my_did, t) {
                    c.grant_from_them = g.clone();
                    changed = true;
                }
                if v.allows(Cap::Call) && !v.is_legacy() && outlives(c.call_grant_from_them.as_ref(), g, Cap::Call, &v.iss, &my_did, t) {
                    c.call_grant_from_them = Some(g.clone());
                    changed = true;
                }
            }
        }
        if changed {
            self.log("tokens taken");
            self.persist_in_background(me.epoch);
            self.events.on_contacts_changed();
        }
        ctrl.send(&Msg::TokensTaken).await?;
        ctrl.finish();
        Ok(())
    }

    /// Delivers what `token_outbox` holds for `did` (fresh tokens minted each round; a call token
    /// only while calls are still allowed) to one of their devices, retrying with backoff while
    /// the endpoint is up. One task per DID.
    pub(crate) fn tokens_kick(self: &Arc<Self>, did: String) {
        if !self.caps.delivering.lock().insert(did.clone()) {
            return;
        }
        let this = self.clone();
        self.handle.spawn(async move {
            let mut round = 0usize;
            loop {
                let Ok(me) = this.me() else { break };
                let Ok(ep) = this.endpoint() else { break };
                let (caps, devices) = {
                    let mut s = this.shared.lock();
                    let st = &mut s.state;
                    let contact = st.contacts.iter().find(|c| c.did == did).cloned();
                    let blocked = st.blocked.contains(&did) || crate::block::blocked_me(st, &did);
                    let Some(contact) = contact.filter(|_| !blocked) else {
                        st.token_outbox.remove(&did);
                        break;
                    };
                    let mut caps: BTreeSet<String> = st.token_outbox.get(&did).cloned().unwrap_or_default();
                    if !contact.calls_allowed {
                        caps.remove(proto::CAP_CALL);
                    }
                    if caps.is_empty() {
                        st.token_outbox.remove(&did);
                        break;
                    }
                    (caps, contact.devices)
                };
                let grants: Vec<SignedGrant> = caps
                    .iter()
                    .filter_map(|c| Cap::parse(c))
                    .map(|c| this.fresh_token(&me, &did, c))
                    .collect();
                let mut taken = false;
                for d in &devices {
                    match this.deliver_tokens(&ep, d.device, d.relay.as_deref(), &grants).await {
                        Ok(()) => {
                            taken = true;
                            break;
                        }
                        Err(e) => tracing::debug!("tokens to {did}: {e}"),
                    }
                }
                if taken {
                    {
                        let mut s = this.shared.lock();
                        if let Some(q) = s.state.token_outbox.get_mut(&did) {
                            q.retain(|c| !caps.contains(c));
                            if q.is_empty() {
                                s.state.token_outbox.remove(&did);
                            }
                        }
                    }
                    let _ = this.persist_async(me.epoch).await;
                    this.log(format!("tokens delivered to {did}"));
                    round = 0;
                    continue;
                }
                if devices.is_empty() {
                    break;
                }
                tokio::time::sleep(crate::wire::retry_pause(&RETRY_SECS, round)).await;
                round += 1;
            }
            this.caps.delivering.lock().remove(&did);
        });
    }

    async fn deliver_tokens(&self, ep: &iroh::Endpoint, device: [u8; 32], relay: Option<&str>, grants: &[SignedGrant]) -> Result<(), Error> {
        let conn = crate::wire::dial_call(ep, addr_for(&device, relay)?).await?;
        let reply = crate::wire::exchange(conn, &Msg::Tokens { grants: grants.to_vec() }).await?;
        match reply {
            Some(Msg::TokensTaken) => Ok(()),
            other => Err(Error::Protocol(format!("tokens not taken: {other:?}"))),
        }
    }

    /// After start: deliver whatever is still owed.
    pub(crate) fn tokens_started(self: &Arc<Self>) {
        let dids: Vec<String> = self.shared.lock().state.token_outbox.keys().cloned().collect();
        for did in dids {
            self.tokens_kick(did);
        }
    }
}

#[cfg(test)]
mod tests;
