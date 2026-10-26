//! Adding contacts: tickets redeemed by us, contact records.

use super::*;

impl NodeCtx {
    pub async fn add_contact(self: Arc<Self>, text: String) -> Result<Contact, Error> {
        self.add_contact_from(text, crate::caps::ContactOrigin::Ticket).await
    }

    /// `add_contact_outcome` for callers that need the contact now (a group chat request ticket,
    /// which its owner takes at once): a request held for their user is an error here.
    pub async fn add_contact_from(self: Arc<Self>, text: String, origin: crate::caps::ContactOrigin) -> Result<Contact, Error> {
        match self.add_contact_outcome(text, origin).await? {
            crate::contact_req::AddOutcome::Added { contact } => Ok(contact),
            crate::contact_req::AddOutcome::Requested { .. } => Err(Error::Protocol(crate::contact_req::PENDING.into())),
        }
    }

    /// Redeems a ticket, giving them the tokens `origin` decides (P6: a group chat request gives
    /// message only). Their device either adds us at once (`Added`) or holds our hello as a
    /// request for its user (`Requested`): our half is kept in `sent_requests`, and their
    /// Welcome arrives later on a connection they dial (`welcome_incoming`). A code request
    /// (`ContactOrigin::Ticket`) whose owner cannot be reached is queued and is `Requested` too:
    /// `request_out.rs` keeps redialling (waking their phone when the code carries a wake token)
    /// until it is delivered or the code expires.
    pub async fn add_contact_outcome(
        self: Arc<Self>,
        text: String,
        origin: crate::caps::ContactOrigin,
    ) -> Result<crate::contact_req::AddOutcome, Error> {
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
        // Offline: nothing to queue on (`AddError::Offline`).
        self.endpoint()?;
        // We scanned their ticket ourselves: that is a way back from our block (unblock first, so
        // they are told).
        if self.unblock_inner(&claim.iss)? {
            self.persist_async(me.epoch).await?;
            self.notice_kick(claim.iss.clone());
            // Wanted again: a fresh token (new tag) once they hold us as a contact.
            self.wake_kick();
        }
        let policy = crate::caps::policy_for(&self.shared.lock().state, &claim.iss, origin);
        // Only to read our half (deterministic for this code); every delivery makes a fresh hello.
        let (_, pending) = proto::contact_hello_with(&me.id, me.device, &ticket, &me.profile.name, now_ms(), GRANT_TTL, policy)?;
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
                    ticket: Some(ticket.to_text()),
                    last_wake_ms: prior.as_ref().and_then(|p| p.last_wake_ms),
                    wake_dead: false,
                },
            );
            prior
        };
        self.persist_async(me.epoch).await?;
        // With push on, our hello carries a wake token for them: the wake task registers one for
        // an open request (`push.rs` `wanted`).
        if self.push_target().is_some() {
            self.wake_kick();
        }
        self.log(format!("adding contact {}: dialing {}", claim.name, crate::short(&hex::encode(claim.device))));
        // A request by code is queued when their device cannot be reached (and their phone is
        // woken when the code carries a wake token); a group chat request ticket is not.
        let queue = origin == crate::caps::ContactOrigin::Ticket;
        self.first_request(&me, &claim.iss, queue, prior).await
    }

    /// `lift_block`: only when the user scanned this contact's ticket; a contact who arrives on
    /// their own (they scanned ours) must not undo a removal.
    pub async fn save_contact(
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
        self.sink.on_contacts_changed();
        Ok(contact)
    }
}

pub fn stored_contact(new: &proto::NewContact, relay: Option<String>, calls_allowed: bool) -> StoredContact {
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
pub fn apply_contact(st: &mut State, stored: StoredContact, device: [u8; 32], lift_block: bool) -> Result<(), Error> {
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
pub fn retire_ticket(st: &mut State, keep: bool) {
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
pub use account_model::tickets::current_nonce;
