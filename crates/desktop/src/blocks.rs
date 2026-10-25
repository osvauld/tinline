//! Contact blocking and call permissions on the desktop, on the core's real API (`Node::block`,
//! `unblock`, `block_state`, `blocked_contacts`, `not_delivered_reason`;
//! `crates/core/src/block.rs`; and P6's `allow_calls`, `calls_allowed`, `can_call`;
//! `crates/core/src/caps.rs`).
//!
//! Decisions (rotation-open-decisions.md §0): blocking is visible to the blocked person (U5
//! revised); a contact's action is Block and there is no "Remove contact" (U19); every block
//! asks the other app to delete its contact entry for us, so after an unblock they have to add
//! us again (U20); chats are never deleted (U24). A personal block does not touch groups.
//!
//! `Blocks` is what the screens read: refreshed from the core on contact/chat changes, never
//! queried from `view`. The core calls go through `BlockApi` so the flows can be tested with a
//! stand-in that follows the core's rules.

use std::collections::{HashMap, HashSet};

use p2pcore::{BlockState, BlockedContact, Error, Node, PeerBlocks};

/// The core's blocking calls.
pub trait BlockApi {
    fn block(&self, did: String) -> Result<(), Error>;
    fn unblock(&self, did: String) -> Result<(), Error>;
    fn block_state(&self, did: String) -> BlockState;
    fn blocked_contacts(&self) -> Vec<BlockedContact>;
    fn not_delivered_reason(&self, peer_did: String, message_id: String) -> Option<String>;
    /// We let them call us (U13, "Allow calls").
    fn calls_allowed(&self, did: String) -> bool;
    /// We hold their call token: the call button.
    fn holds_call_token(&self, did: String) -> bool;
    /// `block_state`, `calls_allowed` and `holds_call_token` for each of `dids` (one core call
    /// on `Node`).
    fn peer_blocks(&self, dids: Vec<String>) -> Vec<PeerBlocks> {
        dids.into_iter()
            .map(|did| PeerBlocks {
                state: self.block_state(did.clone()),
                calls_allowed: self.calls_allowed(did.clone()),
                can_call: self.holds_call_token(did.clone()),
                did,
            })
            .collect()
    }
}

impl BlockApi for Node {
    fn block(&self, did: String) -> Result<(), Error> {
        Node::block(self, did)
    }
    fn unblock(&self, did: String) -> Result<(), Error> {
        Node::unblock(self, did)
    }
    fn block_state(&self, did: String) -> BlockState {
        Node::block_state(self, did)
    }
    fn blocked_contacts(&self) -> Vec<BlockedContact> {
        Node::blocked_contacts(self)
    }
    fn not_delivered_reason(&self, peer_did: String, message_id: String) -> Option<String> {
        Node::not_delivered_reason(self, peer_did, message_id)
    }
    fn calls_allowed(&self, did: String) -> bool {
        Node::calls_allowed(self, did)
    }
    fn holds_call_token(&self, did: String) -> bool {
        Node::can_call(self, did)
    }
    fn peer_blocks(&self, dids: Vec<String>) -> Vec<PeerBlocks> {
        Node::peer_blocks(self, dids)
    }
}

/// What sits where the composer goes (variants and rules: `present::block`).
pub use present::block::ComposerState as Composer;

#[derive(Default)]
pub struct Blocks {
    /// Settings › Privacy › Blocked people, newest first (one list across our devices).
    pub list: Vec<BlockedContact>,
    /// Block state of each conversation on screen (contacts and kept chats).
    states: HashMap<String, BlockState>,
    /// (peer, message id) of our messages that will not arrive because the peer blocked us.
    undelivered: HashSet<(String, String)>,
    /// Contacts we let call us ("Allow calls" on).
    allowed: HashSet<String>,
    /// Contacts whose call token we hold (they let us call them).
    tokens: HashSet<String>,
}

/// Why the call button does or doesn't dial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallGate {
    Ok,
    /// Blocked on either side, or no longer a contact (`Composer` says which).
    Closed(Composer),
    /// They let us message them but not call (no call token from them; P6).
    NotAllowed,
}

impl Blocks {
    /// Re-reads the blocked list and the state of each of `dids`.
    pub fn refresh(&mut self, api: &dyn BlockApi, dids: impl IntoIterator<Item = String>) {
        self.list = api.blocked_contacts();
        self.states.clear();
        self.allowed.clear();
        self.tokens.clear();
        for PeerBlocks { did, state, calls_allowed, can_call } in api.peer_blocks(dids.into_iter().collect()) {
            if calls_allowed {
                self.allowed.insert(did.clone());
            }
            if can_call {
                self.tokens.insert(did.clone());
            }
            if state.blocked_by_me || state.blocked_me.is_some() {
                self.states.insert(did, state);
            }
        }
        // A block from the list whose DID was not asked about (another device's block of
        // someone no longer listed here) still counts.
        for b in &self.list {
            self.states.entry(b.did.clone()).or_insert_with(|| BlockState {
                blocked_by_me: true,
                blocked_at: Some(b.blocked_at),
                blocked_me: None,
            });
        }
        self.undelivered.retain(|(peer, _)| self.states.get(peer).is_some_and(|s| s.blocked_me.is_some()));
    }

    /// Asks the core why each of our pending messages to `peer` is not delivered.
    /// `pending` is the ids of our own messages still pending.
    pub fn refresh_undelivered<'a>(&mut self, api: &dyn BlockApi, peer: &str, pending: impl IntoIterator<Item = &'a str>) {
        self.undelivered.retain(|(p, _)| p != peer);
        if self.blocked_me(peer).is_none() {
            return;
        }
        for id in pending {
            if api.not_delivered_reason(peer.to_string(), id.to_string()).is_some() {
                self.undelivered.insert((peer.to_string(), id.to_string()));
            }
        }
    }

    pub fn blocked_by_me(&self, did: &str) -> bool {
        self.states.get(did).is_some_and(|s| s.blocked_by_me)
    }

    /// When they blocked us (unix seconds, from their signed notice).
    pub fn blocked_me(&self, did: &str) -> Option<u64> {
        self.states.get(did).and_then(|s| s.blocked_me.as_ref()).map(|n| n.signed_at)
    }

    /// `contact`: they are in our contacts (or it is the "You" conversation).
    pub fn composer(&self, did: &str, contact: bool) -> Composer {
        present::block::composer_state(self.blocked_by_me(did), self.blocked_me(did).is_some(), contact)
    }

    /// Calling is off while either side blocks, for someone who is not a contact, and when they
    /// have not allowed calls from us.
    pub fn call_gate(&self, did: &str, contact: bool) -> CallGate {
        use present::block as pb;
        let composer = self.composer(did, contact);
        match pb::call_gate(composer, self.tokens.contains(did)) {
            pb::CallGate::Ok => CallGate::Ok,
            pb::CallGate::NotAllowed => CallGate::NotAllowed,
            pb::CallGate::Closed => CallGate::Closed(composer),
        }
    }

    pub fn can_call(&self, did: &str, contact: bool) -> bool {
        self.call_gate(did, contact) == CallGate::Ok
    }

    /// The "Allow calls" switch: whether we let them call us.
    pub fn calls_allowed(&self, did: &str) -> bool {
        self.allowed.contains(did)
    }

    /// The red line under the name, in lists and headers.
    pub fn status(&self, did: &str) -> Option<String> {
        present::block::block_status(self.blocked_by_me(did), self.blocked_me(did).is_some())
    }

    pub fn not_delivered(&self, peer: &str, id: &str) -> bool {
        self.undelivered.contains(&(peer.to_string(), id.to_string()))
    }
}

#[cfg(test)]
#[path = "blocks_tests.rs"]
mod tests;
