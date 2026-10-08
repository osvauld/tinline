//! Remembers a vault's data key for an account that has no passphrase, so the app opens straight
//! to Home. One OS-keyring entry per account (named by its DID). With a passphrase the desktop
//! asks for it at start and keeps nothing here.
//!
//! There is no plaintext fallback for new setups (security review S2): with no secure keyring the
//! app requires a passphrase. An install that already keeps its key in the old `unlock.key` file
//! still works (the file is renamed per account) and Settings recommends adding a passphrase,
//! which deletes it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use p2pcore::{LockState, Node};

const SERVICE: &str = "com.osvauld.tinline";
const LEGACY_USER: &str = "vault-key";
const LEGACY_FILE: &str = "unlock.key";

/// Where the key of an account is kept.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Where {
    Keyring,
    /// The old plaintext file from before the no-fallback policy (existing installs only).
    File,
}

/// A secret store addressed by entry name; the seam that lets tests inject failures.
pub trait Store: Send + Sync {
    fn set(&self, user: &str, value: &str) -> Result<(), String>;
    fn get(&self, user: &str) -> Option<String>;
    fn delete(&self, user: &str);
}

/// The OS keyring (Secret Service, Keychain, Credential Manager).
pub struct Keyring;

impl Store for Keyring {
    fn set(&self, user: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, user).and_then(|e| e.set_password(value)).map_err(|e| e.to_string())
    }
    fn get(&self, user: &str) -> Option<String> {
        keyring::Entry::new(SERVICE, user).and_then(|e| e.get_password()).ok()
    }
    fn delete(&self, user: &str) {
        if let Ok(e) = keyring::Entry::new(SERVICE, user) {
            let _ = e.delete_credential();
        }
    }
}

/// A store that cannot hold anything: a machine with no keyring (or a test hook simulating one).
pub struct Unavailable;

impl Store for Unavailable {
    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err("no secure keyring is available".into())
    }
    fn get(&self, _: &str) -> Option<String> {
        None
    }
    fn delete(&self, _: &str) {}
}

/// In-memory store for tests, with a switch that makes writes fail.
#[cfg(test)]
#[derive(Default)]
pub struct Mem {
    pub map: Mutex<HashMap<String, String>>,
    pub fail: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl Store for Mem {
    fn set(&self, user: &str, value: &str) -> Result<(), String> {
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("keyring write failed".into());
        }
        self.map.lock().unwrap().insert(user.into(), value.into());
        Ok(())
    }
    fn get(&self, user: &str) -> Option<String> {
        self.map.lock().unwrap().get(user).cloned()
    }
    fn delete(&self, user: &str) {
        self.map.lock().unwrap().remove(user);
    }
}

/// The store the app uses. Test-hooks builds can set P2P_KEYRING=none (no keyring at all).
pub fn system() -> &'static dyn Store {
    static S: OnceLock<Box<dyn Store>> = OnceLock::new();
    S.get_or_init(|| match crate::test_env("P2P_KEYRING").as_deref() {
        Some("none") => Box::new(Unavailable),
        _ => Box::new(Keyring),
    })
    .as_ref()
}

fn canon(data: &Path) -> String {
    data.canonicalize().unwrap_or_else(|_| data.to_path_buf()).display().to_string()
}

fn legacy_user(data: &Path) -> String {
    format!("{LEGACY_USER}:{}", canon(data))
}

/// One entry per data dir and account, so profiles, accounts and test peers never share a key.
fn user(data: &Path, did: &str) -> String {
    format!("{LEGACY_USER}:{}:{did}", canon(data))
}

fn file(data: &Path, did: &str) -> PathBuf {
    let id: String = did.strip_prefix("did:key:").unwrap_or(did).chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    data.join(format!("unlock-{id}.key"))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// Whether the store really keeps secrets (a write, read and delete of a probe entry).
pub fn available(st: &dyn Store, data: &Path) -> bool {
    let probe = format!("probe:{}", canon(data));
    let ok = st.set(&probe, "ok").is_ok() && st.get(&probe).as_deref() == Some("ok");
    st.delete(&probe);
    ok
}

/// The DID of the account the node has selected (or, for an uncommitted identity, is building).
pub fn current_did(node: &Node) -> Option<String> {
    node.accounts().into_iter().find(|a| a.current).map(|a| a.did)
}

/// Stores `key` for `did`. Fails when there is no secure keyring: nothing else is ever written.
pub fn save(st: &dyn Store, data: &Path, did: &str, key: &[u8]) -> Result<(), String> {
    st.set(&user(data, did), &hex(key))?;
    let _ = std::fs::remove_file(file(data, did));
    Ok(())
}

pub fn load(st: &dyn Store, data: &Path, did: &str) -> Option<Vec<u8>> {
    if let Some(k) = st.get(&user(data, did)).and_then(|s| unhex(&s)) {
        return Some(k);
    }
    std::fs::read_to_string(file(data, did)).ok().and_then(|s| unhex(&s))
}

/// Drops the remembered key of `did` (it got a passphrase, or the account was removed).
pub fn clear(st: &dyn Store, data: &Path, did: &str) {
    st.delete(&user(data, did));
    let f = file(data, did);
    if f.is_file() {
        // Overwrite before unlinking; a copy-on-write filesystem may defeat this, but it costs nothing.
        let len = std::fs::metadata(&f).map(|m| m.len() as usize).unwrap_or(0);
        let _ = std::fs::write(&f, vec![b'0'; len]);
        let _ = std::fs::remove_file(f);
    }
}

/// Where the key of `did` lives now, for Settings; `None` when nothing is stored.
pub fn location(st: &dyn Store, data: &Path, did: &str) -> Option<Where> {
    if file(data, did).is_file() {
        Some(Where::File)
    } else if st.get(&user(data, did)).is_some() {
        Some(Where::Keyring)
    } else {
        None
    }
}

/// One-time move of the single pre-accounts entry (keyring entry or `unlock.key`) to the account
/// core migrated into `accounts/<id>/`, which is the current one on the first start. Only for an
/// account without a passphrase; never overwrites an existing per-account key.
pub fn migrate_legacy(st: &dyn Store, data: &Path, node: &Node) {
    let Some(acct) = node.accounts().into_iter().find(|a| a.current) else { return };
    if acct.has_passphrase {
        return;
    }
    let did = acct.did;
    if let Some(v) = st.get(&legacy_user(data)) {
        if st.get(&user(data, &did)).is_some() {
            st.delete(&legacy_user(data));
        } else if st.set(&user(data, &did), &v).is_ok() {
            st.delete(&legacy_user(data));
        }
    }
    let old = data.join(LEGACY_FILE);
    if old.is_file() {
        let new = file(data, &did);
        if new.exists() || st.get(&user(data, &did)).is_some() {
            let _ = std::fs::remove_file(&old);
        } else {
            let _ = std::fs::rename(&old, &new);
        }
    }
}

/// Opens the current account (locked, no passphrase) with its remembered key. Returns whether it
/// is now unlocked. A key that does not open the vault is kept (never delete the only copy on a
/// guess); the key-lost screen offers restore.
pub fn auto_unlock(st: &dyn Store, data: &Path, node: &Node) -> bool {
    if node.lock_state() != LockState::Locked || node.has_passphrase() {
        return false;
    }
    let Some(did) = current_did(node) else { return false };
    let Some(key) = load(st, data, &did) else { return false };
    match node.unlock_with_key(key) {
        Ok(()) => true,
        Err(p2pcore::Error::WrongPassphrase) => {
            eprintln!("keystore: the stored key does not open this vault");
            false
        }
        Err(e) => {
            eprintln!("keystore: unlock failed: {e}");
            false
        }
    }
}

/// Keeps the current unlocked account's remembered key in step with its passphrase: dropped when
/// it has one, stored when it has none. A failure is returned, never swallowed (S1).
pub fn sync(st: &dyn Store, data: &Path, node: &Node) -> Result<(), String> {
    let Some(did) = current_did(node) else { return Ok(()) };
    if node.has_passphrase() {
        clear(st, data, &did);
        return Ok(());
    }
    if file(data, &did).is_file() {
        return Ok(()); // an existing install that already relies on the file
    }
    let key = node.unlock_key().ok_or("the account is locked")?;
    save(st, data, &did, &key)
}

/// Finishes a setup: saves the key first and only then makes the identity durable
/// (`commit_identity`). If the key cannot be stored nothing is committed, so the caller can
/// retry or add a passphrase while the identity is still in memory. Safe to call again.
pub fn commit(st: &dyn Store, data: &Path, node: &Node) -> Result<(), String> {
    sync(st, data, node)?;
    if let Err(e) = node.commit_identity() {
        // Do not leave a key for an account that was never written.
        if let Some(did) = current_did(node) {
            st.delete(&user(data, &did));
        }
        return Err(e.to_string());
    }
    Ok(())
}

#[cfg(test)]
#[path = "keystore_tests.rs"]
mod tests;
