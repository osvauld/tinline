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
        self.block_on(inner.add_contact(ticket))?
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
        safety_number(&mine, &did)
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
        self.inner.persist()
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
        self.inner.events.on_contacts_changed();
        saved
    }
}

impl From<&StoredContact> for Contact {
    fn from(c: &StoredContact) -> Self {
        Contact {
            did: c.did.clone(),
            name: c.name.clone(),
            device: c
                .devices
                .first()
                .and_then(|d| PublicKey::from_bytes(&d.device).ok())
                .map(|k| k.to_string())
                .unwrap_or_default(),
            added_at: c.added_at,
            alias: c.alias.clone(),
            verified: c.verified,
            lost_touch: false,
        }
    }
}

/// What the UI should call this contact: our alias if any, else their own name.
pub(crate) fn display_name(c: &StoredContact) -> String {
    c.alias.clone().unwrap_or_else(|| c.name.clone())
}

/// The safety number for the pair: each side's 30 digits come from its own key, and the two
/// halves are joined in sorted order, so both phones compute the same string.
pub(super) fn safety_number(mine: &str, theirs: &str) -> Result<String, Error> {
    let half = |did: &str| -> Result<String, Error> {
        let key = identity::public_key_from_did(did).ok_or_else(|| Error::Protocol("not a valid DID".into()))?;
        let mut h = Sha512::new().chain_update(b"tinline-safety-v1").chain_update(key).finalize();
        // Iterated, as a (cheap) brake on grinding keys whose digits look alike.
        for _ in 0..5200 {
            h = Sha512::new().chain_update(h).chain_update(key).finalize();
        }
        Ok(h[..30]
            .chunks_exact(5)
            .map(|c| format!("{:05}", c.iter().fold(0u64, |n, b| n << 8 | *b as u64) % 100_000))
            .collect())
    };
    let (a, b) = (half(mine)?, half(theirs)?);
    let all = if a <= b { a + &b } else { b + &a };
    Ok(all.as_bytes().chunks(5).map(|g| std::str::from_utf8(g).unwrap_or("")).collect::<Vec<_>>().join(" "))
}

/// The first word of `text` that looks like a contact card (`OSVC2:` or `osvc1.`), so a card
/// pasted with a greeting around it still works.
pub(crate) fn find_card(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '(' | ')'))
        .map(|w| w.trim_end_matches(['.', ',', ';', '!', '?']))
        .find(|w| {
            w.get(..6).is_some_and(|h| h.eq_ignore_ascii_case("OSVC2:") || h.eq_ignore_ascii_case("osvc1."))
                && w.len() > 20
        })
        .map(str::to_string)
}
