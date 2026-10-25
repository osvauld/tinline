//! Several accounts on one device (`docs/multi-online.md`): a device with two accounts, the
//! platform's remembered keys, and what reaches its screens.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use p2pcore::{AccountEvents, AccountRef, BackgroundMode, Chat, ChatEvents, DeliveryState, Message};

use super::{PASS, Peer, eventually, online};

/// The platform side of background accounts: the keys it remembered, and what the core told it.
#[derive(Default)]
pub struct Accounts {
    keys: Mutex<HashMap<String, Vec<u8>>>,
    /// `on_background_message`: (account DID, from DID, from name).
    pub background: Mutex<Vec<(String, String, String)>>,
    pub changed: AtomicU32,
    /// How often `account_key` was asked.
    pub asked: AtomicU32,
}

impl Accounts {
    pub fn remember(&self, did: &str, key: Vec<u8>) {
        self.keys.lock().unwrap().insert(did.to_string(), key);
    }

    /// The background messages for account `did`, as (from DID, from name).
    pub fn messages_for(&self, did: &str) -> Vec<(String, String)> {
        self.background.lock().unwrap().iter().filter(|(a, _, _)| a == did).map(|(_, f, n)| (f.clone(), n.clone())).collect()
    }
}

impl AccountEvents for Accounts {
    fn account_key(&self, did: String) -> Option<Vec<u8>> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.keys.lock().unwrap().get(&did).cloned()
    }

    fn on_background_message(&self, account: AccountRef, from_did: String, from_name: String) {
        self.background.lock().unwrap().push((account.did, from_did, from_name));
    }

    fn on_accounts_changed(&self) {
        self.changed.fetch_add(1, Ordering::SeqCst);
    }
}

/// The open account's chat screen: every message it was told about, by conversation.
#[derive(Default)]
pub struct Screen {
    pub added: Mutex<Vec<(String, String)>>,
}

impl Screen {
    /// Messages shown in the conversation with `did`.
    pub fn shown(&self, did: &str) -> usize {
        self.added.lock().unwrap().iter().filter(|(p, _)| p == did).count()
    }
}

impl ChatEvents for Screen {
    fn on_message_added(&self, m: Message) {
        self.added.lock().unwrap().push((m.peer_did, m.id));
    }
    fn on_message_changed(&self, _: Message) {}
    fn on_chat_changed(&self, _: Chat) {}
    fn on_delivery_changed(&self, _: String, _: String, _: DeliveryState) {}
    fn on_transfer_progress(&self, _: String, _: String, _: u64, _: u64, _: bool) {}
    fn on_presence_changed(&self, _: String, _: bool) {}
}

/// One device holding two accounts, `a` (open, started) and `b` (asleep), in `OnWake`.
pub struct Device {
    pub p: Peer,
    pub a: String,
    pub b: String,
    /// `b`'s data key, as the platform remembered it.
    pub b_key: Vec<u8>,
    pub a_key: Vec<u8>,
    pub acc: Arc<Accounts>,
    pub screen: Arc<Screen>,
    /// Message and call wake hints for `b`, from its first contact in `b_contacts`.
    pub b_msg_hint: Option<String>,
    pub b_call_hint: Option<String>,
}

/// `c` scans `p`'s open account's code; waits until both hold each other.
pub fn befriend(c: &Peer, p: &Peer, did: &str) {
    c.node.add_contact(p.node.my_ticket().unwrap()).unwrap();
    eventually(60, "contacts both ways", || {
        c.node.contacts().iter().any(|x| x.did == did) && p.node.contacts().iter().any(|x| x.did == c.did)
    });
}

/// Builds the device: account `b` first (its contacts `b_contacts` scan its code and its wake
/// hints are made), then `a` (contacts `a_contacts`). `b_key`: whether the platform remembers
/// `b`'s key (`false`: a passphrase-only account).
pub fn device(prefix: &str, tag: &str, b_contacts: &[&Peer], a_contacts: &[&Peer], b_key: bool) -> Device {
    let p = Peer::blank(prefix, &format!("{tag}-dev"));
    p.node.create_identity("Bea".into(), PASS.into()).unwrap();
    let b = p.node.profile().unwrap().did;
    let b_secret = p.node.unlock_key().unwrap();
    online(&p.node);
    for c in b_contacts {
        befriend(c, &p, &b);
    }
    let (b_msg_hint, b_call_hint) = match b_contacts.first() {
        Some(c) => (Some(p.node.wake_hint_for_test(c.did.clone(), false).unwrap()), Some(p.node.wake_hint_for_test(c.did.clone(), true).unwrap())),
        None => (None, None),
    };
    // Still `OpenOnly`: `b` goes to sleep as on any switch today.
    p.node.begin_new_account().unwrap();
    p.node.create_identity("Ada".into(), PASS.into()).unwrap();
    let a = p.node.profile().unwrap().did;
    let a_key = p.node.unlock_key().unwrap();
    online(&p.node);
    for c in a_contacts {
        befriend(c, &p, &a);
    }
    let acc = Arc::new(Accounts::default());
    acc.remember(&a, a_key.clone());
    if b_key {
        acc.remember(&b, b_secret.clone());
    }
    p.node.set_account_events(acc.clone());
    let screen = Arc::new(Screen::default());
    p.node.set_chat_events(screen.clone());
    p.node.set_background_mode(BackgroundMode::OnWake);
    Device { p, a, b, b_key: b_secret, a_key, acc, screen, b_msg_hint, b_call_hint }
}

impl Device {
    /// The switcher's unread count of account `did`.
    pub fn unread(&self, did: &str) -> u32 {
        self.p.node.accounts().into_iter().find(|s| s.did == did).expect("account listed").unread
    }

    pub fn background(&self) -> Vec<String> {
        self.p.node.background_accounts_for_test()
    }

    /// The open account's DID.
    pub fn open(&self) -> String {
        self.p.node.profile().unwrap().did
    }

    /// Switches to `did`, unlocking it with its key if it opened locked.
    pub fn switch(&self, did: &str) {
        self.p.node.switch_account(did.to_string()).unwrap();
        let key = if did == self.a { self.a_key.clone() } else { self.b_key.clone() };
        self.p.node.unlock_with_key(key).unwrap();
    }
}
