//! R1a: acceptance tests for endpoint rotation (U1, U2, U5 revised, U8 of
//! `docs/design/rotation-open-decisions.md` on `endpoint-rotation-blocking`), on real nodes over
//! real iroh endpoints (needs network, like `calls.rs`). These define "done" for a feature that
//! is not built yet. The 14 placeholders for the unbuilt rotation API now live in
//! `docs/design/rotation-acceptance-checklist.md`; only tests that run today stay here.
//!
//! Target behaviour:
//! - U2: each device has a PRIVATE endpoint (known to accepted contacts only; what tickets and
//!   contacts dial) and a separate GROUP endpoint (known to group hosts only). Same DID.
//! - The private endpoint rotates on a configurable interval (Settings: every day / 3 days /
//!   week; shows last and next change and "contacts told N of M"). Blocking never triggers it.
//! - U1/U8: after a rotation the device sends one DID-signed note (device list + private
//!   endpoints, higher seq) to every unblocked contact; online contacts switch without user
//!   action; offline ones catch up when either side next reaches the other. No server, no DHT,
//!   no derived future addresses.
//! - The old endpoint keeps accepting for a grace window, then stops.
//! - A blocked contact is not told and loses reach after the next rotation (U5 revised).
//! - Linked devices rotate independently; contacts reach all of them.
//! - A crash between rotation steps leaves one consistent, reachable state.
//! - Both sides rotated while apart, no shared group: "lost contact" is reported (not a silent
//!   hang); re-sharing a fresh ticket/QR restores the SAME contact with history and verification.
//! - The DID never changes; the writer ID (P9b: tied to the install, not a key) never changes.
//!
//! Every test carries `// KIND: acceptance` and a line naming the decision it enforces.
//!
//! Note: today the device key IS the (only) iroh endpoint id, and `contact_devices_for_test`
//! lists the keys a contact dials. Under U2 that list must hold private endpoints only.

use std::time::Duration;

mod support;
use support::eventually_200ms as eventually;
use support::*;

const TAG: &str = "r1a-rot";

/// a and b, each a contact of the other (b scanned a's ticket).
fn pair(tag: &str) -> (Peer, Peer) {
    Peer::pair(TAG, tag)
}

/// Drops the node and opens the same data directory again (a process restart).
fn restart(p: &mut Peer) {
    p.reopen();
    // As in writer_id.rs: `chat_open` does not retry while the old tasks still hold chat.redb.
    std::thread::sleep(Duration::from_secs(2));
    p.node.unlock(p.pass.clone()).unwrap();
    online(&p.node);
}

fn dials(p: &Peer, did: &str) -> Vec<String> {
    p.node.contact_devices_for_test(did.to_string())
}

/// `from` sends a message to `to` and it arrives and is acknowledged.
fn chat_works(from: &Peer, to: &Peer, text: &str) {
    let m = from.node.send_text(to.did.clone(), text.into(), None).unwrap().id;
    eventually(60, &format!("'{text}' arrives"), || holds(to, &from.did, &m));
    eventually(30, &format!("'{text}' delivered"), || delivered(from, &to.did, &m));
}

// KIND: acceptance
// Decision: U1 (rotation is periodic, never implicit) + DID stability — with no rotation due, a
// restart keeps the endpoint, the DID and the writer ID, and the contact keeps reaching us.
#[test]
fn endpoint_is_stable_without_a_due_rotation() {
    let (mut a, b) = pair("stable");
    let (dev, wid) = (a.node.device_key_for_test().unwrap(), a.node.writer_id_for_test());
    assert_eq!(dials(&b, &a.did), vec![dev.clone()]);
    restart(&mut a);
    assert_eq!(a.node.device_key_for_test().unwrap(), dev);
    assert_eq!(a.node.profile().unwrap().did, a.did);
    eventually(20, "chat store open", || a.node.writer_id_for_test().is_some());
    assert_eq!(a.node.writer_id_for_test(), wid);
    chat_works(&b, &a, "after restart");
}
