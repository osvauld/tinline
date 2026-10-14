//! Join admission and the same-DID rule.

use super::world::*;

// GC-S01. H refuses each bad Join on a populated call; H's state is unchanged and the Roster
// too. Positive control: a valid Join is accepted.
#[test]
fn gcall_s01_bad_joins_are_refused_and_leave_host_state_unchanged() {
    let (mut s, cid) = base();
    for m in [A1, BOB] {
        s.join(m).unwrap();
    }
    s.advance(3_000);
    let carol = s.d[CAROL].id.clone();
    let (cep, mep) = (s.d[CAROL].ep, s.d[MALLORY].ep);
    let j = |id: &Identity, gid: &str, cid: &str, g: u32, t: u64| w::sign_join(id, gid, cid, g, t).1;
    let now = s.now;
    let mut tampered = j(&carol, &gid(), &cid, 1, now);
    {
        use base64::Engine;
        let e = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let mut v: serde_json::Value = serde_json::from_slice(&e.decode(&tampered.payload).unwrap()).unwrap();
        v["issued_ms"] = (now + 1).into();
        tampered.payload = e.encode(serde_json::to_vec(&v).unwrap());
    }
    let a1_join = s.up_ctl.iter().find_map(|(m, msg)| match msg {
        GroupCallMsg::Join { join, .. } if *m == A1 => Some(join.clone()),
        _ => None,
    });
    let bob_join = j(&s.d[BOB].id.clone(), &gid(), &cid, 1, now);
    s.auth.endpoints.entry(s.did(MALLORY)).or_default().insert(mep);
    let cases: Vec<(&str, [u8; 32], SignedBlob, Refusal)> = vec![
        ("non-member", mep, j(&s.d[MALLORY].id.clone(), &gid(), &cid, 1, now), Refusal::NotMember),
        ("tampered body", cep, tampered, Refusal::BadSignature),
        ("wrong gid", cep, j(&carol, &"e".repeat(64), &cid, 1, now), Refusal::WrongGroup),
        ("wrong call_id", cep, j(&carol, &gid(), &w::new_call_id(), 1, now), Refusal::WrongCall),
        ("future host_gen", cep, j(&carol, &gid(), &cid, 2, now), Refusal::StaleGen),
        ("replayed accepted Join", s.d[A1].ep, a1_join.unwrap(), Refusal::Replay),
        ("bob's Join from carol's connection", cep, bob_join, Refusal::EndpointMismatch),
    ];
    let roster = s.roster(H1);
    for (case, ep, blob, want) in cases {
        let before = s.hosts[&H1].snapshot();
        assert_eq!(s.raw_join(H1, ep, blob).unwrap_err(), want, "{case}");
        assert_eq!(s.hosts[&H1].snapshot(), before, "{case}");
        assert_eq!(s.roster(H1), roster, "{case}");
    }
    // Oversize control never reaches the engine.
    let mut huge = ((w::MAX_CONTROL + 1) as u32).to_be_bytes().to_vec();
    huge.extend([b' '; 16]);
    assert_eq!(w::decode_control(&huge).map_err(Refusal::from), Err(Refusal::Oversize));
    // Positive control, then a removed member (U12: refusal is immediate, and H ejects her).
    s.join(CAROL).unwrap();
    assert_eq!(s.roster(H1).len(), 3);
    s.auth.removed.insert(s.did(CAROL));
    s.step();
    assert!(s.ended(CAROL, ClientEnd::Removed));
    assert_eq!(s.roster(H1).len(), 2);
    s.advance(3_000);
    let before = s.hosts[&H1].snapshot();
    assert_eq!(s.raw_join(H1, cep, j(&carol, &gid(), &cid, 1, s.now)).unwrap_err(), Refusal::NotMember);
    assert_eq!(s.hosts[&H1].snapshot(), before);
}

// GC-S17. One DID on two linked devices: the newer Join replaces the older (one slot per
// DID); the older device is closed "answered elsewhere"; replays and older Joins are refused.
#[test]
fn gcall_s17_same_did_on_two_devices_newer_join_replaces_older() {
    let (mut s, cid) = base();
    s.join(OLGA).unwrap();
    s.join(A1).unwrap();
    let alice = s.did(A1);
    let a1_join = s.up_ctl.iter().rev().find_map(|(m, msg)| match msg {
        GroupCallMsg::Join { join, .. } if *m == A1 => Some(join.clone()),
        _ => None,
    });
    let a1_conn = s.route[&A1].1;
    let slot = s.hosts[&H1].slot_of(&alice).unwrap();
    s.advance(JOIN_MIN_GAP_MS);
    s.join(A2).unwrap();
    let r = s.roster(H1);
    assert_eq!((r.len(), r.values().filter(|d| **d == alice).count()), (2, 1), "one slot per DID");
    assert_eq!(s.hosts[&H1].slot_of(&alice), Some(slot));
    assert_eq!(s.hosts[&H1].conn_of(&alice), Some(s.route[&A2].1));
    assert!(s.ended(A1, ClientEnd::AnsweredElsewhere));
    assert!(s.joined(A2));
    let e = s.hosts.get_mut(&H1).unwrap();
    assert_eq!(e.on_datagram(a1_conn, &w::up_audio(&audio_dgram(1, 10)), s.now), Err(Refusal::NotParticipant));
    s.advance(JOIN_MIN_GAP_MS);
    let ep1 = s.d[A1].ep;
    let older = w::sign_join(&s.d[A1].id.clone(), &gid(), &cid, 1, T0 - 1).1;
    for (case, blob, want) in [("replay", a1_join.unwrap(), Refusal::Replay), ("older issued_ms", older, Refusal::Stale)] {
        let before = s.hosts[&H1].snapshot();
        assert_eq!(s.raw_join(H1, ep1, blob), Err(want), "{case}");
        assert_eq!(s.hosts[&H1].snapshot(), before, "{case}");
    }
    s.join(A1).unwrap();
    assert!(s.joined(A1));
    assert!(s.ended(A2, ClientEnd::AnsweredElsewhere));
    assert_eq!(s.roster(H1).len(), 2);
}
