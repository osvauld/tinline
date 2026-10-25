//! Token delivery and call permissions: the runtime is `node_runtime::caps`, re-exported here;
//! this module holds the `Node` API.

use proto::{Cap, SignedGrant, Unrecorded};

pub use node_runtime::caps::*;

use crate::node::{Node, now_ms};
use crate::Error;

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
                cut.call = cut.call.max(now_ms());
                if let Some(q) = st.token_outbox.get_mut(&did) {
                    q.remove(proto::CAP_CALL);
                }
                false
            }
        };
        let saved = self.inner.persist();
        if deliver {
            self.inner.ctx.tokens_kick(did);
        }
        // The contact's wake token follows its call permission.
        self.inner.ctx.wake_kick();
        self.inner.sink.on_contacts_changed();
        saved.map_err(Error::from)
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
        s.state
            .contacts
            .iter()
            .find(|c| c.did == did)
            .and_then(|c| c.call_grant_from_them.as_ref())
            .is_some_and(|g| call_token_ok(g, &did, me.id.did(), now_ms()))
    }
}

/// Whether `grant` is a call token from `did` to `me_did` that has not expired at `now` (ms).
pub(crate) fn call_token_ok(grant: &SignedGrant, did: &str, me_did: &str, now: u64) -> bool {
    proto::verify_token(grant, did, me_did, Cap::Call, now, &Unrecorded).is_ok()
}

impl Node {
    /// Adds a contact from a ticket with the tokens `origin` decides (see `ContactOrigin`).
    /// `add_contact` is `ContactOrigin::Ticket`; the group packet will add chat-request contacts
    /// through `Inner::add_contact_from` with `GroupRequest`.
    #[doc(hidden)]
    pub fn add_contact_as_for_test(&self, ticket: String, origin: ContactOrigin) -> Result<crate::Contact, Error> {
        self.run(|inner| inner.ctx.clone().add_contact_from(ticket, origin))
    }

    /// For tests: ids (hex nonces) of the (message, call) tokens we hold from `did`.
    #[doc(hidden)]
    pub fn held_token_ids_for_test(&self, did: String) -> (Option<String>, Option<String>) {
        let s = self.inner.shared.lock();
        let c = s.state.contacts.iter().find(|c| c.did == did);
        let id = |g: Option<&SignedGrant>| g.and_then(|g| g.peek().ok()).map(|g| hex::encode(g.nonce));
        (
            id(c.map(|c| &c.grant_from_them)),
            id(c.and_then(|c| c.call_grant_from_them.as_ref())),
        )
    }
}
