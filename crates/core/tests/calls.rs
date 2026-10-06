//! Two real nodes in one process, over iroh (needs network for relays/discovery): the call
//! state machine's edge cases that the e2e scripts don't reach.

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

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
    let dir = tempdir::new(name);
    let (tx, rx) = mpsc::channel();
    let node = Node::new(dir.0.to_string_lossy().into(), Arc::new(Tap(Mutex::new(tx)))).unwrap();
    node.create_identity(name.into()).unwrap();
    node.start().unwrap();
    let t = Instant::now();
    while !node.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    let did = node.profile().unwrap().did;
    Peer { node, rx, did, _dir: dir }
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
    assert_eq!(expect(&a, 10, "callee ended", ended(&call.call_id)), "missed");
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
    assert!(expect(&b, 10, "caller ended", ended(&call.call_id)).starts_with("declined"));
    assert_eq!(expect(&a, 10, "callee ended", ended(&call.call_id)), "declined");
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
