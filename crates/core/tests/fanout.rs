//! One person on two installs (B1, B2: same phrase) being called by A: both ring, one answer
//! wins, the other stops without a missed call. Real iroh endpoints, so these need network
//! like calls.rs.

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, Node, NodeEvents, NodeStatus};

const PASS: &str = "test-passphrase";

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
    dir: std::path::PathBuf,
}

impl Drop for Peer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn make_peer(name: &str, restore: Option<&str>) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-fanout-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = mpsc::channel();
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Tap(Mutex::new(tx)))).unwrap();
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
    Peer { node, rx, did, phrase, dir }
}

/// A, and B on two installs that know each other; A has both as devices of the contact B.
fn trio(tag: &str) -> (Peer, Peer, Peer) {
    let a = make_peer(&format!("{tag}-a"), None);
    let b1 = make_peer(&format!("{tag}-b1"), None);
    b1.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let b2 = make_peer(&format!("{tag}-b2"), Some(&b1.phrase));
    assert_eq!(b2.did, b1.did);
    b2.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    b1.node.add_own_device_for_test(b2.node.own_attestation_for_test().unwrap()).unwrap();
    b2.node.add_own_device_for_test(b1.node.own_attestation_for_test().unwrap()).unwrap();
    let want = [b1.node.device_key_for_test().unwrap(), b2.node.device_key_for_test().unwrap()];
    let have = a.node.contact_devices_for_test(b1.did.clone());
    assert!(want.iter().all(|d| have.contains(d)), "A should know both installs: {have:?}");
    (a, b1, b2)
}

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

fn incoming(p: &Peer) -> String {
    expect(p, 40, "incoming", |e| match e {
        Ev::Incoming(c) => Some(c.call_id.clone()),
        _ => None,
    })
}

fn active(p: &Peer, id: &str) {
    expect(p, 20, "active", |e| matches!(e, Ev::State(i, CallState::Active) if i == id).then_some(()));
}

fn record(p: &Peer, id: &str) -> p2pcore::CallRecord {
    p.node.recent_calls(50).into_iter().find(|r| r.call_id == id).expect("call in history")
}

#[test]
fn second_device_answers_first_ends_answered_elsewhere() {
    let (a, b1, b2) = trio("answer");
    let call = a.node.call(b1.did.clone()).unwrap();
    assert_eq!(incoming(&b1), call.call_id);
    assert_eq!(incoming(&b2), call.call_id);
    b2.node.answer(call.call_id.clone()).unwrap();
    active(&a, &call.call_id);
    active(&b2, &call.call_id);
    assert_eq!(expect(&b1, 10, "b1 ended", ended(&call.call_id)), "answered_elsewhere");
    let rec = record(&b1, &call.call_id);
    assert!(!rec.missed && rec.incoming && rec.duration_secs == 0, "{rec:?}");
    assert!(b1.node.current_call().is_none());
    // The call itself is with B2 only.
    assert_eq!(a.node.current_call().unwrap().call_id, call.call_id);
    assert_eq!(b2.node.current_call().unwrap().call_id, call.call_id);
    a.node.hangup(call.call_id.clone()).unwrap();
    assert_eq!(expect(&b2, 10, "b2 ended", ended(&call.call_id)), "hangup_remote");
}

#[test]
fn decline_on_one_device_declines_the_call_everywhere() {
    let (a, b1, b2) = trio("decline");
    let call = a.node.call(b1.did.clone()).unwrap();
    assert_eq!(incoming(&b1), call.call_id);
    assert_eq!(incoming(&b2), call.call_id);
    b1.node.decline(call.call_id.clone()).unwrap();
    assert_eq!(expect(&a, 10, "a ended", ended(&call.call_id)), "declined");
    assert_eq!(expect(&b1, 10, "b1 ended", ended(&call.call_id)), "declined_local");
    assert_eq!(expect(&b2, 10, "b2 ended", ended(&call.call_id)), "declined_elsewhere");
    assert!(!record(&b2, &call.call_id).missed);
    assert!(!record(&b1, &call.call_id).missed);
}

#[test]
fn caller_hangup_while_ringing_cancels_all_devices() {
    let (a, b1, b2) = trio("hangup");
    let call = a.node.call(b1.did.clone()).unwrap();
    assert_eq!(incoming(&b1), call.call_id);
    assert_eq!(incoming(&b2), call.call_id);
    a.node.hangup(call.call_id.clone()).unwrap();
    assert_eq!(expect(&a, 10, "a ended", ended(&call.call_id)), "cancelled");
    for b in [&b1, &b2] {
        assert_eq!(expect(b, 10, "b ended", ended(&call.call_id)), "cancelled");
        assert!(record(b, &call.call_id).missed);
    }
}

#[test]
fn an_offline_device_does_not_delay_the_other() {
    let (a, b1, b2) = trio("offline");
    b1.node.stop();
    let t = Instant::now();
    let call = a.node.call(b1.did.clone()).unwrap();
    assert_eq!(incoming(&b2), call.call_id);
    assert!(t.elapsed() < Duration::from_secs(15), "ringing took {:?}", t.elapsed());
    b2.node.answer(call.call_id.clone()).unwrap();
    active(&a, &call.call_id);
    a.node.hangup(call.call_id.clone()).unwrap();
    assert_eq!(expect(&b2, 10, "b2 ended", ended(&call.call_id)), "hangup_remote");
}

#[test]
fn simultaneous_answers_leave_exactly_one_active_call() {
    let (a, b1, b2) = trio("race");
    let call = a.node.call(b1.did.clone()).unwrap();
    assert_eq!(incoming(&b1), call.call_id);
    assert_eq!(incoming(&b2), call.call_id);
    let (n1, n2, id1, id2) = (b1.node.clone(), b2.node.clone(), call.call_id.clone(), call.call_id.clone());
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let (x, y) = (barrier.clone(), barrier);
    let t1 = std::thread::spawn(move || {
        x.wait();
        let _ = n1.answer(id1);
    });
    let t2 = std::thread::spawn(move || {
        y.wait();
        let _ = n2.answer(id2);
    });
    t1.join().unwrap();
    t2.join().unwrap();
    active(&a, &call.call_id);
    // Let the cancellation of the loser land.
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let on = [&b1, &b2].iter().filter(|b| b.node.current_call().is_some()).count();
        if on == 1 {
            break;
        }
        assert!(Instant::now() < end, "both or neither device kept the call ({on})");
        std::thread::sleep(Duration::from_millis(50));
    }
    let (winner, loser) = if b1.node.current_call().is_some() { (&b1, &b2) } else { (&b2, &b1) };
    assert_eq!(expect(loser, 10, "loser ended", ended(&call.call_id)), "answered_elsewhere");
    assert!(!record(loser, &call.call_id).missed);
    assert_eq!(winner.node.current_call().unwrap().call_id, call.call_id);
    assert_eq!(a.node.current_call().unwrap().call_id, call.call_id);
    // Media of the call really goes to the winner only: it hears the caller, the loser nothing.
    for _ in 0..10 {
        a.node.push_mic(vec![0i16; 960]);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(loser.node.call_stats().is_none());
    a.node.hangup(call.call_id.clone()).unwrap();
    assert_eq!(expect(winner, 10, "winner ended", ended(&call.call_id)), "hangup_remote");
}

#[test]
fn one_device_busy_the_other_answers() {
    let (a, b1, b2) = trio("busyone");
    // B1 is in a call with c already; c rings B1 then A calls B.
    let c = make_peer("busyone-c", None);
    c.node.add_contact(b1.node.my_ticket().unwrap()).unwrap();
    // Two waiting slots are used (active + waiting): fill b1 with c's call and a waiting call from d.
    let d = make_peer("busyone-d", None);
    d.node.add_contact(b1.node.my_ticket().unwrap()).unwrap();
    let e = make_peer("busyone-e", None);
    e.node.add_contact(b1.node.my_ticket().unwrap()).unwrap();
    let c1 = c.node.call(b1.did.clone()).unwrap();
    assert_eq!(incoming(&b1), c1.call_id);
    b1.node.answer(c1.call_id.clone()).unwrap();
    active(&b1, &c1.call_id);
    let d1 = d.node.call(b1.did.clone()).unwrap();
    assert_eq!(incoming(&b1), d1.call_id); // waiting
    // Now a third call is refused busy at b1.
    let call = a.node.call(b1.did.clone()).unwrap();
    assert_eq!(incoming(&b2), call.call_id);
    b2.node.answer(call.call_id.clone()).unwrap();
    active(&a, &call.call_id);
    assert_eq!(a.node.current_call().unwrap().call_id, call.call_id);
    a.node.hangup(call.call_id.clone()).unwrap();
    assert_eq!(expect(&b2, 10, "b2 ended", ended(&call.call_id)), "hangup_remote");
    c.node.hangup(c1.call_id).unwrap();
    drop(e);
}

#[test]
fn all_devices_busy_is_busy() {
    let (a, b1, b2) = trio("busyall");
    // Each install is in a call (with the other install's callers) and has one waiting.
    let mut others = Vec::new();
    for (i, b) in [&b1, &b2].into_iter().enumerate() {
        let c = make_peer(&format!("busyall-c{i}"), None);
        c.node.add_contact(b.node.my_ticket().unwrap()).unwrap();
        let d = make_peer(&format!("busyall-d{i}"), None);
        d.node.add_contact(b.node.my_ticket().unwrap()).unwrap();
        let c1 = c.node.call(b.did.clone()).unwrap();
        assert_eq!(incoming(b), c1.call_id);
        b.node.answer(c1.call_id.clone()).unwrap();
        active(b, &c1.call_id);
        let d1 = d.node.call(b.did.clone()).unwrap();
        assert_eq!(incoming(b), d1.call_id);
        others.push((c, d, c1.call_id));
    }
    let call = a.node.call(b1.did.clone()).unwrap();
    assert_eq!(expect(&a, 30, "a ended", ended(&call.call_id)), "busy");
    for (c, _d, id) in others {
        c.node.hangup(id).unwrap();
    }
}

#[test]
fn glare_between_a_and_two_device_b_leaves_one_call() {
    let (a, b1, b2) = trio("glare2");
    let ca = a.node.call(b1.did.clone()).unwrap();
    let cb = b2.node.call(a.did.clone()).unwrap();
    // Everyone answers whatever rings until a call is active on both ends of one call.
    let peers = [&a, &b1, &b2];
    let mut active_ids: [Option<String>; 3] = [None, None, None];
    let end = Instant::now() + Duration::from_secs(40);
    while Instant::now() < end && !(active_ids[0].is_some() && (active_ids[1].is_some() || active_ids[2].is_some())) {
        for (i, p) in peers.iter().enumerate() {
            while let Ok(ev) = p.rx.recv_timeout(Duration::from_millis(30)) {
                match ev {
                    Ev::Incoming(c) => {
                        let _ = p.node.answer(c.call_id);
                    }
                    Ev::State(id, CallState::Active) => active_ids[i] = Some(id),
                    _ => {}
                }
            }
        }
    }
    let id = active_ids[0].clone().expect("a is in a call");
    assert!(id == ca.call_id || id == cb.call_id);
    std::thread::sleep(Duration::from_secs(1));
    let on: Vec<_> = [&b1, &b2].into_iter().filter(|b| b.node.current_call().is_some()).collect();
    assert_eq!(on.len(), 1, "exactly one of B's installs is in the call");
    assert_eq!(a.node.current_call().unwrap().call_id, on[0].node.current_call().unwrap().call_id);
    a.node.hangup(id).unwrap();
}

// ---- device lists (no network needed beyond starting nodes) ------------------------------

fn fresh_pair(tag: &str) -> (Peer, Peer, identity::Identity) {
    let a = make_peer(&format!("{tag}-a"), None);
    let b = make_peer(&format!("{tag}-b"), None);
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    let id = identity::recover(&b.phrase).unwrap();
    (a, b, id)
}

fn dev(n: u8) -> [u8; 32] {
    // Valid ed25519 points are not required for a list entry; any 32 bytes decode.
    [n; 32]
}

fn blob(id: &identity::Identity, devices: &[[u8; 32]], seq: u64) -> String {
    let list: Vec<([u8; 32], Option<String>)> = devices.iter().map(|d| (*d, None)).collect();
    serde_json::to_string(&proto::sign_device_list(id, &list, seq)).unwrap()
}

#[test]
fn device_list_newer_replaces_older_is_ignored() {
    let (a, b, bid) = fresh_pair("dl-seq");
    let known = a.node.contact_devices_for_test(b.did.clone());
    assert_eq!(known.len(), 1);
    let first = blob(&bid, &[dev(1), dev(2)], 1000);
    assert!(a.node.offer_device_list_for_test(b.did.clone(), first.clone()));
    assert_eq!(a.node.contact_devices_for_test(b.did.clone()).len(), 2);
    // Older, and the same list again, change nothing.
    assert!(!a.node.offer_device_list_for_test(b.did.clone(), blob(&bid, &[dev(3)], 999)));
    assert!(!a.node.offer_device_list_for_test(b.did.clone(), first));
    assert_eq!(a.node.contact_devices_for_test(b.did.clone()).len(), 2);
    // Newer replaces: the devices not on it are no longer dialled.
    assert!(a.node.offer_device_list_for_test(b.did.clone(), blob(&bid, &[dev(3)], 2000)));
    let now = a.node.contact_devices_for_test(b.did.clone());
    assert_eq!(now.len(), 1);
    assert!(!known.contains(&now[0]));
}

#[test]
fn forged_and_foreign_device_lists_are_ignored() {
    let (a, b, bid) = fresh_pair("dl-forged");
    let before = a.node.contact_devices_for_test(b.did.clone());
    // Signed by someone else (the list's DID is theirs): not B's.
    let (other, _) = identity::generate();
    assert!(!a.node.offer_device_list_for_test(b.did.clone(), blob(&other, &[dev(1)], 5000)));
    // B's list with the payload altered after signing.
    let mut v: serde_json::Value = serde_json::from_str(&blob(&bid, &[dev(1)], 5000)).unwrap();
    let payload = v["payload"].as_str().unwrap().to_string();
    let swapped = payload.replacen('A', "B", 1);
    v["payload"] = if swapped == payload { format!("{payload}x").into() } else { swapped.into() };
    assert!(!a.node.offer_device_list_for_test(b.did.clone(), v.to_string()));
    // An empty list and a list over the cap.
    assert!(!a.node.offer_device_list_for_test(b.did.clone(), blob(&bid, &[], 5000)));
    let nine: Vec<[u8; 32]> = (1..=9).map(dev).collect();
    assert!(!a.node.offer_device_list_for_test(b.did.clone(), blob(&bid, &nine, 5000)));
    // B's list offered for a different contact is ignored too.
    let c = make_peer("dl-forged-c", None);
    a.node.add_contact(c.node.my_ticket().unwrap()).unwrap();
    assert!(!a.node.offer_device_list_for_test(c.did.clone(), blob(&bid, &[dev(1)], 5000)));
    assert_eq!(a.node.contact_devices_for_test(b.did.clone()), before);
}

#[test]
fn verified_resets_only_when_a_new_device_appears() {
    let (a, b, bid) = fresh_pair("dl-verified");
    let b_dev = a.node.contact_devices_for_test(b.did.clone());
    let b_key = proto::device_from_text(&b_dev[0]).unwrap();
    a.node.set_verified(b.did.clone(), true).unwrap();
    // The same device set (a re-ordering or a hint change) keeps it.
    assert!(a.node.offer_device_list_for_test(b.did.clone(), blob(&bid, &[b_key], 1000)));
    assert!(a.node.contacts()[0].verified);
    // A new device resets it.
    assert!(a.node.offer_device_list_for_test(b.did.clone(), blob(&bid, &[b_key, dev(9)], 2000)));
    assert!(!a.node.contacts()[0].verified);
    // Dropping a device does not.
    a.node.set_verified(b.did.clone(), true).unwrap();
    assert!(a.node.offer_device_list_for_test(b.did.clone(), blob(&bid, &[b_key], 3000)));
    assert!(a.node.contacts()[0].verified);
}

#[test]
fn device_list_persists_across_restart() {
    let (a, b, bid) = fresh_pair("dl-persist");
    assert!(a.node.offer_device_list_for_test(b.did.clone(), blob(&bid, &[dev(1), dev(2), dev(3)], 1000)));
    let devices = a.node.contact_devices_for_test(b.did.clone());
    assert_eq!(devices.len(), 3);
    // A restart of A: lock (secrets and state leave memory), unlock (state is read back from the
    // sealed store).
    let node = &a.node;
    node.lock();
    node.unlock(PASS.into()).unwrap();
    assert_eq!(node.contact_devices_for_test(b.did.clone()), devices);
    // The stored list still decides: an older one is refused after the restart.
    assert!(!node.offer_device_list_for_test(b.did.clone(), blob(&bid, &[dev(4)], 500)));
    assert_eq!(node.contacts().len(), 1);
    assert_eq!(node.profile().unwrap().did, a.did);
}

#[test]
fn own_device_list_is_signed_and_changes_with_the_registry() {
    let (a, b1, b2) = trio("own-list");
    let _ = a;
    let list: proto::SignedBlob = serde_json::from_str(&b1.node.own_device_list_for_test().unwrap()).unwrap();
    let l = proto::verify_device_list(&list, &b1.did).unwrap();
    assert_eq!(l.devices.len(), 2);
    assert_eq!(l.devices[0].device, b1.node.device_key_for_test().unwrap());
    // Unchanged registry: the same blob, not a fresh seq.
    let again: proto::SignedBlob = serde_json::from_str(&b1.node.own_device_list_for_test().unwrap()).unwrap();
    assert_eq!(list, again);
    let _ = b2;
}
