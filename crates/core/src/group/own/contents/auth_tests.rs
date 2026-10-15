//! Admission guards with manually seeded signed registry entries, not QR-link tests.
use super::*;
use crate::{CallInfo, CallState, Node, NodeEvents, NodeStatus};

struct Quiet;
impl NodeEvents for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}

struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tinline-owned-content-auth-{}-{}",
            std::process::id(),
            hex::encode(proto::new_device_secret())
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn group_content_admission_requires_current_epoch_and_nonremoved_own_device() {
    let directory = Directory::new();
    let node = Node::new(directory.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    // An uncommitted synthetic identity; no passphrase derivation, Node::start,
    // phone operation, public network or user's persistent account is involved.
    node.create_identity("synthetic admission".into(), String::new())
        .unwrap();
    let me = node.inner.me().unwrap();
    let remote = cryptography::signature::public_key(&proto::new_device_secret());
    assert!(matches!(
        node.inner.own_group_admit(me.epoch, &remote),
        Err(Error::Rejected(_))
    ));
    assert!(matches!(
        node.inner
            .own_group_admit(me.epoch.wrapping_add(1), &remote),
        Err(Error::Locked)
    ));
    let attestation = proto::attest(&me.id, remote, crate::node::now());
    assert_eq!(
        proto::verify_attestation(&attestation).unwrap().did,
        me.id.did()
    );
    node.inner
        .shared
        .lock()
        .state
        .registry
        .push(crate::store::OwnDevice {
            device: remote,
            attestation,
            label: "synthetic sibling".into(),
            removed: false,
            last_seen: 0,
            relay: None,
        });
    node.inner.own_group_admit(me.epoch, &remote).unwrap();
    node.inner
        .shared
        .lock()
        .state
        .registry
        .iter_mut()
        .find(|r| r.device == remote)
        .unwrap()
        .removed = true;
    assert!(matches!(
        node.inner.own_group_admit(me.epoch, &remote),
        Err(Error::Rejected(_))
    ));
    assert!(matches!(
        node.inner
            .group_own_content_apply(me.epoch, &remote, "not-a-document", &[]),
        Err(Error::Rejected(_))
    ));
    assert!(matches!(
        node.inner.group_own_content_export(
            me.epoch,
            &remote,
            "not-a-document",
            &VersionVector::default()
        ),
        Err(Error::Rejected(_))
    ));
    assert!(matches!(
        node.inner.own_group_admit(me.epoch, &me.device),
        Err(Error::Rejected(_))
    ));
    drop(node);
    drop(directory);
}

#[test]
fn group_content_capability_marker_is_not_a_document_or_membership_proof() {
    assert!(parse_doc(CAPABILITY).is_none());
    assert_eq!(CAPABILITY, "cap/group-contents/v1");
}
