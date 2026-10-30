use super::*;

fn doc(name: &str) -> (String, Vec<u8>) {
    (name.into(), VersionVector::default().encode())
}

#[test]
fn legacy_peer_never_negotiates_group_contents_from_doc_names_alone() {
    assert!(!supported(&[doc(&format!(
        "group/{}/meta",
        "a".repeat(64)
    ))]));
    assert!(supported(&[doc(contents::CAPABILITY)]));
    assert!(!supported(&[(contents::CAPABILITY.into(), vec![255])]));
}

#[test]
fn shared_catalog_is_bounded_meta_first_and_includes_both_domains() {
    let gid = "a".repeat(64);
    let group_meta = doc(&format!("group/{gid}/meta"));
    let group_day = doc(&format!("group/{gid}/2026-01-01"));
    let dm = doc(&format!("dm/{gid}/2026-01-01"));
    let result = catalog(
        vec![dm.clone(); MAX_OWN_DOCS],
        vec![group_meta.clone(), group_day.clone()],
    );
    // Test the saturation case separately from two-domain order.
    assert_eq!(result.len(), MAX_OWN_DOCS - MARKERS);
    let mixed = catalog(
        vec![dm.clone()],
        vec![group_meta.clone(), group_day.clone()],
    );
    assert_eq!(mixed, vec![group_meta, dm, group_day]);
}

#[test]
fn capability_plus_catalog_remains_compatible_with_legacy_hello_count_bound() {
    let data = catalog(
        (0..MAX_OWN_DOCS).map(|i| doc(&format!("dm:{i}"))).collect(),
        Vec::new(),
    );
    // Five markers (contents, history, receipts, own wake, DM receipts) and four fixed account docs.
    assert_eq!(data.len() + MARKERS + 4, MAX_OWN_DOCS + 4);
}

#[test]
fn advisory_cache_is_bounded_and_does_not_evict_account_ack_state() {
    let mut peer = PeerChat::default();
    for i in 0..MAX_OWN_DOCS + 5 {
        remember(&mut peer, format!("doc:{i}"), VersionVector::default());
        assert!(peer.docs.len() <= MAX_OWN_DOCS);
    }
    assert!(peer.docs.contains_key(&format!("doc:{}", MAX_OWN_DOCS + 4)));
    assert!(peer.acks.is_empty());
}
