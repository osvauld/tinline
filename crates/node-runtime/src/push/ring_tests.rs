//! Slice 5 on in-process nodes: `on_wake` and the wake ring (`ring.rs`). Real iroh endpoints on
//! loopback and the fake relay of `tests.rs`. The sender "A" holds B's token; B is woken with a
//! hint sealed the way the relay would (B's hint key, the tag B issued to A). Names start with
//! `wake_` (`cargo test -p node-runtime --lib wake`).

use std::sync::Weak;
use std::time::{Duration, Instant};

use iroh::address_lookup::memory::MemoryLookup;
use iroh::endpoint::Accepting;
use iroh::endpoint::Connection;
use tokio::sync::OwnedSemaphorePermit;
use wake::WakeAction;
use wake_relay_core::Hint;

use super::tests::*;
use super::*;
use crate::call::Cmd;
use crate::hooks::NodeHooks;
use crate::wire::Ctrl;
use crate::{CallState, Me};

/// The engine side of a test node: only `chat_kick` does anything, and it is recorded.
#[derive(Default)]
struct Kicks(Mutex<Vec<String>>);

impl NodeHooks for Kicks {
    fn group_unpark(self: Arc<Self>) {}
    fn chat_started(self: Arc<Self>) {}
    fn selfsync_started(self: Arc<Self>) {}
    fn group_started(self: Arc<Self>) {}
    fn group_invites_started(self: Arc<Self>) {}
    fn chat_sessions_close(self: Arc<Self>) {}
    fn gcall_shutdown(self: Arc<Self>) -> BoxFuture<()> {
        Box::pin(async {})
    }
    fn group_close(self: Arc<Self>) -> BoxFuture<()> {
        Box::pin(async {})
    }
    fn chat_close(self: Arc<Self>) -> BoxFuture<()> {
        Box::pin(async {})
    }
    fn selfsync_sessions_close(self: Arc<Self>) {}
    fn incoming_self(self: Arc<Self>, _: Accepting) -> BoxFuture<Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
    fn incoming_link(self: Arc<Self>, _: Accepting) -> BoxFuture<Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
    fn incoming_chat(self: Arc<Self>, _: Accepting, _: bool, _: OwnedSemaphorePermit) -> BoxFuture<Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
    fn chat_conn(self: Arc<Self>, _: Connection, _: bool, _: OwnedSemaphorePermit) -> BoxFuture<Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
    fn group_letter(self: Arc<Self>, _: Ctrl, _: Msg, _: [u8; 32], _: Arc<Me>) -> BoxFuture<Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
    fn in_group_call(&self) -> bool {
        false
    }
    fn chat_call_activity(self: Arc<Self>, _: &str) {}
    fn chat_close_sessions(self: Arc<Self>, _: &str) {}
    fn chat_kick(self: Arc<Self>, did: String) {
        self.0.lock().push(did);
    }
    fn chat_kick_pending(self: Arc<Self>) {}
    fn group_rotation_policy(self: Arc<Self>) -> Result<crate::RotationPolicy, Error> {
        Err(Error::NotStarted)
    }
    fn profile_for_new_contact(self: Arc<Self>, _: &str) {}
}

/// `a` holds the token `b` issued, and `b` has an engine that records its dials.
async fn pair(lookup: &MemoryLookup) -> (T, T, Arc<Kicks>) {
    let (a, b) = (node(lookup).await, node(lookup).await);
    befriend(&a, &b);
    set_target(&b, Some(fcm(2)));
    eventually("A holds B's token", || a.held_from(&b).is_some()).await;
    let kicks = Arc::new(Kicks::default());
    let hooks: Arc<dyn NodeHooks> = kicks.clone();
    b.ctx.install_hooks(Arc::downgrade(&hooks) as Weak<dyn NodeHooks>);
    (a, b, kicks)
}

/// The push the relay would send B for a wake A asked for.
fn hint(b: &T, a: &T, kind: Kind, sent_at_ms: u64) -> String {
    let (key, tag) = (b.state(|st| st.wake.as_ref().unwrap().hint_key), b.issued_to(a).unwrap().tag);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&proto::new_device_secret()[..12]);
    wake_relay_core::hint::seal(&key, &Hint { tag, kind, sent_at_ms }, nonce).unwrap()
}

fn fresh(b: &T, a: &T, kind: Kind) -> String {
    hint(b, a, kind, now_ms())
}

fn wake_events(t: &T, did: &str) -> Vec<CallState> {
    let id = wake_call_id(did);
    t.rec.events.lock().iter().filter(|(i, _)| *i == id).map(|(_, s)| s.clone()).collect()
}

fn ended(states: &[CallState]) -> usize {
    states.iter().filter(|s| matches!(s, CallState::Ended { .. })).count()
}

fn states(t: &T) -> Vec<CallState> {
    t.rec.states.lock().clone()
}

// ---- messages ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_w2_a_msg_wake_dials_the_contact_back() {
    let lookup = MemoryLookup::new();
    let (a, b, kicks) = pair(&lookup).await;
    let action = b.ctx.on_wake(&fresh(&b, &a, Kind::Msg)).unwrap();
    assert_eq!(action, WakeAction::DialBack { did: a.did() });
    assert_eq!(*kicks.0.lock(), vec![a.did()], "the same chat dial a local send starts");
    assert!(!b.ctx.wake_ringing(&a.did()), "a message wake never rings");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_a_msg_wake_before_the_node_is_up_dials_when_it_starts() {
    let lookup = MemoryLookup::new();
    let (a, b, kicks) = pair(&lookup).await;
    let ep = b.ctx.shared.lock().endpoint.take();
    assert_eq!(b.ctx.on_wake(&fresh(&b, &a, Kind::Msg)).unwrap(), WakeAction::DialBack { did: a.did() });
    assert!(kicks.0.lock().is_empty(), "no endpoint yet, nothing to dial with");
    b.ctx.shared.lock().endpoint = ep;
    b.ctx.wake_started();
    assert_eq!(*kicks.0.lock(), vec![a.did()]);
    b.ctx.wake_started();
    assert_eq!(kicks.0.lock().len(), 1, "queued once");
}

// ---- calls ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_w4b_a_call_wake_rings_and_the_real_call_consumes_the_ring() {
    let lookup = MemoryLookup::new();
    let (a, b, kicks) = pair(&lookup).await;
    assert_eq!(b.ctx.on_wake(&fresh(&b, &a, Kind::Call)).unwrap(), WakeAction::Ring { did: a.did() });
    assert!(b.ctx.wake_ringing(&a.did()));
    assert!(kicks.0.lock().is_empty(), "B never dials for a call");

    // A's redial gets through: the usual CallHello.
    let info = a.ctx.clone().start_call(b.did(), false).unwrap();
    eventually("B gets the normal incoming call", || b.rec.incoming.lock().len() == 1).await;
    assert_eq!(b.rec.incoming.lock()[0].call_id, info.call_id);
    assert!(!b.ctx.wake_ringing(&a.did()), "the ring was consumed");
    eventually("A hears Ringing", || states(&a).contains(&CallState::Ringing)).await;
    assert!(a.rec.incoming.lock().is_empty(), "B never dialled A");
    assert!(kicks.0.lock().is_empty());
    // Silent: nothing was reported for the wake ring, not even later.
    b.ctx.set_wake_ring_timing(0, 0);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(wake_events(&b, &a.did()).is_empty());
    assert!(!states(&b).contains(&CallState::Active), "not answered by itself");

    a.ctx.command(&info.call_id, Cmd::Hangup).unwrap();
    eventually("the call ends", || matches!(states(&a).last(), Some(CallState::Ended { .. }))).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_w4_no_call_hello_ends_the_ring_after_the_window() {
    let lookup = MemoryLookup::new();
    let (a, b, _kicks) = pair(&lookup).await;
    b.ctx.set_wake_ring_timing(400, 0);
    let t0 = Instant::now();
    assert_eq!(b.ctx.on_wake(&fresh(&b, &a, Kind::Call)).unwrap(), WakeAction::Ring { did: a.did() });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(wake_events(&b, &a.did()).is_empty(), "still ringing inside the window");

    eventually("the ring ends", || ended(&wake_events(&b, &a.did())) == 1).await;
    assert!(t0.elapsed() >= Duration::from_millis(400));
    assert_eq!(wake_events(&b, &a.did()), vec![CallState::Ended { reason: "cancelled".into() }]);
    assert!(!b.ctx.wake_ringing(&a.did()));
    // A hung up: nothing ever arrives, and the ring does not come back.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(ended(&wake_events(&b, &a.did())), 1);
    assert!(b.rec.incoming.lock().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_a_repeat_call_wake_extends_one_ring_up_to_the_cap() {
    let lookup = MemoryLookup::new();
    let (a, b, _kicks) = pair(&lookup).await;
    b.ctx.set_wake_ring_timing(400, 900);
    let t0 = Instant::now();
    // A wake every 150 ms keeps pushing the end out; the cap stops it at 900 ms.
    while t0.elapsed() < Duration::from_millis(1_500) && ended(&wake_events(&b, &a.did())) == 0 {
        assert_eq!(b.ctx.on_wake(&fresh(&b, &a, Kind::Call)).unwrap(), WakeAction::Ring { did: a.did() });
        assert_eq!(b.ctx.push.rings.map.lock().len(), 1, "never a second ring");
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    eventually("the ring ends", || ended(&wake_events(&b, &a.did())) == 1).await;
    let took = t0.elapsed();
    assert!(took >= Duration::from_millis(850), "extended past one window: {took:?}");
    assert!(took < Duration::from_millis(1_400), "but capped: {took:?}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(ended(&wake_events(&b, &a.did())), 1, "one Ended for one ring");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_answered_early_accepts_the_call_when_it_arrives() {
    let lookup = MemoryLookup::new();
    let (a, b, _kicks) = pair(&lookup).await;
    assert_eq!(b.ctx.on_wake(&fresh(&b, &a, Kind::Call)).unwrap(), WakeAction::Ring { did: a.did() });
    b.ctx.answer_wake_ring(&a.did()).unwrap();
    assert!(a.ctx.answer_wake_ring(&b.did()).is_err(), "A has no ring");

    let info = a.ctx.clone().start_call(b.did(), false).unwrap();
    eventually("the call connects without another press", || states(&a).contains(&CallState::Active)).await;
    assert_eq!(b.rec.incoming.lock().len(), 1, "the app still hears of the call, by DID");
    assert!(!b.ctx.wake_ringing(&a.did()));
    assert!(wake_events(&b, &a.did()).is_empty());
    a.ctx.command(&info.call_id, Cmd::Hangup).unwrap();
    eventually("the call ends", || matches!(states(&a).last(), Some(CallState::Ended { .. }))).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_answered_early_but_no_call_in_time_just_ends() {
    let lookup = MemoryLookup::new();
    let (a, b, _kicks) = pair(&lookup).await;
    b.ctx.set_wake_ring_timing(300, 0);
    b.ctx.on_wake(&fresh(&b, &a, Kind::Call)).unwrap();
    b.ctx.answer_wake_ring(&a.did()).unwrap();
    eventually("the ring ends", || ended(&wake_events(&b, &a.did())) == 1).await;
    assert!(matches!(b.ctx.answer_wake_ring(&a.did()), Err(Error::NotFound)));

    // A late call is an ordinary one: it rings and nobody accepts it for the user.
    let info = a.ctx.clone().start_call(b.did(), false).unwrap();
    eventually("B rings", || b.rec.incoming.lock().len() == 1).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!states(&a).contains(&CallState::Active));
    a.ctx.command(&info.call_id, Cmd::Hangup).unwrap();
    eventually("the call ends", || matches!(states(&a).last(), Some(CallState::Ended { .. }))).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_declined_at_the_ring_turns_the_call_away_unrung() {
    let lookup = MemoryLookup::new();
    let (a, b, _kicks) = pair(&lookup).await;
    b.ctx.on_wake(&fresh(&b, &a, Kind::Call)).unwrap();
    b.ctx.decline_wake_ring(&a.did()).unwrap();
    let _ = a.ctx.clone().start_call(b.did(), false).unwrap();
    eventually("A is told no", || matches!(states(&a).last(), Some(CallState::Ended { reason }) if reason == "declined")).await;
    assert!(b.rec.incoming.lock().is_empty(), "the app never hears of a call the user declined");
    assert!(wake_events(&b, &a.did()).is_empty(), "and no Ended for a ring it dismissed itself");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_an_unavailable_node_refuses_a_call_wake() {
    let lookup = MemoryLookup::new();
    let (a, b, _kicks) = pair(&lookup).await;
    b.edit(|st| st.availability.unavailable = true);
    assert_eq!(b.ctx.on_wake(&fresh(&b, &a, Kind::Call)).unwrap(), WakeAction::IgnoreCall);
    assert!(!b.ctx.wake_ringing(&a.did()));
}

// ---- refusals ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_w6_a_blocked_contact_is_ignored_in_both_directions() {
    let lookup = MemoryLookup::new();
    let (a, b, kicks) = pair(&lookup).await;
    let (msg, call) = (fresh(&b, &a, Kind::Msg), fresh(&b, &a, Kind::Call));

    // We block them (the revoke may not have reached the relay yet).
    b.edit(|st| {
        st.blocked.insert(a.did());
    });
    assert_eq!(b.ctx.on_wake(&msg).unwrap(), WakeAction::Ignore);
    assert_eq!(b.ctx.on_wake(&call).unwrap(), WakeAction::IgnoreCall);
    assert!(kicks.0.lock().is_empty(), "never a dial to a blocked contact");
    assert!(!b.ctx.wake_ringing(&a.did()));

    // They block us.
    let notice = proto::sign_block_notice(&a.me.id, &b.did(), true, now_ms());
    b.edit(|st| {
        st.blocked.clear();
        st.notices.insert(a.did(), crate::store::HeldNotice { notice, name: "friend".into(), blocked: true, at: now_ms() });
    });
    assert_eq!(b.ctx.on_wake(&msg).unwrap(), WakeAction::Ignore);
    assert_eq!(b.ctx.on_wake(&call).unwrap(), WakeAction::IgnoreCall);
    assert!(kicks.0.lock().is_empty());
    assert!(!b.ctx.wake_ringing(&a.did()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_w5_a_stale_call_wake_is_refused() {
    let lookup = MemoryLookup::new();
    let (a, b, _kicks) = pair(&lookup).await;
    let old = hint(&b, &a, Kind::Call, now_ms() - 31_000);
    assert_eq!(b.ctx.on_wake(&old).unwrap(), WakeAction::IgnoreCall);
    assert!(!b.ctx.wake_ringing(&a.did()));
    // A message wake has no staleness rule: it still dials.
    assert_eq!(b.ctx.on_wake(&hint(&b, &a, Kind::Msg, now_ms() - 600_000)).unwrap(), WakeAction::DialBack { did: a.did() });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_garbage_and_foreign_hints_are_ignored() {
    let lookup = MemoryLookup::new();
    let (a, b, kicks) = pair(&lookup).await;
    for junk in ["", "zzzz", "not base64 at all !!", &"A".repeat(2_000), "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"] {
        assert_eq!(b.ctx.on_wake(junk).unwrap(), WakeAction::Ignore, "{junk:.20}");
    }
    // Sealed with someone else's key.
    let other = Hint { tag: b.issued_to(&a).unwrap().tag, kind: Kind::Msg, sent_at_ms: now_ms() };
    let foreign = wake_relay_core::hint::seal(&[7; 32], &other, [1; 12]).unwrap();
    assert_eq!(b.ctx.on_wake(&foreign).unwrap(), WakeAction::Ignore);
    // Our key, a tag we never issued: refused by kind.
    let unknown = Hint { tag: [9; 16], kind: Kind::Call, sent_at_ms: now_ms() };
    let key = b.state(|st| st.wake.as_ref().unwrap().hint_key);
    let sealed = wake_relay_core::hint::seal(&key, &unknown, [2; 12]).unwrap();
    assert_eq!(b.ctx.on_wake(&sealed).unwrap(), WakeAction::IgnoreCall);
    assert!(kicks.0.lock().is_empty() && !b.ctx.wake_ringing(&a.did()));
    // A node that never had a push target has nothing to match.
    let c = node(&lookup).await;
    assert_eq!(c.ctx.on_wake(&fresh(&b, &a, Kind::Msg)).unwrap(), WakeAction::Ignore);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_without_an_open_account_is_an_error_not_a_guess() {
    let lookup = MemoryLookup::new();
    let (a, b, kicks) = pair(&lookup).await;
    let h = fresh(&b, &a, Kind::Call);
    b.ctx.shared.lock().me = None;
    assert!(matches!(b.ctx.on_wake(&h), Err(Error::NoIdentity | Error::Locked)));
    assert!(kicks.0.lock().is_empty());
}

// ---- tokens ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_clearing_the_target_revokes_what_the_hints_name() {
    let lookup = MemoryLookup::new();
    let (a, b, _kicks) = pair(&lookup).await;
    let token = b.issued_to(&a).unwrap().token;
    let h = fresh(&b, &a, Kind::Msg);
    set_target(&b, None);
    eventually("the relay is told", || b.relay.revokes.lock().contains(&token)).await;
    // A hint for the old tag that is still in flight is refused: the tag is no longer issued.
    assert_eq!(b.ctx.on_wake(&h).unwrap(), WakeAction::Ignore);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_w14_a_device_key_rotation_keeps_the_token_working() {
    let lookup = MemoryLookup::new();
    let (a, b, kicks) = pair(&lookup).await;
    let (token, tag) = (b.issued_to(&a).unwrap().token, b.issued_to(&a).unwrap().tag);
    // The contacts get the same token again under the new key (what rotation does).
    b.ctx.wake_resend_all();
    eventually("the outbox settles", || b.state(|st| st.wake_outbox.is_empty())).await;
    assert_eq!(b.issued_to(&a).unwrap().token, token);
    assert_eq!(b.issued_to(&a).unwrap().tag, tag);
    assert_eq!(a.held_from(&b).unwrap().token, token);
    assert_eq!(b.ctx.on_wake(&fresh(&b, &a, Kind::Msg)).unwrap(), WakeAction::DialBack { did: a.did() });
    assert_eq!(kicks.0.lock().len(), 1);
}
