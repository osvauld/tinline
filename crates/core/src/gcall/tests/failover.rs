//! Stale control after redial, host loss and failover, permit revocation.

use super::world::*;

// GC-S03. After a redial to gen 2, gen-1 control from the old host (End, CallStart replayed
// or freshly signed, Roster) is ignored by every board and client, state unchanged.
#[test]
fn gcall_s03_stale_end_callstart_and_roster_after_redial_are_ignored() {
    let (mut s, cid) = base();
    for m in [A1, CAROL] {
        s.join(m).unwrap();
    }
    let gen1 = s.hosts[&H1].start_blob().clone();
    s.d[H1].online = false;
    s.advance(4_000);
    assert_eq!(s.hosts[&BOB].call_start().host_gen, 2);
    let h1 = s.d[H1].id.clone();
    let end = Signed::sign(
        &w::End {
            gid: gid(),
            call_id: cid.clone(),
            host_gen: 1,
            host_did: s.did(H1),
            reason: EndReason::Ended,
            next_host: None,
        },
        &h1,
    )
    .unwrap();
    let mut fresh = s.d[H1].board.start(&cid).cloned().unwrap();
    fresh.host_gen = 1;
    fresh.host_ref = s.href(H1);
    let fresh = Signed::sign(&fresh, &h1).unwrap();
    let roster = Signed::sign(
        &w::Roster {
            gid: gid(),
            call_id: cid.clone(),
            host_gen: 1,
            host_did: s.did(H1),
            epoch: 99,
            entries: vec![],
        },
        &h1,
    )
    .unwrap();
    for i in [OLGA, A1, BOB, CAROL, HP] {
        let before = s.d[i].board.clone();
        assert_eq!(s.d[i].board.accept_end(&end, &s.auth), Err(Refusal::StaleGen), "end at {i}");
        assert_eq!(s.d[i].board.accept_start(&gen1, &s.auth), Err(Refusal::StaleGen), "replayed start at {i}");
        assert_eq!(s.d[i].board.accept_start(&fresh, &s.auth), Err(Refusal::StaleGen), "fresh gen-1 at {i}");
        assert_eq!(s.d[i].board, before);
    }
    for m in [A1, CAROL] {
        let c = s.clients.get_mut(&m).unwrap();
        let before = (c.state(), c.host_gen(), c.epoch(), c.roster().clone());
        assert_eq!(c.on_control(GroupCallMsg::End { end: end.clone() }, s.now, &s.auth), Err(Refusal::StaleGen));
        assert_eq!(c.on_control(GroupCallMsg::Roster { roster: roster.clone() }, s.now, &s.auth), Err(Refusal::StaleGen));
        assert_eq!((c.state(), c.host_gen(), c.epoch(), c.roster().clone()), before);
        assert_eq!(s.host_of(m), (BOB, 2));
    }
}

// GC-S08. Host lost after 3 missed heartbeats. (a) backups[0] up: redial it at once with
// gen + 1; its CallStart lists the old backups minus itself (G9). (b) backups[0] down: reach
// backups[1] after ANSWER_TIMEOUT_MS. (c) both down: "host lost". (d) split: some members reach
// b0, others b1; after the relay b0 wins and b1's members move.
#[test]
fn gcall_s08_host_drop_redials_backup0_else_backup1_and_split_resolves_to_backup0() {
    let setup = || {
        let (mut s, cid) = base();
        for m in [A1, CAROL, HP] {
            s.join(m).unwrap();
        }
        s.advance(2_000);
        (s, cid)
    };
    let last_hb = |s: &World| s.now - (s.now - T0) % HEARTBEAT_MS;
    // (a)
    let (mut s, cid) = setup();
    let lost = last_hb(&s) + HEARTBEAT_MISSES * HEARTBEAT_MS;
    s.d[H1].online = false;
    s.advance(10_000);
    for m in [A1, CAROL, HP] {
        assert_eq!(s.host_of(m), (BOB, 2), "{m}");
        assert_eq!(s.clients[&m].call_start().call_id, cid);
        let at = s.joins.iter().rev().find(|j| j.0 == m).unwrap().3;
        assert!(at >= lost && at <= lost + FRAME_MS, "{m} redialed at {at}, lost at {lost}");
    }
    assert_eq!(s.roster(BOB).len(), 3);
    assert_eq!(s.hosts[&BOB].call_start().backups, vec![s.href(OLGA)], "G9");
    // (b)
    let (mut s, _) = setup();
    let lost = last_hb(&s) + HEARTBEAT_MISSES * HEARTBEAT_MS;
    s.d[H1].online = false;
    s.d[BOB].online = false; // unreachable: skipped at once
    s.advance(12_000);
    for m in [A1, CAROL, HP] {
        assert_eq!(s.host_of(m), (OLGA, 2));
        let at = s.joins.iter().rev().find(|j| j.0 == m).unwrap().3;
        assert!(at <= lost + FRAME_MS, "an unreachable backup is skipped at once: {at} vs {lost}");
    }
    // (b') BOB is reachable but silent: the redial waits ANSWER_TIMEOUT_MS.
    let (mut s, _) = setup();
    let lost = last_hb(&s) + HEARTBEAT_MISSES * HEARTBEAT_MS;
    s.d[H1].online = false;
    s.d[BOB].board = CallBoard::new(&gid(), None); // cannot take over, never answers
    s.advance(12_000);
    for m in [A1, CAROL, HP] {
        assert_eq!(s.host_of(m), (OLGA, 2));
        let at = s.joins.iter().rev().find(|j| j.0 == m).unwrap().3;
        assert!(at >= lost + ANSWER_TIMEOUT_MS && at <= lost + ANSWER_TIMEOUT_MS + FRAME_MS, "{at}");
    }
    // (c)
    let (mut s, _) = setup();
    for h in [H1, BOB, OLGA] {
        s.d[h].online = false;
    }
    s.advance(12_000);
    for m in [A1, CAROL, HP] {
        assert!(s.ended(m, ClientEnd::HostLost), "{m}");
        assert!(!s.joined(m));
    }
    // (d) split
    let (mut s, cid) = setup();
    s.hold_relay = true;
    s.cut.insert((HP, BOB));
    s.d[H1].online = false;
    s.advance(4_000);
    assert_eq!(s.hosts[&BOB].call_start().host_gen, 2);
    assert_eq!(s.hosts[&OLGA].call_start().host_gen, 2, "b1 started gen 2 too");
    assert_eq!(s.host_of(HP), (OLGA, 2));
    s.cut.clear();
    s.flush_relay();
    assert!(!s.hosts.contains_key(&OLGA), "b1 ended superseded");
    for m in [A1, CAROL, HP] {
        assert_eq!(s.host_of(m), (BOB, 2), "{m}");
        assert!(s.joined(m));
        assert_eq!(s.clients[&m].call_start().call_id, cid);
    }
    assert_eq!(s.roster(BOB).len(), 3);
}

// G10 and permit checks at the client: a Roster listing a non-member, or control from a host
// whose permit was revoked, is refused; a revoked host makes members redial the next backup.
#[test]
fn gcall_client_refuses_rogue_roster_and_redials_when_the_host_permit_is_revoked() {
    let (mut s, cid) = base();
    for m in [A1, CAROL] {
        s.join(m).unwrap();
    }
    let h1 = s.d[H1].id.clone();
    let (j, b) = w::sign_join(&s.d[MALLORY].id.clone(), &gid(), &cid, 1, s.now).unwrap();
    let rogue = Signed::sign(
        &w::Roster {
            gid: gid(),
            call_id: cid.clone(),
            host_gen: 1,
            host_did: s.did(H1),
            epoch: 999,
            entries: vec![w::RosterEntry::new(9, &j, &b)],
        },
        &h1,
    )
    .unwrap();
    let c = s.clients.get_mut(&A1).unwrap();
    assert_eq!(c.on_control(GroupCallMsg::Roster { roster: rogue }, s.now, &s.auth), Err(Refusal::NotMember));
    s.auth.permits.remove(&s.href(H1).permit_digest);
    s.step();
    for m in [A1, CAROL] {
        assert_eq!(s.host_of(m), (BOB, 2), "{m}");
        assert!(s.joined(m));
    }
}
