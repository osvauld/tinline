//! Two real nodes in one process, over iroh (needs network for relays/discovery): the call
//! state machine's edge cases that the e2e scripts don't reach.

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

const PASS: &str = "test-passphrase";

use p2pcore::{CallInfo, CallState, Node, NodeEvents, NodeStatus};

#[derive(Debug, Clone)]
enum Ev {
    Incoming(CallInfo),
    State(String, CallState),
}

struct Tap(Mutex<mpsc::Sender<Ev>>);

impl NodeEvents for Tap {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, c: CallInfo) {
        let _ = self.0.lock().unwrap().send(Ev::Incoming(c));
    }
    fn on_call_state(&self, id: String, s: CallState) {
        let _ = self.0.lock().unwrap().send(Ev::State(id, s));
    }
    fn on_log(&self, _: String) {}
}

struct Peer {
    node: Arc<Node>,
    rx: mpsc::Receiver<Ev>,
    did: String,
    phrase: String,
    _dir: tempdir::Dir,
}

mod tempdir {
    pub struct Dir(pub std::path::PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    pub fn new(tag: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("p2pcore-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        Dir(p)
    }
}

fn peer(name: &str) -> Peer {
    make_peer(name, None)
}

/// With `phrase`: the same person on a new install (new device key).
fn make_peer(name: &str, restore: Option<&str>) -> Peer {
    let dir = tempdir::new(name);
    let (tx, rx) = mpsc::channel();
    let node = Node::new(dir.0.to_string_lossy().into(), Arc::new(Tap(Mutex::new(tx)))).unwrap();
    let phrase = match restore {
        Some(p) => {
            node.restore_identity(p.into(), name.into(), PASS.into()).unwrap();
            p.to_string()
        }
        None => node.create_identity(name.into(), PASS.into()).unwrap(),
    };
    node.start().unwrap();
    let t = Instant::now();
    while !node.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    let did = node.profile().unwrap().did;
    Peer { node, rx, did, phrase, _dir: dir }
}

/// a and b, each a contact of the other.
fn pair(tag: &str) -> (Peer, Peer) {
    let a = peer(&format!("{tag}-a"));
    let b = peer(&format!("{tag}-b"));
    let ticket = a.node.my_ticket().unwrap();
    b.node.add_contact(ticket).unwrap();
    (a, b)
}

/// Next event matching `f`, failing the test after `secs`.
fn expect<T>(p: &Peer, secs: u64, what: &str, mut f: impl FnMut(&Ev) -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        let left = end.saturating_duration_since(Instant::now());
        match p.rx.recv_timeout(left) {
            Ok(ev) => {
                if let Some(t) = f(&ev) {
                    return t;
                }
            }
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
}

fn ended(id: &str) -> impl FnMut(&Ev) -> Option<String> + '_ {
    move |ev| match ev {
        Ev::State(i, CallState::Ended { reason }) if i == id => Some(reason.clone()),
        _ => None,
    }
}

#[test]
fn caller_cancels_while_ringing() {
    let (a, b) = pair("cancel");
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
    let (a, b) = pair("decline");
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
    let (a, b) = pair("dialing");
    // Nobody home: a's endpoint is gone, so b's dial would sit out its 30 s timeout.
    a.node.stop();
    let call = b.node.call(a.did.clone()).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    b.node.hangup(call.call_id.clone()).unwrap();
    assert_eq!(expect(&b, 5, "ended", ended(&call.call_id)), "cancelled");
    assert!(t.elapsed() < Duration::from_secs(2), "hangup took {:?}", t.elapsed());
    assert!(b.node.current_call().is_none());
}

#[test]
fn glare_leaves_exactly_one_call() {
    let (a, b) = pair("glare");
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
                    Ev::Incoming(c) => p.node.answer(c.call_id).unwrap(),
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
    let (a, b) = pair("order");
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
    let (a, b) = pair("history");
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
    // Removing the contact deletes the history with them.
    a.node.remove_contact(b.did.clone()).unwrap();
    assert!(a.node.recent_calls(10).is_empty());
    assert_eq!(b.node.recent_calls(10).len(), 1);
}

#[test]
fn unavailable_turns_callers_away_quietly() {
    let (a, b) = pair("away");
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
    let (a, b) = pair("alias");
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
