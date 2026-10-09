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

/// Switches to `did`; opens it with its remembered key when it has one. `Ok(true)` = unlocked.
pub fn switch_to(st: &dyn Store, data: &Path, node: &Node, did: &str) -> Result<bool, Error> {
    node.switch_account(did.to_string())?;
    Ok(keystore::auto_unlock(st, data, node))
}

/// Removes a non-current account and the key remembered for it.
pub fn remove(st: &dyn Store, data: &Path, node: &Node, did: &str) -> Result<(), Error> {
    node.remove_account(did.to_string())?;
    keystore::clear(st, data, did);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use super::*;
    use crate::keystore::Mem;
    use p2pcore::{CallInfo, CallState, LockState, NodeEvents, NodeStatus};

    struct Quiet;
    impl NodeEvents for Quiet {
        fn on_status(&self, _: NodeStatus) {}
        fn on_contacts_changed(&self) {}
        fn on_incoming_call(&self, _: CallInfo) {}
        fn on_call_state(&self, _: String, _: CallState) {}
        fn on_log(&self, _: String) {}
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("tinline-flow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn open(d: &Path) -> Arc<Node> {
        Node::new(d.to_string_lossy().into(), Arc::new(Quiet)).unwrap()
    }

    #[test]
    fn label_rules() {
        assert_eq!(valid_label("  Work desktop "), Some("Work desktop".into()));
        assert_eq!(valid_label("   "), None);
        assert_eq!(valid_label(&"x".repeat(65)), None);
        assert_eq!(valid_label(&"x".repeat(64)).map(|s| s.len()), Some(64));
        assert_eq!(valid_label("a\nb"), None);
    }

    #[test]
    fn setup_reports_success_only_after_the_key_is_saved_and_committed() {
        let d = dir("setup");
        let st = Mem::default();
        let node = open(&d);
        node.create_identity("Ann".into(), String::new()).unwrap();

        st.fail.store(true, Ordering::SeqCst);
        let e = finish_setup(&st, &d, &node, Some("Work desktop")).unwrap_err();
        assert!(matches!(e, SetupErr::KeyNotSaved(_)));
        assert!(!node.identity_committed());

        // Retry (the label is already held in memory).
        st.fail.store(false, Ordering::SeqCst);
        finish_setup(&st, &d, &node, None).unwrap();
        assert!(node.identity_committed());
        assert_eq!(node.device_label().as_deref(), Some("Work desktop"));
        drop(node);

        let node = open(&d);
        assert_eq!(node.lock_state(), LockState::Locked);
        assert!(keystore::auto_unlock(&st, &d, &node));
        assert_eq!(node.device_label().as_deref(), Some("Work desktop"), "the label is sealed with the account");
    }

    #[test]
    fn add_passphrase_instead_recovers_a_failed_setup() {
        let d = dir("instead");
        let st = Mem::default();
        let node = open(&d);
        node.create_identity("Ann".into(), String::new()).unwrap();
        st.fail.store(true, Ordering::SeqCst);
        assert!(finish_setup(&st, &d, &node, Some("Laptop")).is_err());
        add_passphrase_instead(&st, &d, &node, "a long passphrase".into()).unwrap();
        assert!(node.identity_committed());
        drop(node);
        let node = open(&d);
        node.unlock("a long passphrase".into()).unwrap();
        assert_eq!(node.device_label().as_deref(), Some("Laptop"));
    }

    #[test]
    fn passphrase_setup_with_no_keyring_needs_no_key() {
        let d = dir("pass-nokr");
        let node = open(&d);
        node.create_identity("Ann".into(), "a long passphrase".into()).unwrap();
        finish_setup(&keystore::Unavailable, &d, &node, Some("Desk")).unwrap();
        assert!(node.identity_committed());
    }

    #[test]
    fn switch_remove_and_in_call_style_errors() {
        let d = dir("switch");
        let st = Mem::default();
        let node = open(&d);
        node.create_identity("Ann".into(), String::new()).unwrap();
        finish_setup(&st, &d, &node, Some("A")).unwrap();
        let ann = keystore::current_did(&node).unwrap();
        node.begin_new_account().unwrap();
        node.create_identity("Bo".into(), "bo passphrase".into()).unwrap();
        finish_setup(&st, &d, &node, Some("B")).unwrap();
        let bo = keystore::current_did(&node).unwrap();

        // Ann has a remembered key: switching opens her straight away.
        assert!(switch_to(&st, &d, &node, &ann).unwrap());
        // Bo has a passphrase: switching leaves him locked for the unlock screen.
        assert!(!switch_to(&st, &d, &node, &bo).unwrap());
        assert_eq!(node.lock_state(), LockState::Locked);
        let list = node.accounts();
        assert_eq!(list.len(), 2);
        assert!(list.iter().find(|a| a.did == bo).unwrap().current);

        // The current account cannot be removed; another can, and its key goes with it.
        assert!(remove(&st, &d, &node, &bo).is_err());
        assert!(remove(&st, &d, &node, &ann).is_ok());
        assert!(keystore::load(&st, &d, &ann).is_none());
        assert_eq!(node.accounts().len(), 1);
    }

    #[test]
    fn restoring_a_phrase_already_here_is_account_exists() {
        let d = dir("exists");
        let st = Mem::default();
        let node = open(&d);
        let phrase = node.create_identity("Ann".into(), "a long passphrase".into()).unwrap();
        finish_setup(&st, &d, &node, Some("A")).unwrap();
        node.begin_new_account().unwrap();
        let e = node.restore_identity(phrase, "Ann".into(), String::new()).unwrap_err();
        assert!(matches!(e, Error::AccountExists(_)));
        // Nothing was half-created: the user can still open the existing account.
        assert!(!node.has_identity());
        assert_eq!(node.accounts().len(), 1);
    }
}
