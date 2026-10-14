//! Floods and phone degrade.

use super::world::*;

// GC-S13. Floods are bounded and other members are unaffected.
#[test]
fn gcall_s13_floods_are_bounded_and_other_members_are_unaffected() {
    let (mut s, cid) = base();
    for m in [OLGA, A1, CAROL] {
        s.join(m).unwrap();
    }
    let a1_conn = s.route[&A1].1;
    let olga = s.did(OLGA);
    let olga_slot = s.hosts[&H1].slot_of(&olga).unwrap();
    let mut big = vec![w::UP_AUDIO];
    big.extend(vec![0u8; w::MAX_UP_DATAGRAM]);
    let (mut accepted, secs) = (0u64, 5u64);
    let d0 = s.down.len();
    for _ in 0..secs * 50 {
        s.speak(OLGA, 20).unwrap();
        s.speak(A1, 25).unwrap();
        for _ in 0..4 {
            accepted += u64::from(s.speak(CAROL, 30).is_ok());
        }
        let e = s.hosts.get_mut(&H1).unwrap();
        assert_eq!(e.on_datagram(a1_conn, &big, s.now), Err(Refusal::Oversize));
        s.step();
    }
    assert!(accepted <= secs * MAX_UP_PPS + UP_BURST, "flood bounded: {accepted}");
    assert!(accepted >= secs * MAX_UP_PPS, "rate honoured: {accepted}");
    assert_eq!(s.hosts[&H1].dropped(&olga), 0, "olga never dropped");
    let olga_to_a1 = s.down[d0..].iter().filter(|(m, b)| *m == A1 && b[1] == olga_slot).count() as u64;
    assert_eq!(olga_to_a1, secs * 50, "every olga frame forwarded");
    // Level-only flood: same bucket, never forwarded; bad lengths dropped.
    let (rx0, d1) = (s.hosts[&H1].level_only_rx(), s.down.len());
    for _ in 0..50 {
        for _ in 0..4 {
            let _ = s.send_up(CAROL, w::up_level(10).to_vec());
        }
        s.step();
    }
    let rx = s.hosts[&H1].level_only_rx() - rx0;
    assert!(rx <= MAX_UP_PPS + UP_BURST, "level-only flood bounded: {rx}");
    assert_eq!(s.down.len(), d1, "level-only datagrams are never forwarded");
    for bad in [vec![w::UP_LEVEL], vec![w::UP_LEVEL, 1, 2]] {
        assert_eq!(s.send_up(CAROL, bad), Err(Refusal::Malformed));
    }
    // Join spam: a newer Join after the gap is fine; more within JOIN_MIN_GAP_MS are refused
    // with H's state unchanged.
    let carol = s.d[CAROL].id.clone();
    let cep = s.d[CAROL].ep;
    let first = w::sign_join(&carol, &gid(), &cid, 1, s.now + 1).1;
    assert!(s.raw_join(H1, cep, first).is_ok());
    let roster = s.roster(H1);
    for i in 0..5 {
        let r = w::sign_join(&carol, &gid(), &cid, 1, s.now + 2 + i).1;
        let before = s.hosts[&H1].snapshot();
        assert_eq!(s.raw_join(H1, cep, r), Err(Refusal::RateLimited));
        assert_eq!(s.hosts[&H1].snapshot(), before);
    }
    assert_eq!(s.roster(H1), roster);
    // More than MAX_CONTROL_PER_S frames on one connection are refused.
    let e = s.hosts.get_mut(&H1).unwrap();
    let junk = GroupCallMsg::Heartbeat { epoch: 0 };
    let refused: Vec<Refusal> = (0..MAX_CONTROL_PER_S + 2).map(|_| e.on_control(a1_conn, junk.clone(), s.now, &s.auth).unwrap_err()).collect();
    assert_eq!(refused.iter().filter(|r| **r == Refusal::RateLimited).count(), 2);
}

// GC-S16. Phone degrade: the §2.4 table, then while hosting.
#[test]
fn gcall_s16_phone_degrade_reduces_k_refuses_joins_and_hands_over() {
    use PhoneAction::*;
    let p = |battery: u8, plugged: bool, thermal: Thermal, metered: bool| Cond { thermal, metered, ..Cond::phone(battery, plugged) };
    for (c, want) in [
        (p(30, false, Thermal::None, false), Advertise),
        (p(29, false, Thermal::None, false), StopAdvertising),
        (p(14, false, Thermal::None, false), Limited),
        (p(8, false, Thermal::None, false), Limited),
        (p(7, false, Thermal::None, false), End),
        (p(5, true, Thermal::None, false), Advertise),
        (p(90, true, Thermal::Moderate, false), StopAdvertising),
        (p(90, true, Thermal::Severe, false), Limited),
        (p(90, true, Thermal::Critical, false), End),
        (p(90, true, Thermal::None, true), StopAdvertising),
        (Cond { metered_override: true, ..p(90, true, Thermal::None, true) }, Advertise),
    ] {
        assert_eq!(phone_policy(&c), want, "{c:?}");
    }
    let mut s = World::new();
    assert_eq!(s.start_call(HP, OLGA, vec![]), Err(Refusal::Refused), "a phone never hosts listen-only");
    let cid = s.start_call(HP, HP, vec![H1, BOB]).unwrap();
    assert_eq!(s.hosts[&HP].k(), K_PHONE, "a phone host on Wi-Fi forwards the four loudest");
    for m in [HP, OLGA, A1, BOB] {
        s.join(m).unwrap();
    }
    // Limited at 14 %: K = 1, no provisional slot, new Joins refused.
    let c14 = Cond::phone(14, false);
    let acts = s.hosts.get_mut(&HP).unwrap().set_cond(c14, &s.auth);
    s.host_actions(HP, acts);
    s.pump();
    assert!(s.notices.contains(&(HP, HostNotice::Limited)));
    let before = s.hosts[&HP].snapshot();
    let r = w::sign_join(&s.d[CAROL].id.clone(), &gid(), &cid, 1, s.now).1;
    let cep = s.d[CAROL].ep;
    assert_eq!(s.raw_join(HP, cep, r), Err(Refusal::Limited));
    assert_eq!(s.hosts[&HP].snapshot(), before);
    for _ in 0..30 {
        for (m, l) in [(HP, 20), (OLGA, 22), (A1, 24)] {
            s.speak(m, l).unwrap();
        }
        s.step();
        assert!(s.hosts[&HP].forwarder().forwarded().len() <= 1);
    }
    // End at 7 %: End{host_leaving, next_host: backups[0]}; members redial H1 at once.
    let t = s.now;
    let acts = s.hosts.get_mut(&HP).unwrap().set_cond(Cond::phone(7, false), &s.auth);
    s.host_actions(HP, acts);
    s.pump();
    assert!(s.host_ended.contains(&(HP, EndReason::HostLeaving, Some(s.href(H1)))));
    for m in [OLGA, A1, BOB] {
        assert_eq!(s.host_of(m), (H1, 2), "{m}");
        assert!(s.joins.contains(&(m, H1, 2, t)), "no heartbeat wait for {m}");
    }
    // Wi-Fi to metered mid-call: everyone stays, Joins beyond the metered cap are refused, and
    // the phone's own user leaving ends the call with host_leaving.
    let mut s = World::new();
    s.d[HP].cond = Cond::phone(80, true);
    let cid = s.start_call(HP, HP, vec![H1]).unwrap();
    for m in [HP, OLGA, A1, BOB, CAROL] {
        s.join(m).unwrap();
    }
    let metered = Cond { metered: true, ..Cond::phone(80, true) };
    let acts = s.hosts.get_mut(&HP).unwrap().set_cond(metered, &s.auth);
    assert_eq!(acts, vec![HostAction::Notice(HostNotice::MeteredData)]);
    assert_eq!(s.hosts[&HP].k(), K_METERED, "metered: the two loudest only");
    assert_eq!(s.roster(HP).len(), 5, "continues above the metered cap");
    let acts = s.clients.get_mut(&A1).unwrap().leave(s.now);
    s.client_actions(A1, acts);
    s.pump();
    s.advance(JOIN_MIN_GAP_MS);
    let r = w::sign_join(&s.d[A1].id.clone(), &gid(), &cid, 1, s.now).1;
    let a1ep = s.d[A1].ep;
    let before = s.hosts[&HP].snapshot();
    assert_eq!(s.raw_join(HP, a1ep, r), Err(Refusal::Full));
    assert_eq!(s.hosts[&HP].snapshot(), before);
    assert_eq!(s.roster(HP).len(), CAP_PHONE_METERED);
    let acts = s.clients.get_mut(&HP).unwrap().leave(s.now);
    s.client_actions(HP, acts);
    s.pump();
    assert!(!s.hosts.contains_key(&HP));
    for m in [OLGA, BOB, CAROL] {
        assert_eq!(s.host_of(m), (H1, 2), "{m}");
    }
}
