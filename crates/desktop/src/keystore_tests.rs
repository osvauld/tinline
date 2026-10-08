use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::*;
use p2pcore::{CallInfo, CallState, NodeEvents, NodeStatus};

struct Quiet;
impl NodeEvents for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tinline-ks-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn open(d: &Path) -> Arc<Node> {
    Node::new(d.to_string_lossy().into(), Arc::new(Quiet)).unwrap()
}

#[test]
fn failed_key_save_commits_nothing_and_retry_succeeds() {
    let d = dir("fail");
    let st = Mem::default();
    let node = open(&d);
    node.create_identity("Ann".into(), String::new()).unwrap();
    assert!(!node.identity_committed());

    st.fail.store(true, Ordering::SeqCst);
    assert!(commit(&st, &d, &node).is_err());
    assert!(!node.identity_committed());
    drop(node);
    // Process "death" here: nothing was written, so onboarding starts again, no lost-key account.
    let again = open(&d);
    assert!(!again.has_identity());
    assert!(again.accounts().is_empty());

    again.create_identity("Ann".into(), String::new()).unwrap();
    st.fail.store(true, Ordering::SeqCst);
    assert!(commit(&st, &d, &again).is_err());
    st.fail.store(false, Ordering::SeqCst);
    commit(&st, &d, &again).unwrap(); // Retry
    assert!(again.identity_committed());
    drop(again);

    let reopened = open(&d);
    assert_eq!(reopened.lock_state(), LockState::Locked);
    assert!(auto_unlock(&st, &d, &reopened));
    assert_eq!(reopened.lock_state(), LockState::Unlocked);
}

#[test]
fn adding_a_passphrase_instead_commits_when_the_key_cannot_be_saved() {
    let d = dir("addpass");
    let st = Mem::default();
    st.fail.store(true, Ordering::SeqCst);
    let node = open(&d);
    node.create_identity("Ann".into(), String::new()).unwrap();
    assert!(commit(&st, &d, &node).is_err());
    node.set_passphrase(None, "correct horse".into()).unwrap();
    assert!(node.identity_committed());
    sync(&st, &d, &node).unwrap();
    drop(node);
    let node = open(&d);
    assert!(node.has_passphrase());
    node.unlock("correct horse".into()).unwrap();
}

#[test]
fn no_keyring_is_detected_and_nothing_is_written_to_disk() {
    let d = dir("nokr");
    assert!(!available(&Unavailable, &d));
    assert!(available(&Mem::default(), &d));
    let node = open(&d);
    node.create_identity("Ann".into(), String::new()).unwrap();
    assert!(commit(&Unavailable, &d, &node).is_err());
    let leftovers = std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().starts_with("unlock")).count();
    assert_eq!(leftovers, 0, "no plaintext key file for new setups");
}

#[test]
fn keys_are_per_account() {
    let d = dir("peracct");
    let st = Mem::default();
    let node = open(&d);
    node.create_identity("Ann".into(), String::new()).unwrap();
    commit(&st, &d, &node).unwrap();
    let ann = current_did(&node).unwrap();
    node.begin_new_account().unwrap();
    node.create_identity("Bo".into(), String::new()).unwrap();
    commit(&st, &d, &node).unwrap();
    let bo = current_did(&node).unwrap();
    assert_ne!(ann, bo);
    assert_ne!(load(&st, &d, &ann), load(&st, &d, &bo));

    node.switch_account(ann.clone()).unwrap();
    assert!(auto_unlock(&st, &d, &node));
    assert_eq!(node.profile().unwrap().name, "Ann");
    node.switch_account(bo.clone()).unwrap();
    assert!(auto_unlock(&st, &d, &node));
    assert_eq!(node.profile().unwrap().name, "Bo");

    clear(&st, &d, &ann);
    assert!(load(&st, &d, &ann).is_none());
    assert!(load(&st, &d, &bo).is_some());
}

#[test]
fn legacy_single_entry_moves_to_the_migrated_account() {
    let d = dir("legacy");
    let st = Mem::default();
    let node = open(&d);
    node.create_identity("Ann".into(), String::new()).unwrap();
    commit(&st, &d, &node).unwrap();
    let did = current_did(&node).unwrap();
    let key = node.unlock_key().unwrap();
    // Put things back the way the previous version left them.
    clear(&st, &d, &did);
    st.set(&legacy_user(&d), &hex(&key)).unwrap();
    std::fs::write(d.join(LEGACY_FILE), hex(&key)).unwrap();
    drop(node);

    let node = open(&d);
    migrate_legacy(&st, &d, &node);
    assert!(st.get(&legacy_user(&d)).is_none());
    assert!(st.get(&user(&d, &did)).is_some());
    // The keyring copy won; the redundant file is gone.
    assert!(!d.join(LEGACY_FILE).exists());
    assert!(auto_unlock(&st, &d, &node));
}

#[test]
fn legacy_file_only_install_keeps_working_and_is_flagged() {
    let d = dir("legacyfile");
    let st = Mem::default();
    let node = open(&d);
    node.create_identity("Ann".into(), String::new()).unwrap();
    commit(&st, &d, &node).unwrap();
    let did = current_did(&node).unwrap();
    let key = node.unlock_key().unwrap();
    clear(&st, &d, &did);
    std::fs::write(d.join(LEGACY_FILE), hex(&key)).unwrap();
    drop(node);

    let node = open(&d);
    migrate_legacy(&Unavailable, &d, &node);
    assert!(!d.join(LEGACY_FILE).exists());
    assert_eq!(location(&Unavailable, &d, &did), Some(Where::File));
    assert!(auto_unlock(&Unavailable, &d, &node));
    // Re-syncing an unlocked file-based account does not demand a keyring.
    sync(&Unavailable, &d, &node).unwrap();
    // Adding a passphrase deletes the file.
    node.set_passphrase(None, "pw pw pw".into()).unwrap();
    sync(&Unavailable, &d, &node).unwrap();
    assert_eq!(location(&Unavailable, &d, &did), None);
}
