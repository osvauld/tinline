//! The joiner's request outbox (`docs/protocol.md` "Contact requests", `docs/push.md` "Code wake
//! tokens"): a contact request whose owner's device could not be reached is queued, not dropped.
//!
//! - [`NodeCtx::send_request`] is one delivery: a fresh `ContactHello` from the stored code
//!   (`SentRequest::ticket`), carrying our wake token for them when we have one, a dial, the
//!   answer. With `early_wake` it wakes the owner's phone with the code's token (if it carries
//!   one) when the dial has not connected after `MSG_EARLY_WAKE_MS`, while the dial goes on.
//! - [`NodeCtx::request_kick`] retries it in the background: after a failed dial it wakes the owner
//!   (on `wake::msg_wake_due`'s schedule: 0, 30 s, 2, 10, 30 min, then hourly) and redials on the fast schedule
//!   (`push::MSG_REDIAL_AT_MS`), else on [`QUEUE_RETRY_SECS`], until the owner answers
//!   (`ContactPending`, a Welcome, a refusal or their block notice) or the code expires. The
//!   queue is the sealed `State::sent_requests`, so it survives a restart (`requests_started`).

use std::sync::atomic::Ordering;

use super::*;
use crate::contact_req::AddOutcome;

/// The pause between attempts of a queued request when no wake is taking effect (PROVISIONAL).
pub const QUEUE_RETRY_SECS: [u64; 7] = [2, 5, 15, 30, 60, 300, 900];

/// What a delivered request did.
pub enum Attempt {
    /// Their device took us at once (a re-add, a group chat request ticket).
    Added(Contact),
    /// Their device holds it for their user.
    Pending,
}

/// Why a delivery did not happen.
pub enum SendErr {
    /// Not reached, or no answer: try again later. `maybe_delivered`: the hello went out.
    Unreached { error: Error, maybe_delivered: bool },
    /// A final answer (refused, blocked, the code expired); the request is gone.
    Final(Error),
}

impl NodeCtx {
    /// Dial timeout for contact setup (requests, Welcomes): [`DIAL_TIMEOUT`] unless a test set
    /// another one.
    pub fn setup_dial_timeout(&self) -> Duration {
        match self.requests.dial_ms.load(Ordering::Relaxed) {
            0 => DIAL_TIMEOUT,
            ms => Duration::from_millis(ms),
        }
    }

    /// Tests: a short setup dial timeout (0 restores the default).
    pub fn set_setup_dial_timeout(&self, ms: u64) {
        self.requests.dial_ms.store(ms, Ordering::Relaxed);
    }

    /// One delivery of our request to `did` (see the module docs).
    pub async fn send_request(
        self: &Arc<Self>,
        me: &Me,
        did: &str,
        early_wake: bool,
    ) -> Result<Attempt, SendErr> {
        let unreached = |error: Error| SendErr::Unreached {
            error,
            maybe_delivered: false,
        };
        let ep = self.endpoint().map_err(unreached)?;
        let now = now();
        let (req, policy) = {
            let s = self.shared.lock();
            let req = s
                .state
                .sent_requests
                .get(did)
                .cloned()
                .ok_or(SendErr::Final(Error::NotFound))?;
            let origin = if req.calls_allowed {
                crate::caps::ContactOrigin::Ticket
            } else {
                crate::caps::ContactOrigin::GroupRequest
            };
            (req, crate::caps::policy_for(&s.state, did, origin))
        };
        let text = req.ticket.clone().ok_or(SendErr::Final(Error::NotFound))?;
        let ticket = ContactTicket::from_text(&text).map_err(|e| SendErr::Final(e.into()))?;
        let claim = match ticket.verify(now) {
            Ok(c) => c,
            Err(e) => {
                self.drop_request(me, did).await;
                return Err(SendErr::Final(e.into()));
            }
        };
        let (hello, _) = proto::contact_hello_with(
            &me.id,
            me.device,
            &ticket,
            &me.profile.name,
            now_ms(),
            GRANT_TTL,
            policy,
        )
        .map_err(|e| SendErr::Final(e.into()))?;
        let handover = self
            .await_wake_handover(did, crate::push::HANDOVER_WAIT)
            .await;
        let mut hello = crate::push::with_handover(hello, handover);
        if let Msg::ContactHello { relay, .. } = &mut hello {
            *relay = relay_of(&ep);
        }
        let addr = addr_for(&claim.device, req.relay.as_deref()).map_err(SendErr::Final)?;
        let _early = (early_wake && self.can_wake_code(did)).then(|| self.early_code_wake(did));
        let conn = match tokio::time::timeout(
            self.setup_dial_timeout(),
            crate::version::connect(&ep, addr, &crate::version::CALL),
        )
        .await
        {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return Err(unreached(e)),
            Err(_) => return Err(unreached(Error::Timeout)),
        };
        drop(_early);
        self.wake_reached(did);
        let remote = *conn.remote_id().as_bytes();
        let (send, recv) = conn.open_bi().await.map_err(|e| unreached(Error::net(e)))?;
        let mut ctrl = Ctrl::new(send, recv);
        ctrl.send(&hello).await.map_err(unreached)?;
        // From here the hello may have reached them.
        let sent = |error: Error| SendErr::Unreached {
            error,
            maybe_delivered: true,
        };
        let reply = tokio::time::timeout(HELLO_TIMEOUT, ctrl.recv())
            .await
            .map_err(|_| sent(Error::Timeout))?
            .map_err(sent)?
            .ok_or_else(|| sent(Error::eof("peer closed")))?;
        let name = req.pending.name.clone();
        let new = match reply {
            Msg::Reject { reason } => {
                self.drop_request(me, did).await;
                return Err(SendErr::Final(Error::Rejected(clip_reason(&reason))));
            }
            // They block us: their app's notice is the answer (and is taken here).
            Msg::Notice { notice } if self.is_block_from(did, &notice) => {
                self.shared.lock().state.sent_requests.remove(did);
                let _ = self.take_notice(&notice);
                return Err(SendErr::Final(Error::Rejected(
                    crate::block::BLOCKED.into(),
                )));
            }
            Msg::ContactPending => {
                ctrl.finish();
                conn.close(0u32.into(), b"pending");
                if let Some(r) = self.shared.lock().state.sent_requests.get_mut(did) {
                    r.delivered = true;
                    r.ticket = None;
                }
                self.persist_async(me.epoch).await.map_err(SendErr::Final)?;
                self.log(format!("contact request to {name} delivered"));
                return Ok(Attempt::Pending);
            }
            other => {
                let new =
                    proto::accept_contact_welcome(&me.id, &req.pending, &other, remote, now_ms())
                        .map_err(|e| sent(e.into()))?;
                (new, crate::push::handover_in(&other).cloned())
            }
        };
        ctrl.finish();
        conn.close(0u32.into(), b"added");
        let (new, handover) = new;
        self.shared.lock().state.sent_requests.remove(did);
        // We scanned their ticket ourselves: that is the one way back from a block.
        let contact = self
            .save_contact(me.epoch, new, req.relay.clone(), true, req.calls_allowed)
            .await
            .map_err(SendErr::Final)?;
        if let Some(h) = handover {
            self.hold_setup_token(me, did, claim.device, &h);
        }
        self.log(format!("contact {} added", contact.name));
        self.list_for_new_contact(&contact.did);
        Ok(Attempt::Added(contact))
    }

    /// Keeps the wake token `did`'s `device` handed over in the contact setup (they are our
    /// contact now). Saved in the background.
    pub fn hold_setup_token(
        self: &Arc<Self>,
        me: &Me,
        did: &str,
        device: [u8; 32],
        h: &proto::WakeHandover,
    ) {
        let held = {
            let mut s = self.shared.lock();
            s.epoch == me.epoch && crate::push::hold_handover(&mut s.state, did, device, h)
        };
        if held {
            tracing::info!("wake token from the contact setup accepted");
            self.persist_in_background(me.epoch);
        } else {
            tracing::info!("wake token from the contact setup refused");
        }
    }

    /// Sends the code wake after `MSG_EARLY_WAKE_MS` unless the guard is dropped first.
    fn early_code_wake(self: &Arc<Self>, did: &str) -> crate::push::EarlyWake {
        let (this, did) = (self.clone(), did.to_string());
        crate::push::EarlyWake::spawn(&self.handle, async move {
            tokio::time::sleep(Duration::from_millis(crate::push::MSG_EARLY_WAKE_MS)).await;
            // Detached: a dial that ends meanwhile must not cut the relay request short.
            let ctx = this.clone();
            this.handle.spawn(async move { ctx.wake_code(&did).await });
        })
    }

    async fn drop_request(self: &Arc<Self>, me: &Me, did: &str) {
        self.shared.lock().state.sent_requests.remove(did);
        self.persist_logged(me.epoch).await;
    }

    /// Retries the queued request to `did` in the background (see the module docs). `pause_first`:
    /// a dial has just failed, so wait before the next.
    pub fn request_kick(self: &Arc<Self>, did: String, pause_first: bool) {
        if !self.requests.sending.lock().insert(did.clone()) {
            return;
        }
        let this = self.clone();
        self.handle.spawn(async move {
            let mut round = 0usize;
            let mut pause = pause_first;
            loop {
                if pause {
                    let wait = this
                        .fast_redial(&did)
                        .unwrap_or_else(|| crate::wire::retry_pause(&QUEUE_RETRY_SECS, round));
                    round += 1;
                    tokio::time::sleep(wait).await;
                }
                pause = true;
                let Ok(me) = this.me() else { break };
                let Some(req) = this.shared.lock().state.sent_requests.get(&did).cloned() else {
                    break;
                };
                if req.delivered || req.ticket.is_none() {
                    break;
                }
                if now() >= req.exp {
                    this.drop_request(&me, &did).await;
                    this.log(format!(
                        "gave up on the contact request to {}: their code expired",
                        req.pending.name
                    ));
                    this.sink.on_contacts_changed();
                    break;
                }
                if this.endpoint().is_err() {
                    // Stopped: `requests_started` picks it up again.
                    break;
                }
                match this.send_request(&me, &did, false).await {
                    Ok(Attempt::Pending) => {
                        this.sink.on_contacts_changed();
                        break;
                    }
                    Ok(Attempt::Added(contact)) => {
                        if let Some(ev) = this.requests.events.lock().clone() {
                            this.handle
                                .spawn_blocking(move || ev.on_request_accepted(contact));
                        }
                        break;
                    }
                    Err(SendErr::Final(e)) => {
                        this.log(format!(
                            "contact request to {} not taken: {e}",
                            req.pending.name
                        ));
                        this.sink.on_contacts_changed();
                        break;
                    }
                    Err(SendErr::Unreached { error, .. }) => {
                        tracing::debug!("contact request to {}: {error}", crate::short(&did));
                        this.wake_code(&did).await;
                    }
                }
            }
            this.requests.sending.lock().remove(&did);
        });
    }

    /// After start: the queued requests go out again.
    pub fn requests_started(self: &Arc<Self>) {
        let dids: Vec<String> = self
            .shared
            .lock()
            .state
            .sent_requests
            .iter()
            .filter(|(_, r)| !r.delivered && r.ticket.is_some())
            .map(|(d, _)| d.clone())
            .collect();
        for did in dids {
            self.request_kick(did, false);
        }
    }

    /// `add_contact_outcome` once our half is kept: the first delivery, then the queue.
    pub(crate) async fn first_request(
        self: &Arc<Self>,
        me: &Me,
        did: &str,
        queue: bool,
        prior: Option<crate::store::SentRequest>,
    ) -> Result<AddOutcome, Error> {
        let name = self
            .shared
            .lock()
            .state
            .sent_requests
            .get(did)
            .map(|r| r.pending.name.clone())
            .unwrap_or_default();
        let restore = |this: &Arc<Self>| {
            let mut s = this.shared.lock();
            match &prior {
                Some(p) => s.state.sent_requests.insert(did.to_string(), p.clone()),
                None => s.state.sent_requests.remove(did),
            };
        };
        match self.send_request(me, did, queue).await {
            Ok(Attempt::Added(contact)) => Ok(AddOutcome::Added { contact }),
            Ok(Attempt::Pending) => Ok(AddOutcome::Requested {
                did: did.to_string(),
                name,
            }),
            Err(SendErr::Final(e)) => {
                // Refused: what we held before stands (a block notice already removed it).
                if matches!(&e, Error::Rejected(r) if r != crate::block::BLOCKED) {
                    restore(self);
                    self.persist_logged(me.epoch).await;
                }
                Err(e)
            }
            Err(SendErr::Unreached { error, .. }) if queue => {
                self.log(format!("contact request to {name} queued: {error}"));
                self.wake_code(did).await;
                self.request_kick(did.to_string(), true);
                Ok(AddOutcome::Requested {
                    did: did.to_string(),
                    name,
                })
            }
            Err(SendErr::Unreached {
                error,
                maybe_delivered,
            }) => {
                if maybe_delivered {
                    // The hello may have reached them: our half stays (unlisted, not retried) so
                    // a late Accept still lands.
                    if let Some(r) = self.shared.lock().state.sent_requests.get_mut(did) {
                        r.ticket = None;
                    }
                } else {
                    restore(self);
                }
                self.persist_logged(me.epoch).await;
                Err(error)
            }
        }
    }
}
