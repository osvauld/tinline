//! `Node` API: contacts, safety numbers, availability and call history (UniFFI-exported).

use super::*;

#[uniffi::export]
impl Node {
    /// Finds a contact card in `text` (a pasted message, the clipboard) and reads it without
    /// dialling anyone. `None` when there is no valid, unexpired card or it is our own.
    pub fn peek_card(&self, text: String) -> Option<CardPeek> {
        let ticket = find_card(&text)?;
        let claim = ContactTicket::from_text(&ticket).ok()?.verify(now()).ok()?;
        let me = self.inner.me().ok()?;
        if claim.iss == me.id.did() {
            return None;
        }
        let known = self.inner.shared.lock().state.contacts.iter().any(|c| c.did == claim.iss);
        Some(CardPeek { ticket, name: claim.name, did: claim.iss, known })
    }

    /// Redeems someone's ticket: dials them, exchanges grants, stores them. Blocks until done.
    pub fn add_contact(&self, ticket: String) -> Result<Contact, Error> {
        let inner = self.inner.clone();
        self.block_on(inner.ctx.add_contact(ticket))?.map_err(Error::from)
    }

    pub fn contacts(&self) -> Vec<Contact> {
        let s = self.inner.shared.lock();
        s.state
            .contacts
            .iter()
            .map(|c| Contact { lost_touch: s.state.lost_touch.contains(&c.did), ..Contact::from(c) })
            .collect()
    }

    /// Sets the name only we see for `did`; `None` or blank clears it. Trimmed, stripped of
    /// control characters and cut to 64 characters.
    pub fn rename_contact(&self, did: String, alias: Option<String>) -> Result<(), Error> {
        let alias = alias
            .map(|a| proto::sanitize_name(a.trim()).chars().take(MAX_ALIAS_CHARS).collect::<String>())
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty());
        self.update_contact(&did, |c| c.alias = alias)
    }

    /// Marks that the safety numbers were compared (or not).
    pub fn set_verified(&self, did: String, verified: bool) -> Result<(), Error> {
        self.update_contact(&did, |c| c.verified = verified)
    }

    /// 60 digits in 12 groups of 5, the same on both phones: read it out or compare it in
    /// person. Derived from the two long-term identity keys only, not from any device.
    pub fn safety_number(&self, did: String) -> Result<String, Error> {
        let mine = self.profile().ok_or(Error::NoIdentity)?.did;
        safety_number(&mine, &did).map_err(Error::from)
    }

    /// Finished calls, newest first, at most `limit`. Re-read on `on_call_state` `Ended`.
    pub fn recent_calls(&self, limit: u32) -> Vec<CallRecord> {
        self.inner.history().list(None, limit as usize)
    }

    /// Finished calls with one person, newest first.
    pub fn calls_with(&self, did: String, limit: u32) -> Vec<CallRecord> {
        self.inner.history().list(Some(&did), limit as usize)
    }

    /// Turns calls on or off. Off, incoming calls are not shown and the caller just fails to
    /// get through; they are logged with reason `unavailable`. `until` (unix seconds) switches
    /// it back on by itself. Adding contacts still works.
    pub fn set_available(&self, available: bool, until: Option<u64>) -> Result<(), Error> {
        self.inner.me()?;
        if self.inner.shared.lock().profile.is_some() {
            return self.inner.profile_set_field("availability".into(), Some(serde_json::json!({"available": available, "until": if available { None } else { until }})));
        }
        self.inner.shared.lock().state.availability = crate::store::AvailabilityState {
            unavailable: !available,
            until: if available { None } else { until },
        };
        self.inner.persist().map_err(Error::from)
    }

    pub fn availability(&self) -> Availability {
        self.inner.availability()
    }

    /// Superseded by `block` (U19: a contact's action is Block; the contact and history are
    /// kept). Kept as an alias until the apps switch over.
    pub fn remove_contact(&self, did: String) -> Result<(), Error> {
        self.block(did)
    }
}

impl Node {
    pub(super) fn update_contact(&self, did: &str, f: impl FnOnce(&mut StoredContact)) -> Result<(), Error> {
        {
            let mut s = self.inner.shared.lock();
            f(s.state.contacts.iter_mut().find(|c| c.did == did).ok_or(Error::NotFound)?);
        }
        let saved = self.inner.persist();
        self.inner.sink.on_contacts_changed();
        saved.map_err(Error::from)
    }
}
