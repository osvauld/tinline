use super::*;

fn doc(name: &str, vv: Vec<u8>) -> (String, Vec<u8>) {
    (name.into(), vv)
}

fn empty() -> Vec<u8> {
    VersionVector::default().encode()
}

#[test]
fn only_the_exact_marker_negotiates_dm_receipts() {
    assert!(supported(&[doc(CAPABILITY, empty())]));
    // An older own device: account docs, shards and the group markers, no dm-receipt marker.
    let older = [
        doc(DOC_NAME, empty()),
        doc(group_receipts::CAPABILITY, empty()),
        doc(&format!("dm/{}/2026-10-28", "a".repeat(64)), empty()),
    ];
    assert!(!supported(&older));
    assert!(
        !supported(&[doc(CAPABILITY, vec![255])]),
        "a malformed marker is no marker"
    );
}

#[test]
fn an_older_peer_is_never_sent_a_receipt_frame() {
    let mut s = DmReceipts::default();
    assert!(
        !s.hello_due() && !s.may_push(),
        "nothing before (or without) negotiation"
    );
    s.sent_hello = true;
    assert!(!s.may_push(), "not negotiated: still nothing");
    let mut s = DmReceipts {
        enabled: true,
        ..DmReceipts::default()
    };
    assert!(
        s.hello_due() && !s.may_push(),
        "hello first, pushes after it"
    );
    s.sent_hello = true;
    assert!(!s.hello_due() && s.may_push());
}

#[test]
fn the_dirty_mark_names_the_ledger_day() {
    let pair = "a".repeat(64);
    let mark = chat_runtime::chat::engine::own_receipts::mark(&pair, "2026-10-28");
    assert_eq!(
        marked(&mark),
        Some(format!("dm/{pair}/2026-10-28").as_str())
    );
    assert!(doc_ok(marked(&mark).unwrap()).is_ok());
    assert_eq!(
        marked(&format!("dm/{pair}/2026-10-28")),
        None,
        "a shard mark is not a receipt mark"
    );
    assert!(doc_ok("group/x/meta").is_err());
}
