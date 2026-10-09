//! Accounts on disk: layout, migration from the single-account layout, sealed persistence,
//! switching, and the no-passphrase commit. No network.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use p2pcore::{CallInfo, CallState, Error, LockState, Node, NodeEvents, NodeStatus};
use serde_json::json;
use sha2::Digest;

const PASS: &str = "correct horse battery";

struct Quiet;
impl NodeEvents for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}

struct Root(PathBuf);
impl Root {
    fn new(tag: &str) -> Root {
        let p = std::env::temp_dir().join(format!("p2pcore-accounts-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Root(p)
    }
    fn node(&self) -> Arc<Node> {
        self.try_node().unwrap()
    }
    fn try_node(&self) -> Result<Arc<Node>, Error> {
        Node::new(self.0.to_string_lossy().into(), Arc::new(Quiet))
    }
    fn acct(&self, did: &str) -> PathBuf {
        self.0.join("accounts").join(did.strip_prefix("did:key:").unwrap())
    }
    fn current(&self) -> Option<String> {
        std::fs::read_to_string(self.0.join("current")).ok().map(|s| s.trim().to_string())
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn contains(hay: &[u8], needle: &str) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// None of `markers` appears in any file under `dir`.
fn assert_sealed(dir: &Path, markers: &[String]) {
    for f in files(dir) {
        let bytes = std::fs::read(&f).unwrap();
        for m in markers {
            assert!(!contains(&bytes, m), "{m:?} found in plaintext in {}", f.display());
        }
    }
}

fn assert_no_phrase(dir: &Path, phrase: &str) {
    let w: Vec<&str> = phrase.split_whitespace().collect();
    for f in files(dir) {
        let bytes = std::fs::read(&f).unwrap();
        for pair in w.windows(2) {
            assert!(!contains(&bytes, &format!("{} {}", pair[0], pair[1])), "phrase in {}", f.display());
        }
    }
}

/// Contacts, grants, ticket, block list, call history: what must never be readable on disk.
struct Fixture {
    state: serde_json::Value,
    calls: serde_json::Value,
    markers: Vec<String>,
}

fn fixture(my_did: &str) -> Fixture {
    let (other, _) = identity::generate();
    let grant = proto::issue_grant(&other, my_did, 1_700_000_000, 10 * 365 * 24 * 3600);
    let grant_v = serde_json::to_value(&grant).unwrap();
    let state = json!({
        "contacts": [{
            "did": other.did(), "name": "Zebra Contact", "devices": [vec![9u8; 32]], "relay": null,
            "grant_from_them": grant_v, "added_at": 1, "alias": "Secret Alias", "verified": true
        }],
        "revoked": ["REVOKEDGRANTMARK"],
        "blocked": ["did:key:z6MkBLOCKEDMARK"],
        "redeemed": ["NONCEMARK"],
        "ticket": "OSVC2:TICKETMARK",
    });
    let calls = json!([{
        "call_id": "abc", "peer_did": other.did(), "peer_name": "Called Marker", "incoming": true,
        "started_at": 5, "duration_secs": 3, "reason": "hangup_local", "direct": true, "missed": false
    }]);
    let markers = vec![
        "Zebra Contact".into(),
        "Secret Alias".into(),
        "REVOKEDGRANTMARK".into(),
        "z6MkBLOCKEDMARK".into(),
        "NONCEMARK".into(),
        "TICKETMARK".into(),
        "Called Marker".into(),
        other.did().strip_prefix("did:key:").unwrap().to_string(),
        grant_v["payload"].as_str().unwrap().to_string(),
        grant_v["signature"].as_str().unwrap().to_string(),
    ];
    Fixture { state, calls, markers }
}

/// Turns a freshly made account back into the single-account layout (a pre-multi-account
/// install), with `fx` as its state.json / calls.json. Returns the chat.redb it had.
fn to_old_layout(root: &Root, did: &str, fx: &Fixture) -> Vec<u8> {
    let acct = root.acct(did);
    std::fs::rename(acct.join("account.json"), root.0.join("profile.json")).unwrap();
    let chat = std::fs::read(acct.join("chat.redb")).unwrap();
    std::fs::rename(acct.join("chat.redb"), root.0.join("chat.redb")).unwrap();
    std::fs::remove_dir_all(&acct).unwrap();
    std::fs::remove_file(root.0.join("current")).unwrap();
    std::fs::write(root.0.join("state.json"), serde_json::to_vec_pretty(&fx.state).unwrap()).unwrap();
    std::fs::write(root.0.join("calls.json"), serde_json::to_vec_pretty(&fx.calls).unwrap()).unwrap();
    chat
}

/// A V2 account in the old layout, returning (phrase, did, key, chat.redb bytes, fixture).
fn old_install(root: &Root) -> (String, String, Vec<u8>, Vec<u8>, Fixture) {
    let n = root.node();
    let phrase = n.create_identity("alice".into(), PASS.into()).unwrap();
    let did = n.profile().unwrap().did;
    let key = n.unlock_key().unwrap();
    n.lock();
    drop(n);
    let fx = fixture(&did);
    let chat = to_old_layout(root, &did, &fx);
    (phrase, did, key, chat, fx)
}

fn check_unlocked_state(n: &Node) {
    let c = n.contacts();
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].name.as_str(), c[0].alias.as_deref(), c[0].verified), ("Zebra Contact", Some("Secret Alias"), true));
    let calls = n.recent_calls(10);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].peer_name, "Called Marker");
}

#[test]
fn new_layout_name_is_clear_secrets_are_not() {
    let root = Root::new("layout");
    let n = root.node();
    let phrase = n.create_identity("visible name".into(), PASS.into()).unwrap();
    let did = n.profile().unwrap().did;
    assert!(n.identity_committed());
    assert_eq!(root.current().unwrap(), did.strip_prefix("did:key:").unwrap());
    let acct = root.acct(&did);
    assert!(acct.join("account.json").exists() && acct.join("account.redb").exists());
    let json = std::fs::read(acct.join("account.json")).unwrap();
    assert!(contains(&json, "visible name"), "the account name is clear, by design");
    assert_no_phrase(&acct, &phrase);
    let v: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(v["did"], did.as_str());
    let accounts = n.accounts();
    assert_eq!(accounts.len(), 1);
    assert!(accounts[0].current && accounts[0].has_passphrase && accounts[0].name == "visible name");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&acct), 0o700);
        assert_eq!(mode(&acct.join("account.json")), 0o600);
        assert_eq!(mode(&acct.join("account.redb")), 0o600);
    }
}

#[test]
fn v2_migration_moves_everything_and_seals_at_first_unlock() {
    let root = Root::new("mig-v2");
    let (phrase, did, key, chat, fx) = old_install(&root);

    let n = root.node();
    for gone in ["profile.json", "state.json", "calls.json", "chat.redb"] {
        assert!(!root.0.join(gone).exists(), "{gone} should have moved");
    }
    let acct = root.acct(&did);
    assert_eq!(root.current().unwrap(), did.strip_prefix("did:key:").unwrap());
    assert_eq!(std::fs::read(acct.join("chat.redb")).unwrap(), chat);
    // Still plain JSON until the first unlock.
    assert!(acct.join("state.json").exists() && acct.join("calls.json").exists());
    assert_eq!(n.lock_state(), LockState::Locked);
    assert!(n.contacts().is_empty() && n.recent_calls(10).is_empty());
    assert!(matches!(n.set_available(false, None), Err(Error::Locked)));

    assert!(matches!(n.unlock("nope".into()), Err(Error::WrongPassphrase)));
    assert!(acct.join("state.json").exists(), "a failed unlock imports nothing and deletes nothing");
    n.unlock(PASS.into()).unwrap();
    check_unlocked_state(&n);
    assert!(!acct.join("state.json").exists() && !acct.join("calls.json").exists());
    assert!(n.device_label().is_none());
    n.set_device_label("Private Phone Label".into()).unwrap();
    n.set_available(false, None).unwrap();

    n.lock();
    assert!(n.contacts().is_empty() && n.recent_calls(10).is_empty() && n.device_label().is_none());
    drop(n);

    let mut markers = fx.markers.clone();
    markers.push("Private Phone Label".into());
    assert_sealed(&acct, &markers);
    assert_no_phrase(&acct, &phrase);

    let n = root.node();
    n.unlock_with_key(key).unwrap();
    check_unlocked_state(&n);
    assert_eq!(n.device_label().as_deref(), Some("Private Phone Label"));
    assert!(!n.availability().available);
}

#[test]
fn legacy_clear_profile_migrates_then_seals_on_conversion() {
    let root = Root::new("mig-legacy");
    let (id, m) = identity::generate();
    let phrase = m.to_string();
    let did = id.did().to_string();
    std::fs::write(
        root.0.join("profile.json"),
        serde_json::to_vec_pretty(&json!({"mnemonic": phrase, "name": "old", "device_secret": vec![7u8; 32]})).unwrap(),
    )
    .unwrap();
    let fx = fixture(&did);
    std::fs::write(root.0.join("state.json"), serde_json::to_vec(&fx.state).unwrap()).unwrap();
    std::fs::write(root.0.join("calls.json"), serde_json::to_vec(&fx.calls).unwrap()).unwrap();

    let n = root.node();
    let acct = root.acct(&did);
    assert!(!root.0.join("profile.json").exists() && acct.join("account.json").exists());
    assert_eq!(n.lock_state(), LockState::NeedsPassphrase);
    assert_eq!(n.profile().unwrap().did, did);
    check_unlocked_state(&n); // works as before, from JSON
    assert!(acct.join("state.json").exists());
    assert!(matches!(n.set_device_label("x".into()), Err(Error::Protocol(_))));

    n.set_passphrase(None, PASS.into()).unwrap();
    check_unlocked_state(&n);
    assert!(!acct.join("state.json").exists() && !acct.join("calls.json").exists());
    n.set_device_label("Legacy Phone".into()).unwrap();
    let mut markers = fx.markers.clone();
    markers.push("Legacy Phone".into());
    assert_sealed(&acct, &markers);
    assert_no_phrase(&acct, &phrase);
    n.set_available(false, None).unwrap();
    drop(n);

    let n = root.node();
    assert_eq!(n.lock_state(), LockState::Locked);
    assert!(n.contacts().is_empty());
    n.unlock(PASS.into()).unwrap();
    check_unlocked_state(&n);
    assert!(!n.availability().available);
}

/// The three places a crash can leave a migration, each resumed by the next start.
#[test]
fn interrupted_migration_resumes() {
    for crash in ["after-chat", "after-profile", "after-pointer"] {
        let root = Root::new(&format!("resume-{crash}"));
        let (_, did, _, chat, _) = old_install(&root);
        let id = did.strip_prefix("did:key:").unwrap().to_string();
        let tmp = root.0.join("accounts").join(format!("{id}.migrating"));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::rename(root.0.join("chat.redb"), tmp.join("chat.redb")).unwrap();
        if crash != "after-chat" {
            for f in ["state.json", "calls.json"] {
                std::fs::rename(root.0.join(f), tmp.join(f)).unwrap();
            }
            std::fs::rename(root.0.join("profile.json"), tmp.join("account.json")).unwrap();
        }
        if crash == "after-pointer" {
            std::fs::write(root.0.join("current"), &id).unwrap();
        }
        let n = root.node();
        assert!(!tmp.exists() && root.acct(&did).join("account.json").exists(), "{crash}");
        assert_eq!(root.current().unwrap(), id, "{crash}");
        assert_eq!(std::fs::read(root.acct(&did).join("chat.redb")).unwrap(), chat, "{crash}");
        assert_eq!(n.lock_state(), LockState::Locked, "{crash}");
        n.unlock(PASS.into()).unwrap();
        check_unlocked_state(&n);
        assert_eq!(n.accounts().len(), 1);
    }
}

/// Every file and its content hash, so a failed comparison prints names, not megabytes.
fn snapshot(root: &Path) -> Vec<(PathBuf, String)> {
    let mut v: Vec<_> = files(root)
        .into_iter()
        .map(|p| {
            let h = format!("{:x}", sha2::Sha256::digest(std::fs::read(&p).unwrap()));
            (p, h)
        })
        .collect();
    v.sort();
    v
}

#[test]
fn failed_migration_loses_nothing() {
    // The account already exists: refuse, touch nothing.
    let root = Root::new("mig-exists");
    let (_, did, ..) = old_install(&root);
    let dest = root.acct(&did);
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("keep"), b"someone else's").unwrap();
    let before = snapshot(&root.0);
    assert!(root.try_node().is_err());
    assert_eq!(snapshot(&root.0), before);

    // A profile that does not parse is an error, not an empty account.
    let root = Root::new("mig-garbage");
    std::fs::write(root.0.join("profile.json"), b"{ not json").unwrap();
    std::fs::write(root.0.join("state.json"), b"{}").unwrap();
    let before = snapshot(&root.0);
    assert!(root.try_node().is_err());
    assert_eq!(snapshot(&root.0), before);

    // A half-finished earlier attempt that disagrees with the root: stop, keep both.
    let root = Root::new("mig-conflict");
    let (_, did, ..) = old_install(&root);
    let id = did.strip_prefix("did:key:").unwrap();
    let tmp = root.0.join("accounts").join(format!("{id}.migrating"));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("state.json"), b"{\"older\":true}").unwrap();
    let before = snapshot(&root.0);
    assert!(root.try_node().is_err());
    assert_eq!(snapshot(&root.0), before);
}

#[test]
fn two_accounts_switch_and_stay_apart() {
    let root = Root::new("switch");
    let n = root.node();
    let phrase_a = n.create_identity("alice".into(), PASS.into()).unwrap();
    let a = n.profile().unwrap().did;
    let key_a = n.unlock_key().unwrap();
    n.set_device_label("A phone".into()).unwrap();
    n.set_available(false, None).unwrap();

    n.begin_new_account().unwrap();
    assert!(!n.has_identity() && n.profile().is_none() && root.current().is_none());
    assert_eq!(n.accounts().len(), 1);
    n.create_identity("bob".into(), "other pass".into()).unwrap();
    let b = n.profile().unwrap().did;
    assert_ne!(a, b);
    assert!(n.device_label().is_none());
    assert!(n.availability().available, "settings are per account");
    let list = n.accounts();
    assert_eq!(list.len(), 2);
    assert_eq!(list.iter().filter(|s| s.current).map(|s| s.did.as_str()).collect::<Vec<_>>(), [b.as_str()]);
    assert!(list.iter().any(|s| s.did == a && s.name == "alice" && s.has_passphrase));

    n.switch_account(a.clone()).unwrap();
    assert_eq!(n.lock_state(), LockState::Locked);
    assert_eq!(n.profile().unwrap().did, a);
    assert!(n.unlock_key().is_none() && n.device_label().is_none());
    assert_eq!(root.current().unwrap(), a.strip_prefix("did:key:").unwrap());
    // Keys and passphrases belong to their account.
    assert!(matches!(n.unlock("other pass".into()), Err(Error::WrongPassphrase)));
    n.unlock_with_key(key_a.clone()).unwrap();
    assert_eq!(n.device_label().as_deref(), Some("A phone"));
    assert!(!n.availability().available);

    // The remembered key of A does not open B.
    n.switch_account(b.clone()).unwrap();
    assert!(matches!(n.unlock_with_key(key_a), Err(Error::WrongPassphrase)));
    n.unlock("other pass".into()).unwrap();
    assert!(n.availability().available);

    assert!(matches!(n.remove_account(b.clone()), Err(Error::Protocol(_))));
    n.remove_account(a.clone()).unwrap();
    assert!(!root.acct(&a).exists());
    assert_eq!(n.accounts().len(), 1);
    assert!(matches!(n.remove_account(a.clone()), Err(Error::NotFound)));
    assert!(n.switch_account(a.clone()).is_err());

    // A removed account's phrase can be restored again.
    drop(n);
    let n = root.node();
    assert_eq!(n.profile().unwrap().did, b);
    n.unlock("other pass".into()).unwrap();
    n.begin_new_account().unwrap();
    n.restore_identity(phrase_a, "alice again".into(), PASS.into()).unwrap();
    assert_eq!(n.profile().unwrap().did, a);
    assert_eq!(n.accounts().len(), 2);
}

#[test]
fn existing_account_is_never_overwritten() {
    let root = Root::new("exists");
    let n = root.node();
    let phrase = n.create_identity("alice".into(), PASS.into()).unwrap();
    let did = n.profile().unwrap().did;
    let account_json = std::fs::read(root.acct(&did).join("account.json")).unwrap();
    assert!(matches!(n.create_identity("x".into(), PASS.into()), Err(Error::HaveIdentity)));
    // The open (unlocked) account is never restored over: the caller is told to switch to it.
    // With and without a passphrase.
    assert!(matches!(n.restore_identity(phrase.clone(), "again".into(), "other".into()), Err(Error::AccountExists(d)) if d == did));
    assert!(matches!(n.restore_identity(phrase.clone(), "again".into(), "".into()), Err(Error::AccountExists(d)) if d == did));
    assert_eq!(p2pcore::did_of_phrase(phrase.clone()).unwrap(), did);
    assert_eq!(std::fs::read(root.acct(&did).join("account.json")).unwrap(), account_json);
    assert_eq!(n.profile().unwrap().name, "alice");
}

/// S1: without a passphrase the key lives only in the platform's keystore, so nothing may reach
/// the disk before the platform confirms it saved the key.
#[test]
fn uncommitted_identity_leaves_nothing_on_disk() {
    let root = Root::new("commit");
    let n = root.node();
    let phrase = n.create_identity("alice".into(), "".into()).unwrap();
    assert!(n.has_identity() && !n.identity_committed());
    assert_eq!(n.lock_state(), LockState::Unlocked);
    let key = n.unlock_key().unwrap();
    assert!(files(&root.0).iter().all(|f| f.starts_with(root.0.join("accounts"))) && root.current().is_none());
    assert!(!root.0.join("accounts").read_dir().unwrap().any(|_| true));
    assert_eq!(n.accounts().len(), 1, "the pending identity is listed, as current");
    n.set_device_label("Pending phone".into()).unwrap();

    // Process death before the keystore answered: nothing, onboarding starts again.
    drop(n);
    assert!(files(&root.0).is_empty());
    let n = root.node();
    assert!(!n.has_identity() && n.accounts().is_empty());

    // The same for lock().
    n.restore_identity(phrase.clone(), "alice".into(), "".into()).unwrap();
    n.lock();
    assert!(!n.has_identity() && files(&root.0).is_empty());

    // Committed: durable, and only the key opens it.
    n.restore_identity(phrase.clone(), "alice".into(), "".into()).unwrap();
    let key2 = n.unlock_key().unwrap();
    assert_ne!(key, key2, "a new data key per enrolment");
    n.set_device_label("Committed phone".into()).unwrap();
    n.commit_identity().unwrap();
    n.commit_identity().unwrap(); // idempotent
    assert!(n.identity_committed());
    let did = n.profile().unwrap().did;
    assert!(root.acct(&did).join("account.json").exists());
    assert_eq!(root.current().unwrap(), did.strip_prefix("did:key:").unwrap());
    assert_sealed(&root.0, &["Committed phone".into()]);
    assert_no_phrase(&root.0, &phrase);
    drop(n);

    let n = root.node();
    assert_eq!(n.lock_state(), LockState::Locked);
    assert!(n.identity_committed() && !n.has_passphrase());
    n.unlock_with_key(key2).unwrap();
    assert_eq!(n.device_label().as_deref(), Some("Committed phone"));

    // A restore of the phrase of the open account is refused, committed or not.
    assert!(matches!(n.restore_identity(phrase, "again".into(), "".into()), Err(Error::AccountExists(_))));
}

#[test]
fn adding_a_passphrase_to_an_uncommitted_identity_commits_it() {
    let root = Root::new("commit-pass");
    let n = root.node();
    n.create_identity("alice".into(), "".into()).unwrap();
    assert!(!n.identity_committed());
    n.set_passphrase(None, PASS.into()).unwrap();
    assert!(n.identity_committed() && n.has_passphrase());
    drop(n);
    let n = root.node();
    n.unlock(PASS.into()).unwrap();
}

#[test]
fn device_label_rules_and_sealing() {
    let root = Root::new("label");
    let n = root.node();
    assert!(matches!(n.set_device_label("x".into()), Err(Error::NoIdentity)));
    n.create_identity("alice".into(), PASS.into()).unwrap();
    let did = n.profile().unwrap().did;
    assert!(n.device_label().is_none());
    for bad in ["", "   ", "a\nb", &"x".repeat(65), "tab\there"] {
        assert!(matches!(n.set_device_label(bad.into()), Err(Error::Protocol(_))), "{bad:?}");
    }
    n.set_device_label("  Work desktop ".into()).unwrap();
    assert_eq!(n.device_label().as_deref(), Some("Work desktop"));
    n.set_device_label("x".repeat(64)).unwrap();
    n.set_device_label("Work desktop".into()).unwrap();
    n.lock();
    assert!(matches!(n.set_device_label("y".into()), Err(Error::Locked)));
    drop(n);
    assert_sealed(&root.acct(&did), &["Work desktop".into()]);
    let n = root.node();
    n.unlock(PASS.into()).unwrap();
    assert_eq!(n.device_label().as_deref(), Some("Work desktop"));
}

#[test]
fn account_names_cannot_escape_the_accounts_dir() {
    let root = Root::new("escape");
    let n = root.node();
    for bad in ["did:key:../../etc", "../x", "", "did:key:", "did:key:z/../..", "did:web:example.com"] {
        assert!(n.switch_account(bad.into()).is_err(), "{bad}");
        assert!(n.remove_account(bad.into()).is_err(), "{bad}");
    }
}
