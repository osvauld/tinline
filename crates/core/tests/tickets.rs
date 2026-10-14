//! Ticket and contact rules between real nodes (needs network, like calls.rs).

use std::sync::Arc;

use p2pcore::{Error, Node};

mod support;
use support::wait::now_secs as now;
use support::*;

struct Peer {
    node: Arc<Node>,
    did: String,
    _dir: TempDir,
}

fn peer(tag: &str, started: bool) -> Peer {
    let dir = TempDir::new("tickets", tag);
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    node.auto_accept_contacts_for_test(true);
    node.create_identity(tag.into(), PASS.into()).unwrap();
    if started {
        online(&node);
    }
    let did = node.profile().unwrap().did;
    Peer { node, did, _dir: dir }
}

#[test]
fn reset_tickets_are_dead_and_live_ones_are_reusable() {
    let (a, b, c) = (peer("reset-a", true), peer("reset-b", true), peer("reset-c", true));
    let t1 = a.node.my_ticket().unwrap();
    assert_eq!(a.node.my_ticket().unwrap(), t1, "stable");
    a.node.reset_ticket().unwrap();
    let t2 = a.node.my_ticket().unwrap();
    assert_ne!(t1, t2);
    // The reset ticket is unspent but no longer live.
    assert!(matches!(b.node.add_contact(t1.clone()), Err(Error::Rejected(_))));
    assert!(a.node.contacts().is_empty());
    b.node.add_contact(t2.clone()).unwrap();
    // Reusable (2026-10-13): a second joiner adds with the same ticket, which stays the same.
    c.node.add_contact(t2.clone()).unwrap();
    assert_eq!(a.node.my_ticket().unwrap(), t2);
    assert_eq!(a.node.contacts().len(), 2);
    // The same joiner may use it again (2026-10-14: good until it expires or is reset); no one new.
    b.node.add_contact(t2).unwrap();
    assert_eq!(a.node.contacts().len(), 2);
}

#[test]
fn blocked_contact_cannot_come_back_through_our_ticket() {
    let (a, b) = (peer("block-a", true), peer("block-b", true));
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    a.node.block(b.did.clone()).unwrap();
    // U19: a block keeps the contact (shown as Blocked).
    assert_eq!(a.node.contacts().len(), 1);
    let fresh = a.node.my_ticket().unwrap();
    assert!(matches!(b.node.add_contact(fresh), Err(Error::Rejected(r)) if r == p2pcore::BLOCKED));
    assert!(a.node.block_state(b.did.clone()).blocked_by_me, "still blocked");
    // Scanning their ticket ourselves is a way back: it unblocks them.
    a.node.add_contact(b.node.my_ticket().unwrap()).unwrap();
    assert_eq!(a.node.contacts().len(), 1);
    assert!(!a.node.block_state(b.did.clone()).blocked_by_me);
}

#[test]
fn ticket_with_unusable_relay_is_refused() {
    let a = peer("relay-a", false);
    let (other, _) = identity::generate();
    let dev = proto::device_public(&proto::new_device_secret());
    for relay in ["http://10.0.0.1/", "ftp://relay.example/", "https://not a url"] {
        let t = proto::issue_contact_ticket(&other, dev, "evil", Some(relay.into()), now(), 600);
        match a.node.add_contact(t.to_text()) {
            Err(Error::Protocol(m)) => assert!(m.contains("relay"), "{relay}: {m}"),
            other => panic!("{relay}: expected a relay refusal, got {other:?}"),
        }
    }
}

/// S6: the contact and the redeemed (joiner, nonce) are one durable write, made before the
/// Welcome. If it fails, the joiner gets no Welcome and the ticket is still good.
#[test]
fn failed_write_sends_no_welcome_and_spends_nothing() {
    let (a, b) = (peer("s6-a", true), peer("s6-b", true));
    let t = a.node.my_ticket().unwrap();
    a.node.fail_next_writes(1);
    assert!(matches!(b.node.add_contact(t.clone()), Err(Error::Rejected(_))));
    assert!(b.node.contacts().is_empty(), "no Welcome, so no contact over there");
    assert!(a.node.contacts().is_empty(), "nor over here");
    assert_eq!(a.node.my_ticket().unwrap(), t, "the ticket is unspent and still current");

    // Retrying works, and what was written survives a restart.
    b.node.add_contact(t.clone()).unwrap();
    assert_eq!(a.node.contacts().len(), 1);
    assert_eq!(a.node.my_ticket().unwrap(), t, "reusable: still the same ticket");
    a.node.lock();
    a.node.unlock(PASS.into()).unwrap();
    a.node.start().unwrap();
    assert_eq!(a.node.contacts().len(), 1);
    // The contact is durable: the same joiner coming again (2026-10-14: allowed) adds no one new.
    b.node.add_contact(t).unwrap();
    assert_eq!(a.node.contacts().len(), 1);
}
