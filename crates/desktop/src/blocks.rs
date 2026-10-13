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

use p2pcore::{BlockState, BlockedContact, Error, Node};

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
}

/// What sits where the composer goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Composer {
    Open,
    /// We block them: "You blocked X" and Unblock.
    BlockedByMe,
    /// They block us: no composer, just why.
    BlockedMe,
    /// A kept chat with someone who is no longer a contact (their block deleted our entry for
    /// them, then they unblocked): they have to be added again before messaging (U20).
    NotContact,
}

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
    /// Test-hooks demos: who may call us, whom we may call, whom we blocked.
    #[cfg(feature = "test-hooks")]
    pub fn demo(&mut self, allowed: &[String], tokens: &[String], blocked: &[String]) {
        self.allowed = allowed.iter().cloned().collect();
        self.tokens = tokens.iter().cloned().collect();
        for d in blocked {
            self.states.insert(d.clone(), BlockState { blocked_by_me: true, blocked_at: Some(0), blocked_me: None });
        }
    }

    /// Re-reads the blocked list and the state of each of `dids`.
    pub fn refresh(&mut self, api: &dyn BlockApi, dids: impl IntoIterator<Item = String>) {
        self.list = api.blocked_contacts();
        self.states.clear();
        self.allowed.clear();
        self.tokens.clear();
        for did in dids {
            if api.calls_allowed(did.clone()) {
                self.allowed.insert(did.clone());
            }
            if api.holds_call_token(did.clone()) {
                self.tokens.insert(did.clone());
            }
            let st = api.block_state(did.clone());
            if st.blocked_by_me || st.blocked_me.is_some() {
                self.states.insert(did, st);
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
        if self.blocked_by_me(did) {
            Composer::BlockedByMe
        } else if self.blocked_me(did).is_some() {
            Composer::BlockedMe
        } else if !contact {
            Composer::NotContact
        } else {
            Composer::Open
        }
    }

    /// Calling is off while either side blocks, for someone who is not a contact, and when they
    /// have not allowed calls from us.
    pub fn call_gate(&self, did: &str, contact: bool) -> CallGate {
        match self.composer(did, contact) {
            Composer::Open if self.tokens.contains(did) => CallGate::Ok,
            Composer::Open => CallGate::NotAllowed,
            other => CallGate::Closed(other),
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
    pub fn status(&self, did: &str) -> Option<&'static str> {
        match self.composer(did, true) {
            Composer::BlockedByMe => Some("Blocked"),
            Composer::BlockedMe => Some("Blocked you"),
            _ => None,
        }
    }

    pub fn not_delivered(&self, peer: &str, id: &str) -> bool {
        self.undelivered.contains(&(peer.to_string(), id.to_string()))
    }
}

pub fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// "Block X?" and its explanation (U5 revised, U19, U20, U24).
pub fn block_dialog(name: &str) -> (String, Vec<String>) {
    let first = first_name(name);
    (
        format!("Block {first}?"),
        vec![
            format!("{first} can\u{2019}t message or call you, on any of your devices."),
            format!("{first} will see that you blocked them, and their app removes you from their contacts."),
            format!("Your chat history with {first} stays."),
            format!("To talk again after you unblock, {first} has to add you again with your code."),
            "Groups you share are not affected.".into(),
        ],
    )
}

/// Tapping call on someone who has not allowed calls from us, and a call that ended that way.
pub fn calls_not_allowed_text(name: &str) -> String {
    format!("{} hasn\u{2019}t allowed calls from you. You can still message them.", first_name(name))
}

/// Under one of our messages that will not arrive.
pub fn not_delivered_text(name: &str) -> String {
    format!("Not delivered: {} blocked you", first_name(name))
}

/// The signed notice in the blocked person's chat: (title, body).
pub fn blocked_you_card(name: &str) -> (String, String) {
    let first = first_name(name);
    (
        format!("{first} blocked you"),
        format!("You can\u{2019}t message or call {first}. Your earlier messages stay here."),
    )
}

/// In place of the composer of a kept chat with someone who is no longer a contact.
pub fn not_contact_line(name: &str) -> String {
    let first = first_name(name);
    format!("{first} is no longer in your contacts. Add {first} again with their code to message.")
}

/// What the blocker sees in place of the composer.
pub fn blocked_by_me_line(name: &str) -> String {
    let first = first_name(name);
    format!("You blocked {first}. They can\u{2019}t message or call you.")
}

/// Settings › Privacy › Unblocking.
pub const UNBLOCK_NOTE: &str = "They are told you unblocked them and can message you again once they add you back: their app removed you from their contacts when you blocked them. Your chat history is kept either way.";

#[cfg(test)]
mod tests {
    use super::*;
    use p2pcore::BlockNotice;
    use std::cell::RefCell;

    /// Follows the core's rules (`crates/core/src/block.rs`): blocking needs a contact and keeps
    /// it; a notice from someone deletes our contact entry for them but keeps the chat; only
    /// pending messages to someone who blocks us are "not delivered".
    #[derive(Default)]
    struct Core {
        contacts: RefCell<Vec<String>>,
        blocked: RefCell<Vec<(String, u64)>>,
        notices: RefCell<HashMap<String, u64>>,
        pending: RefCell<HashSet<(String, String)>>,
        chats: RefCell<HashSet<String>>,
        clock: RefCell<u64>,
        /// We let them call; their token held by us.
        allowed: RefCell<HashSet<String>>,
        tokens: RefCell<HashSet<String>>,
    }

    impl Core {
        fn with(contacts: &[&str]) -> Self {
            let c = Core::default();
            for d in contacts {
                c.contacts.borrow_mut().push(d.to_string());
                c.chats.borrow_mut().insert(d.to_string());
                // Ticket contacts: message + call both ways.
                c.allowed.borrow_mut().insert(d.to_string());
                c.tokens.borrow_mut().insert(d.to_string());
            }
            c
        }
        /// `from` blocked us: their notice arrives.
        fn notice(&self, from: &str, at: u64) {
            self.notices.borrow_mut().insert(from.into(), at);
            self.contacts.borrow_mut().retain(|c| c != from);
        }
        fn unblock_notice(&self, from: &str) {
            self.notices.borrow_mut().remove(from);
        }
        fn dids(&self) -> Vec<String> {
            let mut v: Vec<String> = self.chats.borrow().iter().cloned().collect();
            v.sort();
            v
        }
    }

    impl BlockApi for Core {
        fn block(&self, did: String) -> Result<(), Error> {
            if !self.contacts.borrow().contains(&did) {
                return Err(Error::NotFound);
            }
            if !self.blocked.borrow().iter().any(|(d, _)| *d == did) {
                *self.clock.borrow_mut() += 1;
                let at = 1000 + *self.clock.borrow();
                self.blocked.borrow_mut().push((did, at));
            }
            Ok(())
        }
        fn unblock(&self, did: String) -> Result<(), Error> {
            self.blocked.borrow_mut().retain(|(d, _)| *d != did);
            Ok(())
        }
        fn block_state(&self, did: String) -> BlockState {
            let at = self.blocked.borrow().iter().find(|(d, _)| *d == did).map(|(_, a)| *a);
            BlockState {
                blocked_by_me: at.is_some(),
                blocked_at: at,
                blocked_me: self.notices.borrow().get(&did).map(|a| BlockNotice { from_did: did.clone(), signed_at: *a, reason: "blocked".into() }),
            }
        }
        fn blocked_contacts(&self) -> Vec<BlockedContact> {
            let mut v: Vec<BlockedContact> =
                self.blocked.borrow().iter().map(|(d, a)| BlockedContact { did: d.clone(), name: d.to_uppercase(), blocked_at: *a }).collect();
            v.sort_by(|a, b| b.blocked_at.cmp(&a.blocked_at));
            v
        }
        fn not_delivered_reason(&self, peer: String, id: String) -> Option<String> {
            (self.notices.borrow().contains_key(&peer) && self.pending.borrow().contains(&(peer, id))).then(|| "blocked".into())
        }
        fn calls_allowed(&self, did: String) -> bool {
            self.contacts.borrow().contains(&did) && self.allowed.borrow().contains(&did)
        }
        fn holds_call_token(&self, did: String) -> bool {
            !self.notices.borrow().contains_key(&did) && self.contacts.borrow().contains(&did) && self.tokens.borrow().contains(&did)
        }
    }

    #[test]
    fn blocking_a_contact_keeps_contact_and_chat_and_lists_them() {
        let core = Core::with(&["ann", "bob"]);
        let mut b = Blocks::default();
        b.refresh(&core, core.dids());
        assert_eq!(b.composer("ann", true), Composer::Open);
        assert!(b.can_call("ann", true));

        // Block from the contact page (after "Block Ann?"): the core call, then a refresh.
        core.block("ann".into()).unwrap();
        b.refresh(&core, core.dids());

        assert!(core.contacts.borrow().contains(&"ann".to_string()), "the contact is kept");
        assert!(core.chats.borrow().contains("ann"), "the chat is kept");
        assert_eq!(b.composer("ann", true), Composer::BlockedByMe);
        assert!(!b.can_call("ann", true));
        assert_eq!(b.status("ann"), Some("Blocked"));
        assert_eq!(b.list.iter().map(|c| c.did.as_str()).collect::<Vec<_>>(), ["ann"]);
        assert_eq!(b.composer("bob", true), Composer::Open);

        // Blocking again is a no-op; the list has them once.
        core.block("ann".into()).unwrap();
        b.refresh(&core, core.dids());
        assert_eq!(b.list.len(), 1);
    }

    #[test]
    fn the_blocked_list_is_newest_first_and_unblock_clears_it() {
        let core = Core::with(&["ann", "bob"]);
        let mut b = Blocks::default();
        core.block("ann".into()).unwrap();
        core.block("bob".into()).unwrap();
        b.refresh(&core, core.dids());
        assert_eq!(b.list.iter().map(|c| c.did.as_str()).collect::<Vec<_>>(), ["bob", "ann"]);

        // Unblock from Settings › Privacy.
        core.unblock("ann".into()).unwrap();
        b.refresh(&core, core.dids());
        assert_eq!(b.list.iter().map(|c| c.did.as_str()).collect::<Vec<_>>(), ["bob"]);
        assert_eq!(b.composer("ann", true), Composer::Open);
        assert!(b.can_call("ann", true));
        assert_eq!(b.status("ann"), None);
    }

    #[test]
    fn a_block_made_on_another_device_shows_even_for_an_unlisted_did() {
        let core = Core::with(&["ann"]);
        core.block("ann".into()).unwrap();
        let mut b = Blocks::default();
        b.refresh(&core, Vec::new());
        assert!(b.blocked_by_me("ann"));
        assert_eq!(b.composer("ann", true), Composer::BlockedByMe);
    }

    #[test]
    fn being_blocked_disables_the_composer_and_marks_pending_messages() {
        let core = Core::with(&["meera"]);
        core.pending.borrow_mut().insert(("meera".into(), "m2".into()));
        let mut b = Blocks::default();
        b.refresh(&core, core.dids());
        b.refresh_undelivered(&core, "meera", ["m1", "m2"]);
        assert!(!b.not_delivered("meera", "m2"), "pending, but not because of a block");

        // Meera's signed notice arrives: our contact entry for her goes, the chat stays.
        core.notice("meera", 1_700_000_000);
        b.refresh(&core, core.dids());
        assert!(!core.contacts.borrow().contains(&"meera".to_string()));
        assert!(core.chats.borrow().contains("meera"), "the chat is kept, read-only");
        assert_eq!(b.composer("meera", false), Composer::BlockedMe);
        assert!(!b.can_call("meera", false));
        assert_eq!(b.status("meera"), Some("Blocked you"));
        assert_eq!(b.blocked_me("meera"), Some(1_700_000_000));
        assert!(b.list.is_empty(), "their block is not on our blocked list");

        b.refresh_undelivered(&core, "meera", ["m1", "m2"]);
        assert!(b.not_delivered("meera", "m2"));
        assert!(!b.not_delivered("meera", "m1"), "a delivered message stays delivered");
        assert_eq!(not_delivered_text("Meera K"), "Not delivered: Meera blocked you");

        // Blocking someone who blocked us: their contact entry is gone, so the core refuses.
        assert!(core.block("meera".into()).is_err());

        // Their unblock clears the state, the chat stays; messaging needs a re-add (U20).
        core.unblock_notice("meera");
        b.refresh(&core, core.dids());
        assert!(!b.not_delivered("meera", "m2"));
        assert!(core.chats.borrow().contains("meera"), "the chat is still kept");
        assert_eq!(b.composer("meera", false), Composer::NotContact);
        assert!(!b.can_call("meera", false));
        assert_eq!(b.status("meera"), None);
        assert!(not_contact_line("Meera K").contains("Add Meera again"));
    }

    #[test]
    fn call_permissions_gate_the_call_button() {
        let core = Core::with(&["ann", "priya"]);
        // Priya (a group chat-request contact) let us message her, not call.
        core.tokens.borrow_mut().remove("priya");
        core.allowed.borrow_mut().remove("priya");
        let mut b = Blocks::default();
        b.refresh(&core, core.dids());
        assert_eq!(b.call_gate("ann", true), CallGate::Ok);
        assert!(b.calls_allowed("ann"));
        assert_eq!(b.call_gate("priya", true), CallGate::NotAllowed);
        assert!(!b.can_call("priya", true));
        assert!(!b.calls_allowed("priya"));
        assert_eq!(b.composer("priya", true), Composer::Open, "messaging is unaffected");
        assert!(calls_not_allowed_text("Priya N").starts_with("Priya hasn"));

        // We allow Priya to call us: only our switch changes.
        core.allowed.borrow_mut().insert("priya".into());
        b.refresh(&core, core.dids());
        assert!(b.calls_allowed("priya"));
        assert_eq!(b.call_gate("priya", true), CallGate::NotAllowed);

        // A block closes calling whatever the tokens say.
        core.block("ann".into()).unwrap();
        b.refresh(&core, core.dids());
        assert_eq!(b.call_gate("ann", true), CallGate::Closed(Composer::BlockedByMe));
    }

    #[test]
    fn the_block_dialog_says_what_happens() {
        let (title, lines) = block_dialog("Arjun Oommen");
        assert_eq!(title, "Block Arjun?");
        let all = lines.join(" ");
        // U5 revised: they see it. U20: their app removes us; re-add to talk again. U24: history stays.
        assert!(all.contains("will see that you blocked them"));
        assert!(all.contains("removes you from their contacts"));
        assert!(all.contains("chat history with Arjun stays"));
        assert!(all.contains("has to add you again"));
        assert!(all.contains("Groups you share are not affected"));
        assert!(!all.to_lowercase().contains("delete"), "nothing is deleted by a block");
    }
}
