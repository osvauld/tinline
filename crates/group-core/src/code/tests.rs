use super::*;

// KIND: unit (G3 stage 2) — the link is short and round-trips, for every kind of relay hint.
#[test]
fn links_are_short_and_round_trip() {
    let cases = [
        Some("https://use1-1.relay.n0.iroh.link./".to_string()),
        Some("https://aps1-1.relay.n0.iroh.link/".to_string()),
        Some("https://xyz9-2.relay.n0.iroh.link./".to_string()),
        Some("http://127.0.0.1:3340/".to_string()),
        None,
    ];
    for relay in cases {
        let sc = ShortCode { endpoint: [0xab; 32], relay: relay.clone(), code: [0xff; CODE_LEN] };
        let link = sc.to_link();
        let back = ShortCode::parse(&link).unwrap();
        assert_eq!(back.endpoint, sc.endpoint);
        assert_eq!(back.code, sc.code);
        assert_eq!(back.relay.as_deref().and_then(relay_host), relay.as_deref().and_then(relay_host), "{link}");
        assert_eq!(back.relay.is_some(), relay.is_some());
        if relay.as_deref().is_some_and(|r| r.contains("use1-1") || r.contains("aps1-1")) || relay.is_none() {
            assert!(link.len() <= 80, "{} chars: {link}", link.len());
        }
        // The bare code and an upper-case scheme work too.
        assert_eq!(ShortCode::parse(link.strip_prefix(LINK_PREFIX).unwrap()).unwrap(), back);
        assert_eq!(ShortCode::parse(&link.replace("tinline://g/", "TINLINE://G/")).unwrap(), back);
    }
    let max = ShortCode { endpoint: [0xff; 32], relay: Some("https://euc1-1.relay.n0.iroh.link./".into()), code: [0xff; CODE_LEN] }.to_link();
    assert!(max.len() <= 80, "{}", max.len());
    assert!(ShortCode::parse("tlgroup1:AAAA").is_none());
    assert!(ShortCode::parse("tinline://g/").is_none());
    assert!(ShortCode::parse("tinline://g/0OIl").is_none());
}

// KIND: unit (hardening G-01/G-17) — hostile text never panics: a multi-byte char straddling the
// link prefix, and ids too short (or not hex) to slice.
#[test]
fn hostile_links_and_ids_do_not_panic() {
    for t in ["tinline://gé", "tinline://é", "TINLINE://G/ü", "ééééééé", "tinline://g", ""] {
        assert!(ShortCode::parse(t).is_none(), "{t}");
    }
    assert_eq!(crate::short("abc"), "abc");
    assert_eq!(crate::short("é"), "é");
    assert_eq!(crate::short("0123456789"), "01234567");
    assert!(!crate::valid_gid("abc"));
    assert!(!crate::valid_gid(&"é".repeat(32)));
    assert!(!crate::valid_gid(&"A".repeat(64)));
    assert!(crate::valid_gid(&"0a".repeat(32)));
}

fn row(gid: &str, kind: CodeKind, expires: u64) -> CodeRow {
    CodeRow { gid: gid.into(), kind, invite: None, expires, by: "did:key:o".into() }
}

// KIND: unit (public group §2) — an open link's code never expires: pruning keeps it (and its
// issued row), lookups return it, and it counts as one of the group's codes; an expired invite
// code next to it is swept.
#[test]
fn open_code_never_expires_and_counts() {
    let s = GroupStore::in_memory("alpha beta");
    let now = crdt::now_ms();
    s.put_code(&[1; CODE_LEN], &row("g", CodeKind::Open, NO_EXPIRY)).unwrap();
    s.put_issued(&[1; CODE_LEN], &IssuedRow { gid: "g".into(), host_did: "h".into(), host_dev: "00".into(), expires: NO_EXPIRY }).unwrap();
    s.put_code(&[2; CODE_LEN], &row("g", CodeKind::Device, now - 1)).unwrap();
    s.put_code(&[3; CODE_LEN], &row("g", CodeKind::Device, now + 60_000)).unwrap();
    s.put_code(&[4; CODE_LEN], &row("other", CodeKind::Open, NO_EXPIRY)).unwrap();
    assert_eq!(s.prune_codes(Some("g")).unwrap(), 2, "the open code and the live device code");
    assert!(s.code(&[2; CODE_LEN]).unwrap().is_none(), "expired: swept");
    let open = s.code(&[1; CODE_LEN]).unwrap().expect("the open code stays");
    assert_eq!(open.kind, CodeKind::Open);
    assert!(open.invite.is_none());
    assert!(s.issued(&[1; CODE_LEN]).unwrap().is_some(), "its issued row stays");
    // Dropping it (closing the group) forgets both rows.
    s.drop_code(&[1; CODE_LEN]).unwrap();
    assert!(s.code(&[1; CODE_LEN]).unwrap().is_none());
    assert!(s.issued(&[1; CODE_LEN]).unwrap().is_none());
    assert_eq!(s.prune_codes(Some("g")).unwrap(), 1);
}
