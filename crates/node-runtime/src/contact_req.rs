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
use std::sync::atomic::AtomicBool;

use parking_lot::Mutex;
use proto::Msg;

use crate::Error;
use crate::{Contact, ContactRequestSink, Me, NodeCtx, addr_for, apply_contact, now, stored_contact};
use crate::store::{HeldRequest, WelcomeOut};
use crate::wire::Ctrl;

/// At most this many requests wait for an answer; more are refused (a code can reach anyone, and
/// DIDs cost nothing to make). PROVISIONAL.
pub const MAX_HELD: usize = 64;
/// How long an accepting device keeps trying to deliver its Welcome, and how long a joiner keeps
/// its half after the code expired (an Accept on the code's last day may land later).
pub const WELCOME_GRACE: u64 = 7 * 24 * 3600;
/// The `Protocol` text of `add_contact_from` (which must return a contact) when the owner holds
/// the hello as a request: p2pcore's `add_contact` shows it as it always has. Callers that can
/// take a request use `add_contact_outcome`, which says so with a typed `AddOutcome::Requested`.
pub const PENDING: &str = "they hold it as a contact request";

const RETRY_SECS: [u64; 6] = [2, 5, 15, 30, 60, 300];

/// A contact request, either way: one made with our code (`contact_requests`, "Waiting for you")
/// or one we made with theirs (`sent_contact_requests`, "Waiting for them").
#[derive(Debug, Clone, PartialEq, Eq)]
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


/// What adding someone by their code did.
#[derive(Debug, Clone)]
pub enum AddOutcome {
    /// They added us at once (a re-add of an existing contact, or a group chat request ticket).
    Added { contact: Contact },
    /// Their device holds it for their user: "Request sent. Your line opens when Tara accepts."
    /// `ContactRequestEvents::on_request_accepted` follows if they accept.
    Requested { did: String, name: String },
}

/// Runtime state that is not persisted.
#[derive(Default)]
pub struct ReqRt {
    pub events: Mutex<Option<Arc<dyn ContactRequestSink>>>,
    /// DIDs with a Welcome delivery task running.
    pub delivering: Mutex<HashSet<String>>,
    /// Tests: welcome every good hello at once, as before requests.
    pub auto_accept: AtomicBool,
    /// DIDs with a queued request being retried (`request_out.rs`).
    pub sending: Mutex<HashSet<String>>,
    /// Tests: the setup dial timeout in ms (0: `DIAL_TIMEOUT`).
    pub dial_ms: std::sync::atomic::AtomicU64,
}

/// Whether we hold a wake token from `did` (any of its devices), for the setup's log lines.
fn holds_token(st: &crate::store::State, did: &str) -> bool {
    st.wake.as_ref().is_some_and(|w| w.held.iter().any(|e| e.did == did))
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

pub fn request_view(did: &str, r: &HeldRequest) -> ContactRequest {
    ContactRequest { did: did.to_string(), name: r.name.clone(), at: r.at, expires_at: r.exp }
}

impl NodeCtx {
    pub fn request_event(&self, request: ContactRequest) {
        if let Some(ev) = self.requests.events.lock().clone() {
            self.handle.spawn_blocking(move || ev.on_contact_request(request));
        }
    }

    /// The Accept: the contact, the redeemed (DID, nonce), the request's removal and the Welcome
    /// to deliver are one durable write, as `accept_hello` does for a Welcome sent at once.
    pub fn accept_request(&self, me: &Me, did: &str) -> Result<Contact, Error> {
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
            return Err(Error::Rejected(crate::block::BLOCKED.into()));
        }
        // Checked again: the code may have expired since, or another device of ours added them.
        // Under the key their ticket named: after a rotation, the previous one.
        let on = req.on.unwrap_or(me.device);
        let (welcome, new) = proto::accept_contact_hello_with(&me.id, on, &req.hello, req.device, now * 1000, &proto::Unrecorded, crate::GRANT_TTL, |d| {
            crate::caps::policy_for(&next, d, crate::caps::ContactOrigin::Ticket)
        })?;
        if new.did != did {
            return Err(Error::Protocol("request does not match".into()));
        }
        let calls_allowed = matches!(&welcome, Msg::ContactWelcome { call_grant_for_you: Some(_), .. });
        let stored = stored_contact(&new, req.relay.clone(), calls_allowed);
        let contact = Contact::from(&stored);
        let out = WelcomeOut { welcome, device: req.device, relay: req.relay.clone(), from: Some(on), until: now + WELCOME_GRACE };
        // Their hello carried their wake token for us: kept with the contact, so the Welcome
        // can wake their phone (`welcome_kick`).
        let handover = crate::push::handover_in(&req.hello).cloned();
        // Drawn once, so the durable copy and the one in memory agree if this creates the state.
        let fresh = wake::WakeState::new(proto::new_device_secret());
        let apply = |st: &mut crate::store::State| {
            st.redeemed.insert(proto::redeemed_key(&new.did, &new.redeemed_nonce));
            st.contact_requests.remove(did);
            st.welcome_outbox.insert(did.to_string(), out.clone());
            if on != me.device {
                // They hold our previous key from the ticket: they are owed the new one.
                st.list_outbox.insert(did.to_string());
            }
            apply_contact(st, stored.clone(), new.device, false)?;
            if let Some(h) = &handover {
                st.wake.get_or_insert_with(|| fresh.clone());
                crate::push::hold_handover(st, &new.did, new.device, h);
            }
            Ok::<(), Error>(())
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
        tracing::info!("kept a wake token from the request of {}: {}", crate::short(did), yes_no(holds_token(&s.state, did)));
        Ok(contact)
    }

    /// A connection that opened with `ContactWelcome`: their user accepted our request. Taken
    /// only if it answers a request of ours to the DID and device it comes from.
    pub async fn welcome_incoming(self: &Arc<Self>, mut ctrl: Ctrl, msg: Msg, remote: [u8; 32], me: &Me) -> Result<(), Error> {
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
            Some((did, req)) if self.refusal_for(&did).is_none() => match proto::accept_contact_welcome(&me.id, &req.pending, &msg, remote, now * 1000) {
                Ok(new) => {
                    let handover = crate::push::handover_in(&msg).cloned();
                    let stored = stored_contact(&new, req.relay.clone(), req.calls_allowed);
                    let contact = Contact::from(&stored);
                    let applied = {
                        let mut s = self.shared.lock();
                        if s.epoch != me.epoch {
                            return Err(Error::Locked);
                        }
                        s.state.sent_requests.remove(&did);
                        // Not a way past a block made since (refused above; checked again here).
                        let applied = apply_contact(&mut s.state, stored, new.device, false);
                        // Their wake token for us came with the Welcome.
                        if applied.is_ok() && let Some(h) = &handover {
                            crate::push::hold_handover(&mut s.state, &did, new.device, h);
                        }
                        if applied.is_ok() {
                            tracing::info!("kept a wake token from the welcome of {}: {}", crate::short(&did), yes_no(holds_token(&s.state, &did)));
                        }
                        applied
                    };
                    match applied {
                        Ok(()) => {
                            self.persist_async(me.epoch).await?;
                            Some(contact)
                        }
                        Err(e) => {
                            tracing::warn!("welcome from {}: {e}", crate::short(&did));
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
        let reply = if taken.is_some() { Msg::WelcomeTaken } else { Msg::Reject { reason: crate::REFUSED.into() } };
        ctrl.send(&reply).await?;
        ctrl.finish();
        if let Some(contact) = taken {
            self.log(format!("{} accepted our request", contact.name));
            self.sink.on_contacts_changed();
            self.list_for_new_contact(&contact.did);
            if let Some(ev) = self.requests.events.lock().clone() {
                self.handle.spawn_blocking(move || ev.on_request_accepted(contact));
            }
        }
        Ok(())
    }

    /// Delivers the Welcome for an accepted request to `did`'s device, retrying with backoff
    /// while the endpoint is up, until taken, refused (they hold no such request) or `until`.
    pub fn welcome_kick(self: &Arc<Self>, did: String) {
        if !self.requests.delivering.lock().insert(did.clone()) {
            return;
        }
        let this = self.clone();
        self.handle.spawn(async move {
            let mut round = 0usize;
            loop {
                let Ok(me) = this.me() else { break };
                let Some(out) = this.shared.lock().state.welcome_outbox.get(&did).cloned() else { break };
                if now() > out.until {
                    this.welcome_done(&did, &out, me.epoch, "gave up delivering the welcome").await;
                    break;
                }
                // From the endpoint of the key the Welcome is made under (the joiner checks it).
                let Some(ep) = this.endpoint_of(out.from.unwrap_or(me.device)) else {
                    if this.endpoint().is_err() {
                        break;
                    }
                    this.welcome_done(&did, &out, me.epoch, "gave up delivering the welcome: the address it was asked at has closed").await;
                    break;
                };
                // Our wake token for them goes with it (`docs/push.md` "Tokens at setup").
                // (`out` itself stays as queued: `welcome_done` matches it.)
                let h = match round {
                    0 => this.await_wake_handover(&did, crate::push::HANDOVER_WAIT).await,
                    _ => this.wake_handover(&did),
                };
                if round == 0 {
                    tracing::info!("welcome to {} carries a wake token: {}", crate::short(&did), yes_no(h.is_some()));
                }
                let send = WelcomeOut { welcome: crate::push::with_handover(out.welcome.clone(), h), ..out.clone() };
                match this.deliver_welcome(&ep, &did, &send).await {
                    Ok(true) => {
                        this.welcome_done(&did, &out, me.epoch, "welcome delivered").await;
                        // A list owed to them waited for this (`lists_kick`).
                        this.lists_kick(did.clone());
                        break;
                    }
                    Ok(false) => {
                        this.welcome_done(&did, &out, me.epoch, "welcome refused (no request of theirs)").await;
                        break;
                    }
                    Err(e) => {
                        tracing::debug!("welcome to {}: {e}", crate::short(&did));
                        // Their phone may be asleep: wake it with the token from their hello.
                        this.wake_unreached(&did, &[out.device]).await;
                    }
                }
                let pause = this.fast_redial(&did).unwrap_or_else(|| crate::wire::retry_pause(&RETRY_SECS, round));
                tokio::time::sleep(pause).await;
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
        self.persist_logged(epoch).await;
        self.log(format!("{what}: {did}"));
    }

    /// `Ok(true)` taken, `Ok(false)` refused for good; `Err` try again later.
    async fn deliver_welcome(self: &Arc<Self>, ep: &iroh::Endpoint, did: &str, out: &WelcomeOut) -> Result<bool, Error> {
        let addr = addr_for(&out.device, out.relay.as_deref())?;
        let conn = tokio::time::timeout(self.setup_dial_timeout(), crate::version::connect(ep, addr, &crate::version::CALL))
            .await
            .map_err(|_| Error::Timeout)??;
        self.wake_reached(did);
        let reply = crate::wire::exchange(conn, &out.welcome).await?;
        match reply {
            Some(Msg::WelcomeTaken) => Ok(true),
            Some(Msg::Reject { .. }) => Ok(false),
            // They block us now: take their notice; nothing more to deliver.
            Some(Msg::Notice { notice }) if self.is_block_from(did, &notice) => {
                let _ = self.take_notice(&notice);
                Ok(false)
            }
            other => Err(crate::not_taken("welcome", other.as_ref())),
        }
    }

    /// After start: deliver the Welcomes still owed, and forget requests that can't be answered.
    pub fn welcomes_started(self: &Arc<Self>) {
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

