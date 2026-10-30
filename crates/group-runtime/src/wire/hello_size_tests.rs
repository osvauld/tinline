//! How big a member's `Hello` gets, against `HELLO_FRAME` (security review 2026-10-29).
//! A hello carries the meta doc's version vector and one per day doc of the last `HELLO_DAYS`
//! days; each grows by one entry per device that wrote in that doc.

use loro::VersionVector;

use super::*;
use crate::record::endpoint_auth;

/// A version vector with `writers` random peers, each with a counter of `ops`.
fn vv(writers: u64, ops: i32, salt: u64) -> Vec<u8> {
    let mut v = VersionVector::new();
    for w in 0..writers {
        // Loro peer ids are random u64s: spread them over the whole range.
        let peer = (w ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) | (1 << 63);
        v.insert(peer, ops);
    }
    v.encode()
}

fn hello_bytes(writers: u64) -> usize {
    let (account, _) = identity::generate();
    let auth = endpoint_auth(&account, [7; 32], [9; 32]);
    let days = (0..HELLO_DAYS as u64)
        .map(|d| (format!("2026-10-{:02}", 20 + d), vv(writers, 50_000, d + 1)))
        .collect();
    let hello = GroupMsg::Hello {
        auth,
        group: "g".repeat(64),
        // The meta doc: every member that ever joined or was granted something writes there.
        vv: vv(writers, 50_000, 0),
        days,
        invite: None,
        hosts: Vec::new(),
        relay: Some("https://aps1-1.relay.n0.iroh.link./".into()),
        presence: true,
    };
    cryptography::codec::to_bytes(&hello).unwrap().len()
}

#[test]
fn hello_size_by_writers() {
    let mut rows = Vec::new();
    for writers in [1u64, 10, 100, 500, 1_000, 5_000, 10_000] {
        let n = hello_bytes(writers);
        rows.push((writers, n));
        eprintln!(
            "hello with {writers:>6} writers per doc: {n:>9} bytes ({:.1} KiB)",
            n as f64 / 1024.0
        );
    }
    // The cap must hold a group whose every doc had 1,000 writers.
    let at_1000 = rows.iter().find(|r| r.0 == 1_000).unwrap().1;
    assert!(
        at_1000 < HELLO_FRAME,
        "1,000 writers: {at_1000} bytes over HELLO_FRAME"
    );
}
