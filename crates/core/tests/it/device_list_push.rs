//! P4: acceptance tests for pushing our DID-signed device list to contacts (decision U8,
//! `docs/design/rotation-open-decisions.md` §0 on `endpoint-rotation-blocking`; W2-D findings §12).
//!
//! Target behaviour:
//! - Whenever the set of our installs changes (a device linked, unlinked, restored with a new
//!   device key; later: a private-endpoint rotation) the install that sees the change re-signs the
//!   list (higher seq) and sends it to every UNBLOCKED contact as `Msg::Devices`, retried until
//!   taken (outbox, like the token outbox). A contact that was offline gets the latest list when
//!   it is reachable again.
//! - The receiver keeps the highest valid seq and dials the list's devices. Stale, replayed,
//!   forged, tampered and wrong-DID lists are refused, and so is a list whose seq is more than one
//!   day (ms) ahead of the receiver's clock (PROVISIONAL (coordinator) seq bound).
//! - A blocked contact is never sent the list.
//!
//! Real iroh endpoints (needs network, like `calls.rs`). Waits are bounded polls; "never arrives"
//! claims are bounded and stated per test. The shared harness is in `support/`.

use std::time::Duration;

use crate::support;
use support::eventually_200ms as eventually;
use support::*;

const TAG: &str = "p4";
const S: Suite = Suite(TAG);
/// Longest gap between two delivery attempts of the outbox (its backoff tops out at 60 s).
const RETRY_MAX: u64 = 60;


/// a and b, each a contact of the other (b scanned a's ticket).
fn pair(tag: &str) -> (Peer, Peer) {
    let (a, b) = Peer::pair(TAG, tag);
    assert!(a.node.contacts().iter().any(|c| c.did == b.did), "a has b");
    (a, b)
}

/// `e` (existing, online) links a fresh install for real (QR + code); returns it committed and
/// online.
fn link_fresh(e: &Peer, name: &str) -> Peer {
    e.link_fresh(TAG, name, LinkAs { new_pass: PASS, ..LinkAs::default() })
}

/// Registers another (never started) install of `p`'s account with `p` (`add_own_device_for_test`,
/// as in `devices.rs`). Returns its device key.
fn register_sibling(p: &Peer, name: &str) -> String {
    let s = Peer::blank(TAG, name);
    s.node.restore_identity(p.phrase.clone(), name.into(), PASS.into()).unwrap();
    let key = s.key();
    p.node.add_own_device_for_test(s.node.own_attestation_for_test().unwrap()).unwrap();
    key
}

/// `from` calls `to_did` (whose device(s) listed for `to_did`); `ringer` rings; the caller hangs up.
fn call_rings_on(from: &Peer, to_did: &str, ringer: &Peer) {
    let _ = ringer.rx.try_iter().count();
    let call = from.node.call(to_did.to_string()).unwrap();
    assert_eq!(incoming(ringer, 40), call.call_id);
    from.node.hangup(call.call_id).unwrap();
    eventually(20, "call over", || from.node.current_call().is_none() && ringer.node.current_call().is_none());
}

fn own_list(p: &Peer) -> proto::SignedDeviceList {
    serde_json::from_str(&p.node.own_device_list_for_test().unwrap()).unwrap()
}

// ---- link / unlink / restore reach the contact without a call ---------------------------------

// KIND: acceptance (U8)
// B links a second install for real. Without any call or re-share A learns it, and with B's first
// install offline A reaches the new install directly: its call rings there and its message lands
// there.
#[test]
fn linking_a_device_lets_the_contact_reach_it_without_a_call() {
    let (a, b) = pair("link");
    assert_eq!(a.dials(&b), vec![b.key()]);
    let b2 = link_fresh(&b, "link-b2");
    let b2_key = b2.key();
    eventually(30, "A learns B2 from the push", || a.dials(&b).contains(&b2_key));
    assert!(a.dials(&b).contains(&b.key()), "B1 still listed");
    eventually(10, "B logs the push", || b.logged("device list delivered"));

    // B2 has A as a contact (own-device sync) before B1 goes away.
    eventually(60, "B2 synced A", || b2.node.contacts().iter().any(|c| c.did == a.did));
    // Only B2 is up now: A reaches it directly.
    b.node.stop();
    call_rings_on(&a, &b.did, &b2);
    let m = a.node.send_text(b.did.clone(), "to the new device".into(), None).unwrap().id;
    eventually(60, "B2 holds A's message", || holds(&b2, &a.did, &m));
    eventually(30, "A sees it delivered", || delivered(&a, &b.did, &m));
}

// KIND: acceptance (U8)
// B unlinks its second install: A stops dialling it, without a call.
#[test]
fn unlinking_a_device_reaches_the_contact_without_a_call() {
    let (a, b) = pair("unl");
    let b2 = register_sibling(&b, "unl-b2");
    eventually(30, "A learns B2", || a.dials(&b).contains(&b2));
    b.node.unlink_device(b2.clone(), Some(PASS.into())).unwrap();
    let mine = proto::verify_device_list(&own_list(&b), &b.did).unwrap();
    assert!(
        mine.devices
            .iter()
            .all(|e| proto::device_to_text(&e.device) != b2),
        "B's own list dropped B2"
    );
    eventually(30, "A drops B2", || !a.dials(&b).contains(&b2));
    assert_eq!(a.dials(&b), vec![b.key()]);
}

// KIND: acceptance (U8; K1 restore stall, list part)
// B restores over the same directory (fresh device key, same DID). Started again, B pushes the
// list naming only the new key; A dials the new key and its call rings on the restored device.
#[test]
fn a_restored_device_is_reachable_by_the_contact() {
    let (a, mut b) = pair("rst");
    let (old, new) = b.restore_in_place();
    online(&b.node);
    eventually(30, "A learns the restored key", || a.dials(&b) == vec![new.clone()]);
    assert!(!a.dials(&b).contains(&old));
    call_rings_on(&a, &b.did.clone(), &b);
}

// KIND: acceptance (U8: "contacts that were offline get it when either side next reaches the
// other")
// A is offline when B adds a device. When A comes back, B's outbox retry delivers the latest list
// (bounded by the outbox backoff).
#[test]
fn an_offline_contact_gets_the_latest_list_when_it_is_back() {
    let (a, b) = pair("off");
    a.node.stop();
    let b2 = register_sibling(&b, "off-b2");
    let b3 = register_sibling(&b, "off-b3");
    std::thread::sleep(Duration::from_secs(3));
    online(&a.node);
    eventually(RETRY_MAX + 30, "A gets B's latest list", || {
        let d = a.dials(&b);
        d.contains(&b2) && d.contains(&b3)
    });
}

// ---- blocked contacts are excluded -----------------------------------------------------------

// KIND: acceptance (U8 + U5 revised: a blocked contact is excluded from device lists)
// B has contacts A and C and blocks C. B then adds a device: A learns it, C does not (bounded:
// 20 s after A learned it).
#[test]
fn a_blocked_contact_is_not_told() {
    let (a, b) = pair("blk");
    let c = S.peer("blk-c");
    c.node.add_contact(b.node.my_ticket().unwrap()).unwrap();
    b.node.block(c.did.clone()).unwrap();
    let b2 = register_sibling(&b, "blk-b2");
    eventually(30, "A learns B2", || a.dials(&b).contains(&b2));
    let to_a = format!("device list delivered to {}", a.did);
    eventually(10, "B logs the delivery to A", || b.logged(&to_a));
    let to_c = format!("device list delivered to {}", c.did);
    assert!(!within(20, || b.logged(&to_c)), "B never sends C its list");
    assert!(!c.dials(&b).contains(&b2), "C never learns B2");
}

// ---- refusals on the wire ---------------------------------------------------------------------

// KIND: acceptance (U8, U10 every device verifies)
// Lists pushed over the wire (`Msg::Devices`) that are stale, replayed, forged, signed by another
// DID, or more than one day ahead of A's clock leave A's contact entry unchanged; a valid newer
// one (an hour ahead: within the bound) is taken.
#[test]
fn stale_replayed_forged_and_far_future_pushes_are_refused() {
    let (a, b) = pair("rej");
    let bid = identity::recover(&b.phrase).unwrap();
    let mal = identity::recover(&identity::Mnemonic::from_entropy(&[9; 32]).unwrap().to_string()).unwrap();
    let k = |t: &str| proto::device_from_text(t).unwrap();
    let mine = b.key();
    let x = proto::device_to_text(&[7u8; 32]);
    let t = now_ms();
    let good = proto::sign_device_list(&bid, &[(k(&mine), None), (k(&x), None)], t + 1000);
    assert!(b.node.push_device_list_for_test(a.did.clone(), serde_json::to_string(&good).unwrap()).unwrap());
    eventually(20, "A takes the good list", || a.dials(&b) == vec![mine.clone(), x.clone()]);

    let stale = proto::sign_device_list(&bid, &[(k(&mine), None)], t);
    let forged = {
        let m = proto::sign_device_list(&mal, &[(k(&x), None)], t + 5000);
        let mut claim = m.peek().unwrap();
        claim.did = b.did.clone();
        proto::SignedDeviceList::from_parts(
            cryptography::codec::to_bytes(&claim).unwrap(),
            *m.sig(),
        )
    };
    let wrong_did = proto::sign_device_list(&mal, &[(k(&x), None)], t + 5000);
    let far_future = proto::sign_device_list(&bid, &[(k(&x), None)], t + 2 * 24 * 3600 * 1000);
    let max_seq = proto::sign_device_list(&bid, &[(k(&x), None)], u64::MAX);
    for (what, blob) in [
        ("stale", stale),
        ("replayed", good.clone()),
        ("forged", forged),
        ("wrong did", wrong_did),
        ("two days ahead", far_future),
        ("u64::MAX", max_seq),
    ] {
        let before = a.dump();
        b.node.push_device_list_for_test(a.did.clone(), serde_json::to_string(&blob).unwrap()).unwrap();
        assert_eq!(a.dump(), before, "{what}: unchanged");
        assert_eq!(a.dials(&b), vec![mine.clone(), x.clone()], "{what}");
    }
    // Within the bound (an hour ahead) and newer: taken.
    let ahead = proto::sign_device_list(&bid, &[(k(&mine), None)], t + 3600 * 1000);
    b.node.push_device_list_for_test(a.did.clone(), serde_json::to_string(&ahead).unwrap()).unwrap();
    eventually(20, "A takes the hour-ahead list", || a.dials(&b) == vec![mine.clone()]);
    // B's own next list still wins after that (its seq is above the one it last signed).
    assert!(proto::verify_device_list(&own_list(&b), &b.did).unwrap().seq > 0);
}
