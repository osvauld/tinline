//! Adding contacts: tickets redeemed by us, contact records.

use super::*;

impl Inner {
    pub(crate) async fn add_contact(self: Arc<Self>, text: String) -> Result<Contact, Error> {
        self.add_contact_from(text, crate::caps::ContactOrigin::Ticket).await
    }

    /// `add_contact_outcome` for callers that need the contact now (a group chat request ticket,
    /// which its owner takes at once): a request held for their user is an error here.
    pub(crate) async fn add_contact_from(self: Arc<Self>, text: String, origin: crate::caps::ContactOrigin) -> Result<Contact, Error> {
        match self.add_contact_outcome(text, origin).await? {
            crate::contact_req::AddOutcome::Added { contact } => Ok(contact),
            crate::contact_req::AddOutcome::Requested { .. } => Err(Error::Protocol(crate::contact_req::PENDING.into())),
        }
    }

    /// Redeems a ticket, giving them the tokens `origin` decides (P6: a group chat request gives
    /// message only). Their device either adds us at once (`Added`) or holds our hello as a
    /// request for its user (`Requested`): our half is kept in `sent_requests`, and their
    /// Welcome arrives later on a connection they dial (`welcome_incoming`).
    pub(crate) async fn add_contact_outcome(
        self: Arc<Self>,
        text: String,
        origin: crate::caps::ContactOrigin,
    ) -> Result<crate::contact_req::AddOutcome, Error> {
        use crate::contact_req::AddOutcome;
        let me = self.me()?;
        let ticket = ContactTicket::from_text(&text)?;
        let claim = ticket.verify(now())?;
        if claim.iss == me.id.did() {
            return Err(Error::Protocol("that is your own ticket".into()));
        }
        // The relay is signed by its owner but gets dialled and stored here: same sanity check
        // as for a hint, and a ticket that fails it is refused rather than half-used.
        let relay = relay_hint(&claim.relay);
        if claim.relay.is_some() && relay.is_none() {
            return Err(Error::Protocol("that ticket names a relay we won't use".into()));
        }
        let ep = self.endpoint()?;
        // We scanned their ticket ourselves: that is a way back from our block (unblock first, so
        // they are told).
        if self.unblock_inner(&claim.iss)? {
            self.persist_async(me.epoch).await?;
            self.notice_kick(claim.iss.clone());
        }
        let policy = crate::caps::policy_for(&self.shared.lock().state, &claim.iss, origin);
        let (mut hello, pending) = proto::contact_hello_with(
            &me.id,
            me.device,
            &ticket,
            &me.profile.name,
            now_ms(),
            GRANT_TTL,
            policy,
        )?;
        if let Msg::ContactHello { relay, .. } = &mut hello {
            *relay = relay_of(&ep);
        }
        // Our half is kept before the hello leaves: their user's Accept can dial back any time
        // after. A request already delivered with an earlier hello stays delivered.
        let prior = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return Err(Error::Locked);
            }
            let prior = s.state.sent_requests.get(&claim.iss).cloned();
            s.state.sent_requests.insert(
                claim.iss.clone(),
                crate::store::SentRequest {
                    pending: pending.clone(),
                    relay: relay.clone(),
                    calls_allowed: policy.call,
                    exp: claim.exp,
                    at: prior.as_ref().map_or_else(now_ms, |p| p.at),
                    delivered: prior.as_ref().is_some_and(|p| p.delivered),
                },
            );
            prior
        };
        self.persist_async(me.epoch).await?;
        let addr = addr_for(&claim.device, relay.as_deref())?;
        self.log(format!(
            "adding contact {}: dialing {}",
            claim.name, addr.id
        ));
        // Not reached: they never saw this hello, so what we held before stands.
        let undo = |this: &Arc<Self>| {
            let mut s = this.shared.lock();
            match &prior {
                Some(p) => s.state.sent_requests.insert(claim.iss.clone(), p.clone()),
                None => s.state.sent_requests.remove(&claim.iss),
            };
        };
        let conn = match tokio::time::timeout(DIAL_TIMEOUT, crate::version::connect(&ep, addr, &crate::version::CALL)).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                undo(&self);
                let _ = self.persist_async(me.epoch).await;
                return Err(e.into());
            }
            Err(_) => {
                undo(&self);
                let _ = self.persist_async(me.epoch).await;
                return Err(Error::Timeout);
            }
        };
        let remote = *conn.remote_id().as_bytes();
        let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
        let mut ctrl = Ctrl::new(send, recv);
        ctrl.send(&hello).await?;
        // From here the hello may have reached them: on a timeout our half stays (unlisted) so
        // a late Accept still lands.
        let reply = tokio::time::timeout(HELLO_TIMEOUT, ctrl.recv())
            .await
            .map_err(|_| Error::Timeout)??
            .ok_or_else(|| Error::Protocol("peer closed".into()))?;
        let new = match reply {
            Msg::Reject { reason } => {
                undo(&self);
                let _ = self.persist_async(me.epoch).await;
                return Err(Error::Rejected(clip_reason(&reason)));
            }
            // They block us: their app's notice is the answer (and is taken here).
            Msg::Notice { notice } if self.is_block_from(&claim.iss, &notice) => {
                self.shared.lock().state.sent_requests.remove(&claim.iss);
                let _ = self.take_notice(&notice);
                return Err(Error::Rejected(crate::BLOCKED.into()));
            }
            Msg::ContactPending => {
                ctrl.finish();
                conn.close(0u32.into(), b"pending");
                if let Some(r) = self.shared.lock().state.sent_requests.get_mut(&claim.iss) {
                    r.delivered = true;
                }
                self.persist_async(me.epoch).await?;
                self.log(format!("contact request to {} delivered", claim.name));
                return Ok(AddOutcome::Requested { did: claim.iss.clone(), name: pending.name.clone() });
            }
            other => proto::accept_contact_welcome(&me.id, &pending, &other, remote, now_ms())?,
        };
        ctrl.finish();
        conn.close(0u32.into(), b"added");
        self.shared.lock().state.sent_requests.remove(&claim.iss);
        // We scanned their ticket ourselves: that is the one way back from a block.
        let contact = self.save_contact(me.epoch, new, relay, true, policy.call).await?;
        self.log(format!("contact {} added", contact.name));
        self.list_for_new_contact(&contact.did);
        Ok(AddOutcome::Added { contact })
    }

    /// `lift_block`: only when the user scanned this contact's ticket; a contact who arrives on
    /// their own (they scanned ours) must not undo a removal.
    pub(crate) async fn save_contact(
        self: &Arc<Self>,
        epoch: u64,
        new: proto::NewContact,
        relay: Option<String>,
        lift_block: bool,
        calls_allowed: bool,
    ) -> Result<Contact, Error> {
        let stored = stored_contact(&new, relay, calls_allowed);
        let contact = Contact::from(&stored);
        {
            let mut s = self.shared.lock();
            if s.epoch != epoch {
                return Err(Error::Locked);
            }
            apply_contact(&mut s.state, stored, new.device, lift_block)?;
        }
        self.persist_async(epoch).await?;
        self.events.on_contacts_changed();
        Ok(contact)
    }
}

pub(crate) fn stored_contact(new: &proto::NewContact, relay: Option<String>, calls_allowed: bool) -> StoredContact {
    StoredContact {
        did: new.did.clone(),
        name: proto::sanitize_name(&new.name),
        devices: vec![ContactDevice { device: new.device, relay }],
        device_list: None,
        grant_from_them: new.grant_from_them.clone(),
        call_grant_from_them: new.call_grant_from_them.clone(),
        calls_allowed,
        added_at: now(),
        alias: None,
        verified: false,
    }
}

/// Adds `stored`, or refreshes the contact of that DID. A device we have not seen before resets
/// `verified`: the safety numbers were compared with another phone.
pub(crate) fn apply_contact(st: &mut State, stored: StoredContact, device: [u8; 32], lift_block: bool) -> Result<(), Error> {
    // A block is lifted only by `unblock` (which signs the notice); `add_contact` does that
    // before it gets here.
    if !lift_block && st.blocked.contains(&stored.did) {
        return Err(Error::Rejected(REFUSED.into()));
    }
    // A fresh ticket swap is how two sides that lost touch find each other again.
    st.lost_touch.remove(&stored.did);
    match st.contacts.iter_mut().find(|c| c.did == stored.did) {
        Some(existing) => {
            existing.name = stored.name;
            let relay = stored
                .devices
                .first()
                .and_then(|d| d.relay.clone())
                .or_else(|| existing.relay_for(&device).map(String::from));
            if !existing.has_device(&device) {
                existing.verified = false;
            }
            existing.devices.retain(|d| d.device != device);
            existing.devices.insert(0, ContactDevice { device, relay });
            existing.devices.truncate(MAX_DEVICES);
            // A re-add carries fresh tokens both ways; ours to them follow `calls_allowed`,
            // which an existing contact keeps.
            existing.grant_from_them = stored.grant_from_them;
            existing.call_grant_from_them = stored.call_grant_from_them;
        }
        None => st.contacts.push(stored),
    }
    Ok(())
}

/// Stops handing out the current ticket (the next `my_ticket` mints a fresh one). `keep`: it
/// stays redeemable until it expires (reusable codes); otherwise every ticket we handed out
/// stops working at once (`reset_ticket`, a block).
pub(crate) fn retire_ticket(st: &mut State, keep: bool) {
    if keep {
        let now = now();
        if let Some(c) = st.ticket.as_deref().and_then(|t| ContactTicket::from_text(t).ok()).and_then(|t| t.verify(now).ok()) {
            st.live_tickets.insert(c.nonce, c.exp);
        }
    } else {
        st.live_tickets.clear();
    }
    st.ticket = None;
}

// Whether a ticket of ours still adds contacts is `account_model::tickets` (`ticket_live`, read by
// the issuance store).
#[cfg(test)]
pub(super) use account_model::tickets::current_nonce;
