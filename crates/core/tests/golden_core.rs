//! Golden tests for `p2pcore` persisted formats, regenerated on 2026-10-15 for redb 4 (no back-compat before release); the crypto upgrades
//! (argon2 0.6, aes-gcm 0.11, sha2 0.11, dalek 3, bip39 3) passed on the old fixture unchanged.
//!
//! The vault (`profile.json`), the store keys (`group/store.rs` / `rekey.rs` `key_from_phrase`, BLAKE3
//! `derive_key` over the lower-cased single-spaced phrase) and the sealed redb files are private modules, so
//! they are covered end to end through `Node`: `tests/golden/node/` is a real data directory written once by
//! the pre-upgrade code (generator below) for the public BIP-39 test phrase; the test opens a COPY with
//! `Node::new`, unlocks it with the passphrase, reads the phrase back out of the vault and reads the group
//! that was sealed under the phrase-derived key. If any KDF/AEAD/redb byte moved, this fails.
//! Not reachable without changing visibility (the search keys are pinned by `rekey::tests`): direct `vault::seal/open`
//! vectors, `rekey::old_key`, and the invite/link codes inside core (the codes themselves are `proto`
//! and are frozen in `crates/proto/tests/golden_wire.rs`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use p2pcore::{CallInfo, CallState, Error, Node, NodeEvents, NodeStatus};

const PHRASE: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const PASS: &str = "golden passphrase";
const GROUP_NAME: &str = "Golden group";
const DID: &str = "did:key:z6MknncVmedphgDSbQoYLyNaFRFdB2WtyjxcFdKV6zTfcYvs";

struct Quiet;
impl NodeEvents for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/node")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

#[test]
#[ignore = "generator: rewrites tests/golden/node and prints DID"]
fn dump() {
    let dir = std::env::temp_dir().join(format!("p2pcore-golden-gen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    node.restore_identity(PHRASE.into(), "Golden".into(), PASS.into())
        .unwrap();
    println!("const DID: &str = \"{}\";", node.profile().unwrap().did);
    node.start().unwrap();
    node.group_create(GROUP_NAME.into()).unwrap();
    node.stop();
    drop(node);
    let _ = std::fs::remove_dir_all(fixture());
    copy_dir(&dir, &fixture());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn did_of_phrase_is_stable() {
    assert_eq!(p2pcore::did_of_phrase(PHRASE.into()).unwrap(), DID);
    // Extra whitespace around the phrase is tolerated today.
    assert_eq!(
        p2pcore::did_of_phrase(format!("  {PHRASE}\n")).unwrap(),
        DID
    );
    assert!(matches!(
        p2pcore::did_of_phrase("abandon abandon".into()),
        Err(Error::BadPhrase)
    ));
}

#[test]
fn account_dir_id_is_the_did_suffix() {
    assert_eq!(
        p2pcore::accounts::id_of(DID).unwrap(),
        DID.strip_prefix("did:key:").unwrap()
    );
    assert!(p2pcore::accounts::id_of("did:key:zNotADid").is_err());
}

#[test]
fn captured_data_dir_unlocks_and_reads_back() {
    let dir = std::env::temp_dir().join(format!("p2pcore-golden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&fixture(), &dir);
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();

    // Clear profile fields are readable while locked.
    let info = node.profile().unwrap();
    assert_eq!(info.did, DID);
    assert_eq!(info.name, "Golden");

    // Vault: wrong passphrase refused, right one opens it and returns the phrase (Argon2id + AES-GCM).
    assert!(matches!(
        node.recovery_phrase("wrong".into()),
        Err(Error::WrongPassphrase)
    ));
    assert_eq!(node.recovery_phrase(PASS.into()).unwrap(), PHRASE);
    assert!(matches!(
        node.unlock("wrong".into()),
        Err(Error::WrongPassphrase)
    ));
    node.unlock(PASS.into()).unwrap();

    // The group store is sealed under key_from_phrase(phrase); reading the name proves the key,
    // the AEAD layout and the redb file format all still match.
    let groups = node.groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, GROUP_NAME);

    node.stop();
    drop(node);
    let _ = std::fs::remove_dir_all(&dir);
}
