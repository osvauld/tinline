//! Remembers the vault's data key for an identity that has no passphrase, so the app opens
//! straight to Home. The OS keyring (Secret Service, Keychain, Credential Manager) holds it;
//! if there is none, a 0600 file in the data dir does, and Settings says so.
//!
//! With a passphrase the desktop asks for it at start and keeps nothing here.

use std::path::{Path, PathBuf};

use p2pcore::{LockState, Node};

const SERVICE: &str = "com.osvauld.tinline";
const USER: &str = "vault-key";
const FILE: &str = "unlock.key";

/// Where the key ended up.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Where {
    Keyring,
    /// No keyring available: a plain file in the app's data dir.
    File,
}

fn file(data: &Path) -> PathBuf {
    data.join(FILE)
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

fn entry() -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(SERVICE, USER)
}

/// Stores `key`. Falls back to the data-dir file when the keyring refuses.
pub fn save(data: &Path, key: &[u8]) -> Result<Where, String> {
    match entry().and_then(|e| e.set_password(&hex(key))) {
        Ok(()) => {
            let _ = std::fs::remove_file(file(data));
            Ok(Where::Keyring)
        }
        Err(e) => {
            eprintln!("keystore: no system keyring ({e}); keeping the key in {FILE} in the data dir");
            write_file(data, key).map(|_| Where::File)
        }
    }
}

#[cfg(unix)]
fn write_file(data: &Path, key: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = data.join(format!("{FILE}.tmp"));
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(|e| e.to_string())?;
    f.write_all(hex(key).as_bytes()).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file(data)).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn write_file(data: &Path, key: &[u8]) -> Result<(), String> {
    std::fs::write(file(data), hex(key)).map_err(|e| e.to_string())
}

pub fn load(data: &Path) -> Option<Vec<u8>> {
    if let Some(k) = entry().and_then(|e| e.get_password()).ok().and_then(|s| unhex(&s)) {
        return Some(k);
    }
    std::fs::read_to_string(file(data)).ok().and_then(|s| unhex(&s))
}

pub fn clear(data: &Path) {
    if let Ok(e) = entry() {
        let _ = e.delete_credential();
    }
    let _ = std::fs::remove_file(file(data));
}

/// Where the key lives now, for Settings; `None` when nothing is stored.
pub fn location(data: &Path) -> Option<Where> {
    if file(data).is_file() {
        Some(Where::File)
    } else if entry().and_then(|e| e.get_password()).is_ok() {
        Some(Where::Keyring)
    } else {
        None
    }
}

/// Opens a locked node that has no passphrase with the stored key. Returns whether it is now
/// unlocked; a key that no longer opens the vault is forgotten.
pub fn auto_unlock(node: &Node, data: &Path) -> bool {
    if node.lock_state() != LockState::Locked || node.has_passphrase() {
        return false;
    }
    let Some(key) = load(data) else { return false };
    match node.unlock_with_key(key) {
        Ok(()) => true,
        Err(p2pcore::Error::WrongPassphrase) => {
            clear(data);
            false
        }
        Err(e) => {
            eprintln!("keystore: unlock failed: {e}");
            false
        }
    }
}

/// After a create / restore / unlock: keep the key if there is no passphrase, drop it if there is.
pub fn sync(node: &Node, data: &Path) {
    if node.has_passphrase() {
        clear(data);
    } else if let Some(k) = node.unlock_key()
        && let Err(e) = save(data, &k)
    {
        eprintln!("keystore: could not store the key: {e}");
    }
}
