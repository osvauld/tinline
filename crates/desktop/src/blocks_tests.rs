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
    assert_eq!(b.status("ann"), Some("Blocked".to_string()));
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
    assert_eq!(b.status("meera"), Some("Blocked you".to_string()));
    assert_eq!(b.blocked_me("meera"), Some(1_700_000_000));
    assert!(b.list.is_empty(), "their block is not on our blocked list");

    b.refresh_undelivered(&core, "meera", ["m1", "m2"]);
    assert!(b.not_delivered("meera", "m2"));
    assert!(!b.not_delivered("meera", "m1"), "a delivered message stays delivered");

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
