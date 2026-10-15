//! Production API acceptance tests. Network cases use real independent iroh endpoints.
use p2pcore::{Error, Node};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod support;
use support::{Quiet, TempDir};

struct Peer {
    node: Arc<Node>,
    _dir: TempDir,
    did: String,
    phrase: String,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.node.stop();
    }
}
fn peer(tag: &str) -> Peer {
    let dir = TempDir::new("profile-sync", tag);
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    node.auto_accept_contacts_for_test(true);
    let phrase = node.create_identity(tag.into(), "pass".into()).unwrap();
    let did = node.profile().unwrap().did;
    Peer {
        node,
        _dir: dir,
        did,
        phrase,
    }
}
fn fields(node: &Node, did: &str) -> serde_json::Value {
    serde_json::from_str(
        &node
            .account_profile(did.into())
            .unwrap()
            .unwrap()
            .fields_json,
    )
    .unwrap()
}
fn wait(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(50);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out: {what}");
}
fn png(color: u8) -> Vec<u8> {
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        8,
        8,
        image::Rgb([color, 20, 30]),
    ));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn default_picture_actions_automatically_request_old_copy_deletion() {
    let p = peer("automatic-delete");
    p.node.set_profile_picture(png(10)).unwrap();
    let first = p
        .node
        .account_profile(p.did.clone())
        .unwrap()
        .unwrap()
        .picture_hash
        .unwrap();
    assert!(
        p.node
            .picture_deletion_permits(p.did.clone())
            .unwrap()
            .is_empty()
    );
    p.node.set_profile_picture(png(10)).unwrap();
    assert_eq!(
        p.node
            .account_profile(p.did.clone())
            .unwrap()
            .unwrap()
            .picture_hash
            .as_ref(),
        Some(&first)
    );
    assert!(
        p.node
            .picture_deletion_permits(p.did.clone())
            .unwrap()
            .is_empty(),
        "same-picture no-op cannot request deletion of the current image"
    );
    p.node.set_profile_picture(png(20)).unwrap();
    let second = p
        .node
        .account_profile(p.did.clone())
        .unwrap()
        .unwrap()
        .picture_hash
        .unwrap();
    let permits = p.node.picture_deletion_permits(p.did.clone()).unwrap();
    assert_eq!(
        permits.len(),
        1,
        "replacement automatically signs the old-hash request"
    );
    let blob: proto::profile::SignedPictureDeletion = serde_json::from_str(&permits[0]).unwrap();
    assert_eq!(
        proto::profile::verify_picture_deletion(&blob, &p.did)
            .unwrap()
            .claim
            .picture_hash,
        first
    );
    p.node.fail_next_writes(1);
    assert!(p.node.clear_profile_picture().is_err());
    assert_eq!(
        p.node
            .account_profile(p.did.clone())
            .unwrap()
            .unwrap()
            .picture_hash
            .as_ref(),
        Some(&second)
    );
    assert_eq!(
        p.node.picture_deletion_permits(p.did.clone()).unwrap(),
        permits,
        "failed removal cannot publish a request"
    );
    p.node.clear_profile_picture().unwrap();
    assert!(p.node.profile_picture(p.did.clone()).unwrap().is_none());
    let permits = p.node.picture_deletion_permits(p.did.clone()).unwrap();
    assert_eq!(
        permits.len(),
        2,
        "removal automatically signs the last selected hash"
    );
    let hashes: Vec<_> = permits
        .iter()
        .map(|raw| {
            let blob: proto::profile::SignedPictureDeletion = serde_json::from_str(raw).unwrap();
            proto::profile::verify_picture_deletion(&blob, &p.did)
                .unwrap()
                .claim
                .picture_hash
        })
        .collect();
    assert!(hashes.contains(&first) && hashes.contains(&second));
    p.node.clear_profile_picture().unwrap();
    assert_eq!(
        p.node.picture_deletion_permits(p.did.clone()).unwrap(),
        permits
    );
    p.node.lock();
    p.node.unlock("pass".into()).unwrap();
    assert_eq!(
        p.node.picture_deletion_permits(p.did.clone()).unwrap(),
        permits,
        "requests survive lock/unlock"
    );
}

#[test]
fn account_profile_persists_extensions_and_picture_and_lock_hides_them() {
    let p = peer("persistence");
    p.node
        .set_profile_field(
            "example:interests".into(),
            Some(r#"{"topics":["music"]}"#.into()),
        )
        .unwrap();
    p.node.set_profile_picture(png(50)).unwrap();
    let before = fields(&p.node, &p.did);
    let hash = p
        .node
        .account_profile(p.did.clone())
        .unwrap()
        .unwrap()
        .picture_hash
        .unwrap();
    let picture = p.node.profile_picture(p.did.clone()).unwrap().unwrap();
    p.node.lock();
    assert!(matches!(
        p.node.account_profile(p.did.clone()),
        Err(Error::Locked)
    ));
    assert!(matches!(
        p.node
            .set_profile_field("status".into(), Some("\"bad\"".into())),
        Err(Error::Locked)
    ));
    p.node.unlock("pass".into()).unwrap();
    assert_eq!(fields(&p.node, &p.did), before);
    assert_eq!(
        p.node.profile_picture(p.did.clone()).unwrap().unwrap(),
        picture
    );
    assert_eq!(
        p.node
            .account_profile(p.did.clone())
            .unwrap()
            .unwrap()
            .picture_hash
            .unwrap(),
        hash
    );
    p.node.clear_profile_picture().unwrap();
    assert!(p.node.profile_picture(p.did.clone()).unwrap().is_none());
}

#[test]
fn invalid_fields_and_failed_writes_leave_profile_unchanged() {
    let p = peer("validation");
    let before = fields(&p.node, &p.did);
    assert!(
        p.node
            .set_profile_field("picture".into(), Some("{}".into()))
            .is_err()
    );
    assert!(
        p.node
            .set_profile_field("status".into(), Some("true".into()))
            .is_err()
    );
    assert!(
        p.node
            .set_profile_field("x".repeat(257), Some("null".into()))
            .is_err()
    );
    assert!(p.node.set_profile_picture(vec![0; 100]).is_err());
    assert_eq!(fields(&p.node, &p.did), before);
    p.node.fail_next_writes(1);
    assert!(
        p.node
            .set_profile_field("status".into(), Some("\"failed\"".into()))
            .is_err()
    );
    assert_eq!(fields(&p.node, &p.did), before);
    p.node
        .set_profile_field("status".into(), Some("\"saved\"".into()))
        .unwrap();
    assert_eq!(fields(&p.node, &p.did)["status"], "saved");
    assert!(p.node.set_name("spoof\u{202e}name".into()).is_err());
    assert_eq!(fields(&p.node, &p.did)["name"], before["name"]);
    p.node.lock();
    assert!(p.node.set_name("spoof\u{202e}name".into()).is_err());
    p.node.unlock("pass".into()).unwrap();
    assert_eq!(fields(&p.node, &p.did)["name"], before["name"]);
    p.node.set_profile_picture(png(35)).unwrap();
    let picture_before = fields(&p.node, &p.did);
    p.node.fail_next_writes(1);
    assert!(p.node.replace_profile_picture(png(45)).is_err());
    assert_eq!(fields(&p.node, &p.did), picture_before);
    assert!(
        p.node
            .picture_deletion_permits(p.did.clone())
            .unwrap()
            .is_empty()
    );
    assert!(p.node.profile_picture(p.did.clone()).unwrap().is_some());
}

#[test]
fn profiles_and_pictures_sync_without_a_message_and_catch_up_after_offline() {
    let a = peer("direct-a");
    let b = peer("direct-b");
    a.node
        .set_profile_field("status".into(), Some("\"Initial\"".into()))
        .unwrap();
    a.node.set_profile_picture(png(70)).unwrap();
    a.node.start().unwrap();
    b.node.start().unwrap();
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    wait("initial picture", || {
        b.node
            .profile_picture(a.did.clone())
            .ok()
            .flatten()
            .is_some()
    });
    assert_eq!(fields(&b.node, &a.did)["status"], "Initial");
    b.node.stop();
    a.node
        .set_profile_field("status".into(), Some("\"Offline update\"".into()))
        .unwrap();
    a.node.set_profile_picture(png(90)).unwrap();
    let expected_hash = a
        .node
        .account_profile(a.did.clone())
        .unwrap()
        .unwrap()
        .picture_hash;
    b.node.start().unwrap();
    wait("offline catch-up", || {
        b.node
            .account_profile(a.did.clone())
            .ok()
            .flatten()
            .is_some_and(|p| p.picture_hash == expected_hash && p.picture_ready)
    });
    assert_eq!(fields(&b.node, &a.did)["status"], "Offline update");
    a.node.clear_profile_picture().unwrap();
    wait("picture removal", || {
        b.node
            .account_profile(a.did.clone())
            .ok()
            .flatten()
            .is_some_and(|p| p.picture_hash.is_none())
    });
    assert!(b.node.profile_picture(a.did.clone()).unwrap().is_none());
}

#[test]
fn linked_devices_merge_profile_fields_and_sync_availability_and_picture() {
    let a = peer("linked-a");
    let mut b = peer("linked-b");
    b.node.stop();
    b.node.begin_new_account().unwrap();
    b.node.remove_account(b.did.clone()).unwrap();
    b.node
        .restore_identity(a.phrase.clone(), "sibling".into(), "pass".into())
        .unwrap();
    b.did = a.did.clone();
    a.node
        .add_own_device_for_test(b.node.own_attestation_for_test().unwrap())
        .unwrap();
    b.node
        .add_own_device_for_test(a.node.own_attestation_for_test().unwrap())
        .unwrap();
    a.node
        .set_profile_field("status".into(), Some("\"Reading\"".into()))
        .unwrap();
    b.node
        .set_profile_field("example:interests".into(), Some("[\"music\"]".into()))
        .unwrap();
    a.node.set_available(false, Some(4_000_000_000)).unwrap();
    a.node.set_profile_picture(png(110)).unwrap();
    a.node.start().unwrap();
    b.node.start().unwrap();
    wait("linked fields", || {
        fields(&b.node, &b.did)
            .get("status")
            .is_some_and(|v| v == "Reading")
            && fields(&a.node, &a.did).get("example:interests").is_some()
    });
    wait("linked picture", || {
        b.node
            .profile_picture(b.did.clone())
            .ok()
            .flatten()
            .is_some()
    });
    assert_eq!(
        fields(&a.node, &a.did)["name"],
        "linked-a",
        "fresh install defaults cannot rename the existing account"
    );
    assert_eq!(fields(&b.node, &b.did)["name"], "linked-a");
    assert!(!b.node.availability().available);
    b.node.set_available(true, None).unwrap();
    wait("availability resumed", || a.node.availability().available);
    let old = b
        .node
        .account_profile(b.did.clone())
        .unwrap()
        .unwrap()
        .picture_hash
        .unwrap();
    b.node.stop();
    a.node.set_profile_picture(png(120)).unwrap();
    let new = a
        .node
        .account_profile(a.did.clone())
        .unwrap()
        .unwrap()
        .picture_hash;
    b.node.start().unwrap();
    wait("sibling catches up with automatic deletion", || {
        b.node
            .account_profile(b.did.clone())
            .ok()
            .flatten()
            .is_some_and(|p| p.picture_hash == new && p.picture_ready)
            && b.node
                .picture_deletion_permits(b.did.clone())
                .unwrap()
                .len()
                == 1
    });
    let raw = b
        .node
        .picture_deletion_permits(b.did.clone())
        .unwrap()
        .remove(0);
    let blob: proto::profile::SignedPictureDeletion = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        proto::profile::verify_picture_deletion(&blob, &a.did)
            .unwrap()
            .claim
            .picture_hash,
        old
    );
    wait("sibling old picture reclaimed", || {
        !b.node.profile_blob_present_for_test(old.clone()).unwrap()
    });
    assert!(b.node.profile_picture(b.did.clone()).unwrap().is_some());
}

#[test]
fn deletion_is_automatic_owner_signed_and_reaches_offline_contacts() {
    let a = peer("delete-a");
    let b = peer("delete-b");
    a.node.set_profile_picture(png(130)).unwrap();
    a.node.start().unwrap();
    b.node.start().unwrap();
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    wait("old picture received", || {
        b.node
            .account_profile(a.did.clone())
            .ok()
            .flatten()
            .is_some_and(|p| p.picture_ready)
    });
    let old = a
        .node
        .account_profile(a.did.clone())
        .unwrap()
        .unwrap()
        .picture_hash
        .unwrap();
    b.node.stop();
    a.node.replace_profile_picture(png(150)).unwrap();
    let new = a
        .node
        .account_profile(a.did.clone())
        .unwrap()
        .unwrap()
        .picture_hash
        .unwrap();
    assert_eq!(
        a.node
            .picture_deletion_permits(a.did.clone())
            .unwrap()
            .len(),
        1
    );
    b.node.start().unwrap();
    wait("replacement received", || {
        b.node
            .account_profile(a.did.clone())
            .ok()
            .flatten()
            .is_some_and(|p| p.picture_hash.as_deref() == Some(new.as_str()) && p.picture_ready)
    });
    assert_eq!(
        a.node
            .picture_deletion_permits(a.did.clone())
            .unwrap()
            .len(),
        1,
        "replacement has no deletion opt-in"
    );
    assert!(
        a.node
            .request_profile_picture_deletion(new.clone())
            .is_err(),
        "current picture is not a deletion target"
    );
    wait("signed deletion caught up", || {
        !b.node
            .picture_deletion_permits(a.did.clone())
            .unwrap()
            .is_empty()
    });
    let blob: proto::profile::SignedPictureDeletion =
        serde_json::from_str(&b.node.picture_deletion_permits(a.did.clone()).unwrap()[0]).unwrap();
    let permit = proto::profile::verify_picture_deletion(&blob, &a.did).unwrap();
    assert_eq!(permit.claim.picture_hash, old);
    assert_eq!(permit.sub, proto::Subject::Anyone);
    wait("old encrypted bytes reclaimed", || {
        !b.node.profile_blob_present_for_test(old.clone()).unwrap()
    });
    assert!(
        b.node.profile_picture(a.did.clone()).unwrap().is_some(),
        "new picture survives"
    );
    b.node.stop();
    a.node.remove_profile_picture().unwrap();
    assert_eq!(
        a.node
            .picture_deletion_permits(a.did.clone())
            .unwrap()
            .len(),
        2
    );
    b.node.start().unwrap();
    wait("automatic removal request arrived", || {
        b.node
            .picture_deletion_permits(a.did.clone())
            .unwrap()
            .len()
            == 2
            && b.node
                .account_profile(a.did.clone())
                .unwrap()
                .unwrap()
                .picture_hash
                .is_none()
    });
    let hashes: Vec<_> = b
        .node
        .picture_deletion_permits(a.did.clone())
        .unwrap()
        .iter()
        .map(|raw| {
            let blob: proto::profile::SignedPictureDeletion = serde_json::from_str(raw).unwrap();
            proto::profile::verify_picture_deletion(&blob, &a.did)
                .unwrap()
                .claim
                .picture_hash
        })
        .collect();
    assert!(hashes.contains(&old) && hashes.contains(&new));
    wait("removed picture bytes reclaimed on both devices", || {
        !b.node.profile_blob_present_for_test(new.clone()).unwrap()
            && !a.node.profile_blob_present_for_test(new.clone()).unwrap()
            && !a.node.profile_blob_present_for_test(old.clone()).unwrap()
    });
    assert!(b.node.profile_picture(a.did.clone()).unwrap().is_none());
}

#[test]
fn cached_contact_profile_and_picture_sync_to_a_sibling_while_owner_is_offline() {
    let a = peer("cache-a");
    let b = peer("cache-b");
    let mut sibling = peer("cache-sibling");
    a.node.set_profile_picture(png(190)).unwrap();
    a.node
        .set_profile_field("example:extra".into(), Some("{\"color\":\"green\"}".into()))
        .unwrap();
    a.node.start().unwrap();
    b.node.start().unwrap();
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    wait("cached picture", || {
        b.node
            .account_profile(a.did.clone())
            .ok()
            .flatten()
            .is_some_and(|p| p.picture_ready)
    });
    a.node.stop();
    sibling.node.begin_new_account().unwrap();
    sibling.node.remove_account(sibling.did.clone()).unwrap();
    sibling
        .node
        .restore_identity(b.phrase.clone(), "sibling".into(), "pass".into())
        .unwrap();
    sibling.did = b.did.clone();
    b.node
        .add_own_device_for_test(sibling.node.own_attestation_for_test().unwrap())
        .unwrap();
    sibling
        .node
        .add_own_device_for_test(b.node.own_attestation_for_test().unwrap())
        .unwrap();
    sibling.node.start().unwrap();
    wait("contact synced", || {
        sibling.node.contacts().iter().any(|c| c.did == a.did)
    });
    wait("cached contact picture from sibling", || {
        sibling
            .node
            .account_profile(a.did.clone())
            .ok()
            .flatten()
            .is_some_and(|p| p.picture_ready)
    });
    assert_eq!(
        fields(&sibling.node, &a.did)["example:extra"],
        serde_json::json!({"color":"green"})
    );
    assert_eq!(
        sibling.node.profile_picture(a.did.clone()).unwrap(),
        b.node.profile_picture(a.did.clone()).unwrap()
    );
}

#[test]
fn concurrent_old_picture_consent_keeps_the_selected_picture_and_converges() {
    let a = peer("consent-current-a");
    let mut b = peer("consent-current-b");
    b.node.begin_new_account().unwrap();
    b.node.remove_account(b.did.clone()).unwrap();
    b.node
        .restore_identity(a.phrase.clone(), "sibling".into(), "pass".into())
        .unwrap();
    b.did = a.did.clone();
    for i in 0..8 {
        a.node
            .set_profile_field("clock".into(), Some(i.to_string()))
            .unwrap();
    }
    a.node.set_profile_picture(png(230)).unwrap();
    let selected = a
        .node
        .account_profile(a.did.clone())
        .unwrap()
        .unwrap()
        .picture_hash
        .unwrap();
    let bytes = a.node.profile_picture(a.did.clone()).unwrap().unwrap();
    b.node.set_profile_picture(png(240)).unwrap();
    b.node
        .request_profile_picture_deletion(selected.clone())
        .unwrap();
    assert!(
        a.node
            .request_profile_picture_deletion(selected.clone())
            .is_err(),
        "local API still refuses current-picture requests"
    );
    a.node
        .add_own_device_for_test(b.node.own_attestation_for_test().unwrap())
        .unwrap();
    b.node
        .add_own_device_for_test(a.node.own_attestation_for_test().unwrap())
        .unwrap();
    a.node.start().unwrap();
    b.node.start().unwrap();
    wait("current picture and deferred consent converge", || {
        b.node
            .account_profile(a.did.clone())
            .ok()
            .flatten()
            .is_some_and(|p| p.picture_hash.as_ref() == Some(&selected) && p.picture_ready)
            && a.node
                .picture_deletion_permits(a.did.clone())
                .unwrap()
                .len()
                == 1
    });
    assert_eq!(fields(&a.node, &a.did), fields(&b.node, &a.did));
    assert_eq!(
        b.node.profile_picture(a.did.clone()).unwrap().unwrap(),
        bytes
    );
    // A delivered old-cache consent cannot collect the concurrently selected current image.
    std::thread::sleep(Duration::from_secs(32));
    assert_eq!(
        a.node.profile_picture(a.did.clone()).unwrap().unwrap(),
        bytes
    );
    assert_eq!(
        b.node.profile_picture(a.did.clone()).unwrap().unwrap(),
        bytes
    );
}

#[test]
fn blocked_contacts_cannot_read_profile_blobs_even_with_the_hash() {
    let a = peer("block-a");
    let b = peer("block-b");
    a.node.set_profile_picture(png(210)).unwrap();
    a.node.start().unwrap();
    b.node.start().unwrap();
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    wait("initial profile", || {
        b.node
            .account_profile(a.did.clone())
            .ok()
            .flatten()
            .is_some_and(|p| p.picture_ready)
    });
    let key = b.node.device_key_for_test().unwrap();
    let old = a
        .node
        .account_profile(a.did.clone())
        .unwrap()
        .unwrap()
        .picture_hash
        .unwrap();
    assert!(
        a.node
            .profile_blob_allowed_for_test(key.clone(), old.clone())
            .unwrap()
    );
    a.node.block(b.did.clone()).unwrap();
    assert!(
        !a.node
            .profile_blob_allowed_for_test(key.clone(), old)
            .unwrap()
    );
    a.node.set_profile_picture(png(220)).unwrap();
    let new = a
        .node
        .account_profile(a.did.clone())
        .unwrap()
        .unwrap()
        .picture_hash
        .unwrap();
    assert!(!a.node.profile_blob_allowed_for_test(key, new).unwrap());
}
