//! Production wire characterization, with real Ed25519 signatures.
//! Larger-roster acceptance is an explicit candidate gate, NOT a chosen schema/default.
use identity::Identity;
use proto::gcall::{self as w, GroupCallMsg, Roster, RosterEntry};

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
                w::sign_join(&member, &gid, &call_id, 1, 1_800_000_000_000 + i as u64);
            RosterEntry::new(i as u8, &join, &blob)
        })
        .collect();
    let r = Roster {
        v: w::GCALL_VERSION,
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
    let signed = w::sign_roster(&host, r.clone());
    assert_eq!(w::open_roster(&signed).unwrap(), r);
    let frame = w::encode_control(&GroupCallMsg::Roster { roster: signed }).unwrap();
    assert!(frame.len() - 4 <= w::MAX_CONTROL);
    eprintln!("signed 12-member roster body: {} bytes", frame.len() - 4);
    let (msg, consumed) = w::decode_control(&frame).unwrap().unwrap();
    assert_eq!(consumed, frame.len());
    let GroupCallMsg::Roster { roster } = msg else {
        panic!("wrong frame")
    };
    assert_eq!(w::open_roster(&roster).unwrap(), r);
}

#[test]
fn larger_rosters_hit_both_semantic_and_transport_bounds_today() {
    for n in [16, 24, 32] {
        let (host, r) = roster(n);
        let signed = w::sign_roster(&host, r);
        assert!(
            w::open_roster(&signed).is_err(),
            "current semantic bound rejects N={n}"
        );
        assert!(
            matches!(
                w::encode_control(&GroupCallMsg::Roster { roster: signed }),
                Err(proto::Error::FrameTooLarge)
            ),
            "current 4KiB frame rejects N={n}"
        );
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
    let signed = w::sign_roster(&host, r.clone());
    // Deliberately fails on current production; run explicitly as the red acceptance gate.
    let frame = w::encode_control(&GroupCallMsg::Roster { roster: signed })
        .expect("candidate goal: convey a 16-member roster with real signatures");
    let (msg, _) = w::decode_control(&frame).unwrap().unwrap();
    let GroupCallMsg::Roster { roster } = msg else {
        panic!("wrong frame")
    };
    assert_eq!(w::open_roster(&roster).unwrap(), r);
}
