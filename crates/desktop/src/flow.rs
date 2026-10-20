//! Account flows that are plain functions of the node and the key store, so the UI's state
//! transitions (setup, key persistence failure, switching) can be tested without a window.

use std::path::Path;

use p2pcore::{Error, Node};

use crate::keystore::{self, Store};

/// Why finishing a setup stopped.
#[derive(Debug, Clone)]
pub enum SetupErr {
    /// The remembered key could not be saved. The identity is still in memory and unlocked and
    /// nothing was committed: retry, or add a passphrase.
    KeyNotSaved(String),
    Other(String),
}

/// 1-64 characters, no control characters (the core's rule), trimmed.
pub fn valid_label(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty() && s.chars().count() <= 64 && !s.chars().any(char::is_control)).then(|| s.to_string())
}

/// A hostname-ish suggestion for "Name this computer".
pub fn suggest_label() -> String {
    let raw = std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .unwrap_or_default();
    valid_label(&raw.chars().take(64).collect::<String>()).unwrap_or_default()
}

/// Final step of create/restore: name the device (kept in memory until the commit), save the
/// remembered key, and only then make the identity durable. Nothing reports success earlier.
/// `label = None` skips the name (a retry).
pub fn finish_setup(st: &dyn Store, data: &Path, node: &Node, label: Option<&str>) -> Result<(), SetupErr> {
    if let Some(l) = label {
        node.set_device_label(l.to_string()).map_err(|e| SetupErr::Other(e.to_string()))?;
    }
    keystore::commit(st, data, node).map_err(SetupErr::KeyNotSaved)?;
    if node.identity_committed() { Ok(()) } else { Err(SetupErr::Other("the account was not saved".into())) }
}

/// "Add a passphrase instead" after a failed key save: the passphrase protects the account without
/// any remembered key, and setting it commits the identity.
pub fn add_passphrase_instead(st: &dyn Store, data: &Path, node: &Node, pass: String) -> Result<(), String> {
    node.set_passphrase(None, pass).map_err(|e| e.to_string())?;
    if !node.identity_committed() {
        node.commit_identity().map_err(|e| e.to_string())?;
    }
    keystore::sync(st, data, node)
}

/// Last step of linking this computer as the new device: the account is unlocked in memory. An
/// optional passphrase protects it (it then needs no remembered key); without one the key is
/// saved first and the identity committed after, exactly like a fresh setup. `None` = retry of a
/// failed key save.
pub fn finish_link(st: &dyn Store, data: &Path, node: &Node, pass: Option<String>) -> Result<(), SetupErr> {
    match pass.filter(|p| !p.is_empty()) {
        Some(p) => add_passphrase_instead(st, data, node, p).map_err(SetupErr::Other),
        None => finish_setup(st, data, node, None),
    }
}

/// Switches to `did`; opens it with its remembered key when it has one. `Ok(true)` = unlocked.
pub fn switch_to(st: &dyn Store, data: &Path, node: &Node, did: &str) -> Result<bool, Error> {
    node.switch_account(did.to_string())?;
    Ok(keystore::auto_unlock(st, data, node))
}

/// Removes a non-current account and the key remembered for it.
pub fn remove(st: &dyn Store, data: &Path, node: &Node, did: &str) -> Result<(), Error> {
    node.remove_account(did.to_string())?;
    keystore::clear_all(st, data, did);
    Ok(())
}

/// "Remember on this computer" for the current passphrase-protected account: the unlock screen applies the switch after a
/// successful unlock, Privacy > Passphrase applies it at once. `Err` = no secure keyring (nothing was stored).
pub fn apply_remember(st: &dyn Store, data: &Path, node: &Node, on: bool) -> Result<(), String> {
    keystore::set_remember(st, data, node, on)
}

#[cfg(test)]
#[path = "flow_tests.rs"]
mod tests;
