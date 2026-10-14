//! R1a: acceptance tests for endpoint rotation (U1, U2, U5 revised, U8 of
//! `docs/design/rotation-open-decisions.md` on `endpoint-rotation-blocking`), on real nodes over
//! real iroh endpoints (needs network, like `calls.rs`). These define "done" for a feature that
//! is not built yet.
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
//! Tests needing missing API call stubs in `mod harness` (`unimplemented!("R1: ...")`) and are
//! `#[ignore = "R1: needs ..."]`. Tests that run today assert target behaviour that already holds.
//! Every test carries `// KIND: acceptance` and a line naming the decision it enforces.
//!
//! Note: today the device key IS the (only) iroh endpoint id, and `contact_devices_for_test`
//! lists the keys a contact dials. Under U2 that list must hold private endpoints only.

use std::time::{Duration, Instant};

use p2pcore::CallState;

mod support;
use support::eventually_200ms as eventually;
use support::*;

/// Grace window used by the tests (production value is open).
const GRACE: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------------------------
// Test-only harness: the API R1 must provide. Every function is a stub today.
// ---------------------------------------------------------------------------------------------

/// Proposed R1 API for endpoint rotation. Each stub documents the behaviour the tests expect.
#[allow(dead_code)]
mod harness {
    use std::time::Duration;

    use p2pcore::{Error, Node};

    /// This device's two transport identities (U2), text form as in `device_key_for_test`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Endpoints {
        /// Known only to accepted contacts; in tickets/QR, device-list notes and 1:1 traffic.
        pub private: String,
        /// Known only to group hosts (U7); never in tickets, contact notes or 1:1 traffic.
        pub group: String,
    }

    /// Settings choices for the private-endpoint rotation interval.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RotationInterval {
        Day,
        ThreeDays,
        Week,
    }

    /// What Settings shows.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RotationStatus {
        pub interval: RotationInterval,
        /// Unix seconds of the last private-endpoint change; `None` before the first.
        pub last_change: Option<u64>,
        /// Unix seconds of the next scheduled change (`last_change` or creation + interval).
        pub next_change: u64,
        /// "Contacts told N of M" for the current endpoint: N = unblocked contacts that
        /// acknowledged the current note, M = unblocked contacts.
        pub contacts_told: u32,
        pub contacts_total: u32,
    }

    /// Reachability of a contact as the UI reports it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Reach {
        /// We hold an endpoint of theirs that answered, and they hold our current one.
        Ok,
        /// Something is still to be told or fetched (one side offline); retrying.
        CatchingUp,
        /// Both sides rotated while apart, no shared group to recover through: shows
        /// "Lost contact — re-share ticket/QR". Never a silent hang.
        LostContact,
    }

    /// Result of dialling one specific endpoint as `from`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Probe {
        /// The endpoint answered and accepted `from` at the app layer.
        Accepted,
        /// The endpoint answered but refused, with the reason given.
        Refused(String),
        /// Nothing answers at that endpoint (retired or offline).
        Unreachable,
    }

    /// Steps of one private-endpoint rotation, in order, for crash injection.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RotationStep {
        /// New key generated in memory.
        KeyGenerated,
        /// New key durably stored next to the old one (both accept).
        KeyStored,
        /// Signed note handed to some contacts but not all.
        NotesPartlySent,
        /// All reachable contacts told; waiting for the grace window.
        NotesSent,
        /// Old key retired (deleted).
        OldRetired,
    }

    pub fn endpoints(_node: &Node) -> Endpoints {
        unimplemented!("R1: two endpoints per device (U2)")
    }

    pub fn set_rotation_interval(_node: &Node, _every: RotationInterval) -> Result<(), Error> {
        unimplemented!("R1: rotation interval setting")
    }

    pub fn rotation_status(_node: &Node) -> RotationStatus {
        unimplemented!("R1: rotation status for Settings")
    }

    /// For tests: moves the rotation scheduler's clock forward by `by` (and runs anything due).
    pub fn advance_rotation_clock_for_test(_node: &Node, _by: Duration) {
        unimplemented!("R1: rotation clock hook")
    }

    /// For tests: performs the periodic private rotation now, as if it were due. Returns the new
    /// private endpoint. Sends the DID-signed note (U1/U8) to every unblocked contact.
    pub fn rotate_private_now_for_test(_node: &Node) -> Result<String, Error> {
        unimplemented!("R1: forced rotation hook")
    }

    /// For tests: the window during which a superseded endpoint still accepts.
    pub fn set_grace_for_test(_node: &Node, _grace: Duration) {
        unimplemented!("R1: grace window hook")
    }

    /// For tests: the next rotation simulates a process crash right after `step` (the node stops
    /// with whatever reached the disk); reopening the directory must recover.
    pub fn crash_rotation_after_for_test(_node: &Node, _step: RotationStep) {
        unimplemented!("R1: crash injection")
    }

    /// For tests: dials `endpoint` (text form) on the 1:1 protocol as `from`.
    pub fn probe_endpoint_for_test(_from: &Node, _endpoint: &str) -> Probe {
        unimplemented!("R1: endpoint probe")
    }

    pub fn contact_reach(_node: &Node, _did: &str) -> Reach {
        unimplemented!("R1: contact reachability / lost contact")
    }

    /// Visible block (see `blocking_acceptance.rs`); here only its effect on rotation matters.
    pub fn block(_node: &Node, _did: &str) -> Result<(), Error> {
        unimplemented!("R1: visible block")
    }
    pub fn unblock(_node: &Node, _did: &str) -> Result<(), Error> {
        unimplemented!("R1: unblock")
    }

    // --- group stubs (U1 group-host recovery) ---
    pub fn create_group(_node: &Node, _name: &str) -> Result<String, Error> {
        unimplemented!("R1: group chat API")
    }
    pub fn add_group_member(_host: &Node, _group: &str, _did: &str) -> Result<(), Error> {
        unimplemented!("R1: group chat API")
    }
    /// Every endpoint (text form) that appeared in group traffic `node` received or sent.
    pub fn endpoints_seen_in_group_traffic_for_test(_node: &Node, _group: &str) -> Vec<String> {
        unimplemented!("R1: group chat API")
    }
}

use harness::{Probe, Reach, RotationInterval, RotationStep};

// ---------------------------------------------------------------------------------------------
// Real-node helpers (the shared harness is in `support/`).
// ---------------------------------------------------------------------------------------------

const TAG: &str = "r1a-rot";

fn peer(name: &str) -> Peer {
    Peer::new(TAG, name)
}

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

/// `e` (existing) links a fresh install; returns it committed and online.
fn link_fresh(e: &Peer, name: &str) -> Peer {
    e.link_fresh(TAG, name, LinkAs::default())
}

fn ended(p: &Peer, id: &str, secs: u64) -> String {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p.rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(Ev::State(i, CallState::Ended { reason })) if i == id => return reason,
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for call {id} to end"),
        }
    }
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

/// `from` calls `to`; it rings; hang up.
fn call_rings(from: &Peer, to: &Peer) {
    let _ = to.rx.try_iter().count();
    let call = from.node.call(to.did.clone()).unwrap();
    assert_eq!(incoming(to, 30), call.call_id);
    from.node.hangup(call.call_id.clone()).unwrap();
    ended(from, &call.call_id, 20);
}

/// Both sides rotate while apart (each one's old endpoint retired before the other is back),
/// so neither holds a live endpoint of the other. Leaves both online.
fn rotate_apart(a: &Peer, b: &Peer) {
    for p in [a, b] {
        harness::set_grace_for_test(&p.node, GRACE);
    }
    b.node.stop();
    harness::rotate_private_now_for_test(&a.node).unwrap();
    harness::advance_rotation_clock_for_test(&a.node, GRACE * 2);
    a.node.stop();
    online(&b.node);
    harness::rotate_private_now_for_test(&b.node).unwrap();
    harness::advance_rotation_clock_for_test(&b.node, GRACE * 2);
    online(&a.node);
}

// ---------------------------------------------------------------------------------------------
// Tests that run today
// ---------------------------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------------------------
// Target behaviour that needs the R1 API (stubs)
// ---------------------------------------------------------------------------------------------

// KIND: acceptance
// Decision: U2 — two endpoints per device: the private one is what tickets and contacts dial;
// the group one is distinct and never appears in 1:1 state; same DID.
#[test]
#[ignore = "R1: needs two endpoints per device (U2)"]
fn private_endpoint_is_what_contacts_dial_and_differs_from_the_group_endpoint() {
    let (a, b) = pair("u2");
    let ep = harness::endpoints(&a.node);
    assert_ne!(ep.private, ep.group);
    assert_eq!(dials(&b, &a.did), vec![ep.private.clone()]);
    assert_eq!(b.node.contacts()[0].device, ep.private);
    assert!(!format!("{:?}", b.node.contacts()).contains(&ep.group));
    assert_eq!(harness::probe_endpoint_for_test(&b.node, &ep.private), Probe::Accepted);
    // The group endpoint does not serve the 1:1 protocol to a contact.
    assert_ne!(harness::probe_endpoint_for_test(&b.node, &ep.group), Probe::Accepted);
}

// KIND: acceptance
// Decision: U2 + U7 — group traffic never reveals a member's private endpoint.
#[test]
#[ignore = "R1: needs group chat API + two endpoints (U2, U7)"]
fn group_traffic_never_carries_the_private_endpoint() {
    let h = peer("u2g-h");
    let a = peer("u2g-a");
    a.node.add_contact(h.node.my_ticket().unwrap()).unwrap();
    let g = harness::create_group(&h.node, "g").unwrap();
    harness::add_group_member(&h.node, &g, &a.did).unwrap();
    let ep = harness::endpoints(&a.node);
    std::thread::sleep(Duration::from_secs(5));
    for p in [&h, &a] {
        let seen = harness::endpoints_seen_in_group_traffic_for_test(&p.node, &g);
        assert!(!seen.contains(&ep.private), "private endpoint leaked into group traffic");
    }
    assert!(harness::endpoints_seen_in_group_traffic_for_test(&h.node, &g).contains(&ep.group));
}

// KIND: acceptance
// Decision: U1 (periodic, configurable rotation; Settings shows interval, last/next change) +
// U5 revised (nothing else triggers it).
#[test]
#[ignore = "R1: needs rotation scheduler + clock hook"]
fn rotation_follows_the_configured_interval() {
    let (a, _b) = pair("sched");
    for every in [RotationInterval::Day, RotationInterval::ThreeDays, RotationInterval::Week] {
        harness::set_rotation_interval(&a.node, every).unwrap();
        assert_eq!(harness::rotation_status(&a.node).interval, every);
    }
    harness::set_rotation_interval(&a.node, RotationInterval::Day).unwrap();
    let day = Duration::from_secs(86_400);
    let before = harness::endpoints(&a.node).private;
    let s0 = harness::rotation_status(&a.node);
    harness::advance_rotation_clock_for_test(&a.node, day - Duration::from_secs(60));
    assert_eq!(harness::endpoints(&a.node).private, before, "rotated early");
    harness::advance_rotation_clock_for_test(&a.node, Duration::from_secs(120));
    eventually(10, "rotated", || harness::endpoints(&a.node).private != before);
    let s1 = harness::rotation_status(&a.node);
    let last = s1.last_change.expect("last change recorded");
    assert!(s0.last_change.map_or(true, |l| last > l));
    assert_eq!(s1.next_change, last + day.as_secs());
    assert_eq!(a.node.profile().unwrap().did, a.did, "the DID never rotates");
}

// KIND: acceptance
// Decision: U1 + U8 — after a rotation an online contact receives the DID-signed note and keeps
// chatting and calling over the new endpoint without user action; Settings counts it as told;
// DID and writer ID unchanged.
#[test]
#[ignore = "R1: needs forced rotation + rotation status"]
fn online_contact_switches_to_the_new_endpoint_without_user_action() {
    let (a, b) = pair("online");
    chat_works(&b, &a, "before");
    let (old, wid) = (harness::endpoints(&a.node).private, a.node.writer_id_for_test());
    let new = harness::rotate_private_now_for_test(&a.node).unwrap();
    assert_ne!(new, old);
    eventually(30, "b learns the new endpoint", || dials(&b, &a.did).contains(&new));
    eventually(30, "told 1 of 1", || {
        let s = harness::rotation_status(&a.node);
        (s.contacts_told, s.contacts_total) == (1, 1)
    });
    harness::set_grace_for_test(&a.node, Duration::ZERO);
    harness::advance_rotation_clock_for_test(&a.node, Duration::from_secs(1));
    assert_eq!(harness::probe_endpoint_for_test(&b.node, &old), Probe::Unreachable, "old endpoint retired");
    chat_works(&b, &a, "to the new endpoint");
    chat_works(&a, &b, "from the new endpoint");
    call_rings(&b, &a);
    assert_eq!(a.node.writer_id_for_test(), wid, "rotation must not change the writer ID (P9b)");
    assert_eq!(a.node.profile().unwrap().did, a.did);
    assert_eq!(harness::contact_reach(&b.node, &a.did), Reach::Ok);
}

// KIND: acceptance
// Decision: U1 — a contact offline during the rotation catches up when it comes back (the side
// whose address is still live retries), before the old endpoint is retired or after.
#[test]
#[ignore = "R1: needs forced rotation"]
fn offline_contact_catches_up_when_it_comes_back() {
    let (a, b) = pair("offback");
    harness::set_grace_for_test(&a.node, GRACE);
    b.node.stop();
    let new = harness::rotate_private_now_for_test(&a.node).unwrap();
    harness::advance_rotation_clock_for_test(&a.node, GRACE * 2); // old endpoint gone
    assert_eq!(harness::rotation_status(&a.node).contacts_told, 0);
    online(&b.node);
    eventually(60, "b learns the new endpoint", || dials(&b, &a.did).contains(&new));
    eventually(30, "told 1 of 1", || harness::rotation_status(&a.node).contacts_told == 1);
    chat_works(&b, &a, "caught up");
    call_rings(&b, &a);
}

// KIND: acceptance
// Decision: U1 — the rotating side was the one offline: it rotated, could not tell the contact,
// went offline; when it comes back it tells the contact (whose address is still live).
#[test]
#[ignore = "R1: needs forced rotation"]
fn offline_contact_catches_up_when_the_rotating_side_comes_back() {
    let (a, b) = pair("rotback");
    harness::set_grace_for_test(&a.node, GRACE);
    b.node.stop();
    let new = harness::rotate_private_now_for_test(&a.node).unwrap();
    harness::advance_rotation_clock_for_test(&a.node, GRACE * 2);
    a.node.stop();
    online(&b.node);
    std::thread::sleep(Duration::from_secs(3));
    online(&a.node);
    eventually(60, "b learns the new endpoint", || dials(&b, &a.did).contains(&new));
    chat_works(&b, &a, "caught up");
}

// KIND: acceptance
// Decision: U1 — the superseded endpoint keeps accepting during the grace window, then stops.
#[test]
#[ignore = "R1: needs forced rotation + grace window"]
fn old_endpoint_stops_accepting_after_the_grace_window() {
    let (a, b) = pair("grace");
    harness::set_grace_for_test(&a.node, GRACE);
    let old = harness::endpoints(&a.node).private;
    let new = harness::rotate_private_now_for_test(&a.node).unwrap();
    assert_eq!(harness::probe_endpoint_for_test(&b.node, &old), Probe::Accepted, "within grace");
    assert_eq!(harness::probe_endpoint_for_test(&b.node, &new), Probe::Accepted);
    harness::advance_rotation_clock_for_test(&a.node, GRACE + Duration::from_secs(1));
    eventually(10, "old endpoint retired", || harness::probe_endpoint_for_test(&b.node, &old) == Probe::Unreachable);
    assert_eq!(harness::probe_endpoint_for_test(&b.node, &new), Probe::Accepted);
}

// KIND: acceptance
// Decision: U5 revised + U1 + U8 — a blocked contact is excluded from the rotation note, never
// learns the new endpoint and loses reach once the old one is retired; the block itself did not
// rotate; "told N of M" counts unblocked contacts only.
#[test]
#[ignore = "R1: needs visible block + forced rotation"]
fn blocked_contact_does_not_learn_the_new_endpoint_and_loses_reach() {
    let a = peer("blkrot-a");
    let b = peer("blkrot-b");
    let c = peer("blkrot-c");
    for p in [&b, &c] {
        p.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    }
    harness::set_grace_for_test(&a.node, GRACE);
    let old = harness::endpoints(&a.node).private;
    let last = harness::rotation_status(&a.node).last_change;
    harness::block(&a.node, &b.did).unwrap();
    assert_eq!(harness::endpoints(&a.node).private, old, "a block must not trigger a rotation");
    assert_eq!(harness::rotation_status(&a.node).last_change, last);

    let new = harness::rotate_private_now_for_test(&a.node).unwrap();
    eventually(30, "c learns", || dials(&c, &a.did).contains(&new));
    std::thread::sleep(Duration::from_secs(5));
    assert!(!dials(&b, &a.did).contains(&new), "the blocked contact learned the new endpoint");
    let s = harness::rotation_status(&a.node);
    assert_eq!((s.contacts_told, s.contacts_total), (1, 1));
    harness::advance_rotation_clock_for_test(&a.node, GRACE * 2);
    assert_eq!(harness::probe_endpoint_for_test(&b.node, &old), Probe::Unreachable);
    // Even if b somehow had the new endpoint, it would be refused as blocked.
    assert_eq!(harness::probe_endpoint_for_test(&b.node, &new), Probe::Refused("blocked".into()));
    let call = b.node.call(a.did.clone()).unwrap();
    ended(&b, &call.call_id, 70);
    chat_works(&c, &a, "c still reaches a");
}

// KIND: acceptance
// Decision: U5 revised / 8b Q5 + U8 — on unblock the contact is sent the CURRENT private
// endpoint (rotated during the block) and can reach us again.
#[test]
#[ignore = "R1: needs visible block + unblock + forced rotation"]
fn unblocked_contact_gets_the_current_endpoint() {
    let (a, b) = pair("unblkrot");
    harness::set_grace_for_test(&a.node, GRACE);
    harness::block(&a.node, &b.did).unwrap();
    let new = harness::rotate_private_now_for_test(&a.node).unwrap();
    harness::advance_rotation_clock_for_test(&a.node, GRACE * 2);
    harness::unblock(&a.node, &b.did).unwrap();
    eventually(60, "b learns the current endpoint", || dials(&b, &a.did).contains(&new));
    chat_works(&b, &a, "back");
}

// KIND: acceptance
// Decision: U2 + U8 — linked devices rotate independently; a contact holds the current private
// endpoint of each and reaches both (calls ring both, messages reach both).
#[test]
#[ignore = "R1: needs forced rotation + device-list push (P4)"]
fn linked_devices_rotate_independently_and_contacts_reach_both() {
    let a = peer("lnk-a");
    let e = peer("lnk-e");
    a.node.add_contact(e.node.my_ticket().unwrap()).unwrap();
    let n = link_fresh(&e, "lnk-n");
    eventually(40, "N has A", || n.node.contacts().iter().any(|c| c.did == a.did));
    let n_old = harness::endpoints(&n.node).private;
    eventually(60, "A knows N (pushed)", || dials(&a, &e.did).contains(&n_old));

    let e_new = harness::rotate_private_now_for_test(&e.node).unwrap();
    assert_eq!(harness::endpoints(&n.node).private, n_old, "rotating E must not rotate N");
    eventually(30, "A learns E's new endpoint", || dials(&a, &e.did).contains(&e_new));
    assert!(dials(&a, &e.did).contains(&n_old), "N's endpoint must survive E's note");
    let n_new = harness::rotate_private_now_for_test(&n.node).unwrap();
    eventually(30, "A learns N's new endpoint", || {
        let d = dials(&a, &e.did);
        d.contains(&e_new) && d.contains(&n_new)
    });

    let call = a.node.call(e.did.clone()).unwrap();
    assert_eq!(incoming(&e, 30), call.call_id);
    assert_eq!(incoming(&n, 30), call.call_id);
    a.node.hangup(call.call_id).unwrap();
    let m = a.node.send_text(e.did.clone(), "to both".into(), None).unwrap().id;
    eventually(60, "E holds it", || holds(&e, &a.did, &m));
    eventually(60, "N holds it", || holds(&n, &a.did, &m));
}

// KIND: acceptance
// Decision: U1 + U8 (storage ordering, section 3b) — a crash after any rotation step leaves one
// consistent state after restart: the current endpoint is either the old or the new one (never
// a third), the contact reaches it without user action, and DID/writer ID are unchanged.
#[test]
#[ignore = "R1: needs forced rotation + crash injection"]
fn restart_mid_rotation_leaves_a_consistent_reachable_state() {
    for (i, step) in [
        RotationStep::KeyGenerated,
        RotationStep::KeyStored,
        RotationStep::NotesPartlySent,
        RotationStep::NotesSent,
        RotationStep::OldRetired,
    ]
    .into_iter()
    .enumerate()
    {
        let (mut a, b) = pair(&format!("crash{i}"));
        let (old, wid) = (harness::endpoints(&a.node).private, a.node.writer_id_for_test());
        harness::crash_rotation_after_for_test(&a.node, step);
        let _ = harness::rotate_private_now_for_test(&a.node);
        restart(&mut a);
        let cur = harness::endpoints(&a.node).private;
        if step == RotationStep::KeyGenerated {
            assert_eq!(cur, old, "{step:?}: a key never stored must not become current");
        }
        if cur != old {
            eventually(60, &format!("{step:?}: b learns the current endpoint"), || dials(&b, &a.did).contains(&cur));
            eventually(60, &format!("{step:?}: told 1 of 1"), || harness::rotation_status(&a.node).contacts_told == 1);
        }
        assert_eq!(harness::probe_endpoint_for_test(&b.node, &cur), Probe::Accepted, "{step:?}");
        chat_works(&b, &a, &format!("after crash at {step:?}"));
        call_rings(&b, &a);
        assert_eq!(a.node.writer_id_for_test(), wid, "{step:?}");
        assert_eq!(a.node.profile().unwrap().did, a.did, "{step:?}");
        assert_eq!(harness::contact_reach(&b.node, &a.did), Reach::Ok, "{step:?}");
    }
}

// KIND: acceptance
// Decision: U1 — both sides rotated while apart, no shared group: each reports "lost contact"
// (not a silent hang, not "offline").
#[test]
#[ignore = "R1: needs forced rotation + lost-contact state"]
fn both_rotated_apart_without_a_shared_group_reports_lost_contact() {
    let (a, b) = pair("lost");
    rotate_apart(&a, &b);
    let m = b.node.send_text(a.did.clone(), "anyone there?".into(), None).unwrap().id;
    eventually(120, "a reports lost contact", || harness::contact_reach(&a.node, &b.did) == Reach::LostContact);
    eventually(120, "b reports lost contact", || harness::contact_reach(&b.node, &a.did) == Reach::LostContact);
    assert!(!delivered(&b, &a.did, &m));
}

// KIND: acceptance
// Decision: U1 (manual re-share) — after "lost contact", scanning a fresh ticket/QR restores the
// SAME contact (one entry, same DID) with its history and verification, and both directions work.
#[test]
#[ignore = "R1: needs forced rotation + lost-contact state"]
fn readding_by_fresh_ticket_restores_the_same_contact_with_history() {
    let (a, b) = pair("readd");
    let early = b.node.send_text(a.did.clone(), "history".into(), None).unwrap().id;
    eventually(60, "history delivered", || delivered(&b, &a.did, &early));
    b.node.set_verified(a.did.clone(), true).unwrap();
    a.node.set_verified(b.did.clone(), true).unwrap();
    rotate_apart(&a, &b);
    eventually(120, "b reports lost contact", || harness::contact_reach(&b.node, &a.did) == Reach::LostContact);

    let c = b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    assert_eq!(c.did, a.did);
    assert_eq!(b.node.contacts().iter().filter(|c| c.did == a.did).count(), 1, "no duplicate contact");
    assert_eq!(a.node.contacts().iter().filter(|c| c.did == b.did).count(), 1);
    assert!(b.node.contacts().iter().any(|c| c.did == a.did && c.verified), "verification kept (same DID)");
    assert!(a.node.contacts().iter().any(|c| c.did == b.did && c.verified));
    assert!(holds(&a, &b.did, &early) && holds(&b, &a.did, &early), "history kept on both sides");
    eventually(60, "reach restored", || {
        harness::contact_reach(&a.node, &b.did) == Reach::Ok && harness::contact_reach(&b.node, &a.did) == Reach::Ok
    });
    chat_works(&b, &a, "after re-share");
    chat_works(&a, &b, "both ways");
    call_rings(&a, &b);
}

// KIND: acceptance
// Decision: U1 (group audience) — both sides rotated while apart but share a group: they
// recover through the group's host without a re-share and never report lost contact.
#[test]
#[ignore = "R1: needs group chat API + forced rotation"]
fn both_rotated_apart_with_a_shared_group_recover_via_the_host() {
    let h = peer("grec-h");
    let (a, b) = pair("grec");
    for p in [&a, &b] {
        p.node.add_contact(h.node.my_ticket().unwrap()).unwrap();
    }
    let g = harness::create_group(&h.node, "g").unwrap();
    harness::add_group_member(&h.node, &g, &a.did).unwrap();
    harness::add_group_member(&h.node, &g, &b.did).unwrap();
    rotate_apart(&a, &b);
    let new_a = harness::endpoints(&a.node).private;
    eventually(120, "b recovers a's endpoint via the host", || dials(&b, &a.did).contains(&new_a));
    assert_ne!(harness::contact_reach(&b.node, &a.did), Reach::LostContact);
    chat_works(&b, &a, "recovered via host");
}
