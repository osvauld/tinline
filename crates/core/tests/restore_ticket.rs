//! P2: the contact ticket after a restore from the recovery phrase on an existing install.
//!
//! Restore over an existing account keeps `account.redb` (and with it the cached `State.ticket`)
//! but makes a fresh device key (`node.rs` `set_identity`). A cached ticket names the device key
//! a joiner dials, so it must never outlive that key. See
//! `docs/archive/rotation/rotation-baseline-report.md` finding O1 and
//! `docs/design/rotation-open-decisions.md` section 5 bug 2.
//!
//! Every test carries one label:
//! - `KIND: invariant`: the correct outcome; must keep holding.
//! - `KIND: limitation`: today's accepted outcome, with the better one named in the comment.
//!
//! Real iroh endpoints (needs network, like `restore.rs`). Waits are bounded polls. The shared
//! harness is in `support/`.

use std::time::{Duration, Instant};

mod support;
use support::eventually_200ms as eventually;
use support::wait::now_secs as now;
use support::*;

const TAG: &str = "p2";

/// A committed identity (with a passphrase, so it is written at once); started or not.
fn peer(name: &str, started: bool) -> Peer {
    Peer::committed(TAG, name, started)
}

fn claim(ticket: &str) -> proto::InviteClaim {
    proto::ContactTicket::from_text(ticket).unwrap().verify(now()).unwrap()
}

// KIND: invariant
// After a restore that replaced the device key, `my_ticket()` never hands out the cached ticket
// that names the dead key: it mints one naming the current key (checked on the not-yet-started
// node, where no relay is known and the relay check cannot force a re-issue by itself, and again
// once online). A contact redeems the live one and both sides can chat.
#[test]
fn after_restore_my_ticket_names_the_current_device_key_and_redeems() {
    let mut b = peer("new-b", true);
    let c = peer("new-c", true);
    let before = b.node.my_ticket().unwrap();
    let (old_dev, new_dev) = b.restore_in_place();
    assert_eq!(proto::device_to_text(&claim(&before).device), old_dev);
    assert_ne!(old_dev, new_dev, "a restore replaces the device key");

    let offline = b.node.my_ticket().unwrap();
    assert_ne!(offline, before, "the pre-restore ticket is not handed out again");
    assert_eq!(claim(&offline).iss, b.did);
    assert_eq!(
        proto::device_to_text(&claim(&offline).device),
        new_dev,
        "it names the current device key"
    );

    online(&b.node);
    let live = b.node.my_ticket().unwrap();
    assert_eq!(proto::device_to_text(&claim(&live).device), new_dev);
    assert_ne!(live, before);

    let added = c.node.add_contact(live).expect("the fresh ticket redeems");
    assert_eq!(added.did, b.did);
    eventually(30, "B lists C", || b.node.contacts().iter().any(|x| x.did == c.did));

    let m1 = c.node.send_text(b.did.clone(), "hello restored B".into(), None).unwrap().id;
    eventually(60, "B holds C's message", || has(&b, &c.did, &m1));
    let m2 = b.node.send_text(c.did.clone(), "hello C".into(), None).unwrap().id;
    eventually(60, "C holds B's message", || has(&c, &b.did, &m2));
}

// KIND: invariant
// No needless churn. A restore over an existing install always makes a fresh device key
// (`set_identity` calls `new_device_secret()`), so the "same key" case is reopening and
// unlocking the same install: the device key is unchanged and the cached ticket is handed out
// as before. Also, once re-issued after a restore, the new ticket is stable across calls.
#[test]
fn unchanged_device_key_keeps_the_cached_ticket() {
    let mut b = peer("same-b", true);
    let before = b.node.my_ticket().unwrap();
    let dev = b.node.device_key_for_test().unwrap();
    b.reopen();
    b.node.unlock(PASS.into()).unwrap();
    assert_eq!(b.node.device_key_for_test().unwrap(), dev, "unlock keeps the device key");
    assert_eq!(b.node.my_ticket().unwrap(), before, "same key: same ticket");

    b.restore_in_place();
    let first = b.node.my_ticket().unwrap();
    assert_eq!(b.node.my_ticket().unwrap(), first, "re-issued once, then stable");
    b.reopen();
    b.node.unlock("pass two".into()).unwrap();
    assert_eq!(b.node.my_ticket().unwrap(), first, "and it survives a restart");
}

// KIND: limitation
// A ticket shared before the restore names the dead device key; nobody holds that key any more,
// so redeeming it cannot reach B. It fails (bounded: the dial times out, observed ~20 s) and
// creates no contact on either side, and B no longer counts its nonce as current, so even a
// joiner that reached B would be refused ("not the current ticket").
// Better (not done here, needs a policy/discovery decision): a prompt, specific "this ticket is
// no longer valid" instead of a dial timeout indistinguishable from "B is offline".
#[test]
fn a_pre_restore_ticket_fails_without_creating_a_contact() {
    let mut b = peer("old-b", true);
    let c = peer("old-c", true);
    let before = b.node.my_ticket().unwrap();
    b.restore_in_place();
    online(&b.node);
    assert_ne!(b.node.my_ticket().unwrap(), before, "B hands out a different ticket now");

    let t = Instant::now();
    let r = c.node.add_contact(before);
    assert!(r.is_err(), "the pre-restore ticket cannot be redeemed: {r:?}");
    assert!(t.elapsed() < Duration::from_secs(90), "fails within a bound, took {:?}", t.elapsed());
    assert!(c.node.contacts().is_empty(), "C added nobody");
    assert!(b.node.contacts().is_empty(), "B added nobody");
}
