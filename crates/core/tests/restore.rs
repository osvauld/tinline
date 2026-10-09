//! Task 34f: the sealed stores are keyed by the recovery phrase, so a forgotten passphrase loses
//! nothing; stores keyed the old way (from the data key) are re-keyed on unlock.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, Error, Node, NodeEvents, NodeStatus};
use storage::{Sealed, Store};

const PASS: &str = "correct horse battery";

struct Tap(Mutex<mpsc::Sender<(String, CallState)>>);
impl NodeEvents for Tap {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, id: String, s: CallState) {
        let _ = self.0.lock().unwrap().send((id, s));
    }
    fn on_log(&self, _: String) {}
}

struct Root(PathBuf);
impl Root {
    fn new(tag: &str) -> Root {
        let p = std::env::temp_dir().join(format!("p2pcore-restore-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Root(p)
    }
    fn node(&self) -> (Arc<Node>, mpsc::Receiver<(String, CallState)>) {
        let (tx, rx) = mpsc::channel();
        (Node::new(self.0.to_string_lossy().into(), Arc::new(Tap(Mutex::new(tx)))).unwrap(), rx)
    }
    fn acct(&self, did: &str) -> PathBuf {
        self.0.join("accounts").join(did.strip_prefix("did:key:").unwrap())
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn norm(phrase: &str) -> String {
    phrase.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Rewrites a store as the version before 34f wrote it: keys from the data key, no marker.
fn downgrade(path: PathBuf, new_domain: &str, old_domain: &str, phrase: &str, dek: &[u8]) {
    let db = Store::open(&path).unwrap();
    let new = Sealed::new(db.clone(), blake3::derive_key(new_domain, norm(phrase).as_bytes()));
    let old = Sealed::new(db.clone(), blake3::derive_key(old_domain, dek));
    let rows = db.scan("").unwrap();
    for (k, _) in rows {
        if k.starts_with('~') {
            continue;
        }
        let plain = new.get(&k).unwrap().unwrap();
        old.put(&k, &plain).unwrap();
    }
    db.delete("~keyver").unwrap();
}

fn wait_online(n: &Node) {
    n.start().unwrap();
    let t = Instant::now();
    while !n.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Sets recognisable data in both stores without a network.
fn mark(n: &Node) {
    n.set_device_label("Kitchen tablet".into()).unwrap();
    n.set_available(false, None).unwrap();
    n.set_auto_download_limit(12345).unwrap();
}

fn check_marks(n: &Node) {
    assert_eq!(n.device_label().as_deref(), Some("Kitchen tablet"));
    assert!(!n.availability().available);
    assert_eq!(n.auto_download_limit().unwrap(), 12345);
}

#[test]
fn forgotten_passphrase_restore_keeps_the_data() {
    let root = Root::new("forgot");
    let (n, _rx) = root.node();
    let phrase = n.create_identity("alice".into(), PASS.into()).unwrap();
    let did = n.profile().unwrap().did;
    let old_device = n.profile().unwrap().device;
    mark(&n);
    drop(n);

    // Restart: locked, and the passphrase is gone from memory. Restore with a new passphrase
    // (directly over the locked account) and the data is all there.
    let (n, _rx) = root.node();
    assert!(matches!(n.unlock("not it".into()), Err(Error::WrongPassphrase)));
    assert_eq!(p2pcore::did_of_phrase(format!("  {}  ", phrase)).unwrap(), did);
    n.restore_identity(phrase.clone(), "alice".into(), "a new passphrase".into()).unwrap();
    check_marks(&n);
    assert_ne!(n.profile().unwrap().device, old_device, "a fresh device key");
    assert_eq!(n.accounts().len(), 1);
    drop(n);

    let (n, _rx) = root.node();
    assert!(matches!(n.unlock(PASS.into()), Err(Error::WrongPassphrase)), "the old passphrase is gone");
    n.unlock("a new passphrase".into()).unwrap();
    check_marks(&n);
    assert!(root.acct(&did).join("account.json").exists());
}

#[test]
fn restore_without_passphrase_commits_over_the_existing_account() {
    let root = Root::new("nopass");
    let (n, _rx) = root.node();
    let phrase = n.create_identity("alice".into(), PASS.into()).unwrap();
    let did = n.profile().unwrap().did;
    mark(&n);
    drop(n);

    let (n, _rx) = root.node();
    n.begin_new_account().unwrap();
    n.restore_identity(phrase.clone(), "alice".into(), "".into()).unwrap();
    assert!(!n.identity_committed());
    assert_eq!(n.device_label().as_deref(), Some("Kitchen tablet"));
    assert!(!n.availability().available);
    // Uncommitted: the old account.json still unlocks with the old passphrase.
    drop(n);
    let (n, _rx) = root.node();
    n.switch_account(did.clone()).unwrap();
    n.unlock(PASS.into()).unwrap();
    check_marks(&n);
    n.begin_new_account().unwrap();
    n.restore_identity(phrase, "alice".into(), "".into()).unwrap();
    let key = n.unlock_key().unwrap();
    n.commit_identity().unwrap();
    drop(n);
    let (n, _rx) = root.node();
    assert!(!n.has_passphrase());
    n.unlock_with_key(key).unwrap();
    check_marks(&n);
    assert_eq!(n.profile().unwrap().did, did);
}

#[test]
fn old_data_key_stores_are_rekeyed_on_unlock() {
    let root = Root::new("migrate");
    let (n, _rx) = root.node();
    let phrase = n.create_identity("alice".into(), PASS.into()).unwrap();
    let did = n.profile().unwrap().did;
    mark(&n);
    let dek = n.unlock_key().unwrap();
    drop(n);

    let dir = root.acct(&did);
    downgrade(dir.join("account.redb"), "tinline/account-store/v2", "tinline/account-store/v1", &phrase, &dek);
    downgrade(dir.join("chat.redb"), "tinline/chat-store/v2", "tinline chat store v1", &phrase, &dek);
    // Really in the old shape now: the marker is gone.
    assert!(Store::open(dir.join("account.redb")).unwrap().get("~keyver").unwrap().is_none());

    let (n, _rx) = root.node();
    n.unlock(PASS.into()).unwrap();
    check_marks(&n);
    drop(n);
    for f in ["account.redb", "chat.redb"] {
        assert_eq!(Store::open(dir.join(f)).unwrap().get("~keyver").unwrap().unwrap(), b"2");
    }
    // And from now on the phrase alone is enough (a restore reads it without the data key).
    let (n, _rx) = root.node();
    n.restore_identity(phrase, "alice".into(), "another".into()).unwrap();
    check_marks(&n);
}

#[test]
fn restore_over_a_never_migrated_store_sets_it_aside() {
    let root = Root::new("aside");
    let (n, _rx) = root.node();
    let phrase = n.create_identity("alice".into(), PASS.into()).unwrap();
    let did = n.profile().unwrap().did;
    mark(&n);
    let dek = n.unlock_key().unwrap();
    drop(n);
    let dir = root.acct(&did);
    downgrade(dir.join("account.redb"), "tinline/account-store/v2", "tinline/account-store/v1", &phrase, &dek);
    downgrade(dir.join("chat.redb"), "tinline/chat-store/v2", "tinline chat store v1", &phrase, &dek);

    // The data key is lost with the passphrase: the old files cannot be read, so they are kept
    // aside (not deleted) and the account is usable again, empty.
    let (n, _rx) = root.node();
    n.restore_identity(phrase, "alice".into(), "fresh".into()).unwrap();
    assert!(n.availability().available && n.device_label().is_none());
    assert!(dir.join("account.redb.unreadable").exists() && dir.join("chat.redb.unreadable").exists());
}

#[test]
fn a_wrong_phrase_touches_nothing() {
    let root = Root::new("wrong");
    let (n, _rx) = root.node();
    let phrase = n.create_identity("alice".into(), PASS.into()).unwrap();
    let did = n.profile().unwrap().did;
    mark(&n);
    drop(n);
    let snap = |d: &PathBuf| {
        ["account.json", "account.redb", "chat.redb"].map(|f| std::fs::read(d.join(f)).unwrap())
    };
    let before = snap(&root.acct(&did));

    let (n, _rx) = root.node();
    assert!(matches!(n.restore_identity("not a valid phrase at all".into(), "x".into(), "p".into()), Err(Error::BadPhrase)));
    // A different valid phrase is a different account: it is created beside, the first untouched.
    let tmp_root = Root::new("wrong-tmp");
    let other = {
        let (tmp, _rx) = tmp_root.node();
        tmp.create_identity("tmp".into(), "x".into()).unwrap()
    };
    assert_ne!(phrase, other);
    n.begin_new_account().unwrap();
    n.restore_identity(other, "bob".into(), "pw".into()).unwrap();
    assert_eq!(n.accounts().len(), 2);
    drop(n);
    assert_eq!(snap(&root.acct(&did)), before);
    let (n, _rx) = root.node();
    n.switch_account(did).unwrap();
    n.unlock(PASS.into()).unwrap();
    check_marks(&n);
}

/// Contacts, call history and chat messages survive a restore (needs the network like calls.rs).
#[test]
fn contacts_history_and_chats_survive_a_forgotten_passphrase() {
    let ra = Root::new("net-a");
    let rb = Root::new("net-b");
    let (a, _arx) = ra.node();
    a.create_identity("alice".into(), PASS.into()).unwrap();
    wait_online(&a);
    let (b, brx) = rb.node();
    let phrase = b.create_identity("bob".into(), PASS.into()).unwrap();
    wait_online(&b);
    let adid = a.profile().unwrap().did;
    let bdid = b.profile().unwrap().did;
    b.add_contact(a.my_ticket().unwrap()).unwrap();
    b.set_chat_events(Arc::new(NoChat));
    let msg = b.send_text(adid.clone(), "hello alice".into(), None).unwrap();
    let call = b.call(adid.clone()).unwrap();
    let end = Instant::now() + Duration::from_secs(40);
    // Alice does not answer; Bob cancels.
    std::thread::sleep(Duration::from_secs(2));
    b.hangup(call.call_id.clone()).unwrap();
    loop {
        let (id, st) = brx.recv_timeout(end.saturating_duration_since(Instant::now())).expect("call ended");
        if id == call.call_id && matches!(st, CallState::Ended { .. }) {
            break;
        }
    }
    assert!(b.recent_calls(10).iter().any(|r| r.call_id == call.call_id));
    b.stop();
    drop(b);

    let (b, _brx) = rb.node();
    b.restore_identity(phrase, "bob".into(), "pass two".into()).unwrap();
    assert_eq!(b.profile().unwrap().did, bdid);
    assert!(b.contacts().iter().any(|c| c.did == adid));
    assert!(b.recent_calls(10).iter().any(|r| r.call_id == call.call_id));
    let day = b.chat_day(adid, None).unwrap();
    assert!(day.messages.iter().any(|m| m.id == msg.id && m.text == "hello alice"));
}

struct NoChat;
impl p2pcore::ChatEvents for NoChat {
    fn on_message_added(&self, _: p2pcore::Message) {}
    fn on_message_changed(&self, _: p2pcore::Message) {}
    fn on_chat_changed(&self, _: p2pcore::Chat) {}
    fn on_delivery_changed(&self, _: String, _: String, _: p2pcore::DeliveryState) {}
    fn on_transfer_progress(&self, _: String, _: String, _: u64, _: u64, _: bool) {}
    fn on_presence_changed(&self, _: String, _: bool) {}
}
