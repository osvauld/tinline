//! P11: a stopped endpoint is closed to the end, and a restart binds the next one only after it.
//!
//! iroh's `Endpoint::close` refuses new connections from its first step, but drops the relay
//! connections only at its end, after the open connections have drained. A close given up half
//! way (the stop used to wait at most 2 s) leaves a half-closed endpoint behind: still connected
//! to its home relay under this device key, refusing every dial that arrives there. After a
//! restart on another home relay, a contact that dials the old one (its stored relay, or a DNS
//! record not yet updated) is refused at once, so the call never rings.
//!
//! `P2P_CLOSE_WAIT_MS=0` makes `stop` return without waiting for the close at all, so the old
//! code abandons it every time. Own test binary: the variable is process-wide.
//! Real iroh endpoints (needs network, like `calls.rs`).

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
    dir: std::path::PathBuf,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.node.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn peer(name: &str) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-close-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = mpsc::channel();
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Tap(Mutex::new(tx)))).unwrap();
    node.auto_accept_contacts_for_test(true);
    node.create_identity(name.into(), PASS.into()).unwrap();
    node.start().unwrap();
    let t = Instant::now();
    while !node.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    let did = node.profile().unwrap().did;
    Peer { node, rx, did, dir }
}

fn expect<T>(p: &Peer, secs: u64, what: &str, mut f: impl FnMut(&Ev) -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p.rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(ev) => {
                if let Some(t) = f(&ev) {
                    return t;
                }
            }
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
}

#[derive(Debug, PartialEq)]
enum Probe {
    /// Connected: a live endpoint holds the key there.
    Reached,
    /// Turned away at once: an endpoint holding the key is closing (or half-closed).
    Refused(String),
    /// Nobody answered within the wait.
    Silent,
}

/// Dials `id` at `relay` from a throwaway endpoint.
fn probe(rt: &tokio::runtime::Runtime, ep: &iroh::Endpoint, id: iroh::PublicKey, relay: &str, wait: Duration) -> Probe {
    let addr = iroh::EndpointAddr::new(id).with_relay_url(relay.parse().unwrap());
    rt.block_on(async {
        match tokio::time::timeout(wait, ep.connect(addr, proto::ALPN)).await {
            Ok(Ok(conn)) => {
                conn.close(0u32.into(), b"probe");
                Probe::Reached
            }
            Ok(Err(e)) => Probe::Refused(e.to_string()),
            Err(_) => Probe::Silent,
        }
    })
}

// KIND: invariant (P11). A restart (stop, then start at once) never leaves the old endpoint
// behind: from the moment `start` returns, a dial to this device key at its old home relay
// reaches the new endpoint or nobody, and is never refused by the old one. Before the fix the
// old endpoint, its close abandoned, kept its relay connection for ~3 s and refused every dial.
#[test]
fn a_restart_leaves_no_refusing_endpoint_behind() {
    unsafe { std::env::set_var("P2P_CLOSE_WAIT_MS", "0") };
    let a = peer("close-a");
    let b = peer("close-b");
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();

    // An active call, so `a` has a connection to drain when it closes.
    let call = b.node.call(a.did.clone()).unwrap();
    let inc = expect(&a, 40, "incoming", |e| match e {
        Ev::Incoming(c) => Some(c.call_id.clone()),
        _ => None,
    });
    a.node.answer(inc).unwrap();
    expect(&b, 20, "active", |e| matches!(e, Ev::State(i, CallState::Active) if *i == call.call_id).then_some(()));

    let rt = tokio::runtime::Runtime::new().unwrap();
    let ep = rt.block_on(iroh::Endpoint::builder(iroh::endpoint::presets::N0).bind()).unwrap();
    let st = a.node.status();
    let id: iroh::PublicKey = st.endpoint_id.parse().unwrap();
    let relay = st.relay.expect("a is online at a relay");
    assert_eq!(probe(&rt, &ep, id, &relay, Duration::from_secs(10)), Probe::Reached, "sanity: a running a is reached");

    a.node.stop();
    a.node.start().unwrap();
    // A few dials over the next seconds (the old close drains in about 3 s at most).
    let end = Instant::now() + Duration::from_secs(6);
    while Instant::now() < end {
        let got = probe(&rt, &ep, id, &relay, Duration::from_secs(2));
        assert!(!matches!(got, Probe::Refused(_)), "a dial to the restarted a was refused: {got:?}");
    }
    rt.block_on(ep.close());
}
