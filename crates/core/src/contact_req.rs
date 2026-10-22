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

use std::sync::Arc;
use std::sync::atomic::Ordering;

use proto::ContactTicket;

pub use node_runtime::contact_req::*;
use node_runtime::ContactRequestSink;

use crate::node::{CardPeek, Contact, Node, find_card, now};
use crate::Error;

/// A contact request, either way: one made with our code (`contact_requests`, "Waiting for you")
/// or one we made with theirs (`sent_contact_requests`, "Waiting for them").
#[uniffi::remote(Record)]
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
#[uniffi::remote(Enum)]
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
#[uniffi::export(rust, foreign)]
pub trait ContactRequestEvents: Send + Sync {
    /// Someone used our code: "Maya wants to add you as a contact". Once per joiner (a repeated
    /// hello is the same request).
    fn on_contact_request(&self, request: ContactRequest);
    /// They accepted the request we sent: our line to them is open.
    fn on_request_accepted(&self, contact: Contact);
}

/// Hands the runtime's contact-request events to the app's `ContactRequestEvents`.
struct RequestEventsAdapter(Arc<dyn ContactRequestEvents>);

impl ContactRequestSink for RequestEventsAdapter {
    fn on_contact_request(&self, request: ContactRequest) {
        self.0.on_contact_request(request);
    }

    fn on_request_accepted(&self, contact: Contact) {
        self.0.on_request_accepted(contact);
    }
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

#[cfg(test)]
mod tests;
