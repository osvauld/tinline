//! The `msg` wake schedule on in-process nodes (`docs/notifications-plan.md` N2): a queued
//! contact request (code wakes), and a contact's device (messages and the Welcome, both through
//! `wake_unreached`). The schedule itself is `wake::msg_wake_due` (unit tests in the wake crate);
//! here the node's use of it, with a fake clock where a week is needed. Names start with `wake_`
//! (`cargo test -p node-runtime --lib wake`).

use iroh::address_lookup::memory::MemoryLookup;

use super::code::take_code_wake;
use super::code_tests::{claim_of, setup_node};
use super::dial_tests::pair_with_token;
use super::tests::*;
use super::*;
use crate::caps::ContactOrigin;
use crate::contact_req::AddOutcome;
use crate::store::SentRequest;

const HOUR: u64 = 3_600_000;

/// B queues a contact request to A, whose phone sleeps (push on, the code carries its token,
/// and nothing starts A again). Returns the nodes; B's outbox keeps retrying in the background.
async fn queued_to_sleeper(lookup: &MemoryLookup) -> (T, T) {
    let (a, b) = (setup_node(lookup).await, setup_node(lookup).await);
    set_target(&a, Some(fcm(1)));
    let code = a.ctx.ticket_with_relay(None).await.unwrap();
    assert!(claim_of(&code).wake.is_some(), "the code carries a token");
    a.ep.close().await;
    let out = b.ctx.clone().add_contact_outcome(code, ContactOrigin::Ticket).await.unwrap();
    assert!(matches!(out, AddOutcome::Requested { .. }));
    (a, b)
}

fn request_of(b: &T, a: &T) -> SentRequest {
    b.state(|st| st.sent_requests.get(&a.did()).cloned()).expect("queued")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_backoff_a_queued_request_wakes_about_170_times_in_its_week() {
    let lookup = MemoryLookup::new();
    let (a, b) = queued_to_sleeper(&lookup).await;
    let mut r = request_of(&b, &a);
    assert!(r.wakes >= 1, "the first failed dial woke A");
    // A fresh copy on a fake clock: tried every 10 s (the outbox redials no faster) for 8 days.
    r.last_wake_ms = None;
    r.wakes = 0;
    let start = now_ms();
    let mut sent = Vec::new();
    let mut t = 0;
    while t < 8 * 24 * HOUR {
        if take_code_wake(&mut r, start + t).is_some() {
            sent.push(t);
        }
        t += 10_000;
    }
    assert!(sent.len() <= 175, "{} wakes for one queued request", sent.len());
    assert!(sent.len() >= 150, "hourly until the code expires: {}", sent.len());
    assert_eq!(&sent[..5], &[0, 30_000, 120_000, 600_000, 1_800_000]);
    assert!(*sent.last().unwrap() <= crate::TICKET_TTL * 1000 + HOUR, "none after the code expired");
    assert_eq!(r.wakes as usize, sent.len());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_backoff_reaching_the_owner_starts_the_request_over() {
    let lookup = MemoryLookup::new();
    let (a, b) = queued_to_sleeper(&lookup).await;
    // Six wakes unanswered, the last just now: the next is an hour away (the background outbox
    // cannot take one meanwhile).
    let now = now_ms();
    b.edit(|st| {
        let r = st.sent_requests.get_mut(&a.did()).unwrap();
        r.wakes = 6;
        r.last_wake_ms = Some(now);
    });
    let before = b.relay.wakes.lock().len();
    assert!(!b.ctx.wake_code(&a.did()).await, "not before the hour");
    assert_eq!(b.relay.wakes.lock().len(), before);
    let mut r = request_of(&b, &a);
    assert!(take_code_wake(&mut r, now + HOUR - 1).is_none());

    b.ctx.wake_reached(&a.did());
    let mut r = request_of(&b, &a);
    assert_eq!(r.wakes, 0, "reached: the schedule starts over");
    assert_eq!(r.last_wake_ms, Some(now), "the 30 s floor stays");
    assert!(take_code_wake(&mut r, now + 29_000).is_none());
    assert!(take_code_wake(&mut r, now + 30_000).is_some(), "prompt again");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_backoff_a_silent_contact_is_woken_hourly_until_reached() {
    let lookup = MemoryLookup::new();
    let (a, b) = pair_with_token(&lookup).await;
    b.ep.close().await;
    let devices = [b.me.device];
    assert!(a.ctx.wake_unreached(&b.did(), &devices).await, "the first failed dial wakes");
    assert_eq!(a.held_from(&b).unwrap().msg_wakes, 1);

    // Five wakes unanswered, the last 31 s ago: the old 30 s throttle would wake again.
    let ago = now_ms() - 31_000;
    a.edit(|st| {
        let h = &mut st.wake.as_mut().unwrap().held[0].held;
        h.msg_wakes = 5;
        h.last_wake_ms = Some(ago);
    });
    assert!(!a.ctx.wake_unreached(&b.did(), &devices).await, "hourly by now");
    assert_eq!(a.relay.wakes.lock().len(), 1);

    a.ctx.wake_reached(&b.did());
    assert_eq!(a.held_from(&b).unwrap().msg_wakes, 0, "reached: the schedule starts over");
    assert!(a.ctx.wake_unreached(&b.did(), &devices).await, "a later silence wakes promptly");
    assert_eq!(a.relay.wakes.lock().len(), 2);
    assert_eq!(a.held_from(&b).unwrap().msg_wakes, 1);
}
