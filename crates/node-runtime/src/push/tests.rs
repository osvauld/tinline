//! Push wake-ups on in-process nodes: real iroh endpoints on loopback (no relay, no network), a
//! fake wake relay. Names start with `wake_` so `cargo test -p node-runtime --lib wake` selects
//! them.

use std::sync::atomic::AtomicU32;
use std::time::Duration;

use iroh::address_lookup::memory::MemoryLookup;
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, SecretKey};
use proto::Cap;
use wake_relay_core::api::IssuedToken;

use super::*;
use crate::store::{Profile, StoredContact};
use crate::{CallFloor, CallInfo, CallState, Me, NodeSink, NodeStatus, Shared};

/// What a test does when the relay is asked to wake a token (the phone starting up).
type WakeHook = Arc<dyn Fn(&str, Kind) + Send + Sync>;

/// A fake relay that records what it was asked and can be told to fail.
#[derive(Default)]
pub(super) struct FakeRelay {
    pub(super) registers: Mutex<Vec<RegisterRequest>>,
    pub(super) revokes: Mutex<Vec<String>>,
    fail_register: AtomicU32,
    fail_revoke: AtomicU32,
    /// Every wake asked for: the token and the kind.
    pub(super) wakes: Mutex<Vec<(String, Kind)>>,
    /// The HTTP status a wake gets; 0: 202.
    pub(super) wake_status: AtomicU32,
    pub(super) on_wake: Mutex<Option<WakeHook>>,
}

/// What the fake relay seals: canonical base64url (so it fits a contact ticket) of a marker,
/// the tag and the kinds. [`fake_tag`] reads the tag back.
fn fake_token(tag: &[u8; 16], kinds: Kinds) -> String {
    let mut raw = b"fake".to_vec();
    raw.extend_from_slice(tag);
    raw.push(kinds.bits());
    wake::token_text(&raw)
}

/// The tag inside a token of the fake relay.
pub(super) fn fake_tag(token: &str) -> Option<[u8; 16]> {
    let raw = wake::token_bytes(token)?;
    raw.get(4..20)?.try_into().ok()
}

pub(super) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn take_one(n: &AtomicU32) -> bool {
    n.try_update(Ordering::SeqCst, Ordering::SeqCst, |v| v.checked_sub(1)).is_ok()
}

impl WakeRelay for FakeRelay {
    fn register(&self, req: RegisterRequest) -> BoxFuture<Result<RegisterResponse, RelayError>> {
        let result = if take_one(&self.fail_register) {
            Err(RelayError::status(503))
        } else {
            let tokens = req.tags.iter().map(|t| IssuedToken { tag: t.tag, token: fake_token(&t.tag, t.kinds) }).collect();
            self.registers.lock().push(req);
            Ok(RegisterResponse { tokens })
        };
        Box::pin(async move { result })
    }

    fn wake(&self, token: String, kind: Kind) -> BoxFuture<u16> {
        self.wakes.lock().push((token.clone(), kind));
        let hook = self.on_wake.lock().clone();
        if let Some(hook) = hook {
            hook(&token, kind);
        }
        let status = match self.wake_status.load(Ordering::SeqCst) {
            0 => 202,
            s => s as u16,
        };
        Box::pin(async move { status })
    }

    fn revoke(&self, token: String) -> BoxFuture<Result<(), RelayError>> {
        let result = if take_one(&self.fail_revoke) {
            Err(RelayError::status(503))
        } else {
            self.revokes.lock().push(token);
            Ok(())
        };
        Box::pin(async move { result })
    }
}

/// What the UI of a node would have been told about calls.
#[derive(Default)]
pub(super) struct Rec {
    pub(super) incoming: Mutex<Vec<CallInfo>>,
    pub(super) states: Mutex<Vec<CallState>>,
    /// The same events with the call id they were reported under.
    pub(super) events: Mutex<Vec<(String, CallState)>>,
}
impl NodeSink for Rec {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, info: CallInfo) {
        self.incoming.lock().push(info);
    }
    fn on_call_state(&self, id: String, state: CallState) {
        self.events.lock().push((id, state.clone()));
        self.states.lock().push(state);
    }
    fn on_log(&self, _: String) {}
}

pub(super) struct TestDir(std::path::PathBuf);
impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("wake-test-{}-{}", std::process::id(), hex(&proto::new_device_secret()[..8])));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        // Only this test's own random directory.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One in-process node: a context over a loopback endpoint that serves the call ALPN.
pub(super) struct T {
    pub(super) ctx: Arc<NodeCtx>,
    pub(super) me: Arc<Me>,
    pub(super) ep: Endpoint,
    pub(super) relay: Arc<FakeRelay>,
    pub(super) rec: Arc<Rec>,
    pub(super) secret: [u8; 32],
    mnemonic: String,
    _dir: TestDir,
}

impl T {
    pub(super) fn did(&self) -> String {
        self.me.id.did().to_string()
    }

    pub(super) fn state<R>(&self, f: impl FnOnce(&State) -> R) -> R {
        f(&self.ctx.shared.lock().state)
    }

    pub(super) fn edit<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut self.ctx.shared.lock().state)
    }

    pub(super) fn held_from(&self, other: &T) -> Option<wake::Held> {
        self.state(|st| st.wake.as_ref()?.held.iter().find(|e| e.did == other.did() && e.device == other.me.device).map(|e| e.held.clone()))
    }

    pub(super) fn issued_to(&self, other: &T) -> Option<Issued> {
        self.state(|st| st.wake.as_ref()?.issued_for(&other.did()).cloned())
    }
}

pub(super) async fn node(lookup: &MemoryLookup) -> T {
    node_with(lookup, State::default()).await
}

async fn node_with(lookup: &MemoryLookup, state: State) -> T {
    let (_id, phrase) = identity::generate();
    node_as(lookup, state, phrase.to_string(), None).await
}

/// A node on `floor`, as another account's slot of one device would be (`docs/multi-online.md`).
pub(super) async fn node_on_floor(lookup: &MemoryLookup, floor: &Arc<CallFloor>) -> T {
    let (_id, phrase) = identity::generate();
    node_as(lookup, State::default(), phrase.to_string(), Some(floor.clone())).await
}

/// Another device of the account `other` belongs to.
pub(super) async fn second_device(lookup: &MemoryLookup, other: &T) -> T {
    node_as(lookup, State::default(), other.mnemonic.clone(), None).await
}

async fn node_as(lookup: &MemoryLookup, state: State, mnemonic: String, floor: Option<Arc<CallFloor>>) -> T {
    let secret = proto::new_device_secret();
    let profile = Profile { mnemonic: mnemonic.clone(), name: "synthetic".into(), device_secret: secret };
    let me = Arc::new(Me::load(profile, 1).unwrap());
    let ep = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::from_bytes(&secret))
        .alpns(vec![proto::ALPN.to_vec()])
        .address_lookup(lookup.clone())
        .bind()
        .await
        .unwrap();
    lookup.add_endpoint_info(ep.addr());
    let dir = TestDir::new();
    let mut shared = Shared::new();
    shared.me = Some(me.clone());
    shared.endpoint = Some(ep.clone());
    shared.state = state;
    let accounts = account_model::accounts::AccountDirs::open(dir.0.clone()).unwrap();
    let rec = Arc::new(Rec::default());
    let mut ctx = NodeCtx::new(tokio::runtime::Handle::current(), accounts, shared, rec.clone());
    if let Some(floor) = floor {
        ctx = ctx.with_floor(floor);
    }
    let ctx = Arc::new(ctx);
    ctx.join_floor();
    let relay = Arc::new(FakeRelay::default());
    ctx.set_wake_relay(relay.clone());
    ctx.serve_private(ep.clone(), true);
    T { ctx, me, ep, relay, rec, secret, mnemonic, _dir: dir }
}

/// The phone starts up again: a new endpoint under the same key, served by the same context.
pub(super) async fn bring_up(ctx: &Arc<NodeCtx>, secret: [u8; 32], lookup: &MemoryLookup) {
    let ep = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::from_bytes(&secret))
        .alpns(vec![proto::ALPN.to_vec()])
        .address_lookup(lookup.clone())
        .bind()
        .await
        .unwrap();
    lookup.add_endpoint_info(ep.addr());
    ctx.shared.lock().endpoint = Some(ep.clone());
    ctx.serve_private(ep, true);
}

pub(super) fn contact_of(other: &T, mine: &Identity, calls_allowed: bool) -> StoredContact {
    StoredContact {
        did: other.did(),
        name: "friend".into(),
        devices: vec![ContactDevice { device: other.me.device, relay: None }],
        device_list: None,
        grant_from_them: proto::issue_token(&other.me.id, mine.did(), Cap::Message, now_ms(), 1_000_000),
        call_grant_from_them: calls_allowed.then(|| proto::issue_token(&other.me.id, mine.did(), Cap::Call, now_ms(), 1_000_000)),
        calls_allowed,
        added_at: 1,
        alias: None,
        verified: false,
    }
}

use identity::Identity;

/// `a` and `b` are contacts of each other (calls allowed both ways).
pub(super) fn befriend(a: &T, b: &T) {
    let ca = contact_of(b, &a.me.id, true);
    a.edit(|st| st.contacts.push(ca));
    let cb = contact_of(a, &b.me.id, true);
    b.edit(|st| st.contacts.push(cb));
}

/// A valid FCM-looking token, distinct per `n`.
pub(super) fn fcm(n: u32) -> Target {
    Target::Fcm { token: format!("fcm-token-for-tests-{n:04}") }
}

/// `set_push_target` on a node with no account files: it still takes effect, only the save fails.
pub(super) fn set_target(t: &T, target: Option<Target>) {
    match t.ctx.set_push_target(target) {
        Ok(()) | Err(Error::NoIdentity) => {}
        Err(e) => panic!("set_push_target: {e}"),
    }
}

pub(super) async fn eventually(what: &str, mut ok: impl FnMut() -> bool) {
    for _ in 0..400 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for: {what}");
}

/// Sends `msg` to `to` from `from`'s endpoint (what a contact or a stranger would do).
async fn send_raw(from: &T, to: &T, msg: &Msg) -> Option<Msg> {
    let conn = crate::wire::dial_call(&from.ep, EndpointAddr::new(iroh::PublicKey::from_bytes(&to.me.device).unwrap())).await.unwrap();
    crate::wire::exchange(conn, msg).await.unwrap()
}

fn wake_tokens(device: [u8; 32], token: &str, kinds: u8) -> Msg {
    Msg::WakeTokens { device, token: token.into(), exp_ms: now_ms() + 24 * 3600 * 1000, kinds }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_target_registers_and_the_contact_holds_the_token() {
    let lookup = MemoryLookup::new();
    let (a, b) = (node(&lookup).await, node(&lookup).await);
    befriend(&a, &b);
    set_target(&a, Some(fcm(1)));
    eventually("B holds a token of A's device", || b.held_from(&a).is_some()).await;

    let held = b.held_from(&a).unwrap();
    let issued = a.issued_to(&b).unwrap();
    assert_eq!(held.token, issued.token, "B holds exactly what the relay sealed for B");
    assert_eq!(held.kinds, Kinds::ALL);
    assert_eq!(held.exp_ms, issued.exp_ms);
    let regs = a.relay.registers.lock().clone();
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, fcm(1));
    assert_eq!(regs[0].tags.len(), 1);
    assert_eq!(regs[0].hint_key, a.state(|st| st.wake.as_ref().unwrap().hint_key));
    eventually("the outbox empties", || a.state(|st| st.wake_outbox.is_empty())).await;
    assert!(b.state(|st| st.wake.as_ref().is_none_or(|w| w.issued.is_empty())), "B has no target, so issued nothing");

    // And the other way round.
    set_target(&b, Some(fcm(2)));
    eventually("A holds a token of B's device", || a.held_from(&b).is_some()).await;
    assert_eq!(a.held_from(&b).unwrap().token, b.issued_to(&a).unwrap().token);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_tokens_from_strangers_and_for_other_devices_are_rejected() {
    let lookup = MemoryLookup::new();
    let (a, friend, stranger) = (node(&lookup).await, node(&lookup).await, node(&lookup).await);
    befriend(&a, &friend);
    let reject = |m: Option<Msg>| matches!(m, Some(Msg::Reject { .. }));

    // Not a contact's device.
    let reply = send_raw(&stranger, &a, &wake_tokens(stranger.me.device, "tok-from-a-stranger", 3)).await;
    assert!(reject(reply), "a stranger gets the generic refusal");
    // A contact naming a device it is not connecting from.
    let reply = send_raw(&friend, &a, &wake_tokens([9; 32], "tok-for-another-device", 3)).await;
    assert!(reject(reply), "the device must be the one on the connection");
    // Kinds the relay never issues.
    for kinds in [0u8, 4, 255] {
        let reply = send_raw(&friend, &a, &wake_tokens(friend.me.device, "tok-with-odd-kinds", kinds)).await;
        assert!(reject(reply), "kinds {kinds}");
    }
    // Already expired.
    let stale = Msg::WakeTokens { device: friend.me.device, token: "tok-expired".into(), exp_ms: now_ms() - 1, kinds: 3 };
    assert!(reject(send_raw(&friend, &a, &stale).await));
    assert!(a.state(|st| st.wake.as_ref().is_none_or(|w| w.held.is_empty())), "nothing was held");

    // A contact we block is refused too, and a good one is taken.
    a.edit(|st| {
        st.blocked.insert(friend.did());
    });
    assert!(reject(send_raw(&friend, &a, &wake_tokens(friend.me.device, "tok-from-blocked", 3)).await));
    assert!(a.held_from(&friend).is_none());
    a.edit(|st| {
        st.blocked.remove(&friend.did());
    });
    let reply = send_raw(&friend, &a, &wake_tokens(friend.me.device, "tok-from-a-friend", 3)).await;
    assert!(matches!(reply, Some(Msg::WakeTokensTaken)));
    assert_eq!(a.held_from(&friend).unwrap().token, "tok-from-a-friend");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_calls_off_hands_a_message_only_token_and_revokes_the_old_one() {
    let lookup = MemoryLookup::new();
    let (a, b) = (node(&lookup).await, node(&lookup).await);
    befriend(&a, &b);
    set_target(&a, Some(fcm(1)));
    eventually("B holds an all-kinds token", || b.held_from(&a).is_some()).await;
    let first = a.issued_to(&b).unwrap();
    assert_eq!(first.kinds, Kinds::ALL);

    a.edit(|st| st.contacts[0].calls_allowed = false);
    a.ctx.wake_kick();
    eventually("B holds a message-only token", || b.held_from(&a).is_some_and(|h| h.kinds == Kinds::MSG)).await;
    let second = a.issued_to(&b).unwrap();
    assert_ne!(first.token, second.token);
    assert_ne!(first.tag, second.tag, "a new tag, so the old token can be revoked alone");
    assert_eq!(b.held_from(&a).unwrap().token, second.token);
    eventually("the old token is revoked at the relay", || a.relay.revokes.lock().contains(&first.token)).await;
    assert!(!a.relay.revokes.lock().contains(&second.token));
    eventually("the revoke outbox empties", || a.state(|st| st.wake.as_ref().unwrap().revoke_outbox.is_empty())).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_block_revokes_and_a_failing_relay_is_retried() {
    let lookup = MemoryLookup::new();
    let (a, b) = (node(&lookup).await, node(&lookup).await);
    befriend(&a, &b);
    set_target(&a, Some(fcm(1)));
    eventually("B holds a token", || b.held_from(&a).is_some()).await;
    let token = a.issued_to(&b).unwrap().token;
    let registers = a.relay.registers.lock().len();

    // The relay refuses the first revoke; the block must still hold locally at once.
    a.relay.fail_revoke.store(1, Ordering::SeqCst);
    a.edit(|st| {
        st.blocked.insert(b.did());
    });
    a.ctx.wake_kick();
    eventually("the token left our state", || a.issued_to(&b).is_none()).await;
    assert!(a.relay.revokes.lock().is_empty() || a.relay.revokes.lock().contains(&token));
    // Retried after the first backoff (2 s) and then done.
    eventually("the revoke reaches the relay", || a.relay.revokes.lock().contains(&token)).await;
    eventually("the outbox empties", || a.state(|st| st.wake.as_ref().unwrap().revoke_outbox.is_empty())).await;
    assert_eq!(a.relay.registers.lock().len(), registers, "a blocked contact is never registered for");
    assert!(a.state(|st| st.wake_outbox.is_empty()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_a_revoke_owed_at_shutdown_is_sent_after_the_next_start() {
    let lookup = MemoryLookup::new();
    let (a, b) = (node(&lookup).await, node(&lookup).await);
    befriend(&a, &b);
    set_target(&a, Some(fcm(1)));
    eventually("B holds a token", || b.held_from(&a).is_some()).await;
    let token = a.issued_to(&b).unwrap().token;

    // The relay is down for good in this run: the block is local, the revoke stays owed.
    a.relay.fail_revoke.store(u32::MAX, Ordering::SeqCst);
    a.edit(|st| {
        st.blocked.insert(b.did());
    });
    a.ctx.wake_kick();
    eventually("the revoke is owed", || a.state(|st| st.wake.as_ref().unwrap().revoke_outbox == [token.clone()])).await;

    // "Restart": the sealed state as written (JSON), a new node, the relay back.
    let saved = serde_json::to_string(&a.state(|st| st.clone())).unwrap();
    let a2 = node_with(&lookup, serde_json::from_str(&saved).unwrap()).await;
    assert!(a2.relay.revokes.lock().is_empty());
    a2.ctx.wake_started();
    eventually("the new run revokes it", || a2.relay.revokes.lock().contains(&token)).await;
    eventually("and the outbox is empty", || a2.state(|st| st.wake.as_ref().unwrap().revoke_outbox.is_empty())).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_target_change_reissues_to_every_contact() {
    let lookup = MemoryLookup::new();
    let (a, b, c) = (node(&lookup).await, node(&lookup).await, node(&lookup).await);
    befriend(&a, &b);
    befriend(&a, &c);
    set_target(&a, Some(fcm(1)));
    eventually("both hold a token", || b.held_from(&a).is_some() && c.held_from(&a).is_some()).await;
    let (old_b, old_c) = (b.held_from(&a).unwrap().token, c.held_from(&a).unwrap().token);
    assert_ne!(old_b, old_c);

    set_target(&a, Some(fcm(2)));
    eventually("both hold a token for the new target", || {
        b.held_from(&a).is_some_and(|h| h.token != old_b) && c.held_from(&a).is_some_and(|h| h.token != old_c)
    })
    .await;
    let regs = a.relay.registers.lock().clone();
    assert_eq!(regs.last().unwrap().target, fcm(2));
    assert_eq!(regs.last().unwrap().tags.len(), 2);
    assert!(a.relay.revokes.lock().is_empty(), "tokens of the old push token die with it");

    // Always-on again: everything we issued is taken back.
    set_target(&a, None);
    let (new_b, new_c) = (b.held_from(&a).unwrap().token, c.held_from(&a).unwrap().token);
    eventually("both are revoked", || {
        let r = a.relay.revokes.lock();
        r.contains(&new_b) && r.contains(&new_c)
    })
    .await;
    assert!(a.ctx.push_target().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_unblock_gets_a_fresh_token_with_a_new_tag() {
    let lookup = MemoryLookup::new();
    let (a, b) = (node(&lookup).await, node(&lookup).await);
    befriend(&a, &b);
    set_target(&a, Some(fcm(1)));
    eventually("B holds a token", || b.held_from(&a).is_some()).await;
    let first = a.issued_to(&b).unwrap();

    a.edit(|st| {
        st.blocked.insert(b.did());
    });
    a.ctx.wake_kick();
    eventually("the old token is revoked", || a.relay.revokes.lock().contains(&first.token)).await;

    // Personal policy only: unblocking changes nothing else; the contact is wanted again.
    a.edit(|st| {
        st.blocked.remove(&b.did());
    });
    a.ctx.wake_kick();
    eventually("B holds a new token", || b.held_from(&a).is_some_and(|h| h.token != first.token)).await;
    let second = a.issued_to(&b).unwrap();
    assert_ne!(first.tag, second.tag);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_a_contact_that_turned_us_away_is_not_retried_in_a_loop() {
    let lookup = MemoryLookup::new();
    let (a, b) = (node(&lookup).await, node(&lookup).await);
    // A knows B, but B does not know A (it deleted us after a block).
    let cb = contact_of(&b, &a.me.id, true);
    a.edit(|st| st.contacts.push(cb));
    set_target(&a, Some(fcm(1)));
    eventually("the loop settled", || !a.ctx.push.task.lock().running).await;
    assert!(b.held_from(&a).is_none());
    assert_eq!(a.state(|st| st.wake_outbox.len()), 1, "still owed, for when they take us again");
    assert_eq!(a.relay.registers.lock().len(), 1, "registered once, not again and again");

    // They hold us again: the next kick delivers it, with the same token.
    let ca = contact_of(&a, &b.me.id, true);
    b.edit(|st| st.contacts.push(ca));
    a.ctx.wake_kick();
    eventually("B holds the token", || b.held_from(&a).is_some()).await;
    assert_eq!(a.relay.registers.lock().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_the_hint_key_is_drawn_once_and_the_state_round_trips() {
    let lookup = MemoryLookup::new();
    let (a, b) = (node(&lookup).await, node(&lookup).await);
    befriend(&a, &b);
    assert!(a.state(|st| st.wake.is_none()), "nothing is created before it is needed");
    set_target(&a, Some(fcm(1)));
    eventually("B holds a token", || b.held_from(&a).is_some()).await;
    let key = a.state(|st| st.wake.as_ref().unwrap().hint_key);
    set_target(&a, Some(fcm(2)));
    assert_eq!(a.state(|st| st.wake.as_ref().unwrap().hint_key), key, "kept across target changes");
    set_target(&b, Some(fcm(3)));
    assert_ne!(b.state(|st| st.wake.as_ref().unwrap().hint_key), key);

    // Everything survives the sealed JSON; an old state without the fields still loads.
    eventually("A holds B's token and issued its own again", || a.held_from(&b).is_some() && a.issued_to(&b).is_some()).await;
    let st = a.state(|st| st.clone());
    let back: State = serde_json::from_str(&serde_json::to_string(&st).unwrap()).unwrap();
    assert_eq!(back.wake, st.wake);
    assert_eq!(back.wake_outbox, st.wake_outbox);
    assert!(back.wake.as_ref().unwrap().held.len() == 1 && !back.wake.as_ref().unwrap().issued.is_empty());
    let old: State = serde_json::from_str("{}").unwrap();
    assert!(old.wake.is_none() && old.wake_outbox.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_a_relay_that_fails_to_register_is_retried() {
    let lookup = MemoryLookup::new();
    let (a, b) = (node(&lookup).await, node(&lookup).await);
    befriend(&a, &b);
    a.relay.fail_register.store(1, Ordering::SeqCst);
    set_target(&a, Some(fcm(1)));
    eventually("B holds a token after the retry", || b.held_from(&a).is_some()).await;
    assert_eq!(a.relay.registers.lock().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_a_bad_push_target_is_refused() {
    let lookup = MemoryLookup::new();
    let a = node(&lookup).await;
    assert!(matches!(a.ctx.set_push_target(Some(Target::Fcm { token: "short".into() })), Err(Error::Protocol(_))));
    assert!(a.state(|st| st.wake.is_none()));
    // Never having had a target, clearing it creates nothing and asks the relay nothing.
    set_target(&a, None);
    assert!(a.state(|st| st.wake.is_none()));
}

#[test]
fn wake_http_relay_uses_the_osvauld_agent_and_the_deployed_url() {
    assert_eq!(RELAY_URL, "https://push.osvauld.com");
    assert!(wake::user_agent(env!("CARGO_PKG_VERSION")).starts_with("Tinline/"));
    assert_eq!(HttpRelay::new("https://x.example/").base, "https://x.example");
    assert!(RelayError::status(410).token_dead() && RelayError::status(400).token_dead());
    assert!(!RelayError::status(503).token_dead() && !RelayError::net("x").token_dead());
}
