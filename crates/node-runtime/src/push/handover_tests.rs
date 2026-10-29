//! The wake tokens of the contact setup (`docs/push.md` "Tokens at setup"): a hello by code never
//! leaves without ours while push is on, reading a code registers it ahead, and a handover that
//! failed is finished later by waking the other phone. In-process nodes and the fake relay, as in
//! `code_tests.rs`. Names start with `wake_` (`cargo test -p node-runtime --lib wake_handover`).

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use iroh::address_lookup::memory::MemoryLookup;

use super::code_tests::*;
use super::tests::*;
use super::*;
use crate::caps::ContactOrigin;
use crate::contact_req::AddOutcome;

/// The token B's hello carried, as A holds the request.
fn hello_token(a: &T, b: &T) -> Option<String> {
    a.state(|st| {
        crate::push::handover_in(&st.contact_requests.get(&b.did())?.hello).map(|h| h.token.clone())
    })
}

// A relay that takes 5 s to seal B's token (longer than the old 3 s wait): the hello still
// carries it, so A can wake B once they are contacts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_handover_a_slow_relay_still_puts_the_token_in_the_hello() {
    let lookup = MemoryLookup::new();
    let (a, b) = (setup_node(&lookup).await, setup_node(&lookup).await);
    set_target(&a, Some(fcm(1)));
    set_target(&b, Some(fcm(2)));
    let ra = listen(&a);
    let code = a.ctx.ticket_with_relay(None).await.unwrap();
    b.relay.register_delay_ms.store(5_000, Ordering::SeqCst);

    let out = b
        .ctx
        .clone()
        .add_contact_outcome(code, ContactOrigin::Ticket)
        .await
        .unwrap();
    assert!(matches!(out, AddOutcome::Requested { .. }));
    eventually("A's user is asked", || ra.requests.lock().len() == 1).await;
    let issued = b.issued_to(&a).expect("B registered a token for A").token;
    assert_eq!(hello_token(&a, &b), Some(issued), "the hello waited for it");
}

// Reading A's code ("Add A?") registers B's token for A at once: the hello that follows has it
// without asking the relay again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_handover_reading_a_code_registers_the_token_ahead() {
    let lookup = MemoryLookup::new();
    let (a, b) = (setup_node(&lookup).await, setup_node(&lookup).await);
    set_target(&a, Some(fcm(1)));
    set_target(&b, Some(fcm(2)));
    let ra = listen(&a);
    let code = a.ctx.ticket_with_relay(None).await.unwrap();

    b.ctx.code_peeked(&a.did());
    eventually("B holds a token for A before adding", || {
        b.issued_to(&a).is_some()
    })
    .await;
    let token = b.issued_to(&a).unwrap().token;
    let registers = b.relay.registers.lock().len();
    // The relay is now very slow: a hello that asked it would wait the whole 15 s.
    b.relay.register_delay_ms.store(60_000, Ordering::SeqCst);

    let started = Instant::now();
    b.ctx
        .clone()
        .add_contact_outcome(code, ContactOrigin::Ticket)
        .await
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "no wait: {:?}",
        started.elapsed()
    );
    eventually("A's user is asked", || ra.requests.lock().len() == 1).await;
    assert_eq!(hello_token(&a, &b), Some(token));
    assert_eq!(
        b.relay.registers.lock().len(),
        registers,
        "registered once, at the read"
    );
}

// A code read but never used (the sheet cancelled): its token is taken back once the read is
// forgotten.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_handover_an_unused_code_read_takes_its_token_back() {
    let lookup = MemoryLookup::new();
    let (a, b) = (setup_node(&lookup).await, setup_node(&lookup).await);
    set_target(&b, Some(fcm(2)));
    b.ctx.code_peeked(&a.did());
    eventually("registered", || b.issued_to(&a).is_some()).await;
    let token = b.issued_to(&a).unwrap().token;

    // What `PEEK_HOLD` running out does.
    b.ctx.push.peeked.lock().clear();
    b.ctx.wake_kick();
    eventually("revoked", || b.relay.revokes.lock().contains(&token)).await;
    assert!(b.issued_to(&a).is_none());
}

// Without push a hello is not held: it goes at once, with no token.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_handover_without_push_the_hello_goes_at_once() {
    let lookup = MemoryLookup::new();
    let (a, b) = (setup_node(&lookup).await, setup_node(&lookup).await);
    set_target(&a, Some(fcm(1)));
    let ra = listen(&a);
    let code = a.ctx.ticket_with_relay(None).await.unwrap();
    let started = Instant::now();
    b.ctx
        .clone()
        .add_contact_outcome(code, ContactOrigin::Ticket)
        .await
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    eventually("A's user is asked", || ra.requests.lock().len() == 1).await;
    assert_eq!(hello_token(&a, &b), None);
}

// The device test of 2026-10-28: after adding, A has no token for B (the handover failed) and
// A's phone is asleep. B's `WakeTokens` round wakes A with the token from A's Welcome, then
// hands its own over, so A can wake B from then on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_handover_a_missing_token_is_handed_over_by_waking_them() {
    let lookup = MemoryLookup::new();
    let (a, b, _ra, rb) = requested_pair(&lookup).await;
    a.ctx.accept_request(&a.me, &b.did()).unwrap();
    a.ctx.welcome_kick(b.did());
    eventually("B hears it was accepted", || rb.accepted.lock().len() == 1).await;
    eventually("B's own round is done", || {
        b.state(|st| st.wake_outbox.is_empty())
    })
    .await;
    let a_token = b
        .held_from(&a)
        .expect("A's Welcome carried A's token")
        .token;
    let b_token = b.issued_to(&a).unwrap().token;

    // A lost B's token, and A sleeps.
    a.edit(|st| st.wake.as_mut().unwrap().held.retain(|e| e.did != b.did()));
    a.ep.close().await;
    let _woke = phone_wakes(&b, &a, &lookup, Duration::from_millis(500));
    b.edit(|st| st.wake_outbox.insert(a.did(), Default::default()));
    b.ctx.wake_kick();

    // Early in the dial, not after its 20 s timeout.
    eventually_within("B wakes A to hand it over", Duration::from_secs(10), || {
        b.relay.wakes.lock().contains(&(a_token.clone(), Kind::Msg))
    })
    .await;
    // The dial that started while A slept may run out; the retry after it reaches A.
    eventually_within("A holds B's token again", Duration::from_secs(45), || {
        a.held_from(&b).map(|h| h.token) == Some(b_token.clone())
    })
    .await;
}
