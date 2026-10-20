use super::*;
use crate::store::{ContactDevice, StoredContact};

fn held(blob: &SignedBlob, to: &str) -> HeldNotice {
    let n = proto::verify_block_notice(blob, to).unwrap();
    HeldNotice { notice: blob.clone(), name: "a".into(), blocked: n.blocked, at: n.at }
}

#[test]
fn intake_takes_newer_notices_only() {
    let (a, _) = identity::generate();
    let (b, _) = identity::generate();
    let (x, _) = identity::generate();
    let block = proto::sign_block_notice(&a, b.did(), true, 100);
    // Fresh block: taken.
    assert!(admit(b.did(), &block, None).is_ok_and(|c| c.blocked && c.from == a.did()));
    // The same notice again (a replay, or every later refusal): stale, nothing changes.
    assert_eq!(admit(b.did(), &block, Some(&held(&block, b.did()))), Err(Refusal::Stale));
    // Unblock, newer: taken. The old block replayed after it: stale.
    let unblock = sign_next(&a, b.did(), false, Some(&block));
    let h = held(&unblock, b.did());
    assert!(h.at > 100 && !h.blocked);
    assert!(admit(b.did(), &unblock, Some(&held(&block, b.did()))).is_ok());
    assert_eq!(admit(b.did(), &block, Some(&h)), Err(Refusal::Stale));
    // An unblock from someone whose block we never held lifts nothing.
    assert_eq!(admit(b.did(), &unblock, None), Err(Refusal::NothingToLift));
    // Forged (X signs as A), or addressed to X: invalid.
    let forged = proto::forge_block_notice_for_test(&x, a.did(), b.did(), true, 200);
    assert_eq!(admit(b.did(), &forged, None), Err(Refusal::Invalid));
    assert_eq!(admit(x.did(), &block, None), Err(Refusal::Invalid));
}

#[test]
fn the_deletion_request_drops_only_an_older_contact_entry() {
    let (a, _) = identity::generate();
    let (b, _) = identity::generate();
    let grant = proto::issue_grant(&a, b.did(), 1, 10);
    let contact = |did: &str, added_at: u64| StoredContact {
        did: did.into(),
        name: "n".into(),
        devices: vec![ContactDevice { device: [7; 32], relay: None }],
        device_list: None,
        grant_from_them: grant.clone(),
        call_grant_from_them: None,
        calls_allowed: true,
        added_at,
        alias: None,
        verified: true,
    };
    let mut st = State { contacts: vec![contact(a.did(), 50), contact("did:other", 50)], ..Default::default() };
    assert!(apply_block_deletion_request(&mut st, a.did(), 60_000));
    assert_eq!(st.contacts.len(), 1);
    assert_eq!(st.contacts[0].did, "did:other");
    // Re-added after the notice was signed: kept.
    let mut st = State { contacts: vec![contact(a.did(), 61)], ..Default::default() };
    assert!(!apply_block_deletion_request(&mut st, a.did(), 60_000));
}

#[test]
fn stranger_notices_are_capped_oldest_first_and_contacts_kept() {
    let (a, _) = identity::generate();
    let blob = proto::sign_block_notice(&a, "did:me", true, 1);
    let h = |at: u64| HeldNotice { notice: blob.clone(), name: String::new(), blocked: true, at };
    let mut st = State::default();
    st.contacts.push(StoredContact {
        did: "did:friend".into(),
        name: "f".into(),
        devices: vec![],
        device_list: None,
        grant_from_them: proto::issue_grant(&a, "did:me", 1, 10),
        call_grant_from_them: None,
        calls_allowed: true,
        added_at: 0,
        alias: None,
        verified: true,
    });
    st.notices.insert("did:friend".into(), h(0)); // oldest of all, but a contact
    for i in 0..MAX_STRANGER_NOTICES as u64 + 10 {
        st.notices.insert(format!("did:x{i}"), h(i + 1));
        cap_notices(&mut st);
    }
    assert_eq!(st.notices.len(), MAX_STRANGER_NOTICES + 1);
    assert!(st.notices.contains_key("did:friend"));
    assert!((0..10).all(|i| !st.notices.contains_key(&format!("did:x{i}"))));
    assert!(st.notices.contains_key(&format!("did:x{}", MAX_STRANGER_NOTICES + 9)));
}
