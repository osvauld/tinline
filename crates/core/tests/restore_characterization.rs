//! K1: characterization of what a restore-over-existing (forgotten passphrase) does today, given
//! that it replaces this install's device key (`node.rs` `set_identity`: a fresh
//! `new_device_secret()`, the old registry entry marked `removed`) while keeping `account.redb`
//! and `chat.redb`. See `docs/archive/rotation/rotation-baseline-report.md` §4 (O1–O6) and
//! `docs/archive/rotation/rotation-author-key-trace.md` §2–§7.
//!
//! Every test carries one label:
//! - `KIND: limitation`: asserts today's (wrong) outcome; the comment names the expected-correct
//!   one. When production code is fixed, the assertion flips and the test must be rewritten.
//! - `KIND: invariant`: holds today and should keep holding.
//!
//! Real iroh endpoints (needs network, like `restore.rs`, `fanout.rs`, `link.rs`). Waits are
//! bounded polls. "Never arrives" claims are bounded too (stated per test) and, where the code
//! leaves a trace, backed by the log line that explains why.
//! The shared harness is in `support/`. Bugs 1 and
//! 3 of decisions section 5 are fixed on `dev` (P1); their tests are invariants now.
//! P9c (U18): every message version is signed by the author's DID key only (device keys sign no
//! chat content), so a message written before a restore verifies on any device. There are no
//! records signed by a device key any more (pre-P9c data is wiped), so the P1/P8 limitation for
//! such records is gone.

use std::time::{Duration, Instant};

use p2pcore::DeliveryState;

mod support;
use support::eventually_200ms as eventually;
use support::*;

const TAG: &str = "rotk1";

impl Peer {
    /// Whether this node refused any chat batch, history snapshot or own-device batch.
    fn refused_chat(&self) -> bool {
        ["rejected chat batch", "rejected history", "rejected own-device batch"].iter().any(|n| self.logged(n))
    }
}

/// A committed identity; started (`online`) or not.
fn peer(name: &str, started: bool) -> Peer {
    Peer::committed_with(TAG, name, started, true)
}

/// `e` (started) links a fresh install of its account.
fn link_fresh(e: &Peer, name: &str) -> Peer {
    e.link_fresh(TAG, name, LinkAs { wait_existing_done: true, ..LinkAs::default() })
}

/// Every call id `call_answered` placed in this process.
static PLACED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Waits for the incoming call `id`. GL: a device can still have an `Incoming` of an earlier call
/// queued (it rang there too: e.g. A2, once B learned it from A's pushed device list before
/// A1's answer carried it); those are skipped, but only calls this process placed: any other id
/// is a real failure.
fn incoming_call(p: &Peer, id: &str, secs: u64) {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p.rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(Ev::Incoming(c)) if c.call_id == id => return,
            Ok(Ev::Incoming(c)) => {
                assert!(PLACED.lock().unwrap().contains(&c.call_id), "an incoming call nobody placed: {} (waiting for {id})", c.call_id);
            }
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for incoming call {id}"),
        }
    }
}

/// Waits until the address `p` publishes (the DNS record a caller's discovery reads) names the
/// home relay `p` is on now. A device that comes back online may pick another home relay than
/// the one its contacts hold for it; a call placed before the record is updated is dialled at
/// the old relay only and times out (P11: a precondition of the call, not a sleep).
fn published(p: &Peer) {
    use iroh::address_lookup::AddressLookup;
    use n0_future::StreamExt;
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dns = iroh::address_lookup::dns::DnsAddressLookup::n0_dns().build();
    eventually(60, "the published address to name the current home relay", || {
        let st = p.node.status();
        let (Some(relay), Ok(id)) = (st.relay, st.endpoint_id.parse::<iroh::PublicKey>()) else {
            return false;
        };
        rt.block_on(async {
            let Some(mut found) = dns.resolve(id) else { return false };
            match tokio::time::timeout(Duration::from_secs(5), found.next()).await {
                Ok(Some(Ok(item))) => item.to_endpoint_addr().relay_urls().any(|u| u.to_string() == relay),
                _ => false,
            }
        })
    });
}

fn verified(p: &Peer, did: &str) -> bool {
    p.node.contacts().into_iter().find(|c| c.did == did).expect("contact").verified
}

/// `caller` calls `callee`, the callee answers, the caller hangs up. The answer carries the
/// callee's signed `DeviceList` to the caller (as in `link.rs`), so afterwards the caller holds
/// it and dials the callee by that list only (`note_device` no longer adds unknown devices).
fn call_answered(caller: &Peer, callee: &Peer) {
    let call = caller.node.call(callee.did.clone()).unwrap();
    PLACED.lock().unwrap().push(call.call_id.clone());
    incoming_call(callee, &call.call_id, 30);
    callee.node.answer(call.call_id.clone()).unwrap();
    eventually(20, "call active", || caller.node.current_call().is_some());
    std::thread::sleep(Duration::from_millis(500));
    caller.node.hangup(call.call_id).unwrap();
    eventually(20, "call over", || caller.node.current_call().is_none() && callee.node.current_call().is_none());
}

/// The chat fixture: B added A by A's ticket. `a_holds_list`: A also holds B's signed device
/// list (A called B once). B sends `m1`, which A holds and B shows Delivered. With `pending`,
/// A goes offline and B sends `m2`, which stays Pending. Then B restores in place (new device
/// key) and is left stopped; A is left stopped iff `pending`.
struct Chat {
    a: Peer,
    b: Peer,
    m1: String,
    m2: Option<String>,
    new_dev: String,
}

fn chat_fixture(tag: &str, a_holds_list: bool, pending: bool, a_verifies_b: bool) -> Chat {
    let a = peer(&format!("{tag}-a"), true);
    let mut b = peer(&format!("{tag}-b"), true);
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    if a_holds_list {
        call_answered(&a, &b);
    }
    if a_verifies_b {
        a.node.set_verified(b.did.clone(), true).unwrap();
    }
    let m1 = b.node.send_text(a.did.clone(), "m1: delivered before restore".into(), None).unwrap().id;
    eventually(60, "A holds m1", || has(&a, &b.did, &m1));
    eventually(60, "B shows m1 delivered", || ticks(&b, &a.did, &m1) == DeliveryState::Delivered);
    let m2 = pending.then(|| {
        a.node.stop();
        let m2 = b.node.send_text(a.did.clone(), "m2: pending at restore".into(), None).unwrap().id;
        std::thread::sleep(Duration::from_secs(1));
        assert_eq!(ticks(&b, &a.did, &m2), DeliveryState::Pending, "A is offline");
        m2
    });
    let (_old_dev, new_dev) = b.restore_in_place();
    Chat { a, b, m1, m2, new_dev }
}

// ---- 2. O3: a message sent but not yet delivered before the restore ---------------------------

// KIND: invariant
// O3 refuted for this configuration. A holds no signed device list for B, so the restored device
// is added to A's devices on its first chat session (`note_device`). B's first new message builds
// on the stranded `m2` ops, A sees `Pending`, pulls the day as a vouched history snapshot from B
// (engine.rs `need_history` -> `chat_pull_day`), and verifies `m2` by B's DID signature (P9c; no
// device key is involved). So `m2` is delivered after all, alongside `m3`.
// Note: the route depends on B writing something new; the live path never re-sends `m2` itself
// (`export_own_since` exports only the new peer).
#[test]
fn o3_pending_message_reaches_a_contact_that_holds_no_device_list() {
    let f = chat_fixture("o3inv", false, true, false);
    let (a, b, m2) = (&f.a, &f.b, f.m2.clone().unwrap());
    online(&a.node);
    online(&b.node);
    let m3 = b.node.send_text(a.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A holds m3", || has(a, &b.did, &m3));
    eventually(30, "A holds m2", || has(a, &b.did, &m2));
    let all = all_msgs(a, &b.did);
    for id in [&f.m1, &m2, &m3] {
        assert_eq!(all.iter().filter(|m| &m.id == id).count(), 1, "exactly once: {id}");
    }
}

// KIND: invariant
// Bug 1 of decisions section 5 (was `o3_pending_message_stalls_all_chat_...`, a limitation). A
// holds B's signed device list (one earlier call), which names only B's OLD key; the restored key
// is accepted for the session but not added to A's devices (P4 territory, still asserted below).
// B's new `m3` builds on the stranded `m2`, A sees `Pending` and pulls the day as a vouched
// snapshot from B over that live session. Both are DID-signed (P9c), `m2` under B's old writer ID
// and `m3` under the new one (restore = new device ID). Before the P1 fix the snapshot was
// judged with A's stored device list and rejected as a whole, and every later message stalled
// the same way.
// Expected (now asserted): `m2` and `m3` reach A once each, B shows `m3` delivered, and a later
// `m4` reaches A too.
#[test]
fn o3_pending_message_and_new_messages_reach_a_contact_holding_our_device_list() {
    let f = chat_fixture("o3fix", true, true, false);
    let (a, b, m2) = (&f.a, &f.b, f.m2.clone().unwrap());
    online(&a.node);
    online(&b.node);
    let m3 = b.node.send_text(a.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A holds m3", || has(a, &b.did, &m3));
    eventually(30, "A holds m2", || has(a, &b.did, &m2));
    eventually(30, "B shows m3 delivered", || ticks(b, &a.did, &m3) == DeliveryState::Delivered);
    let m4 = b.node.send_text(a.did.clone(), "m4: later".into(), None).unwrap().id;
    eventually(60, "A holds m4", || has(a, &b.did, &m4));
    assert!(!a.logged("rejected history"), "no snapshot of B's was rejected");
    let all = all_msgs(a, &b.did);
    for id in [&f.m1, &m2, &m3, &m4] {
        assert_eq!(all.iter().filter(|m| &m.id == id).count(), 1, "exactly once: {id}");
    }
    // P4: B's restored install pushed its new list; A dials only the new key. GL: the list travels
    // as its own push, apart from the chat, so it may land after m4.
    eventually(30, "A's list for B is B's pushed one (P4)", || a.node.contact_devices_for_test(b.did.clone()) == vec![f.new_dev.clone()]);
}

// KIND: invariant
// Control for the test above: with nothing pending at the restore, the same contact (holding B's
// old device list) accepts B's new messages over the live session, because they only build on
// ops A already holds and are proven by B's DID signature.
#[test]
fn o3_without_a_pending_message_new_messages_reach_a_contact_holding_our_device_list() {
    let f = chat_fixture("o3ctl", true, false, false);
    let (a, b) = (&f.a, &f.b);
    online(&b.node);
    let m3 = b.node.send_text(a.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A holds m3", || has(a, &b.did, &m3));
    eventually(30, "B shows m3 delivered", || ticks(b, &a.did, &m3) == DeliveryState::Delivered);
    assert_eq!(all_msgs(a, &b.did).len(), 2, "m1 and m3, once each");
}

// ---- 3. O4: delivery ticks of pre-restore messages --------------------------------------------

// KIND: invariant
// O4 fixed by P9 (was a limitation asserting that `m1` dropped back to one tick and `m2` stayed
// Pending after A held it). A message's tick marker now names the peer id it was written under
// (`mo`: peer and op count) instead of a bare counter paired with the current device's peer,
// so a pre-restore message keeps the ticks of the peer that wrote it: `m1` stays Delivered,
// `m2` is Pending while A lacks it and Delivered once A acknowledges the old peer's ops.
#[test]
fn o4_ticks_of_pre_restore_messages_follow_the_peer_that_wrote_them() {
    let f = chat_fixture("o4", false, true, false);
    let (a, b, m1, m2) = (&f.a, &f.b, &f.m1, f.m2.clone().unwrap());
    assert_eq!(ticks(b, &a.did, m1), DeliveryState::Delivered, "m1 was Delivered before the restore");
    assert_eq!(ticks(b, &a.did, &m2), DeliveryState::Pending);

    online(&a.node);
    online(&b.node);
    let m3 = b.node.send_text(a.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A holds m2 and m3", || has(a, &b.did, &m3) && has(a, &b.did, &m2));
    eventually(30, "B shows m3 delivered", || ticks(b, &a.did, &m3) == DeliveryState::Delivered);
    eventually(30, "B shows m2 delivered", || ticks(b, &a.did, &m2) == DeliveryState::Delivered);
    assert_eq!(ticks(b, &a.did, m1), DeliveryState::Delivered);
}

// ---- 4. O2: the contact's other device and our pre-restore history ----------------------------

/// A has two installs (A1, A2 linked). B's `m1` reaches only A1 (A2 offline), and A1 answers
/// with `r1`, which B holds (so `r1`'s ops build on `m1`'s, in A1's doc and in B's). B restores;
/// a call from the restored B rings A2, whose answer path takes B's new signed device list: A2
/// knows only B's NEW key and does not hold `r1` (it may hold `m1`, which B sends itself since P10b). Left: B and A2 started, A1 stopped.
struct Sib {
    a1: Peer,
    a2: Peer,
    b: Peer,
    m1: String,
    r1: String,
}

fn sibling_fixture(tag: &str) -> Sib {
    let a1 = peer(&format!("{tag}-a1"), true);
    let mut b = peer(&format!("{tag}-b"), true);
    b.node.add_contact(a1.node.my_ticket().unwrap()).unwrap();
    let a2 = link_fresh(&a1, &format!("{tag}-a2"));
    eventually(40, "A2 has B", || a2.node.contacts().iter().any(|c| c.did == b.did));
    // B learns A2 from A's device list on A1's answer.
    call_answered(&b, &a1);
    let a2_dev = a2.node.device_key_for_test().unwrap();
    eventually(20, "B knows A2", || b.node.contact_devices_for_test(a1.did.clone()).contains(&a2_dev));

    a2.node.stop();
    let m1 = b.node.send_text(a1.did.clone(), "m1: only A1 holds it".into(), None).unwrap().id;
    eventually(60, "A1 holds m1", || has(&a1, &b.did, &m1));
    let r1 = a1.node.send_text(b.did.clone(), "r1: A1 answers m1".into(), None).unwrap().id;
    eventually(60, "B holds r1", || has(&b, &a1.did, &r1));
    a1.node.stop();

    let (_old_dev, new_dev) = b.restore_in_place();
    online(&b.node);
    online(&a2.node);
    published(&a2);
    call_answered(&b, &a2);
    eventually(20, "A2 knows only B's new key", || a2.node.contact_devices_for_test(b.did.clone()) == vec![new_dev.clone()]);
    // Since P10b the restored B itself sends A2 its own undelivered `m1` (old writer ID), so A2
    // may already hold `m1` here; `r1` is A1's and B never forwards a contact's ops.
    assert!(!has(&a2, &b.did, &r1), "precondition: A2 does not hold r1");
    Sib { a1, a2, b, m1, r1 }
}

// KIND: invariant
// Bug 3 of decisions section 5, the snapshot route. The restored B writes `m3` to A; A2 gets it
// live, but it builds on `m1`/`r1`, so A2 pulls B's vouched snapshot of the day. It holds `m1`
// (B's old install, which A2 never knew), `r1` (A1, one of A's own devices) and `m3` (B's new
// install). Before P1 a message A2 could not verify made it reject the whole snapshot. P9c: all
// three are proven by DID signatures, so the snapshot is accepted whole.
#[test]
fn o2_contacts_other_device_gets_our_new_messages_although_it_never_knew_our_old_key() {
    let f = sibling_fixture("o2snap");
    let (a2, b) = (&f.a2, &f.b);
    let m3 = b.node.send_text(f.a1.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A2 holds m3", || has(a2, &b.did, &m3));
    eventually(30, "A2 holds r1", || has(a2, &b.did, &f.r1));
    eventually(30, "B shows m3 delivered", || ticks(b, &f.a1.did, &m3) == DeliveryState::Delivered);
    assert!(!a2.refused_chat(), "the snapshot was not rejected");
}

// KIND: invariant
// Bug 3 of decisions section 5, the relay route. With B stopped, A1 comes back and relays the day
// to A2 (own-device sync): one batch with `m1` (B's old install, unknown to A2) and A1's own `r1`.
// Before P1 own-device sync rejected that batch as a whole. Now (P9c) both are DID-signed and
// accepted: `r1` and `m1` reach A2 from A1; the account doc syncs as before; A1 learns B's new
// list from A2.
#[test]
fn o2_contacts_other_device_accepts_the_verifiable_messages_its_sibling_relays() {
    let f = sibling_fixture("o2rel");
    let (a1, a2, b) = (&f.a1, &f.a2, &f.b);
    b.node.stop();
    online(&a1.node);
    a1.node.rename_contact(b.did.clone(), Some("Bee".into())).unwrap();
    a2.node.sync_now_for_test();
    eventually(60, "the account doc syncs A1 -> A2", || {
        a2.node.contacts().iter().any(|c| c.did == b.did && c.alias.as_deref() == Some("Bee"))
    });
    eventually(60, "A2 holds r1, relayed by A1 with m1", || has(a2, &b.did, &f.r1));
    eventually(20, "A2 holds m1 too", || has(a2, &b.did, &f.m1));
    assert!(has(a1, &b.did, &f.m1), "A1 keeps showing m1");
    assert!(!a2.refused_chat());
    let new_dev = a2.node.contact_devices_for_test(b.did.clone());
    eventually(20, "A1 learns B's new list from A2", || a1.node.contact_devices_for_test(b.did.clone()) == new_dev);
}

// KIND: invariant
// P8, strict since P9c. `m1` was written by B before its restore, under B's old writer ID, and
// is signed by B's DID key, which does not change on restore. A2 verifies `m1` against B's DID,
// so it shows `m1` by both routes: B's vouched snapshot (pulled because `m3` builds on `m1`),
// and A1 relaying the day.
#[test]
fn o2_a_pre_restore_message_reaches_a_device_that_never_knew_the_old_key() {
    let f = sibling_fixture("o2did");
    let (a1, a2, b) = (&f.a1, &f.a2, &f.b);
    // Route 1: B's vouched snapshot.
    let m3 = b.node.send_text(a1.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A2 holds m3", || has(a2, &b.did, &m3));
    eventually(30, "A2 shows m1 from B's snapshot", || has(a2, &b.did, &f.m1));
    assert!(!a2.refused_chat(), "nothing was rejected");
    // Route 2: A1 relays the day (a second copy of the same ops changes nothing).
    b.node.stop();
    online(&a1.node);
    a1.node.rename_contact(b.did.clone(), Some("Bee".into())).unwrap();
    a2.node.sync_now_for_test();
    eventually(60, "the account doc syncs A1 -> A2", || {
        a2.node.contacts().iter().any(|c| c.did == b.did && c.alias.as_deref() == Some("Bee"))
    });
    assert!(has(a2, &b.did, &f.m1) && has(a2, &b.did, &f.r1) && has(a2, &b.did, &m3));
    assert!(has(a1, &b.did, &f.m1), "A1 keeps showing m1");
    assert!(!a2.refused_chat());
}

// ---- 5. O6: the contact's `verified` flag -----------------------------------------------------

// KIND: limitation
// O6 confirmed. A marked B verified. B restores (same DID, same safety number) and its first chat
// session presents the new key; `note_device` clears `verified` for any device it has not seen.
// (Before P4 A also dialled the dead key, since no signed list retired it; now B's pushed list
// does, and A dials only the new key.)
// (Observed as well when A holds B's device list: the flag is cleared although the new key is
// not even added.)
// Expected-correct: POLICY-GATED (verified-DID policy; handoff "enforce verified-DID policy").
// One candidate: a DID-attested key change on the same DID keeps `verified`, since the safety
// number is derived from DIDs only. This test only records that today the flag is lost.
#[test]
fn o6_contacts_verified_flag_is_cleared_by_our_restore() {
    let f = chat_fixture("o6", false, false, true);
    let (a, b) = (&f.a, &f.b);
    assert!(verified(a, &b.did), "verified before the restore");
    online(&b.node);
    let m3 = b.node.send_text(a.did.clone(), "m3: after restore".into(), None).unwrap().id;
    eventually(90, "A holds m3", || has(a, &b.did, &m3));
    assert!(!verified(a, &b.did), "the restore cleared verified");
    // P4: B's pushed list retires the dead key; A dials only the new one. GL: the list travels
    // apart from the chat (its own push), so it may land after `m3`.
    eventually(30, "A dials the new key only", || a.node.contact_devices_for_test(b.did.clone()) == vec![f.new_dev.clone()]);
}

// KIND: invariant
// What `verified` attests (the safety number) does not depend on the device key: a restore
// changes neither side's safety number. No network needed.
#[test]
fn o6_safety_numbers_do_not_change_on_restore() {
    let a = peer("o6sn-a", false);
    let mut b = peer("o6sn-b", false);
    let on_a = a.node.safety_number(b.did.clone()).unwrap();
    let on_b = b.node.safety_number(a.did.clone()).unwrap();
    assert_eq!(on_a, on_b);
    b.restore_in_place();
    assert_eq!(b.node.safety_number(a.did.clone()).unwrap(), on_b);
    assert_eq!(a.node.safety_number(b.did.clone()).unwrap(), on_a);
}
