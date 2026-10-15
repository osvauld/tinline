//! Production wire characterization, with real Ed25519 signatures.
//! Larger-roster acceptance is an explicit candidate gate, NOT a chosen schema/default.
use identity::Identity;
use proto::Signed;
use proto::gcall::{self as w, GroupCallMsg, Roster, RosterEntry};

/// A roster signed without the sign-time shape check: what a peer could put on the wire.
fn raw_roster(host: &Identity, r: &Roster) -> Signed<Roster> {
    let body = cryptography::codec::to_bytes(r).unwrap();
    let sig = host.sign(&cryptography::sign::input(
        cryptography::domains::GCALL_ROSTER,
        &[&body],
    ));
    Signed::from_parts(body, sig)
}

#[path = "support/fixtures.rs"]
mod fixtures;
use fixtures::owner as identity;

fn roster(n: usize) -> (Identity, Roster) {
    let host = identity(200);
    let gid = "ab".repeat(32);
    let call_id = w::new_call_id();
    let entries = (0..n)
        .map(|i| {
            let member = identity(i as u8 + 1);
            let (join, blob) =
                w::sign_join(&member, &gid, &call_id, 1, 1_800_000_000_000 + i as u64).unwrap();
            RosterEntry::new(i as u8, &join, &blob)
        })
        .collect();
    let r = Roster {
        gid,
        call_id,
        host_gen: 1,
        host_did: host.did().into(),
        epoch: u32::MAX,
        entries,
    };
    (host, r)
}

#[test]
fn current_twelve_member_roster_roundtrips_with_verified_joins() {
    let (host, r) = roster(12);
    let signed = Signed::sign(&r, &host).unwrap();
    assert_eq!(signed.open().unwrap(), r);
    let frame = w::encode_control(&GroupCallMsg::Roster { roster: signed }).unwrap();
    assert!(frame.len() - 4 <= w::MAX_CONTROL);
    let (msg, consumed) = w::decode_control(&frame).unwrap().unwrap();
    assert_eq!(consumed, frame.len());
    let GroupCallMsg::Roster { roster } = msg else {
        panic!("wrong frame")
    };
    assert_eq!(roster.open().unwrap(), r);
}

#[test]
fn larger_rosters_hit_both_semantic_and_transport_bounds_today() {
    for n in [16, 24, 32] {
        let (host, r) = roster(n);
        assert!(
            Signed::sign(&r, &host).is_err(),
            "current bounds refuse to sign N={n}"
        );
        let signed = raw_roster(&host, &r);
        assert!(
            signed.open().is_err(),
            "current semantic bound rejects N={n}"
        );
        let frame = w::encode_control(&GroupCallMsg::Roster { roster: signed });
        if n == 16 {
            // Compact entries (postcard body, 64-byte sigs) now let 16 fit the 4KiB frame; only
            // the participant bound stops it.
            assert!(frame.is_ok(), "N=16 fits the 4KiB frame");
        } else {
            assert!(
                matches!(frame, Err(proto::Error::FrameTooLarge)),
                "current 4KiB frame rejects N={n}"
            );
        }
    }
}

#[test]
fn oversized_prefix_rejected_before_receiving_a_body() {
    let prefix = ((w::MAX_CONTROL + 1) as u32).to_be_bytes();
    assert!(matches!(
        w::decode_control(&prefix),
        Err(proto::Error::FrameTooLarge)
    ));
}

#[test]
#[ignore = "candidate acceptance: larger signed-roster transport needs a bounded protocol decision"]
fn candidate_sixteen_member_roster_can_be_sent_and_verified() {
    let (host, r) = roster(16);
    let signed = raw_roster(&host, &r);
    // Deliberately fails on current production; run explicitly as the red acceptance gate.
    let frame = w::encode_control(&GroupCallMsg::Roster { roster: signed })
        .expect("candidate goal: convey a 16-member roster with real signatures");
    let (msg, _) = w::decode_control(&frame).unwrap().unwrap();
    let GroupCallMsg::Roster { roster } = msg else {
        panic!("wrong frame")
    };
    assert_eq!(roster.open().unwrap(), r);
}
