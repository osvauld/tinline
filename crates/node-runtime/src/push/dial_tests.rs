//! Slice 3b on in-process nodes: wakes after failed dials (messages and calls) and the wake
//! token reaching every device of a contact. Real iroh endpoints on loopback, the fake relay of
//! `tests.rs`; a "sleeping phone" is a node whose endpoint is closed, and the fake relay's wake
//! hook starts it again. Names start with `wake_` (`cargo test -p node-runtime --lib wake`).

use std::time::{Duration, Instant};

use iroh::EndpointAddr;
use iroh::address_lookup::memory::MemoryLookup;

use super::tests::*;
use super::*;
use crate::CallState;
use crate::call::Cmd;

/// A stand-in for the chat engine's connect: one dial of a device, 700 ms at most.
async fn try_dial(from: &T, device: [u8; 32]) -> bool {
    let addr = EndpointAddr::new(iroh::PublicKey::from_bytes(&device).unwrap());
    match tokio::time::timeout(Duration::from_millis(700), crate::wire::dial_call(&from.ep, addr)).await {
        Ok(Ok(conn)) => {
            conn.close(0u32.into(), b"bye");
            true
        }
        _ => false,
    }
}

/// `chat_kick`'s loop on the same `NodeCtx` methods the engine calls: dial the devices, wake the
/// unreached ones, wait the fast or the normal pause. `true`: reached one within `give_up`.
async fn dial_loop(a: &T, did: &str, devices: &[[u8; 32]], give_up: Duration) -> bool {
    let end = Instant::now() + give_up;
    let mut delay = Duration::from_secs(2);
    while Instant::now() < end {
        let mut unreached = Vec::new();
        for d in devices {
            if try_dial(a, *d).await {
                a.ctx.wake_reached(did);
                return true;
            }
            unreached.push(*d);
        }
        a.ctx.wake_unreached(did, &unreached).await;
        let pause = a.ctx.fast_redial(did).unwrap_or_else(|| {
            let d = delay;
            delay = (delay * 2).min(Duration::from_secs(60));
            d
        });
        tokio::time::sleep(pause).await;
    }
    false
}

/// A wake of `kind` for one of `phones` (by the token each gave `a`) starts that phone after `delay`.
fn phones_start_on_wake(a: &T, lookup: &MemoryLookup, kind: Kind, phones: &[&T], delay: Duration) {
    let table: Vec<(String, Arc<NodeCtx>, [u8; 32])> = phones.iter().map(|p| (p.issued_to(a).unwrap().token, p.ctx.clone(), p.secret)).collect();
    let lookup = lookup.clone();
    *a.relay.on_wake.lock() = Some(Arc::new(move |token, k| {
        if k != kind {
            return;
        }
        for (t, ctx, secret) in &table {
            if t == token {
                let (ctx, secret, lookup) = (ctx.clone(), *secret, lookup.clone());
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    bring_up(&ctx, secret, &lookup).await;
                });
            }
        }
    }));
}

/// `a` and `b` are contacts and `a` holds the wake token `b` issued for it.
async fn pair_with_token(lookup: &MemoryLookup) -> (T, T) {
    let (a, b) = (node(lookup).await, node(lookup).await);
    befriend(&a, &b);
    set_target(&b, Some(fcm(2)));
    eventually("A holds B's token", || a.held_from(&b).is_some()).await;
    (a, b)
}

/// `a` has one contact whose two devices `b1` and `b2` are one account; both know `a`.
async fn pair_of_devices(lookup: &MemoryLookup) -> (T, T, T) {
    let (a, b1) = (node(lookup).await, node(lookup).await);
    let b2 = second_device(lookup, &b1).await;
    befriend(&a, &b1);
    a.edit(|st| st.contacts[0].devices.push(ContactDevice { device: b2.me.device, relay: None }));
    let ca = contact_of(&a, &b2.me.id, true);
    b2.edit(|st| st.contacts.push(ca));
    (a, b1, b2)
}

fn held_by(a: &T, b: &T) -> Option<wake::Held> {
    a.held_from(b)
}

// ---- messages ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_w1_a_contact_that_answers_is_never_woken() {
    let lookup = MemoryLookup::new();
    let (a, b) = pair_with_token(&lookup).await;
    assert!(dial_loop(&a, &b.did(), &[b.me.device], Duration::from_secs(5)).await);
    assert!(a.relay.wakes.lock().is_empty(), "the dial succeeded, so no wake request");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_w2_one_wake_then_the_fast_redials_reach_the_started_phone() {
    let lookup = MemoryLookup::new();
    let (a, b) = pair_with_token(&lookup).await;
    let token = b.issued_to(&a).unwrap().token;
    b.ep.close().await;
    // The phone needs 3 s to start: the normal backoff would try again at +2 s and +6 s after the
    // wake, the fast schedule at +2 s and +4 s.
    phones_start_on_wake(&a, &lookup, Kind::Msg, &[&b], Duration::from_secs(3));

    assert!(dial_loop(&a, &b.did(), &[b.me.device], Duration::from_secs(20)).await, "reached after the wake");
    assert_eq!(*a.relay.wakes.lock(), vec![(token, Kind::Msg)], "exactly one msg wake");
    let since_wake = a.ctx.push.woken.lock().get(&b.did()).map(|t| t.elapsed());
    assert!(since_wake.is_none(), "reaching the contact ends the fast redials");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_w2_fast_redials_beat_the_normal_backoff() {
    let lookup = MemoryLookup::new();
    let (a, b) = pair_with_token(&lookup).await;
    b.ep.close().await;
    phones_start_on_wake(&a, &lookup, Kind::Msg, &[&b], Duration::from_secs(3));
    let started = Instant::now();
    assert!(dial_loop(&a, &b.did(), &[b.me.device], Duration::from_secs(20)).await);
    // Dial fails at 0.7 s and wakes; the phone is up at ~3.7 s; the fast redials land at +2 s and
    // +4 s (ending ~4.7 s); the normal backoff would not try again before ~7.4 s.
    assert!(started.elapsed() < Duration::from_millis(6500), "took {:?}", started.elapsed());
}

// ---- early message wake ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_early_fires_at_3s_while_the_dial_still_hangs() {
    let lookup = MemoryLookup::new();
    let (a, b) = pair_with_token(&lookup).await;
    let token = b.issued_to(&a).unwrap().token;
    b.ep.close().await;
    let early = a.ctx.early_wake(&b.did(), vec![b.me.device]);
    tokio::time::sleep(Duration::from_millis(MSG_EARLY_WAKE_MS - 500)).await;
    assert!(a.relay.wakes.lock().is_empty(), "not before the delay");
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    assert_eq!(*a.relay.wakes.lock(), vec![(token, Kind::Msg)], "one msg wake, the dial is still going");
    drop(early);
    // The post-failure path is throttled away by the early wake.
    assert!(!a.ctx.wake_unreached(&b.did(), &[b.me.device]).await);
    assert_eq!(a.relay.wakes.lock().len(), 1);
    assert!(a.ctx.fast_redial(&b.did()).is_some(), "the fast redials follow the early wake");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_early_is_cancelled_by_a_dial_that_connects_at_1s() {
    let lookup = MemoryLookup::new();
    let (a, b) = pair_with_token(&lookup).await;
    let early = a.ctx.early_wake(&b.did(), vec![b.me.device]);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(try_dial(&a, b.me.device).await);
    drop(early);
    tokio::time::sleep(Duration::from_millis(MSG_EARLY_WAKE_MS + 500)).await;
    assert!(a.relay.wakes.lock().is_empty(), "connected before 3 s, no wake");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_early_does_nothing_without_a_token() {
    let lookup = MemoryLookup::new();
    let (a, b) = (node(&lookup).await, node(&lookup).await);
    befriend(&a, &b);
    b.ep.close().await;
    let _early = a.ctx.early_wake(&b.did(), vec![b.me.device]);
    tokio::time::sleep(Duration::from_millis(MSG_EARLY_WAKE_MS + 500)).await;
    assert!(a.relay.wakes.lock().is_empty(), "no token, no wake");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_w8_several_sends_wake_a_device_once_per_throttle_window() {
    let lookup = MemoryLookup::new();
    let (a, b) = pair_with_token(&lookup).await;
    b.ep.close().await;
    let devices = [b.me.device];
    assert!(a.ctx.wake_unreached(&b.did(), &devices).await, "the first failed dial wakes");
    for _ in 0..4 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!a.ctx.wake_unreached(&b.did(), &devices).await, "throttled");
    }
    assert_eq!(a.relay.wakes.lock().len(), 1, "five failed dials in a second, one wake");

    // 31 s later (the held token remembers when it was last used) it may wake again.
    let ago = now_ms() - 31_000;
    a.edit(|st| st.wake.as_mut().unwrap().held[0].held.last_wake_ms = Some(ago));
    assert!(a.ctx.wake_unreached(&b.did(), &devices).await);
    assert_eq!(a.relay.wakes.lock().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_a_410_drops_the_held_token_and_no_more_wakes_follow() {
    let lookup = MemoryLookup::new();
    let (a, b) = pair_with_token(&lookup).await;
    b.ep.close().await;
    a.relay.wake_status.store(410, Ordering::SeqCst);
    let devices = [b.me.device];
    assert!(!a.ctx.wake_unreached(&b.did(), &devices).await);
    assert!(held_by(&a, &b).is_none(), "the dead token is dropped");
    assert!(a.ctx.fast_redial(&b.did()).is_none(), "a refused wake gives no fast redials");

    a.edit(|st| st.wake.as_mut().unwrap().held.clear());
    a.relay.wake_status.store(0, Ordering::SeqCst);
    assert!(!a.ctx.wake_unreached(&b.did(), &devices).await);
    assert_eq!(a.relay.wakes.lock().len(), 1, "nothing held, nothing asked");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_a_limited_relay_keeps_the_token_and_the_normal_backoff() {
    let lookup = MemoryLookup::new();
    let (a, b) = pair_with_token(&lookup).await;
    b.ep.close().await;
    a.relay.wake_status.store(429, Ordering::SeqCst);
    assert!(!a.ctx.wake_unreached(&b.did(), &[b.me.device]).await);
    assert!(held_by(&a, &b).is_some());
    assert!(a.ctx.fast_redial(&b.did()).is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_w9_every_unreached_device_is_woken() {
    let lookup = MemoryLookup::new();
    let (a, b1, b2) = pair_of_devices(&lookup).await;
    set_target(&b1, Some(fcm(1)));
    set_target(&b2, Some(fcm(2)));
    eventually("A holds a token of each device", || a.state(|st| st.wake.as_ref().is_some_and(|w| w.held.len() == 2))).await;
    let (t1, t2) = (b1.issued_to(&a).unwrap().token, b2.issued_to(&a).unwrap().token);
    assert_ne!(t1, t2);
    b1.ep.close().await;
    b2.ep.close().await;
    phones_start_on_wake(&a, &lookup, Kind::Msg, &[&b1, &b2], Duration::from_millis(300));

    assert!(dial_loop(&a, &b1.did(), &[b1.me.device, b2.me.device], Duration::from_secs(20)).await);
    let mut woken: Vec<String> = a.relay.wakes.lock().iter().map(|(t, _)| t.clone()).collect();
    woken.sort();
    let mut want = vec![t1, t2];
    want.sort();
    assert_eq!(woken, want, "one wake each, no more");
    eventually("the other device is reachable too", || {
        let (a2, d2) = (&a, b2.me.device);
        futures_lite_block(try_dial(a2, d2))
    })
    .await;
}

/// `eventually` polls a sync closure; a dial is async. Blocks on it on the test's own runtime.
fn futures_lite_block(f: impl std::future::Future<Output = bool>) -> bool {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(f))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_the_fast_redial_schedule_ends_after_thirty_seconds() {
    let lookup = MemoryLookup::new();
    let a = node(&lookup).await;
    let push = &a.ctx.push;
    assert!(a.ctx.fast_redial("did:x").is_none(), "no wake, no fast redial");
    let ago = |s: u64| Instant::now().checked_sub(Duration::from_secs(s)).unwrap();
    push.woken.lock().insert("did:x".into(), ago(5));
    let pause = a.ctx.fast_redial("did:x").unwrap();
    assert!(pause <= Duration::from_secs(2) && pause > Duration::from_millis(1900), "5 s in, next at 7 s: {pause:?}");
    push.woken.lock().insert("did:x".into(), ago(25));
    let pause = a.ctx.fast_redial("did:x").unwrap();
    assert!(pause <= Duration::from_secs(5) && pause > Duration::from_millis(4900), "25 s in, next at 30 s: {pause:?}");
    push.woken.lock().insert("did:x".into(), ago(31));
    assert!(a.ctx.fast_redial("did:x").is_none(), "after the last point the normal backoff goes on");
    assert!(push.woken.lock().is_empty());
}

// ---- the token reaches every device ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_every_device_of_a_contact_gets_its_own_copy_of_the_token() {
    let lookup = MemoryLookup::new();
    let (a, b1, b2) = pair_of_devices(&lookup).await;
    set_target(&a, Some(fcm(1)));
    eventually("both devices hold a token", || b1.held_from(&a).is_some() && b2.held_from(&a).is_some()).await;
    let issued = a.issued_to(&b1).unwrap();
    assert_eq!(b1.held_from(&a).unwrap().token, issued.token);
    assert_eq!(b2.held_from(&a).unwrap().token, issued.token, "the same token, held per device");
    eventually("the outbox empties", || a.state(|st| st.wake_outbox.is_empty())).await;
    assert_eq!(a.relay.registers.lock().len(), 1, "one registration for the contact");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_a_device_that_turns_us_away_does_not_stop_the_others() {
    let lookup = MemoryLookup::new();
    let (a, b1, b2) = pair_of_devices(&lookup).await;
    // The second device does not hold us as a contact.
    b2.edit(|st| st.contacts.clear());
    set_target(&a, Some(fcm(1)));
    eventually("the first device holds the token", || b1.held_from(&a).is_some()).await;
    eventually("the loop settled", || !a.ctx.push.task.lock().running).await;
    assert!(b2.held_from(&a).is_none());
    let did = a_contact(&a);
    let owed = a.state(|st| st.wake_outbox.get(&did).cloned());
    assert_eq!(owed, Some(std::iter::once(hex(&b1.me.device)).collect()), "still owed to the second device only");

    // It takes us again: the next kick hands it over and the outbox is done.
    let ca = contact_of(&a, &b2.me.id, true);
    b2.edit(|st| st.contacts.push(ca));
    a.ctx.wake_kick();
    eventually("the second device holds it", || b2.held_from(&a).is_some()).await;
    eventually("the outbox empties", || a.state(|st| st.wake_outbox.is_empty())).await;
    assert_eq!(a.relay.registers.lock().len(), 1);
}

fn a_contact(a: &T) -> String {
    a.state(|st| st.contacts[0].did.clone())
}

// ---- calls ----

fn states(t: &T) -> Vec<CallState> {
    t.rec.states.lock().clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_w4b_w4c_a_call_wakes_the_callee_stays_dialing_and_rings_only_when_told() {
    let lookup = MemoryLookup::new();
    let (a, b) = pair_with_token(&lookup).await;
    let token = b.issued_to(&a).unwrap().token;
    b.ep.close().await;
    // The woken phone needs a moment to start.
    phones_start_on_wake(&a, &lookup, Kind::Call, &[&b], Duration::from_millis(800));

    let info = a.ctx.clone().start_call(b.did(), false).unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(a.relay.wakes.lock().is_empty(), "the first dial is still running");
    assert_eq!(states(&a), vec![CallState::Dialing]);

    eventually("the call wake is sent", || !a.relay.wakes.lock().is_empty()).await;
    assert_eq!(*a.relay.wakes.lock(), vec![(token, Kind::Call)], "one call wake, for the callee's device");
    // The wake is out, the phone is not up yet: still "Calling", nothing rung.
    assert_eq!(states(&a), vec![CallState::Dialing]);
    assert!(b.rec.incoming.lock().is_empty());

    eventually("the callee gets the CallHello from us", || b.rec.incoming.lock().len() == 1).await;
    let incoming = b.rec.incoming.lock()[0].clone();
    assert_eq!(incoming.peer_did, a.did());
    assert_eq!(incoming.call_id, info.call_id, "the normal call, dialled A to B");
    eventually("the caller hears Ringing", || states(&a).contains(&CallState::Ringing)).await;
    assert_eq!(states(&a), vec![CallState::Dialing, CallState::Ringing]);
    assert!(b.ctx.live.lock().call.is_some());
    assert_eq!(a.relay.wakes.lock().len(), 1);

    a.ctx.command(&info.call_id, Cmd::Hangup).unwrap();
    eventually("the call ends", || matches!(states(&a).last(), Some(CallState::Ended { .. }))).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_w4_hanging_up_while_dialing_stops_the_redials() {
    let lookup = MemoryLookup::new();
    let (a, b) = pair_with_token(&lookup).await;
    b.ep.close().await;
    let info = a.ctx.clone().start_call(b.did(), false).unwrap();
    eventually("the call wake is sent", || !a.relay.wakes.lock().is_empty()).await;
    assert_eq!(states(&a), vec![CallState::Dialing]);

    a.ctx.command(&info.call_id, Cmd::Hangup).unwrap();
    eventually("the call ends at once", || matches!(states(&a).last(), Some(CallState::Ended { reason }) if reason == "cancelled")).await;

    // The phone comes up afterwards: nobody dials it for this call.
    bring_up(&b.ctx, b.secret, &lookup).await;
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(b.rec.incoming.lock().is_empty(), "no CallHello ever reaches the callee");
    assert!(b.ctx.live.lock().call.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_a_call_without_a_call_token_does_not_wake_or_redial() {
    let lookup = MemoryLookup::new();
    let (a, b) = (node(&lookup).await, node(&lookup).await);
    befriend(&a, &b);
    // B lets A message but not call, so its token is message-only.
    b.edit(|st| st.contacts[0].calls_allowed = false);
    set_target(&b, Some(fcm(2)));
    eventually("A holds B's token", || a.held_from(&b).is_some()).await;
    assert_eq!(a.held_from(&b).unwrap().kinds, Kinds::MSG);
    assert!(!a.ctx.can_wake(&b.did(), &b.me.device, Kind::Call));
    assert!(a.ctx.can_wake(&b.did(), &b.me.device, Kind::Msg));
    b.ep.close().await;

    let info = a.ctx.clone().start_call(b.did(), false).unwrap();
    // Past the shortened first dial of a call that could be woken: nothing happens, as before.
    tokio::time::sleep(Duration::from_millis(crate::push::CALL_FIRST_DIAL_MS + 2_000)).await;
    assert!(a.relay.wakes.lock().is_empty());
    assert_eq!(states(&a), vec![CallState::Dialing]);
    a.ctx.command(&info.call_id, Cmd::Hangup).unwrap();
    eventually("the call ends", || matches!(states(&a).last(), Some(CallState::Ended { .. }))).await;
}
