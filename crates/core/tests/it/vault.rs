//! Passphrase vault: real Argon2id parameters throughout.

use std::sync::Arc;

use p2pcore::{Error, LockState, Node};

use crate::support;
use support::{Quiet, TempDir};

struct Dir(TempDir);
impl Dir {
    fn new(tag: &str) -> Dir {
        Dir(TempDir::created("vault", tag))
    }
    fn node(&self) -> Arc<Node> {
        Node::new(self.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap()
    }
    fn profile_bytes(&self) -> Vec<u8> {
        let id = std::fs::read_to_string(self.0.join("current")).unwrap();
        std::fs::read(self.0.join("accounts").join(id.trim()).join("account.json")).unwrap()
    }
}

/// No two consecutive mnemonic words appear in the file (single words like "salt" are JSON keys).
fn assert_no_phrase(raw: &str, phrase: &str) {
    let w: Vec<&str> = phrase.split_whitespace().collect();
    for pair in w.windows(2) {
        assert!(!raw.contains(&format!("{} {}", pair[0], pair[1])), "phrase leaked");
    }
}

const PASS: &str = "correct horse battery";

#[test]
fn round_trip_lock_unlock() {
    let dir = Dir::new("roundtrip");
    let n = dir.node();
    assert_eq!(n.lock_state(), LockState::NoIdentity);
    let phrase = n.create_identity("alice".into(), PASS.into()).unwrap();
    assert_eq!(n.lock_state(), LockState::Unlocked);
    let did = n.profile().unwrap().did;

    // Nothing secret in the file.
    let raw = String::from_utf8(dir.profile_bytes()).unwrap();
    assert!(!raw.contains("mnemonic"));
    assert_no_phrase(&raw, &phrase);

    n.lock();
    assert_eq!(n.lock_state(), LockState::Locked);
    // Public parts survive locking.
    let p = n.profile().unwrap();
    assert_eq!((p.did.as_str(), p.name.as_str()), (did.as_str(), "alice"));
    assert!(n.unlock_key().is_none());
    assert!(matches!(n.start(), Err(Error::Locked)));
    assert!(matches!(n.my_ticket(), Err(Error::Locked)));

    n.unlock(PASS.into()).unwrap();
    assert_eq!(n.lock_state(), LockState::Unlocked);
    n.unlock("anything else".into()).unwrap(); // already unlocked: no-op

    // A fresh process sees it locked, and the same identity comes back.
    drop(n);
    let n2 = dir.node();
    assert_eq!(n2.lock_state(), LockState::Locked);
    n2.unlock(PASS.into()).unwrap();
    assert_eq!(n2.profile().unwrap().did, did);
    assert_eq!(n2.recovery_phrase(PASS.into()).unwrap(), phrase);
}

#[test]
fn wrong_passphrase() {
    let dir = Dir::new("wrong");
    let n = dir.node();
    n.create_identity("a".into(), PASS.into()).unwrap();
    n.lock();
    assert!(matches!(n.unlock("not the passphrase".into()), Err(Error::WrongPassphrase)));
    assert_eq!(n.lock_state(), LockState::Locked);
    assert!(matches!(n.recovery_phrase("not the passphrase".into()), Err(Error::WrongPassphrase)));
    n.unlock(PASS.into()).unwrap();
}

#[test]
fn unlock_with_key() {
    let dir = Dir::new("key");
    let n = dir.node();
    n.create_identity("a".into(), PASS.into()).unwrap();
    let key = n.unlock_key().unwrap();
    assert_eq!(key.len(), 32);
    n.lock();
    let mut bad = key.clone();
    bad[0] ^= 1;
    assert!(matches!(n.unlock_with_key(bad), Err(Error::WrongPassphrase)));
    assert!(matches!(n.unlock_with_key(vec![1, 2, 3]), Err(Error::WrongPassphrase)));
    assert_eq!(n.lock_state(), LockState::Locked);
    n.unlock_with_key(key.clone()).unwrap();
    assert_eq!(n.lock_state(), LockState::Unlocked);
    assert_eq!(n.unlock_key().unwrap(), key);
}

#[test]
fn change_passphrase_keeps_key() {
    let dir = Dir::new("change");
    let n = dir.node();
    let phrase = n.create_identity("a".into(), PASS.into()).unwrap();
    let key = n.unlock_key().unwrap();

    assert!(matches!(n.set_passphrase(Some("wrong old one".into()), "new passphrase!".into()), Err(Error::WrongPassphrase)));
    assert!(matches!(n.set_passphrase(None, "new passphrase!".into()), Err(Error::Protocol(_))));
    assert!(matches!(n.set_passphrase(Some(PASS.into()), "".into()), Err(Error::WeakPassphrase)));
    n.set_passphrase(Some(PASS.into()), "new passphrase!".into()).unwrap();
    assert_eq!(n.unlock_key().unwrap(), key);

    drop(n);
    let n = dir.node();
    assert!(matches!(n.unlock(PASS.into()), Err(Error::WrongPassphrase)));
    n.unlock("new passphrase!".into()).unwrap();
    assert_eq!(n.recovery_phrase("new passphrase!".into()).unwrap(), phrase);
    n.lock();
    n.unlock_with_key(key).unwrap(); // the remembered key survived the change
}

#[test]
fn set_name_while_locked_keeps_vault() {
    let dir = Dir::new("name");
    let n = dir.node();
    n.create_identity("a".into(), PASS.into()).unwrap();
    n.lock();
    n.set_name("renamed".into()).unwrap();
    assert_eq!(n.profile().unwrap().name, "renamed");
    n.unlock(PASS.into()).unwrap();
    assert_eq!(n.profile().unwrap().name, "renamed");
}

#[test]
fn no_passphrase_then_add_one() {
    let dir = Dir::new("nopass");
    let n = dir.node();
    let phrase = n.create_identity("a".into(), "".into()).unwrap();
    assert!(!n.has_passphrase());
    let key = n.unlock_key().unwrap();
    assert!(!n.identity_committed());
    n.commit_identity().unwrap(); // the platform's keystore has the key
    assert!(n.identity_committed());
    let raw = String::from_utf8(dir.profile_bytes()).unwrap();
    assert!(!raw.contains("wrapped_dek") && !raw.contains("salt"));
    assert_no_phrase(&raw, &phrase);
    // Unlocked: the phrase needs no passphrase (the platform asks for device auth).
    assert_eq!(n.recovery_phrase("".into()).unwrap(), phrase);

    n.lock();
    assert!(matches!(n.recovery_phrase("".into()), Err(Error::Locked)));
    assert!(matches!(n.unlock("anything".into()), Err(Error::Protocol(_))));
    assert!(matches!(n.set_passphrase(None, PASS.into()), Err(Error::Locked)));
    drop(n);
    let n = dir.node(); // a fresh process: only the remembered key opens it
    assert_eq!(n.lock_state(), LockState::Locked);
    assert!(!n.has_passphrase());
    n.unlock_with_key(key.clone()).unwrap();

    assert!(matches!(n.set_passphrase(Some("x".into()), PASS.into()), Err(Error::Protocol(_))));
    assert!(matches!(n.set_passphrase(None, "".into()), Err(Error::WeakPassphrase)));
    n.set_passphrase(None, "pw".into()).unwrap(); // any length
    assert!(n.has_passphrase());
    assert_eq!(n.unlock_key().unwrap(), key); // same DEK: data is not re-encrypted
    assert!(matches!(n.recovery_phrase("nope".into()), Err(Error::WrongPassphrase)));
    assert_eq!(n.recovery_phrase("pw".into()).unwrap(), phrase);

    drop(n);
    let n = dir.node();
    assert!(n.has_passphrase());
    n.unlock("pw".into()).unwrap();
    n.lock();
    n.unlock_with_key(key).unwrap();
}

/// A version-2 account file written before the passphrase became optional keeps opening, both
/// by passphrase and by key, and still reports a passphrase. A passphrase vault is written with
/// every field the old format had, so the same file is what the previous release produced.
#[test]
fn old_vault_still_opens() {
    let dir = Dir::new("oldvault");
    let (_, m) = identity::generate();
    let phrase = m.to_string();
    let n = dir.node();
    n.restore_identity(phrase.clone(), "old".into(), PASS.into()).unwrap();
    let key = n.unlock_key().unwrap();
    drop(n);
    let v: serde_json::Value = serde_json::from_slice(&dir.profile_bytes()).unwrap();
    assert_eq!(v["version"], 2);
    for f in ["salt", "m", "t", "p", "wrapped_dek", "sealed"] {
        assert!(v["vault"].get(f).is_some(), "{f}");
    }
    let n = dir.node();
    assert!(n.has_passphrase());
    n.unlock(PASS.into()).unwrap();
    assert_eq!(n.recovery_phrase(PASS.into()).unwrap(), phrase);
    n.lock();
    n.unlock_with_key(key).unwrap();
    n.set_passphrase(Some(PASS.into()), "another one".into()).unwrap();
}
