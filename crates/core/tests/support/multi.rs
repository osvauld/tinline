//! Several accounts on one device (`docs/multi-online.md`): a device with two accounts, the
//! platform's remembered keys, and what reaches its screens.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use p2pcore::{AccountEvents, AccountRef, BackgroundMode, CallState, Chat, ChatEvents, DeliveryState, Message, WakeOutcome};

use super::{Ev, PASS, Peer, eventually, expect, online};

/// Chat links close this fast in the isolated children (`with_env("P2P_IDLE_CLOSE_MS", ..)`).
pub const LINK_IDLE_MS: &str = "1500";
/// A background account closes this long after its last session.
pub const BG_IDLE_MS: u64 = 800;

/// What a platform does from inside a callback (calls back into the node).
pub type Reentry = Box<dyn Fn() + Send + Sync>;

/// The platform side of background accounts: the keys it remembered, and what the core told it.
#[derive(Default)]
pub struct Accounts {
    keys: Mutex<HashMap<String, Vec<u8>>>,
    /// `on_background_message`: (account DID, from DID, from name).
    pub background: Mutex<Vec<(String, String, String)>>,
    pub changed: AtomicU32,
    /// How often `account_key` was asked.
    pub asked: AtomicU32,
    /// Run once, inside the next `account_key` / `on_accounts_changed`.
    pub on_key: Mutex<Option<Reentry>>,
    pub on_changed: Mutex<Option<Reentry>>,
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
        let reenter = self.on_key.lock().unwrap().take();
        if let Some(f) = reenter {
            f();
        }
        self.keys.lock().unwrap().get(&did).cloned()
    }

    fn on_background_message(&self, account: AccountRef, from_did: String, from_name: String) {
        self.background.lock().unwrap().push((account.did, from_did, from_name));
    }

    fn on_accounts_changed(&self) {
        self.changed.fetch_add(1, Ordering::SeqCst);
        let reenter = self.on_changed.lock().unwrap().take();
        if let Some(f) = reenter {
            f();
        }
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
    /// `b`'s recovery phrase.
    pub b_phrase: String,
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
    build(prefix, tag, b_contacts, None, a_contacts, b_key).0
}

/// A third account `e` on the device (created between `b` and `a`; its key is remembered).
pub struct Third {
    pub did: String,
    pub msg_hint: Option<String>,
    pub call_hint: Option<String>,
}

/// Like `device`, with a third account `e` whose contacts are `e_contacts`.
pub fn device3(prefix: &str, tag: &str, b_contacts: &[&Peer], e_contacts: &[&Peer], a_contacts: &[&Peer]) -> (Device, Third) {
    let (d, e) = build(prefix, tag, b_contacts, Some(e_contacts), a_contacts, true);
    (d, e.expect("third account"))
}

fn build(prefix: &str, tag: &str, b_contacts: &[&Peer], e_contacts: Option<&[&Peer]>, a_contacts: &[&Peer], b_key: bool) -> (Device, Option<Third>) {
    let p = Peer::blank(prefix, &format!("{tag}-dev"));
    let b_phrase = p.node.create_identity("Bea".into(), PASS.into()).unwrap();
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
    let mut third = None;
    let mut e_secret = Vec::new();
    if let Some(e_contacts) = e_contacts {
        p.node.begin_new_account().unwrap();
        p.node.create_identity("Eve".into(), PASS.into()).unwrap();
        let e = p.node.profile().unwrap().did;
        e_secret = p.node.unlock_key().unwrap();
        online(&p.node);
        for c in e_contacts {
            befriend(c, &p, &e);
        }
        let hint = |call| e_contacts.first().map(|c| p.node.wake_hint_for_test(c.did.clone(), call).unwrap());
        third = Some(Third { msg_hint: hint(false), call_hint: hint(true), did: e });
    }
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
    if let Some(t) = &third {
        acc.remember(&t.did, e_secret);
    }
    p.node.set_account_events(acc.clone());
    let screen = Arc::new(Screen::default());
    p.node.set_chat_events(screen.clone());
    p.node.set_background_mode(BackgroundMode::OnWake);
    (Device { p, a, b, b_key: b_secret, a_key, acc, screen, b_msg_hint, b_call_hint, b_phrase }, third)
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

/// `c` writes to `b` while `b` sleeps; the device gets the message wake for `b`. Returns the
/// message id.
pub fn message_while_asleep(d: &Device, c: &Peer) -> String {
    let m = c.node.send_text(d.b.clone(), "are you there?".into(), None).unwrap();
    let outcome = d.p.node.on_wake(d.b_msg_hint.clone().unwrap()).unwrap();
    assert_eq!(outcome, WakeOutcome::DialedBack { did: c.did.clone() });
    m.id
}

/// Runs `f` on a thread of its own and fails the test if it has not returned within `secs`
/// (a deadlock), instead of hanging the run.
pub fn returns_within<T: Send + 'static>(secs: u64, what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(secs)).unwrap_or_else(|_| panic!("{what}: no return within {secs} s (deadlock?)"))
}

/// The call `id` does not become active on `p` within `secs`.
pub fn never_active(p: &Peer, id: &str, secs: u64) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while let Some(left) = end.checked_duration_since(std::time::Instant::now()) {
        match p.rx.recv_timeout(left) {
            Ok(Ev::State(i, CallState::Active)) if i == id => panic!("call {id} connected"),
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

/// The id of the next incoming call on the device, with the account it rings.
pub fn incoming_with_account(p: &Peer, secs: u64) -> (String, Option<String>) {
    expect(p, secs, "incoming call", |e| match e {
        Ev::Incoming(c) => Some((c.call_id.clone(), c.account.as_ref().map(|a| a.did.clone()))),
        _ => None,
    })
}

pub fn active(p: &Peer, id: &str) {
    expect(p, 30, "call active", |e| matches!(e, Ev::State(i, CallState::Active) if i == id).then_some(()));
}
