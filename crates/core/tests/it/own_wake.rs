//! Own devices wake each other (`docs/push.md` "Own devices"), over real endpoints, with a
//! relay that only records (`record_wakes_for_test`). A "sleeping phone" is a stopped node; the
//! push arriving is the phone starting again.

use std::time::Duration;

use p2pcore::{Node, PushTarget};

use crate::support::{Peer, eventually, online};

fn status(node: &Node, did: &str) -> Option<String> {
    let p = node.account_profile(did.into()).ok()??;
    let v: serde_json::Value = serde_json::from_str(&p.fields_json).ok()?;
    v.get("status")?.as_str().map(str::to_string)
}

/// A phone (push target) and a laptop (none) of one account, both online, each in the other's
/// registry, with recording relays.
fn phone_and_laptop(tag: &str) -> (Peer, Peer) {
    let phone = Peer::new("own-wake", &format!("{tag}-phone"));
    let laptop = Peer::restored("own-wake", &format!("{tag}-laptop"), &phone.phrase);
    (phone, laptop)
}

#[test]
fn own_wake_the_laptop_wakes_the_sleeping_phone_once_and_the_phone_catches_up() {
    let (phone, laptop) = phone_and_laptop("once");
    let phone_relay = phone.node.record_wakes_for_test();
    let laptop_relay = laptop.node.record_wakes_for_test();
    phone
        .node
        .add_own_device_for_test(laptop.node.own_attestation_for_test().unwrap())
        .unwrap();
    laptop
        .node
        .add_own_device_for_test(phone.node.own_attestation_for_test().unwrap())
        .unwrap();
    phone
        .node
        .set_push_target(Some(PushTarget::Fcm {
            token: "fcm-token-for-tests-0001".into(),
        }))
        .unwrap();

    // The token rides the own-device link (SelfMsg::OwnWake).
    eventually(90, "the laptop holds the phone's wake token", || {
        laptop.node.own_wake_tokens_held_for_test() == 1
    });
    assert_eq!(
        phone.node.own_wake_tokens_held_for_test(),
        0,
        "a desktop has no token to give"
    );

    // The phone goes to sleep; the laptop then has a change for it.
    phone.node.stop();
    eventually(60, "the own-device session ended", || {
        laptop.node.own_sessions_for_test() == 0
    });
    assert_eq!(
        laptop_relay.msg_wakes(),
        0,
        "nothing owed yet, nothing woken"
    );
    laptop
        .node
        .set_profile_field("status".into(), Some("\"Changed on the laptop\"".into()))
        .unwrap();
    eventually(90, "one wake for the phone", || {
        laptop_relay.msg_wakes() >= 1
    });
    // More failed dials follow (backoff 20 s, 40 s); the wake throttle (30 s, then 2 min) holds.
    std::thread::sleep(Duration::from_secs(25));
    assert_eq!(laptop_relay.msg_wakes(), 1, "exactly one wake");
    assert_eq!(phone_relay.msg_wakes(), 0);

    // The wake arrives: the phone starts and syncs from the laptop.
    online(&phone.node);
    eventually(90, "the phone has the laptop's change", || {
        status(&phone.node, &phone.did).as_deref() == Some("Changed on the laptop")
    });
}
