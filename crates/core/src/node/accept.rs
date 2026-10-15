//! Incoming connections: the hello handshake and dispatch of the first message.

use super::*;

/// What `accept_hello` did with a good hello.
pub(super) enum Hello {
    /// Added at once: send this `ContactWelcome`.
    Welcome(Msg, Contact),
    /// Held for the user's Accept: send `ContactPending`. `fresh`: the first from that DID.
    Held { request: crate::contact_req::ContactRequest, fresh: bool },
}

pub(super) enum HelloError {
    /// Not acceptable (bad, spent, old or blocked ticket): the joiner is turned away.
    Refused(String),
    /// Acceptable, but we could not make it durable.
    Failed(Error),
    /// From a DID we block: answered with our notice.
    Blocked(proto::SignedBlockNotice),
}

impl Inner {
    /// An incoming `ContactHello`. Checked, then either held as a request for the user's Accept
    /// (the default for a ticket hello, 2026-10-13: nobody becomes a contact without our yes) or,
    /// for a group chat request ticket we already said yes to, an existing contact re-adding, or
    /// the test switch, answered with a `Welcome` at once. Either way the outcome is durable in
    /// one write before the joiner hears anything. Blocking: runs on the blocking pool. If the
    /// write fails nothing changes in memory either, and the joiner can retry.
    pub(super) fn accept_hello(&self, me: &Me, hello: &Msg, remote: [u8; 32], hint: Option<String>) -> Result<Hello, HelloError> {
        let _w = self.writing.lock();
        let (mut next, backing) = {
            let s = self.shared.lock();
            if s.epoch != me.epoch {
                return Err(HelloError::Failed(Error::Locked));
            }
            (s.state.clone(), s.committed.then(|| s.acct.as_ref().and_then(|a| a.backing.clone())).flatten())
        };
        let now = now();
        // Which ticket nonces may still add this joiner (live tickets, one-time request tickets)
        // is the `Tickets` store's call.
        let tickets = crate::store::Tickets { st: &next, now };
        let (welcome, new) = proto::accept_contact_hello_with(&me.id, me.device, hello, remote, now * 1000, &tickets, GRANT_TTL, |did| {
            // A DID we accepted a group chat request from gets message-only tokens (U3, P6).
            let origin = if next.req_tickets.values().any(|t| t.did == did && t.exp >= now) {
                crate::caps::ContactOrigin::GroupRequest
            } else {
                crate::caps::ContactOrigin::Ticket
            };
            crate::caps::policy_for(&next, did, origin)
        })
        .map_err(|e| HelloError::Refused(e.to_string()))?;
        let by_request = next.req_tickets.get(&new.redeemed_nonce).is_some_and(|t| t.did == new.did && t.exp >= now);
        let calls_allowed = matches!(&welcome, Msg::ContactWelcome { call_grant_for_you: Some(_), .. });
        if let Some(notice) = self.refusal_for(&new.did) {
            return Err(HelloError::Blocked(notice));
        }
        let known = next.contacts.iter().any(|c| c.did == new.did);
        let welcome_now = by_request || known || self.requests.auto_accept.load(Ordering::SeqCst);
        if !welcome_now {
            let exp = proto::hello_invite(hello).map_err(|e| HelloError::Refused(e.to_string()))?.exp;
            let fresh = !next.contact_requests.contains_key(&new.did);
            let held = crate::store::HeldRequest {
                hello: hello.clone(),
                device: remote,
                relay: hint,
                name: proto::sanitize_name(&new.name),
                nonce: new.redeemed_nonce.clone(),
                exp,
                // A replayed or repeated hello stays one request, first arrival first.
                at: next.contact_requests.get(&new.did).map_or_else(now_ms, |r| r.at),
            };
            let apply = |st: &mut State| -> Result<(), Error> {
                st.contact_requests.retain(|_, r| r.exp > now);
                if fresh && st.contact_requests.len() >= crate::contact_req::MAX_HELD {
                    return Err(Error::Rejected("too many requests".into()));
                }
                st.contact_requests.insert(new.did.clone(), held.clone());
                Ok(())
            };
            apply(&mut next).map_err(|e| HelloError::Refused(e.to_string()))?;
            if let Some(b) = &backing {
                self.write_state(b, &next).map_err(HelloError::Failed)?;
            }
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return Err(HelloError::Failed(Error::Locked));
            }
            apply(&mut s.state).map_err(|e| HelloError::Refused(e.to_string()))?;
            return Ok(Hello::Held { request: crate::contact_req::request_view(&new.did, &held), fresh });
        }
        let stored = stored_contact(&new, hint, calls_allowed);
        let contact = Contact::from(&stored);
        let apply = |st: &mut State| {
            st.redeemed.insert(proto::redeemed_key(&new.did, &new.redeemed_nonce));
            if by_request {
                st.req_tickets.remove(&new.redeemed_nonce);
            }
            st.contact_requests.remove(&new.did);
            apply_contact(st, stored.clone(), new.device, false)
        };
        apply(&mut next).map_err(HelloError::Failed)?;
        if let Some(b) = &backing {
            self.write_state(b, &next).map_err(HelloError::Failed)?;
        }
        let mut s = self.shared.lock();
        if s.epoch != me.epoch {
            return Err(HelloError::Failed(Error::Locked));
        }
        apply(&mut s.state).map_err(HelloError::Failed)?;
        Ok(Hello::Welcome(welcome, contact))
    }

    /// The slot in `permit` is held until this connection has become a call or a contact, or is
    /// over; `reserved` says it came from the contacts-only pool.
    pub(crate) async fn handle_incoming(
        self: Arc<Self>,
        incoming: iroh::endpoint::Accepting,
        mut permit: OwnedSemaphorePermit,
        reserved: bool,
    ) -> Result<(), Error> {
        // One deadline for the whole way to the hello.
        let (conn, remote, mut ctrl, first) = tokio::time::timeout(HELLO_TIMEOUT, async {
            let conn = incoming.await.map_err(Error::net)?;
            let remote = *conn.remote_id().as_bytes();
            if reserved && !self.is_contact_device(&remote) {
                // A stranger in a contact's slot: it may stay only if the general pool has room.
                match self.pending.clone().try_acquire_owned() {
                    Ok(p) => permit = p,
                    Err(_) => {
                        conn.close(0u32.into(), b"busy");
                        return Err(Error::Busy);
                    }
                }
            }
            let (send, recv) = conn.accept_bi().await.map_err(Error::net)?;
            let mut ctrl = Ctrl::new(send, recv);
            let first = ctrl
                .recv()
                .await?
                .ok_or_else(|| Error::Protocol("peer closed before hello".into()))?;
            Ok::<_, Error>((conn, remote, ctrl, first))
        })
        .await
        .map_err(|_| Error::Timeout)??;
        let me = self.me()?;
        match first {
            hello @ Msg::ContactHello { .. } => {
                let Msg::ContactHello { relay: hint, .. } = &hello else { unreachable!() };
                let hint = relay_hint(hint);
                let (this, me2, hello2) = (self.clone(), me.clone(), hello.clone());
                let accepted = tokio::task::spawn_blocking(move || this.accept_hello(&me2, &hello2, remote, hint))
                    .await
                    .map_err(|_| Error::Io("contact task failed".into()))?;
                match accepted {
                    Ok(Hello::Welcome(welcome, contact)) => {
                        // Durable now; only then does the joiner hear of it.
                        self.events.on_contacts_changed();
                        ctrl.send(&welcome).await?;
                        ctrl.finish();
                        drop(permit);
                        self.log(format!("contact {} added us", contact.name));
                        self.list_for_new_contact(&contact.did);
                        // Let the joiner read the welcome and close first.
                        let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
                    }
                    Ok(Hello::Held { request, fresh }) => {
                        // Durable now: the joiner shows "request sent", our user decides.
                        ctrl.send(&Msg::ContactPending).await?;
                        ctrl.finish();
                        drop(permit);
                        self.log(format!("contact request from {}", request.name));
                        if fresh {
                            self.request_event(request);
                        }
                        let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                    }
                    Err(HelloError::Blocked(notice)) => {
                        self.log("refused a contact hello: blocked");
                        ctrl.send(&Msg::Notice { notice }).await?;
                        ctrl.finish();
                        let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                    }
                    Err(HelloError::Refused(e)) => {
                        self.log(format!("rejected contact hello: {e}"));
                        ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
                        ctrl.finish();
                        let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                    }
                    Err(HelloError::Failed(e)) => {
                        // Nothing was saved, so no Welcome: the joiner must not hold a contact
                        // that we would forget.
                        let _ = ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await;
                        ctrl.finish();
                        let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                        return Err(e);
                    }
                }
                Ok(())
            }
            hello @ Msg::CallHello { .. } => self.incoming_call(conn, ctrl, hello, remote, &me, permit).await,
            welcome @ Msg::ContactWelcome { .. } => {
                // Their user accepted our request and they dialled back with the Welcome.
                drop(permit);
                self.welcome_incoming(ctrl, welcome, remote, &me).await?;
                let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                Ok(())
            }
            Msg::Notice { notice } => {
                drop(permit);
                self.notice_incoming(ctrl, notice).await?;
                let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                Ok(())
            }
            Msg::Tokens { grants } => {
                drop(permit);
                self.tokens_incoming(ctrl, grants).await?;
                let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                Ok(())
            }
            Msg::Devices { list } => {
                drop(permit);
                self.devices_incoming(ctrl, list).await?;
                let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                Ok(())
            }
            letter @ Msg::GroupLetter { .. } => {
                drop(permit);
                self.group_letter_incoming(ctrl, letter, remote, &me).await?;
                let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                Ok(())
            }
            other => {
                tracing::debug!("unexpected first message {}", msg_name(&other));
                ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
                ctrl.finish();
                let _ = tokio::time::timeout(LINGER, conn.closed()).await;
                Ok(())
            }
        }
    }

    pub(crate) fn is_contact_device(&self, device: &[u8; 32]) -> bool {
        self.shared.lock().state.contacts.iter().any(|c| c.has_device(device))
    }

    pub(super) fn live_calls_with(&self, did: &str) -> Vec<Arc<Call>> {
        let live = self.live.lock();
        live.call.iter().chain(live.waiting.iter()).filter(|c| c.info.peer_did == did).cloned().collect()
    }

    /// We blocked `did`: a call with them tells them so and ends.
    pub(crate) fn calls_blocked(&self, did: &str, notice: &proto::SignedBlockNotice) {
        for call in self.live_calls_with(did) {
            let _ = call.cmd.send(Cmd::Block(notice.clone()));
        }
    }

    /// Ends any call with `did` with `reason`, from outside its task.
    pub(crate) fn end_calls_with(&self, did: &str, reason: &str) {
        for call in self.live_calls_with(did) {
            self.end_call(&call, reason.into());
        }
    }
}
