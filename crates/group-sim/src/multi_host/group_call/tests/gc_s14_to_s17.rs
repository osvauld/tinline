use super::*;

// KIND: invariant
// GC-S15. An admin revokes H1's host_call permit mid-call. Members drop the call and redial
// backups[0] at host_gen+1. If backups[0] is also revoked, they skip it and go to backups[1].
// Afterwards, control from the revoked host (a rogue that keeps sending) is refused.
#[test]
fn gc_s15_revoked_host_call_mid_call_members_redial_the_next_valid_backup() {
    let all = [OLGA, A1, A2, BOB, CAROL, HP, H1, H2];
    for both in [false, true] {
        let (mut s, cid) = base();
        for m in [A1, CAROL, HP] {
            s.join_shown(m).unwrap();
        }
        if both {
            let b = s.revoke_op(A1, BOB);
            for i in all {
                let _ = s.accept_revoke(i, &b);
            }
        }
        s.w.rogue.insert(H1); // keeps hosting after revocation
        s.revoke(A1, H1, &all);
        let want = if both { OLGA } else { BOB };
        for m in [A1, CAROL, HP] {
            let j = s.c[m].joined.as_ref().unwrap();
            assert_eq!(
                (j.call_id.as_str(), j.host_gen, s.dev_of(&j.host)),
                (cid.as_str(), 2, want),
                "both={both} {m}"
            );
        }
        // The rogue H1 still sends a Roster and an End at gen 1. Both are refused.
        let r = s.msg(
            H1,
            &cid,
            1,
            Kind::Roster {
                epoch: 50,
                entries: vec![],
            },
        );
        let e = s.msg(
            H1,
            &cid,
            1,
            Kind::End {
                reason: EndReason::Ended,
                next_host: None,
            },
        );
        let before = s.snap_all();
        for m in [A1, CAROL, HP] {
            assert!(s.accept_roster(m, &r.wire()).is_err());
            assert!(s.accept_end(m, &e.wire()).is_err());
        }
        assert_unchanged(&s, &before, "revoked host control");
        assert_eq!(
            s.accept_call_record(A1, &s.record(H1).wire()),
            Err(CR::Revoked)
        );
    }
}

// KIND: invariant
// GC-S16. The phone degrade thresholds (§2.4) as a pure table, then while hosting. A phone
// refuses to host listen-only. Under Limited it drops K to 1 and refuses new Joins. Under End it
// sends End{host_leaving, next_host: backups[0]}, and members redial at once with no heartbeat
// wait. A switch from Wi-Fi to metered keeps the call but refuses Joins beyond CAP_PHONE_METERED.
// The phone's own user leaving also ends the call with host_leaving.
#[test]
fn gc_s16_phone_degrade_reduces_k_refuses_joins_and_hands_over_with_host_leaving() {
    use PhoneAction::*;
    let p = |battery: u8, plugged: bool, thermal: Thermal, metered: bool| Cond {
        thermal,
        metered,
        ..Cond::phone(battery, plugged)
    };
    let table = [
        (p(30, false, Thermal::None, false), Advertise),
        (p(29, false, Thermal::None, false), StopAdvertising),
        (p(15, false, Thermal::None, false), StopAdvertising),
        (p(14, false, Thermal::None, false), Limited),
        (p(8, false, Thermal::None, false), Limited),
        (p(7, false, Thermal::None, false), End),
        (p(5, true, Thermal::None, false), Advertise),
        (p(90, true, Thermal::Light, false), Advertise),
        (p(90, true, Thermal::Moderate, false), StopAdvertising),
        (p(90, true, Thermal::Severe, false), Limited),
        (p(90, true, Thermal::Critical, false), End),
        (p(90, true, Thermal::None, true), StopAdvertising),
        (
            Cond {
                metered_override: true,
                ..p(90, true, Thermal::None, true)
            },
            Advertise,
        ),
    ];
    for (c, want) in table {
        assert_eq!(phone_policy(&c), want, "{c:?}");
    }
    let mut s = Sim::new();
    s.c[HP].cond = Cond::phone(80, false);
    assert_eq!(
        s.start_call(HP, OLGA, vec![]),
        Err(CR::Refused("phone never hosts listen-only"))
    );
    let cid = s
        .start_call(HP, HP, vec![s.host_ref(H1), s.host_ref(BOB)])
        .unwrap();
    assert_eq!(s.c[HP].hosting.as_ref().unwrap().k, K_PHONE);
    for m in [HP, OLGA, A1, BOB] {
        s.join_shown(m).unwrap();
    }
    // Stop advertising at thermal MODERATE: the status says busy, and the call continues.
    s.c[HP].cond.thermal = Thermal::Moderate;
    assert!(s.status(HP, s.now, STATUS_TTL_MS).body.busy);
    s.c[HP].cond.thermal = Thermal::None;
    // Limited at 14 %.
    s.c[HP].cond.battery = 14;
    assert_eq!(phone_policy(&s.c[HP].cond), Limited);
    {
        let hc = s.c[HP].hosting.as_mut().unwrap();
        hc.limited = true;
        hc.k = 1;
        hc.fwd.k = 1;
        hc.fwd.provisional = false; // no K+1 slot while limited
    }
    s.c[HP].log.push("hosting limited".into());
    let before = s.snap_all();
    let r = s.join_req(CAROL, &cid, 1, s.now);
    assert_eq!(s.host_join(HP, &s.w.gep(CAROL), &r), Err(CR::Limited));
    assert_unchanged(&s, &before, "limited");
    for _ in 0..30 {
        let set = s.frame(HP, &[(HP, 20), (OLGA, 22), (A1, 24)]);
        assert!(set.len() <= 1);
    }
    // End at 7 %. Members redial H1 at once.
    s.c[HP].cond.battery = 7;
    assert_eq!(phone_policy(&s.c[HP].cond), End);
    let t = s.now;
    let next = s.next_backup(HP);
    assert_eq!(next, Some(s.host_ref(H1)));
    s.host_end(HP, EndReason::HostLeaving, next, None);
    for m in [OLGA, A1, BOB] {
        let r = s.redials.iter().find(|(d, _)| *d == m).map(|(_, r)| r);
        assert_eq!(
            r,
            Some(&Redial::Joined { host: H1, at_ms: t }),
            "no heartbeat wait for {m}"
        );
        assert_eq!(s.c[m].joined.as_ref().unwrap().host_gen, 2);
    }
    // Wi-Fi to metered mid-call: everyone stays, and new Joins beyond the metered cap are refused.
    let mut s = Sim::new();
    s.c[HP].cond = Cond::phone(80, true);
    let cid = s.start_call(HP, HP, vec![s.host_ref(H1)]).unwrap();
    for m in [HP, OLGA, A1, BOB, CAROL] {
        s.join_shown(m).unwrap();
    }
    s.c[HP].cond.metered = true;
    s.c[HP].hosting.as_mut().unwrap().warned_metered = true; // "data use" warning shown
    assert_eq!(
        roster_of(&s, HP).len(),
        5,
        "continues above the metered cap"
    );
    s.leave(A1).unwrap();
    s.advance(JOIN_MIN_GAP_MS);
    let r = s.join_req(A1, &cid, 1, s.now);
    let before = s.snap_all();
    assert_eq!(s.host_join(HP, &s.w.gep(A1), &r), Err(CR::Full));
    assert_unchanged(&s, &before, "metered cap");
    assert_eq!(roster_of(&s, HP).len(), CAP_PHONE_METERED);
    // The phone's own user leaves, which ends the call with host_leaving.
    s.leave(HP).unwrap();
    assert!(s.c[HP].hosting.is_none());
    for m in [OLGA, BOB, CAROL] {
        assert!(s.c[m].log.iter().any(|l| l == "end:HostLeaving"));
        assert_eq!(s.dev_of(&s.c[m].joined.as_ref().unwrap().host), H1);
    }
}

// KIND: invariant
// GC-S17. One DID joins from two linked devices (alice's A1 and A2). The newer Join replaces the
// older one, the older device's connection closes ("answered elsewhere"), and there is one slot
// per DID. These are refused with state byte-identical: a replay of the old Join, an older-issued
// Join from A1 (ASSUMPTION G5), and A1's datagrams after the switch. A newer Join from A1 takes
// the slot back.
#[test]
fn gc_s17_same_did_on_two_linked_devices_newer_join_replaces_older() {
    let (mut s, cid) = base();
    s.join_shown(OLGA).unwrap();
    s.join_shown(A1).unwrap();
    let a1_join = s.c[H1].hosting.as_ref().unwrap().parts[&s.did(A1)]
        .join
        .clone();
    s.advance(JOIN_MIN_GAP_MS);
    s.join_shown(A2).unwrap();
    let alice = s.did(A1);
    let r = roster_of(&s, H1);
    assert_eq!(
        r.values().filter(|d| **d == alice).count(),
        1,
        "one slot per DID"
    );
    assert_eq!(r.len(), 2);
    assert_eq!(
        s.c[H1].hosting.as_ref().unwrap().parts[&alice].endpoint,
        s.w.gep(A2)
    );
    assert!(s.c[A1].joined.is_none());
    assert!(s.c[A1].log.iter().any(|l| l == "call answered elsewhere"));
    assert!(s.c[A2].joined.is_some());
    let mut d = vec![UP];
    d.extend(audio(1, 10));
    assert_eq!(
        s.host_ingress(H1, &s.w.gep(A1), &d),
        Err(CR::NotParticipant)
    );
    s.advance(JOIN_MIN_GAP_MS);
    let replay = serde_json::to_vec(&JoinReq {
        join: a1_join,
        note: s.w.d[A1].my_note.clone(),
    })
    .unwrap();
    let older = s.join_req(A1, &cid, 1, s.now - 10 * JOIN_MIN_GAP_MS);
    for (case, bytes, want) in [
        ("replay", replay, CR::Replay),
        ("older issued_ms", older, CR::Stale),
    ] {
        let before = s.snap_all();
        assert_eq!(s.host_join(H1, &s.w.gep(A1), &bytes), Err(want), "{case}");
        assert_unchanged(&s, &before, case);
    }
    s.join_shown(A1).unwrap();
    assert_eq!(
        s.c[H1].hosting.as_ref().unwrap().parts[&alice].endpoint,
        s.w.gep(A1)
    );
    assert!(s.c[A2].joined.is_none());
    assert_eq!(roster_of(&s, H1).len(), 2);
}
