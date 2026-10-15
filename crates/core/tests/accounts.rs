//! Accounts on disk: layout, sealed persistence,
//! switching, and the no-passphrase commit. No network.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use p2pcore::{Error, LockState, Node};

mod support;
use support::{Quiet, TempDir};

const PASS: &str = "correct horse battery";

struct Root(TempDir);
impl Root {
    fn new(tag: &str) -> Root {
        Root(TempDir::created("accounts", tag))
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
