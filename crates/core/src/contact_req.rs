//! Adding a contact by code, as a request (2026-10-13; `docs/protocol.md` "Add contact" and
//! "Contact requests").
//!
//! Codes are reusable for 7 days, so nobody becomes our contact without our yes:
//! - The owner's device checks a `ContactHello` as before, then holds it (`State::contact_requests`,
//!   one per joiner DID) and answers `ContactPending`. The user sees "Maya wants to add you as a
//!   contact" (`ContactRequestEvents::on_contact_request`).
//! - Accept (`accept_contact_request`) checks the hello again, stores the contact and the redeemed
//!   (joiner DID, nonce) in one write, then dials the joiner's attested device with the
//!   `ContactWelcome` (`State::welcome_outbox`, retried until taken or a week has passed).
//! - "Not now" sends nothing; the request waits until the code it used expires.
//! - The joiner keeps its half (`State::sent_requests`) and takes the dialled-back Welcome if it
//!   matches (`welcome_incoming`): both have each other, with call tokens both ways (`policy_for`).

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use proto::{ContactTicket, Msg};

use crate::Error;
use crate::node::{CardPeek, Contact, Inner, Me, Node, addr_for, apply_contact, find_card, now, stored_contact};
use crate::store::{HeldRequest, WelcomeOut};
use crate::wire::Ctrl;

/// At most this many requests wait for an answer; more are refused (a code can reach anyone, and
/// DIDs cost nothing to make). PROVISIONAL.
pub(crate) const MAX_HELD: usize = 64;
/// How long an accepting device keeps trying to deliver its Welcome, and how long a joiner keeps
/// its half after the code expired (an Accept on the code's last day may land later).
pub(crate) const WELCOME_GRACE: u64 = 7 * 24 * 3600;
/// `add_contact` (which must return a contact) when the owner holds the hello as a request.
pub(crate) const PENDING: &str = "they hold it as a contact request";

const RETRY_SECS: [u64; 6] = [2, 5, 15, 30, 60, 300];
const DIAL_TIMEOUT: Duration = Duration::from_secs(20);
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// A contact request, either way: one made with our code (`contact_requests`, "Waiting for you")
/// or one we made with theirs (`sent_contact_requests`, "Waiting for them").
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ContactRequest {
    pub did: String,
    /// The name they gave themselves (incoming: unsigned, from their hello) or the signed name in
    /// their code (outgoing).
    pub name: String,
    /// When it arrived / was sent (unix ms).
    pub at: u64,
    /// The code it used stops working then (unix seconds); the request goes with it.
    pub expires_at: u64,
}

/// Our code and when it stops working ("It works until Mon 19 Oct").
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MyCode {
    pub code: String,
    /// Unix seconds.
    pub expires_at: u64,
}

/// What adding someone by their code did.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum AddOutcome {
    /// They added us at once (a re-add of an existing contact, or a group chat request ticket).
    Added { contact: Contact },
    /// Their device holds it for their user: "Request sent. Your line opens when Tara accepts."
    /// `ContactRequestEvents::on_request_accepted` follows if they accept.
    Requested { did: String, name: String },
}

/// What some text holds, read without dialling anyone (`peek_code`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum CodePeek {
    /// No contact code in it (or one that is damaged or forged).
    NotACode,
    /// Our own code.
    Own,
    /// Their code, signed, but past its date: the signed name is readable.
    Expired { name: String, did: String, expired_at: u64 },
    Found { card: CardPeek },
}

/// Why adding by code did not work, by kind, so the apps never read display strings.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum AddError {
    #[error("that doesn't look like a contact code")]
    NotACode,
    #[error("that is your own code")]
    OwnCode,
    #[error("that code has expired")]
    Expired,
    /// We are not online (node not started).
    #[error("you're offline")]
    Offline,
    /// Their device could not be reached in time (off, offline, or no route).
    #[error("couldn't reach them")]
    Unreachable,
    /// Their device turned it away: the code was reset, replaced by a newer one and expired, used
    /// by us already, or something did not check out. They can't say which.
    #[error("their code isn't valid any more")]
    Refused,
    /// They block us (their signed notice).
    #[error("they've chosen not to be in touch")]
    Blocked,
    #[error("{0}")]
    Failed(String),
}

impl From<Error> for AddError {
    fn from(e: Error) -> Self {
        match e {
            Error::Timeout | Error::Net(_) => AddError::Unreachable,
            Error::NotStarted => AddError::Offline,
            Error::Rejected(r) if r == crate::BLOCKED => AddError::Blocked,
            Error::Rejected(_) => AddError::Refused,
            other => AddError::Failed(other.to_string()),
        }
    }
}

/// Registered with `Node::set_contact_request_events`. Called on core threads: do not block.
#[uniffi::export(with_foreign)]
pub trait ContactRequestEvents: Send + Sync {
    /// Someone used our code: "Maya wants to add you as a contact". Once per joiner (a repeated
    /// hello is the same request).
    fn on_contact_request(&self, request: ContactRequest);
    /// They accepted the request we sent: our line to them is open.
    fn on_request_accepted(&self, contact: Contact);
}

/// Runtime state that is not persisted.
#[derive(Default)]
pub(crate) struct ReqRt {
    events: Mutex<Option<Arc<dyn ContactRequestEvents>>>,
    /// DIDs with a Welcome delivery task running.
    delivering: Mutex<HashSet<String>>,
    /// Tests: welcome every good hello at once, as before requests.
    pub(crate) auto_accept: AtomicBool,
}

pub(crate) fn request_view(did: &str, r: &HeldRequest) -> ContactRequest {
    ContactRequest { did: did.to_string(), name: r.name.clone(), at: r.at, expires_at: r.exp }
}

#[uniffi::export]
impl Node {
    pub fn set_contact_request_events(&self, events: Arc<dyn ContactRequestEvents>) {
        *self.inner.requests.events.lock() = Some(events);
    }

    /// Our code and its date. Same rules as `my_ticket`.
    pub fn my_code(&self) -> Result<MyCode, Error> {
        let code = self.my_ticket()?;
        let expires_at = ContactTicket::from_text(&code)?.verify(0)?.exp;
        Ok(MyCode { code, expires_at })
    }

    /// Reads the first contact code in `text` without dialling: whose it is, or why it can't be
    /// used (not a code, our own, expired). The name of an expired code is still signed.
    pub fn peek_code(&self, text: String) -> CodePeek {
        let Some(ticket) = find_card(&text) else { return CodePeek::NotACode };
        // `verify(0)` checks everything but the date.
        let Some(claim) = ContactTicket::from_text(&ticket).ok().and_then(|t| t.verify(0).ok()) else {
            return CodePeek::NotACode;
        };
        if self.inner.me().is_ok_and(|me| claim.iss == me.id.did()) {
            return CodePeek::Own;
        }
        if now() >= claim.exp {
            return CodePeek::Expired { name: proto::sanitize_name(&claim.name), did: claim.iss, expired_at: claim.exp };
        }
        let known = self.inner.shared.lock().state.contacts.iter().any(|c| c.did == claim.iss);
        CodePeek::Found { card: CardPeek { ticket, name: proto::sanitize_name(&claim.name), did: claim.iss, known } }
    }

    /// Adds someone by their code (any text holding one). Usually their device holds it as a
    /// request (`Requested`); blocks until it answered, at most ~45 s (dial 30 s, answer 15 s).
    pub fn add_contact_by_code(&self, text: String) -> Result<AddOutcome, AddError> {
        let ticket = find_card(&text).ok_or(AddError::NotACode)?;
        let claim = ContactTicket::from_text(&ticket).ok().and_then(|t| t.verify(0).ok()).ok_or(AddError::NotACode)?;
        if self.inner.me().is_ok_and(|me| claim.iss == me.id.did()) {
            return Err(AddError::OwnCode);
        }
        if now() >= claim.exp {
            return Err(AddError::Expired);
        }
        let inner = self.inner.clone();
        self.block_on(inner.add_contact_outcome(ticket, crate::caps::ContactOrigin::Ticket))
            .map_err(AddError::from)?
            .map_err(AddError::from)
    }

    /// Requests made with our code, waiting for our answer, oldest first ("Waiting for you").
    pub fn contact_requests(&self) -> Vec<ContactRequest> {
        let now = now();
        let s = self.inner.shared.lock();
        let mut v: Vec<_> = s
            .state
            .contact_requests
            .iter()
            .filter(|(did, r)| r.exp > now && !s.state.blocked.contains(*did) && !s.state.contacts.iter().any(|c| &c.did == *did))
            .map(|(did, r)| request_view(did, r))
            .collect();
        v.sort_by_key(|r| r.at);
        v
    }

    /// Requests we sent that their device holds for their user, oldest first ("Waiting for
    /// them"). Gone once they accept (the contact is there) or the code expires.
    pub fn sent_contact_requests(&self) -> Vec<ContactRequest> {
        let now = now();
        let s = self.inner.shared.lock();
        let mut v: Vec<_> = s
            .state
            .sent_requests
            .iter()
            .filter(|(did, r)| r.delivered && r.exp > now && !s.state.contacts.iter().any(|c| &c.did == *did))
            .map(|(did, r)| ContactRequest { did: did.clone(), name: r.pending.name.clone(), at: r.at, expires_at: r.exp })
            .collect();
        v.sort_by_key(|r| r.at);
        v
    }

    /// Accept: they become our contact (message and call both ways) and our device dials theirs
    /// with the Welcome (retried while they are offline). `NotFound` if there is no such request;
    /// `Rejected("blocked")` if we block them; `Protocol` if the code they used has expired since.
    pub fn accept_contact_request(&self, did: String) -> Result<Contact, Error> {
        let me = self.inner.me()?;
        let inner = self.inner.clone();
        let contact = self.block_on(async move {
            let this = inner.clone();
            tokio::task::spawn_blocking(move || this.accept_request(&me, &did))
                .await
                .map_err(|_| Error::Io("accept task failed".into()))?
        })??;
        self.inner.events.on_contacts_changed();
        self.inner.welcome_kick(contact.did.clone());
        self.inner.list_for_new_contact(&contact.did);
        self.inner.log(format!("accepted contact request from {}", contact.name));
        Ok(contact)
    }
}

impl Node {
    /// For tests that are not about requests: every good hello is welcomed at once, as before.
    #[doc(hidden)]
    pub fn auto_accept_contacts_for_test(&self, on: bool) {
        self.inner.requests.auto_accept.store(on, Ordering::SeqCst);
    }
}

impl Inner {
    pub(crate) fn request_event(&self, request: ContactRequest) {
        if let Some(ev) = self.requests.events.lock().clone() {
            self.handle.spawn_blocking(move || ev.on_contact_request(request));
        }
    }

    /// The Accept: the contact, the redeemed (DID, nonce), the request's removal and the Welcome
    /// to deliver are one durable write, as `accept_hello` does for a Welcome sent at once.
    fn accept_request(&self, me: &Me, did: &str) -> Result<Contact, Error> {
        let _w = self.writing.lock();
        let (mut next, backing) = {
            let s = self.shared.lock();
            if s.epoch != me.epoch {
                return Err(Error::Locked);
            }
            (s.state.clone(), s.committed.then(|| s.acct.as_ref().and_then(|a| a.backing.clone())).flatten())
        };
        let now = now();
        let req = next.contact_requests.get(did).cloned().ok_or(Error::NotFound)?;
        if self.refusal_for(did).is_some() {
            return Err(Error::Rejected(crate::BLOCKED.into()));
        }
        // Checked again: the code may have expired since, or another device of ours added them.
        let (welcome, new) = proto::accept_contact_hello_with(&me.id, me.device, &req.hello, req.device, now, &next.redeemed, crate::node::GRANT_TTL, |d| {
            crate::caps::policy_for(&next, d, crate::caps::ContactOrigin::Ticket)
        })?;
        if new.did != did {
            return Err(Error::Protocol("request does not match".into()));
        }
        let calls_allowed = matches!(&welcome, Msg::ContactWelcome { call_grant_for_you: Some(_), .. });
        let stored = stored_contact(&new, req.relay.clone(), calls_allowed);
        let contact = Contact::from(&stored);
        let out = WelcomeOut { welcome, device: req.device, relay: req.relay.clone(), until: now + WELCOME_GRACE };
        let apply = |st: &mut crate::store::State| {
            st.redeemed.insert(proto::redeemed_key(&new.did, &new.redeemed_nonce));
            st.contact_requests.remove(did);
            st.welcome_outbox.insert(did.to_string(), out.clone());
            apply_contact(st, stored.clone(), new.device, false)
        };
        apply(&mut next)?;
        if let Some(b) = &backing {
            self.write_state(b, &next)?;
        }
        let mut s = self.shared.lock();
        if s.epoch != me.epoch {
            return Err(Error::Locked);
        }
        apply(&mut s.state)?;
        Ok(contact)
    }

    /// A connection that opened with `ContactWelcome`: their user accepted our request. Taken
    /// only if it answers a request of ours to the DID and device it comes from.
    pub(crate) async fn welcome_incoming(self: &Arc<Self>, mut ctrl: Ctrl, msg: Msg, remote: [u8; 32], me: &Me) -> Result<(), Error> {
        let now = now();
        let found = {
            let s = self.shared.lock();
            s.state
                .sent_requests
                .iter()
                .find(|(_, r)| r.pending.device == remote && r.exp + WELCOME_GRACE > now)
                .map(|(d, r)| (d.clone(), r.clone()))
        };
        let taken = match found {
            Some((did, req)) if self.refusal_for(&did).is_none() => match proto::accept_contact_welcome(&me.id, &req.pending, &msg, remote, now) {
                Ok(new) => {
                    let stored = stored_contact(&new, req.relay.clone(), req.calls_allowed);
                    let contact = Contact::from(&stored);
                    let applied = {
                        let mut s = self.shared.lock();
                        if s.epoch != me.epoch {
                            return Err(Error::Locked);
                        }
                        s.state.sent_requests.remove(&did);
                        // Not a way past a block made since (refused above; checked again here).
                        apply_contact(&mut s.state, stored, new.device, false)
                    };
                    match applied {
                        Ok(()) => {
                            self.persist_async(me.epoch).await?;
                            Some(contact)
                        }
                        Err(e) => {
                            tracing::warn!("welcome from {did}: {e}");
                            None
                        }
                    }
                }
                Err(e) => {
                    self.log(format!("refused a welcome: {e}"));
                    None
                }
            },
            _ => None,
        };
        let reply = if taken.is_some() { Msg::WelcomeTaken } else { Msg::Reject { reason: crate::node::REFUSED.into() } };
        ctrl.send(&reply).await?;
        ctrl.finish();
        if let Some(contact) = taken {
            self.log(format!("{} accepted our request", contact.name));
            self.events.on_contacts_changed();
            self.list_for_new_contact(&contact.did);
            if let Some(ev) = self.requests.events.lock().clone() {
                self.handle.spawn_blocking(move || ev.on_request_accepted(contact));
            }
        }
        Ok(())
    }

    /// Delivers the Welcome for an accepted request to `did`'s device, retrying with backoff
    /// while the endpoint is up, until taken, refused (they hold no such request) or `until`.
    pub(crate) fn welcome_kick(self: &Arc<Self>, did: String) {
        if !self.requests.delivering.lock().insert(did.clone()) {
            return;
        }
        let this = self.clone();
        self.handle.spawn(async move {
            let mut round = 0usize;
            loop {
                let Ok(me) = this.me() else { break };
                let Ok(ep) = this.endpoint() else { break };
                let Some(out) = this.shared.lock().state.welcome_outbox.get(&did).cloned() else { break };
                if now() > out.until {
                    this.welcome_done(&did, &out, me.epoch, "gave up delivering the welcome").await;
                    break;
                }
                match this.deliver_welcome(&ep, &did, &out).await {
                    Ok(true) => {
                        this.welcome_done(&did, &out, me.epoch, "welcome delivered").await;
                        break;
                    }
                    Ok(false) => {
                        this.welcome_done(&did, &out, me.epoch, "welcome refused (no request of theirs)").await;
                        break;
                    }
                    Err(e) => tracing::debug!("welcome to {did}: {e}"),
                }
                tokio::time::sleep(Duration::from_secs(RETRY_SECS[round.min(RETRY_SECS.len() - 1)])).await;
                round += 1;
            }
            this.requests.delivering.lock().remove(&did);
        });
    }

    async fn welcome_done(self: &Arc<Self>, did: &str, out: &WelcomeOut, epoch: u64, what: &str) {
        {
            let mut s = self.shared.lock();
            if s.state.welcome_outbox.get(did) == Some(out) {
                s.state.welcome_outbox.remove(did);
            }
        }
        let _ = self.persist_async(epoch).await;
        self.log(format!("{what}: {did}"));
    }

    /// `Ok(true)` taken, `Ok(false)` refused for good; `Err` try again later.
    async fn deliver_welcome(self: &Arc<Self>, ep: &iroh::Endpoint, did: &str, out: &WelcomeOut) -> Result<bool, Error> {
        let addr = addr_for(&out.device, out.relay.as_deref())?;
        let conn = tokio::time::timeout(DIAL_TIMEOUT, crate::version::connect(ep, addr, &crate::version::CALL))
            .await
            .map_err(|_| Error::Timeout)??;
        let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
        let mut ctrl = Ctrl::new(send, recv);
        ctrl.send(&out.welcome).await?;
        let reply = tokio::time::timeout(REPLY_TIMEOUT, ctrl.recv()).await.map_err(|_| Error::Timeout)??;
        ctrl.finish();
        conn.close(0u32.into(), b"bye");
        match reply {
            Some(Msg::WelcomeTaken) => Ok(true),
            Some(Msg::Reject { .. }) => Ok(false),
            // They block us now: take their notice; nothing more to deliver.
            Some(Msg::Notice { notice }) if self.is_block_from(did, &notice) => {
                let _ = self.take_notice(&notice);
                Ok(false)
            }
            other => Err(Error::Protocol(format!("welcome not taken: {other:?}"))),
        }
    }

    /// After start: deliver the Welcomes still owed, and forget requests that can't be answered.
    pub(crate) fn welcomes_started(self: &Arc<Self>) {
        let now = now();
        let dids: Vec<String> = {
            let mut s = self.shared.lock();
            s.state.contact_requests.retain(|_, r| r.exp > now);
            s.state.sent_requests.retain(|_, r| r.exp + WELCOME_GRACE > now);
            s.state.welcome_outbox.keys().cloned().collect()
        };
        for did in dids {
            self.welcome_kick(did);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_by_kind_not_text() {
        assert_eq!(AddError::from(Error::Timeout), AddError::Unreachable);
        assert_eq!(AddError::from(Error::Net("x".into())), AddError::Unreachable);
        assert_eq!(AddError::from(Error::NotStarted), AddError::Offline);
        assert_eq!(AddError::from(Error::Rejected(crate::BLOCKED.into())), AddError::Blocked);
        assert_eq!(AddError::from(Error::Rejected("not accepted".into())), AddError::Refused);
        assert!(matches!(AddError::from(Error::Protocol("x".into())), AddError::Failed(_)));
    }
}
