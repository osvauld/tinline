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
fn finish_link_saves_the_key_or_takes_a_passphrase() {
    // No passphrase: key saved, then committed (and a failed save leaves nothing on disk).
    let d = dir("link-nopass");
    let st = Mem::default();
    let node = open(&d);
    node.create_identity("Ann".into(), String::new()).unwrap();
    st.fail.store(true, Ordering::SeqCst);
    assert!(matches!(finish_link(&st, &d, &node, Some(String::new())), Err(SetupErr::KeyNotSaved(_))));
    assert!(!node.identity_committed());
    st.fail.store(false, Ordering::SeqCst);
    finish_link(&st, &d, &node, None).unwrap();
    assert!(node.identity_committed());

    // Passphrase: no remembered key is needed, even without a keyring.
    let d = dir("link-pass");
    let node = open(&d);
    node.create_identity("Bo".into(), String::new()).unwrap();
    finish_link(&keystore::Unavailable, &d, &node, Some("a long passphrase".into())).unwrap();
    assert!(node.identity_committed());
    drop(node);
    let node = open(&d);
    node.unlock("a long passphrase".into()).unwrap();
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
fn restoring_a_phrase_already_here_restores_that_account_in_place() {
    let d = dir("exists");
    let st = Mem::default();
    let node = open(&d);
    let phrase = node.create_identity("Ann".into(), "a long passphrase".into()).unwrap();
    finish_setup(&st, &d, &node, Some("A")).unwrap();
    let did = node.accounts()[0].did.clone();
    // Forgot the passphrase: restore from the phrase, with no account selected.
    node.begin_new_account().unwrap();
    node.restore_identity(phrase.clone(), "Ann".into(), String::new()).unwrap();
    finish_setup(&st, &d, &node, Some("A")).unwrap();
    let list = node.accounts();
    assert_eq!(list.len(), 1);
    assert!(list[0].current && list[0].did == did && !list[0].has_passphrase);
    // While that account is open, restoring it again is refused and names it.
    let e = node.restore_identity(phrase, "Ann".into(), String::new()).unwrap_err();
    assert!(matches!(e, Error::AccountExists(ref x) if *x == did) || matches!(e, Error::HaveIdentity));
}
