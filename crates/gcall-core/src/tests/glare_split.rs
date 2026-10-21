//! CallStart glare and split ordering on the board.

use super::world::*;

// GC-S05. Two members start a call at once on different hosts; the relay is held so each
// call gets participants. After the relay the lower BLAKE3(call_id) wins everywhere, the
// loser ends `superseded`, and its participants (the loser host's own user included) move.
#[test]
fn gcall_s05_concurrent_starts_converge_and_the_loser_ends_superseded() {
    let mut s = World::new();
    s.hold_relay = true;
    let c1 = s.start_call(H1, OLGA, vec![OLGA]).unwrap();
    let c2 = s.start_call(BOB, BOB, vec![OLGA]).unwrap();
    // Starters get their host's CallStart directly (the CallHostRequest answer).
    let (b1, b2) = (s.held[0].clone(), s.held[1].clone());
    for m in [OLGA, A1] {
        s.d[m].board.accept_start(&b1, &s.auth).unwrap();
        s.join(m).unwrap();
    }
    s.d[CAROL].board.accept_start(&b2, &s.auth).unwrap();
    s.join(CAROL).unwrap();
    s.join(BOB).unwrap(); // BOB's own user, through ConnId::LOCAL
    assert_eq!((s.roster(H1).len(), s.roster(BOB).len()), (2, 2));
    s.flush_relay();
    let (winner, win, lose) = if board::call_hash(&c1) < board::call_hash(&c2) { (c1, H1, BOB) } else { (c2, BOB, H1) };
    assert!(!s.hosts.contains_key(&lose), "loser stopped hosting");
    assert!(s.host_ended.contains(&(lose, EndReason::Superseded, Some(s.href(win)))));
    for m in [OLGA, A1, BOB, CAROL, HP, H1] {
        assert_eq!(s.d[m].board.shown().unwrap().call_id, winner, "shown at {m}");
    }
    for m in [OLGA, A1, BOB, CAROL] {
        assert!(s.joined(m), "{m} joined");
        assert_eq!(s.clients[&m].call_start().call_id, winner);
        assert_eq!(s.host_of(m).0, win);
    }
    assert_eq!(s.roster(win).len(), 4);
    assert_eq!(s.ends.iter().filter(|(_, e, _)| *e == ClientEnd::Superseded).count(), 2);
}

// §5 split after redial when neither gen-2 host is in the previous backups list: the index rule
// cannot decide, so the lower `rank::tie_hash` (§2.3) wins, whichever CallStart arrives first.
#[test]
fn gcall_split_outside_the_previous_backups_falls_back_to_rank_tie_hash() {
    let s = World::new();
    let cid = w::new_call_id();
    let start = |host: usize, host_gen: u32, backups: Vec<usize>| {
        let cs = w::CallStart {
            gid: gid(),
            call_id: cid.clone(),
            host_gen,
            host_ref: s.href(host),
            starter_did: s.did(A1),
            started_ms: T0,
            k: 3,
            max_participants: 8,
            backups: backups.iter().map(|b| s.href(*b)).collect(),
            meta_frontier: "frontier".into(),
        };
        Signed::sign(&cs, &s.d[host].id).unwrap()
    };
    let gen1 = start(H1, 1, vec![OLGA]);
    let (b, p) = (start(BOB, 2, vec![]), start(HP, 2, vec![]));
    assert!(!gen1.open().unwrap().backups.contains(&s.href(BOB)));
    assert!(!gen1.open().unwrap().backups.contains(&s.href(HP)));
    let want = if rank::tie_hash(&cid, &s.href(BOB)) < rank::tie_hash(&cid, &s.href(HP)) {
        BOB
    } else {
        HP
    };
    for (first, second) in [(&b, &p), (&p, &b)] {
        let mut board = CallBoard::new(&gid(), None);
        board.accept_start(&gen1, &s.auth).unwrap();
        board.accept_start(first, &s.auth).unwrap();
        let _ = board.accept_start(second, &s.auth);
        assert_eq!(board.start(&cid).unwrap().host_ref, s.href(want), "lower rank::tie_hash wins in either arrival order");
    }
}
