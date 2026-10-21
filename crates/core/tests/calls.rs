//! Two real nodes in one process, over iroh (needs network for relays/discovery): the call
//! state machine's edge cases that the e2e scripts don't reach.

use std::sync::Arc;
use std::time::{Duration, Instant};

use p2pcore::CallState;

mod support;
use support::*;

const TAG: &str = "test";
const S: Suite = Suite(TAG);

/// With `phrase`: the same person on a new install (new device key).
fn make_peer(name: &str, restore: Option<&str>) -> Peer {
    match restore {
        Some(p) => Peer::restored(TAG, name, p),
        None => Peer::new(TAG, name),
    }
}

#[test]
fn caller_cancels_while_ringing() {
    let (a, b) = S.pair("cancel");
    let call = b.node.call(a.did.clone()).unwrap();
    let inc = expect(&a, 40, "incoming", |e| match e {
        Ev::Incoming(c) => Some(c.call_id.clone()),
        _ => None,
    });
    assert_eq!(inc, call.call_id);
    b.node.hangup(call.call_id.clone()).unwrap();
    assert_eq!(expect(&b, 10, "caller ended", ended(&call.call_id)), "cancelled");
    assert_eq!(expect(&a, 10, "callee ended", ended(&call.call_id)), "cancelled");
    let rec = &a.node.recent_calls(10)[0];
    assert!(rec.missed && rec.incoming && rec.duration_secs == 0 && rec.reason == "cancelled");
    // The slot is free right away on both sides.
    let again = b.node.call(a.did.clone()).expect("slot freed");
    b.node.hangup(again.call_id).unwrap();
}

#[test]
fn callee_declines() {
    let (a, b) = S.pair("decline");
    let call = b.node.call(a.did.clone()).unwrap();
    expect(&a, 40, "incoming", |e| matches!(e, Ev::Incoming(_)).then_some(()));
    a.node.hangup(call.call_id.clone()).unwrap();
    assert_eq!(expect(&b, 10, "caller ended", ended(&call.call_id)), "declined");
    assert_eq!(expect(&a, 10, "callee ended", ended(&call.call_id)), "declined_local");
    // Declining is not a missed call.
    assert!(!a.node.recent_calls(10)[0].missed);
}

#[test]
fn hangup_while_dialing_is_immediate() {
    let (a, b) = S.pair("dialing");
    // Nobody home: a's endpoint is gone, so b's dial would sit out its 30 s timeout. Dropped,
    // not only stopped: a stopped endpoint still holds its socket and refuses b at once.
    a.node.stop();
    let a_did = a.did.clone();
    drop(a);
    let call = b.node.call(a_did).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    b.node.hangup(call.call_id.clone()).unwrap();
    assert_eq!(expect(&b, 5, "ended", ended(&call.call_id)), "cancelled");
    assert!(t.elapsed() < Duration::from_secs(2), "hangup took {:?}", t.elapsed());
    assert!(b.node.current_call().is_none());
}

#[test]
fn glare_leaves_exactly_one_call() {
    let (a, b) = S.pair("glare");
    // Each side answers whatever rings.
    let ca = a.node.call(b.did.clone()).unwrap();
    let cb = b.node.call(a.did.clone()).unwrap();
    // Serve both sides in turn: each answers whatever rings, until both are in a call.
    let mut active: [Option<String>; 2] = [None, None];
    let end = Instant::now() + Duration::from_secs(40);
    while active.iter().any(Option::is_none) && Instant::now() < end {
        for (i, p) in [&a, &b].into_iter().enumerate() {
            while let Ok(ev) = p.rx.recv_timeout(Duration::from_millis(50)) {
                match ev {
                    // The losing call of the glare may already be gone (NotFound): fine.
                    Ev::Incoming(c) => match p.node.answer(c.call_id) {
                        Ok(()) | Err(p2pcore::Error::NotFound) => {}
                        Err(e) => panic!("answer: {e:?}"),
                    },
                    Ev::State(id, CallState::Active) => active[i] = Some(id),
                    _ => {}
                }
            }
        }
    }
    let active: Vec<String> = active.into_iter().flatten().collect();
    assert_eq!(active.len(), 2, "both sides should end up in one active call: {active:?}");
    assert_eq!(active[0], active[1], "the same call on both sides");
    assert!(active[0] == ca.call_id || active[0] == cb.call_id);
    a.node.hangup(active[0].clone()).unwrap();
}

#[test]
fn call_states_arrive_in_order() {
    let (a, b) = S.pair("order");
    let call = b.node.call(a.did.clone()).unwrap();
    let inc = expect(&a, 40, "incoming", |e| match e {
        Ev::Incoming(c) => Some(c.call_id.clone()),
        _ => None,
    });
    a.node.answer(inc).unwrap();
    // Let media run for a moment, then hang up from the caller.
    std::thread::sleep(Duration::from_secs(2));
    b.node.hangup(call.call_id.clone()).unwrap();
    let rank = |s: &CallState| match s {
        CallState::Dialing => 0,
        CallState::Ringing => 1,
        CallState::Active => 2,
        CallState::Ended { .. } => 3,
    };
    for p in [&a, &b] {
        let mut seen = Vec::new();
        expect(p, 10, "ended", |e| match e {
            Ev::State(id, st) if *id == call.call_id => {
                seen.push(rank(st));
                matches!(st, CallState::Ended { .. }).then_some(())
            }
            _ => None,
        });
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "out of order: {seen:?}");
        assert!(seen.contains(&2), "never went active: {seen:?}");
        // Nothing for this call after Ended.
        std::thread::sleep(Duration::from_millis(500));
        while let Ok(ev) = p.rx.try_recv() {
            assert!(!matches!(ev, Ev::State(ref id, _) if *id == call.call_id), "event after Ended: {ev:?}");
        }
    }
}

#[test]
fn history_and_reconnecting_over_a_real_call() {
    let (a, b) = S.pair("history");
    let call = b.node.call(a.did.clone()).unwrap();
    let inc = expect(&a, 40, "incoming", |e| match e {
        Ev::Incoming(c) => Some(c.call_id.clone()),
        _ => None,
    });
    a.node.answer(inc).unwrap();
    expect(&b, 20, "active", |e| matches!(e, Ev::State(_, CallState::Active)).then_some(()));
    // Nobody feeds a microphone yet, so no media flows: that reads as reconnecting.
    std::thread::sleep(Duration::from_millis(2200));
    assert!(a.node.call_stats().unwrap().reconnecting);
    // Media from b clears it on a.
    for _ in 0..10 {
        b.node.push_mic(vec![0i16; 960]);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!a.node.call_stats().unwrap().reconnecting);
    b.node.hangup(call.call_id.clone()).unwrap();
    assert_eq!(expect(&b, 10, "caller ended", ended(&call.call_id)), "hangup_local");
    assert_eq!(expect(&a, 10, "callee ended", ended(&call.call_id)), "hangup_remote");
    let (rb, ra) = (b.node.recent_calls(10), a.node.calls_with(b.did.clone(), 10));
    assert_eq!((rb.len(), ra.len()), (1, 1));
    assert!(!rb[0].incoming && ra[0].incoming && !ra[0].missed);
    assert!(rb[0].duration_secs >= 2 && ra[0].duration_secs >= 2, "{rb:?}");
    assert_eq!(rb[0].peer_did, a.did);
    assert_eq!(rb[0].peer_name, "history-a");
    assert_eq!(rb[0].reason, "hangup_local");
    assert!(a.node.calls_with(a.did.clone(), 10).is_empty());
    // Blocking them keeps the history with them (U19).
    a.node.block(b.did.clone()).unwrap();
    assert_eq!(a.node.recent_calls(10).len(), 1);
    assert_eq!(b.node.recent_calls(10).len(), 1);
}

#[test]
fn unavailable_turns_callers_away_quietly() {
    let (a, b) = S.pair("away");
    assert!(a.node.availability().available);
    a.node.set_available(false, Some(4_000_000_000)).unwrap();
    assert_eq!(a.node.availability().until, Some(4_000_000_000));
    let call = b.node.call(a.did.clone()).unwrap();
    // The caller sees only that it could not get through.
    assert_eq!(expect(&b, 30, "caller ended", ended(&call.call_id)), "unreachable");
    assert!(a.rx.try_recv().is_err(), "nothing may reach a's UI while unavailable");
    let rec = &a.node.recent_calls(10)[0];
    assert_eq!((rec.reason.as_str(), rec.missed, rec.incoming), ("unavailable", false, true));
    assert_eq!(b.node.recent_calls(10)[0].reason, "unreachable");
    // Adding a contact still works, and an expired "until" means available again.
    a.node.set_available(false, Some(1)).unwrap();
    assert!(a.node.availability().available);
    a.node.set_available(false, None).unwrap();
    assert!(!a.node.availability().available);
    a.node.set_available(true, None).unwrap();
    let call = b.node.call(a.did.clone()).unwrap();
    expect(&a, 30, "incoming", |e| matches!(e, Ev::Incoming(_)).then_some(()));
    b.node.hangup(call.call_id).unwrap();
}

#[test]
fn alias_verified_and_safety_number() {
    let (a, b) = S.pair("alias");
    let c = &b.node.contacts()[0];
    assert_eq!((c.alias.clone(), c.verified), (None, false));
    b.node.rename_contact(a.did.clone(), Some("  Mum \u{7}  ".into())).unwrap();
    b.node.set_verified(a.did.clone(), true).unwrap();
    let c = &b.node.contacts()[0];
    assert_eq!((c.alias.as_deref(), c.verified, c.name.as_str()), (Some("Mum"), true, "alias-a"));
    let long = "x".repeat(500);
    b.node.rename_contact(a.did.clone(), Some(long)).unwrap();
    assert_eq!(b.node.contacts()[0].alias.as_ref().unwrap().chars().count(), 64);
    b.node.rename_contact(a.did.clone(), Some("   ".into())).unwrap();
    assert_eq!(b.node.contacts()[0].alias, None);
    assert!(b.node.rename_contact("did:key:zNope".into(), None).is_err());
    assert_eq!(a.node.safety_number(b.did.clone()).unwrap(), b.node.safety_number(a.did.clone()).unwrap());
    // Re-adding from a new device resets verified: a restored copy of a on a fresh install.
    b.node.set_verified(a.did.clone(), true).unwrap();
    let a2 = make_peer("alias-a2", Some(&a.phrase));
    assert_eq!(a2.did, a.did);
    b.node.add_contact(a2.node.my_ticket().unwrap()).unwrap();
    assert!(!b.node.contacts()[0].verified, "a new device must be verified again");
}

/// b is in a call with a; a and c are contacts of b.
fn trio(tag: &str) -> (Peer, Peer, Peer, String) {
    let (a, b) = S.pair(tag);
    let c = S.peer(&format!("{tag}-c"));
    c.node.add_contact(b.node.my_ticket().unwrap()).unwrap();
    let call = a.node.call(b.did.clone()).unwrap();
    let inc = incoming(&b, 40);
    b.node.answer(inc).unwrap();
    expect(&a, 20, "active", |e| matches!(e, Ev::State(_, CallState::Active)).then_some(()));
    (a, b, c, call.call_id)
}

#[test]
fn second_call_declined_ignored_or_answered() {
    // Only calls that arrive during a call read this, when they start.
    unsafe { std::env::set_var("P2P_WAITING_RING_SECS", "4") };
    let (a, b, c, first) = trio("waiting");
    // Made before c's call: on a loaded machine making a node takes longer than the 4 s ring.
    let d = S.peer("waiting-d");
    d.node.add_contact(b.node.my_ticket().unwrap()).unwrap();

    // Declined: c hears busy, the a-b call is untouched.
    let c1 = c.node.call(b.did.clone()).unwrap();
    assert_eq!(incoming(&b, 40), c1.call_id);
    assert_eq!(b.node.waiting_call().unwrap().call_id, c1.call_id);
    assert_eq!(b.node.current_call().unwrap().call_id, first);
    // A third call while one waits is turned away busy at once.
    let d1 = d.node.call(b.did.clone()).unwrap();
    assert_eq!(expect(&d, 20, "third busy", ended(&d1.call_id)), "busy");
    b.node.decline(c1.call_id.clone()).unwrap();
    assert_eq!(expect(&c, 10, "c busy", ended(&c1.call_id)), "busy");
    assert_eq!(expect(&b, 10, "b ended", ended(&c1.call_id)), "declined_local");
    assert!(b.node.waiting_call().is_none());
    assert_eq!(b.node.current_call().unwrap().call_id, first);
    assert!(!b.node.recent_calls(10).iter().find(|r| r.call_id == c1.call_id).unwrap().missed);

    // Ignored: no_answer for c, missed for b, the call goes on.
    let c2 = c.node.call(b.did.clone()).unwrap();
    assert_eq!(incoming(&b, 40), c2.call_id);
    assert_eq!(expect(&c, 20, "c no_answer", ended(&c2.call_id)), "no_answer");
    assert_eq!(expect(&b, 10, "b ended", ended(&c2.call_id)), "no_answer");
    let rec = b.node.recent_calls(10).into_iter().find(|r| r.call_id == c2.call_id).unwrap();
    assert!(rec.missed && rec.incoming);
    assert_eq!(b.node.current_call().unwrap().call_id, first);

    // End & answer: a's call ends, c's becomes the current call and goes active.
    let c3 = c.node.call(b.did.clone()).unwrap();
    assert_eq!(incoming(&b, 40), c3.call_id);
    b.node.end_and_answer(c3.call_id.clone()).unwrap();
    assert_eq!(expect(&a, 10, "a ended", ended(&first)), "hangup_remote");
    assert_eq!(expect(&b, 10, "b ended first", ended(&first)), "hangup_local");
    expect(&c, 20, "c active", |e| matches!(e, Ev::State(i, CallState::Active) if *i == c3.call_id).then_some(()));
    assert_eq!(b.node.current_call().unwrap().call_id, c3.call_id);
    assert!(b.node.waiting_call().is_none());
    c.node.hangup(c3.call_id.clone()).unwrap();
    assert_eq!(expect(&b, 10, "b ended third", ended(&c3.call_id)), "hangup_remote");
}

#[test]
fn waiting_call_rings_normally_when_the_active_call_ends() {
    let (a, b, c, first) = trio("promote");
    let c1 = c.node.call(b.did.clone()).unwrap();
    assert_eq!(incoming(&b, 40), c1.call_id);
    a.node.hangup(first.clone()).unwrap();
    expect(&b, 10, "b ended first", ended(&first));
    // It is announced again as an ordinary incoming call, and can now be answered.
    assert_eq!(incoming(&b, 40), c1.call_id);
    assert!(b.node.waiting_call().is_none());
    b.node.answer(c1.call_id.clone()).unwrap();
    expect(&c, 20, "c active", |e| matches!(e, Ev::State(_, CallState::Active)).then_some(()));
    c.node.hangup(c1.call_id).unwrap();
}

/// Switching to or starting another account is refused while a call rings or runs, and works
/// the moment it is over.
#[test]
fn account_switch_is_refused_during_a_call() {
    let (a, b) = S.pair("switch-incall");
    let call = b.node.call(a.did.clone()).unwrap();
    expect(&a, 40, "incoming", |e| matches!(e, Ev::Incoming(_)).then_some(()));
    for p in [&a, &b] {
        assert!(matches!(p.node.begin_new_account(), Err(p2pcore::Error::InCall)));
        assert!(matches!(p.node.switch_account(p.did.clone()), Err(p2pcore::Error::InCall)));
    }
    a.node.answer(call.call_id.clone()).unwrap();
    expect(&a, 20, "active", |e| matches!(e, Ev::State(_, CallState::Active)).then_some(()));
    expect(&b, 20, "active", |e| matches!(e, Ev::State(_, CallState::Active)).then_some(()));
    assert!(matches!(a.node.begin_new_account(), Err(p2pcore::Error::InCall)));
    assert!(a.node.has_identity() && !a.node.contacts().is_empty());
    b.node.hangup(call.call_id.clone()).unwrap();
    expect(&a, 10, "ended", ended(&call.call_id));
    a.node.begin_new_account().unwrap();
    assert!(!a.node.has_identity() && a.node.contacts().is_empty() && !a.node.status().started);
    a.node.switch_account(a.did.clone()).unwrap();
    a.node.unlock(PASS.into()).unwrap();
    assert_eq!(a.node.contacts().len(), 1);
    assert_eq!(a.node.recent_calls(10).len(), 1, "the call was logged before the switch, and is still there");
}

/// G6: one call at a time. A 1:1 call to someone in a group call is turned away busy, and a
/// group call cannot be started during a 1:1 call.
#[test]
fn group_call_and_one_to_one_exclude_each_other() {
    use p2pcore::gcall::net::{GcallConfig, GcallEvent, GcallEvents, StaticDirectory};
    use p2pcore::gcall::{Cond, HostConfig, HostRef};
    struct NoEvents;
    impl GcallEvents for NoEvents {
        fn on_gcall_event(&self, _: GcallEvent) {}
    }
    let (a, b) = S.pair("gcall-busy");
    let dir = Arc::new(StaticDirectory::default());
    let href = HostRef { holder_did: a.did.clone(), device_slot: 0, permit_digest: "permit-a".into() };
    {
        let mut auth = dir.auth.lock();
        auth.members.insert(a.did.clone());
        auth.permits.insert(href.permit_digest.clone());
    }
    a.node.gcall_configure(GcallConfig { dir, events: Arc::new(NoEvents), host_ref: Some(href.clone()), cond: Cond::desktop(), note: None });
    let host = || {
        a.node.gcall_host(HostConfig {
            gid: "b2".repeat(32),
            call_id: proto::gcall::new_call_id(),
            host_gen: 1,
            host_ref: href.clone(),
            starter_did: a.did.clone(),
            backups: vec![],
            meta_frontier: "f".into(),
            cond: Cond::desktop(),
            own_user_in: true,
        })
    };

    // a is in a group call (its own, alone): b's call gets busy; a cannot place one either.
    host().unwrap();
    let call = b.node.call(a.did.clone()).unwrap();
    assert_eq!(expect(&b, 40, "busy", ended(&call.call_id)), "busy");
    assert!(a.node.current_call().is_none());
    assert!(matches!(a.node.call(b.did.clone()), Err(p2pcore::Error::InCall)));
    a.node.gcall_end();

    // Now a 1:1 call goes through, and a group call is refused while it runs.
    let call = b.node.call(a.did.clone()).unwrap();
    a.node.answer(incoming(&a, 40)).unwrap();
    expect(&b, 20, "active", |e| matches!(e, Ev::State(_, CallState::Active)).then_some(()));
    assert!(matches!(host(), Err(p2pcore::Error::InCall)));
    assert!(a.node.gcall_status().is_none_or(|s| s.hosting.is_none()));
    b.node.hangup(call.call_id.clone()).unwrap();
    expect(&a, 10, "ended", ended(&call.call_id));
    host().expect("a group call once the 1:1 call is over");
    a.node.gcall_end();
}
