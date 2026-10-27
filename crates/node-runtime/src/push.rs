//! Push wake-ups on the node (`docs/push.md`, slice 3a): our wake tokens go to the relay and to
//! our contacts, theirs are held for slice 3b, a block or a removed contact takes ours back.
//!
//! The decisions are the pure `wake::WakeState` (sealed with the account state, `State::wake`);
//! this module does the I/O around it:
//! - [`WakeRelay`]: the relay's three calls behind a trait ([`HttpRelay`] is the real one at
//!   `https://push.osvauld.com`; tests use a fake).
//! - [`NodeCtx::set_push_target`]: the platform push token of this device (`None`: always-on).
//! - One task, [`NodeCtx::wake_kick`], mirrors `tokens_kick`: it works out which contacts want a
//!   token (message for everyone we do not block, plus call when `calls_allowed`), registers
//!   them at the relay, hands each contact its token as `Msg::WakeTokens` (`State::wake_outbox`,
//!   retried until `WakeTokensTaken`), and tells the relay about revoked tokens
//!   (`WakeState::revoke_outbox`, retried like block notices). It re-reads the state each round,
//!   so a block, a removed contact or a call permission change needs only a kick.
//! - [`NodeCtx::wake_tokens_incoming`]: a contact's token, kept only from a known device of a
//!   known, unblocked contact.
//!
//! Tokens are never logged. Unblocking is personal policy only: the contact simply becomes
//! wanted again and gets a fresh token (new tag) once it holds us as a contact.
//!
//! Slice 3b, the sending side: [`NodeCtx::wake_device`] asks the relay to wake one device of a
//! contact (the throttle and the 410/403 bookkeeping are `WakeState::wake_for` / `wake_result`).
//! - Messages: `chat_runtime`'s dial loop calls [`NodeCtx::wake_unreached`] when a dial failed
//!   with something to deliver (each device is woken on `wake::msg_wake_due`'s schedule, which
//!   [`NodeCtx::wake_reached`] starts over), and [`NodeCtx::fast_redial`] for the pause before the next dial
//!   (faster than the normal backoff for [`MSG_REDIAL_AT_MS`] after a wake was sent).
//! - Calls: `call.rs`'s `Leg` wakes the callee device (kind `call`) after its first dial failed
//!   and keeps redialling it every [`CALL_REDIAL_MS`] for [`CALL_WAKE_DIAL_MS`]; the call stays
//!   `Dialing` and the protocol is unchanged (the callee never dials for a call).
//!
//! Slice 5, the receiving side (`ring.rs`): [`NodeCtx::on_wake`], the wake ring, `answer_wake_ring`
//! and `decline_wake_ring`.
//!
//! Code wakes (`code.rs`, `docs/push.md` "Code wake tokens"): a `msg` token in our contact ticket,
//! so a stranger who scanned it can wake us to deliver a contact request, and the tokens both ways
//! in `ContactHello` / `ContactWelcome`.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use proto::Msg;
use tokio::sync::Notify;
use wake::{Issued, WakeState};
pub use wake::{WakeAction, hint_opens};
pub use wake_relay_core::Target;
pub use ring::{Early, WAKE_RING_CAP_MS, WAKE_RING_WINDOW_MS, wake_call_id};
use wake_relay_core::api::{RegisterRequest, RegisterResponse, RevokeRequest, WakeRequest};
use wake_relay_core::{Kind, Kinds};

use crate::hooks::BoxFuture;
use crate::store::{ContactDevice, State};
use crate::wire::{Ctrl, RETRY_SECS};
use crate::{Error, NodeCtx, REFUSED, addr_for, now_ms};

/// The deployed relay (`docs/push.md` "Relay"). `P2P_WAKE_RELAY` overrides it, for tests.
pub const RELAY_URL: &str = "https://push.osvauld.com";
/// One request to the relay.
pub const RELAY_TIMEOUT: Duration = Duration::from_secs(10);
/// How often a running node looks at the 7-day renewal window of its tokens.
pub const RENEW_EVERY: Duration = Duration::from_secs(12 * 3600);

/// After a `msg` wake the contact is redialled at these offsets (ms since the wake), instead of
/// the normal backoff, then the normal backoff goes on (PROVISIONAL: a woken phone needs a few
/// seconds to start its node; the wake crate's 30 s throttle is the real rate limit).
pub const MSG_REDIAL_AT_MS: [u64; 7] = [2_000, 4_000, 7_000, 10_000, 15_000, 20_000, 30_000];
/// A chat dial that has not connected after this long wakes the contact's devices we hold a
/// `msg` token for, in parallel with the dial that keeps running (PROVISIONAL; 2026-10-24 device
/// test: waiting out `DIAL_TIMEOUT` (20 s) cost 20 of 24 s). `docs/push.md` Open 6.
pub const MSG_EARLY_WAKE_MS: u64 = 3_000;
/// How often a running dial re-checks for something to deliver after [`MSG_EARLY_WAKE_MS`].
const EARLY_WAKE_POLL: Duration = Duration::from_millis(250);
/// How often an outgoing call redials a device it has woken (PROVISIONAL, `docs/push.md` Calls).
pub const CALL_REDIAL_MS: u64 = 1_000;
/// How long an outgoing call keeps redialling a woken device, from the wake (PROVISIONAL: about
/// the time A stays in `Dialing`; `docs/push.md` Calls).
pub const CALL_WAKE_DIAL_MS: u64 = 30_000;
/// With a call wake available, the first dial gives up after this (PROVISIONAL), so the redials
/// start in seconds instead of after `DIAL_TIMEOUT`.
pub const CALL_FIRST_DIAL_MS: u64 = 6_000;
/// A first call dial that has not connected after this long sends the `call` wake while it keeps
/// running (PROVISIONAL; 2026-10-25 device test: waiting out the 6 s first dial put the ring at
/// ~8 s). An awake callee usually connects well before it.
pub const CALL_EARLY_WAKE_MS: u64 = 2_000;
/// A call ended while dialling a device it had woken: that device rings from the wake alone, so
/// it is dialled this long more to deliver the end (PROVISIONAL; the wake ring itself ends after
/// `WAKE_RING_WINDOW_MS`, 15 s from the wake. 2026-10-25 device test: hanging up 3 s after the
/// wake left the phone ringing ~12 s).
pub const CALL_LATE_CANCEL_MS: u64 = 10_000;
/// One redial attempt of a woken call (the next starts at least `CALL_REDIAL_MS` after it began).
pub const CALL_ATTEMPT_MS: u64 = 3_000;

/// A relay call that did not succeed. `status` is the HTTP status when the relay answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayError {
    pub status: Option<u16>,
    pub message: String,
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            Some(s) => write!(f, "relay answered {s}: {}", self.message),
            None => write!(f, "relay: {}", self.message),
        }
    }
}

impl RelayError {
    pub fn net(message: impl Into<String>) -> Self {
        Self { status: None, message: message.into() }
    }

    pub fn status(status: u16) -> Self {
        Self { status: Some(status), message: "refused".into() }
    }

    /// The token is unusable anyway (malformed or already dead), so a revoke is done.
    fn token_dead(&self) -> bool {
        matches!(self.status, Some(400 | 410))
    }
}

/// The wake relay's API (`wake_relay_core::api`). Messages carry tokens: implementations must
/// never log them.
pub trait WakeRelay: Send + Sync {
    /// `POST /v1/register`: one sealed token per requested tag.
    fn register(&self, req: RegisterRequest) -> BoxFuture<Result<RegisterResponse, RelayError>>;
    /// `POST /v1/wake`: the HTTP status (202 sent, 410 token dead, 429 limited, ...); `0` when
    /// the relay could not be reached. Feed it to `WakeState::wake_result`.
    fn wake(&self, token: String, kind: Kind) -> BoxFuture<u16>;
    /// `POST /v1/revoke`: `Ok` on 2xx.
    fn revoke(&self, token: String) -> BoxFuture<Result<(), RelayError>>;
}

/// The TLS config for the relay: the ring provider named explicitly (nothing installs a process
/// default) and the bundled webpki roots (no platform verifier, so no JNI setup on Android).
fn tls_config() -> Result<rustls::ClientConfig, rustls::Error> {
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    Ok(rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// The real relay over HTTPS (reqwest on rustls, the stack iroh already links).
#[derive(Clone)]
pub struct HttpRelay {
    base: String,
    client: Arc<std::sync::OnceLock<Result<reqwest::Client, String>>>,
}

impl HttpRelay {
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into().trim_end_matches('/').to_string(), client: Arc::new(std::sync::OnceLock::new()) }
    }

    /// `P2P_WAKE_RELAY` if set, else [`RELAY_URL`].
    pub fn from_env() -> Self {
        Self::new(std::env::var("P2P_WAKE_RELAY").unwrap_or_else(|_| RELAY_URL.to_string()))
    }

    fn client(&self) -> Result<reqwest::Client, RelayError> {
        self.client
            .get_or_init(|| {
                let tls = tls_config().map_err(|e| e.to_string())?;
                reqwest::Client::builder()
                    .tls_backend_preconfigured(tls)
                    // osvauld.com's bot protection answers generic agents with 403 (1010).
                    .user_agent(wake::user_agent(env!("CARGO_PKG_VERSION")))
                    .timeout(RELAY_TIMEOUT)
                    .build()
                    .map_err(|e| e.to_string())
            })
            .clone()
            .map_err(RelayError::net)
    }

    /// POSTs `body` as JSON; the status and the body bytes (at most 64 KiB are read).
    async fn post(&self, path: &str, body: Vec<u8>) -> Result<(u16, Vec<u8>), RelayError> {
        let res = self
            .client()?
            .post(format!("{}{path}", self.base))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            // Only the kind of failure: the text of a reqwest error can carry the URL.
            .map_err(|e| RelayError::net(if e.is_timeout() { "timed out" } else if e.is_connect() { "cannot connect" } else { "request failed" }))?;
        let status = res.status().as_u16();
        let bytes = res.bytes().await.map_err(|_| RelayError::net("reading the answer failed"))?;
        Ok((status, bytes.iter().copied().take(wake_relay_core::api::MAX_BODY_BYTES).collect()))
    }
}

impl WakeRelay for HttpRelay {
    fn register(&self, req: RegisterRequest) -> BoxFuture<Result<RegisterResponse, RelayError>> {
        let this = self.clone();
        Box::pin(async move {
            let body = serde_json::to_vec(&req).map_err(|e| RelayError::net(e.to_string()))?;
            let (status, bytes) = this.post("/v1/register", body).await?;
            if !(200..300).contains(&status) {
                return Err(RelayError::status(status));
            }
            serde_json::from_slice(&bytes).map_err(|_| RelayError::net("unreadable register answer"))
        })
    }

    fn wake(&self, token: String, kind: Kind) -> BoxFuture<u16> {
        let this = self.clone();
        Box::pin(async move {
            let Ok(body) = serde_json::to_vec(&WakeRequest { token, kind }) else { return 0 };
            this.post("/v1/wake", body).await.map_or(0, |(status, _)| status)
        })
    }

    fn revoke(&self, token: String) -> BoxFuture<Result<(), RelayError>> {
        let this = self.clone();
        Box::pin(async move {
            let body = serde_json::to_vec(&RevokeRequest { token }).map_err(|e| RelayError::net(e.to_string()))?;
            let (status, _) = this.post("/v1/revoke", body).await?;
            if (200..300).contains(&status) { Ok(()) } else { Err(RelayError::status(status)) }
        })
    }
}

/// Runtime state of the wake task that is not persisted.
#[derive(Default)]
pub struct PushRt {
    /// The relay client; `None` until first used (then [`HttpRelay::from_env`]).
    relay: Mutex<Option<Arc<dyn WakeRelay>>>,
    task: Mutex<Task>,
    /// Cuts a backoff short when something new needs doing.
    nudge: Notify,
    periodic: AtomicBool,
    /// When a `msg` wake was last accepted for a contact (fast redials follow).
    woken: Mutex<HashMap<String, Instant>>,
    /// Wake rings and queued dial-backs of the receiving side (`ring.rs`).
    pub(super) rings: ring::Rings,
}

#[derive(Default)]
struct Task {
    running: bool,
    /// Kicked while a round was running: go round again before stopping.
    again: bool,
}

/// Cancels a pending [`NodeCtx::early_wake`] when dropped (a wake already being sent goes on).
pub struct EarlyWake(tokio::task::JoinHandle<()>);

impl EarlyWake {
    /// Runs `task` until the guard is dropped.
    pub(crate) fn spawn(handle: &tokio::runtime::Handle, task: impl std::future::Future<Output = ()> + Send + 'static) -> Self {
        Self(handle.spawn(task))
    }
}

impl Drop for EarlyWake {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// What one round of the wake task did.
enum Round {
    /// Nothing is left to do.
    Done,
    /// Something was done; look again at once.
    Progress,
    /// The relay or a contact could not be reached; back off.
    Retry,
    /// No endpoint or no unlocked account (start kicks again).
    Stop,
}

/// The wake state, created on first need with a fresh hint key.
pub fn wake_mut(st: &mut State) -> &mut WakeState {
    st.wake.get_or_insert_with(|| WakeState::new(proto::new_device_secret()))
}

/// Whether `did` is a contact we may exchange wake tokens with: known, not blocked by us, not
/// blocking us.
fn live_contact(st: &State, did: &str) -> bool {
    st.contacts.iter().any(|c| c.did == did) && !st.blocked.contains(did) && !crate::block::blocked_me(st, did)
}

/// Whether we owe `did` a wake token of ours: a live contact, or someone we sent a contact
/// request to that is still open (our hello carries the token, so their Accept can wake us to
/// deliver the Welcome; `docs/push.md` "Tokens at setup").
fn token_wanted(st: &State, did: &str) -> bool {
    live_contact(st, did) || requested(st, did)
}

/// An open contact request of ours to `did` (not yet their contact, not blocked either way).
fn requested(st: &State, did: &str) -> bool {
    let now = crate::now();
    st.sent_requests.get(did).is_some_and(|r| r.exp + crate::contact_req::WELCOME_GRACE > now)
        && !st.contacts.iter().any(|c| c.did == did)
        && !st.blocked.contains(did)
        && !crate::block::blocked_me(st, did)
}

/// The kinds each live contact should hold from us: message always, call when allowed. An open
/// request of ours gets the kinds our hello gave (`SentRequest::calls_allowed`).
fn wanted(st: &State) -> Vec<(String, Kinds)> {
    let kinds = |call: bool| if call { Kinds::ALL } else { Kinds::MSG };
    st.contacts
        .iter()
        .filter(|c| live_contact(st, &c.did))
        .map(|c| (c.did.clone(), kinds(c.calls_allowed)))
        .chain(st.sent_requests.iter().filter(|(d, _)| requested(st, d)).map(|(d, r)| (d.clone(), kinds(r.calls_allowed))))
        .collect()
}

/// Takes back and forgets the tokens of every DID we no longer owe one (a block, a contact
/// deleted by their block notice, a request that was refused or ran out). Whether anything
/// changed.
fn forget_gone(st: &mut State) -> bool {
    let gone: Vec<String> = {
        let Some(w) = &st.wake else { return false };
        let dids: BTreeSet<&String> = w.issued.iter().map(|e| &e.did).chain(w.held.iter().map(|e| &e.did)).collect();
        dids.into_iter().filter(|d| !token_wanted(st, d)).cloned().collect()
    };
    let mut changed = !gone.is_empty();
    for did in &gone {
        if let Some(w) = st.wake.as_mut() {
            w.block(did);
        }
    }
    let before = st.wake_outbox.len();
    let keep: Vec<String> = st.wake_outbox.keys().filter(|d| live_contact(st, d)).cloned().collect();
    st.wake_outbox.retain(|d, _| keep.contains(d));
    changed |= st.wake_outbox.len() != before;
    changed
}

fn random_tag() -> [u8; 16] {
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&proto::new_device_secret()[..16]);
    tag
}

impl NodeCtx {
    /// The relay client (the real one unless [`Self::set_wake_relay`] replaced it). Slice 3b
    /// sends wakes through it.
    pub fn wake_relay(&self) -> Arc<dyn WakeRelay> {
        self.push.relay.lock().get_or_insert_with(|| Arc::new(HttpRelay::from_env())).clone()
    }

    /// Replaces the relay client (tests, or an app pointing at another relay).
    pub fn set_wake_relay(&self, relay: Arc<dyn WakeRelay>) {
        *self.push.relay.lock() = Some(relay);
    }

    /// Asks the relay to wake one device of `did`: only if we hold a live token for it that allows
    /// `kind` and its throttle allows another wake now (`WakeState::wake_for`). The relay's answer
    /// updates the held token (410 drops it, 403 forgets the kind). `true`: the relay accepted
    /// (202). Never logs the token.
    pub async fn wake_device(self: &Arc<Self>, did: &str, device: [u8; 32], kind: Kind) -> bool {
        let Ok(me) = self.me() else { return false };
        let token = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch || !live_contact(&s.state, did) {
                return false;
            }
            match s.state.wake.as_mut().and_then(|w| w.wake_for(did, &device, kind, now_ms())) {
                Some(t) => t,
                None => return false,
            }
        };
        let status = self.wake_relay().wake(token, kind).await;
        {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return false;
            }
            if let Some(w) = s.state.wake.as_mut() {
                w.wake_result(did, &device, kind, status);
            }
        }
        tracing::info!("wake {kind:?} sent for a device of {}: relay status {status} (0: not reached)", crate::short(did));
        // The throttle and a dropped token outlive a restart.
        self.persist_in_background(me.epoch);
        status == 202
    }

    /// Whether we hold a live token that can wake `device` of `did` for `kind` (not whether the
    /// throttle would allow it now).
    pub fn can_wake(&self, did: &str, device: &[u8; 32], kind: Kind) -> bool {
        let s = self.shared.lock();
        let now = now_ms();
        live_contact(&s.state, did)
            && s.state.wake.as_ref().is_some_and(|w| w.held.iter().any(|e| e.did == did && e.device == *device && e.held.exp_ms > now && e.held.kinds.allows(kind)))
    }

    /// A chat dial to `did` failed to reach these devices and there is something to deliver: wake
    /// each one (W9) and remember it, so [`Self::fast_redial`] redials sooner. `true`: at least
    /// one wake was accepted.
    pub async fn wake_unreached(self: &Arc<Self>, did: &str, devices: &[[u8; 32]]) -> bool {
        let mut set = tokio::task::JoinSet::new();
        for device in devices {
            let (this, did, device) = (self.clone(), did.to_string(), *device);
            set.spawn(async move { this.wake_device(&did, device, Kind::Msg).await });
        }
        let mut any = false;
        while let Some(done) = set.join_next().await {
            any |= done.unwrap_or(false);
        }
        if any {
            self.push.woken.lock().insert(did.to_string(), Instant::now());
        }
        any
    }

    /// Call when a chat dial of `did` starts; drop the returned guard as soon as the dial ends.
    /// While it is alive, from [`MSG_EARLY_WAKE_MS`] on, the first time `pending` says there is
    /// something to deliver (it may appear mid-dial: typing dials before the send), every device
    /// in `devices` we hold a live `msg` token for is woken, with the usual throttle, while the
    /// dial goes on. Devices without a token are left alone.
    pub fn early_wake(self: &Arc<Self>, did: &str, devices: Vec<[u8; 32]>, pending: impl Fn() -> bool + Send + 'static) -> EarlyWake {
        let (this, did) = (self.clone(), did.to_string());
        let task = self.handle.spawn(async move {
            tokio::time::sleep(Duration::from_millis(MSG_EARLY_WAKE_MS)).await;
            while !pending() {
                tokio::time::sleep(EARLY_WAKE_POLL).await;
            }
            let wakeable: Vec<[u8; 32]> = devices.into_iter().filter(|d| this.can_wake(&did, d, Kind::Msg)).collect();
            if wakeable.is_empty() {
                return;
            }
            // Detached: a dial that ends while the relay is being asked must not cut the wake
            // (and its bookkeeping) short.
            let ctx = this.clone();
            this.handle.spawn(async move { ctx.wake_unreached(&did, &wakeable).await });
        });
        EarlyWake(task)
    }

    /// The pause before the next dial of `did`, while a `msg` wake sent a moment ago is taking
    /// effect ([`MSG_REDIAL_AT_MS`]); `None`: use the normal backoff.
    pub fn fast_redial(&self, did: &str) -> Option<Duration> {
        let mut woken = self.push.woken.lock();
        let elapsed = woken.get(did)?.elapsed();
        match MSG_REDIAL_AT_MS.iter().map(|ms| Duration::from_millis(*ms)).find(|at| *at > elapsed) {
            Some(at) => Some(at - elapsed),
            None => {
                woken.remove(did);
                None
            }
        }
    }

    /// `did` was reached (a dial connected, or they connected to us): the fast redials are over,
    /// and the `msg` wake schedule of their devices and of a queued request to them starts over
    /// (`wake::msg_wake_due`), so a later silence wakes promptly again. Saved when it changed.
    pub fn wake_reached(self: &Arc<Self>, did: &str) {
        self.push.woken.lock().remove(did);
        let epoch = {
            let mut s = self.shared.lock();
            let st = &mut s.state;
            let mut changed = st.wake.as_mut().is_some_and(|w| w.reached(did));
            if let Some(r) = st.sent_requests.get_mut(did).filter(|r| r.wakes != 0) {
                r.wakes = 0;
                changed = true;
            }
            changed.then_some(s.epoch)
        };
        if let Some(epoch) = epoch {
            self.persist_in_background(epoch);
        }
    }

    /// Our platform push token: `Some` registers contacts' tokens at the relay and hands them
    /// out, `None` (always-on, no push) takes every token we issued back. Saved; the work runs
    /// on the wake task.
    pub fn set_push_target(self: &Arc<Self>, target: Option<Target>) -> Result<(), Error> {
        let me = self.me()?;
        if let Some(t) = &target {
            t.validate().map_err(|e| Error::invalid("push target", e))?;
        }
        {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return Err(Error::Locked);
            }
            let st = &mut s.state;
            let had = st.wake.as_ref().is_some_and(|w| w.target.is_some());
            match target {
                Some(t) => wake_mut(st).set_target(t),
                // Nothing to take back (and no state to create) if we never had a target.
                None if had => wake_mut(st).clear_target(),
                None => {}
            }
            // Tokens issued under another target are gone; they are planned afresh.
            let w = st.wake.as_ref();
            st.wake_outbox.retain(|d, _| w.is_some_and(|w| w.issued_for(d).is_some()));
        }
        let saved = self.persist_for(Some(me.epoch));
        self.wake_kick();
        saved
    }

    /// The push target now, if any.
    pub fn push_target(&self) -> Option<Target> {
        self.shared.lock().state.wake.as_ref().and_then(|w| w.target.clone())
    }

    /// After start: work off what is owed, and look at the renewal window every
    /// [`RENEW_EVERY`] while the node lives.
    pub fn wake_started(self: &Arc<Self>) {
        self.wake_dial_backs_started();
        self.wake_kick();
        if self.push.periodic.swap(true, Ordering::SeqCst) {
            return;
        }
        let weak = Arc::downgrade(self);
        self.handle.spawn(async move {
            loop {
                tokio::time::sleep(RENEW_EVERY).await;
                let Some(this) = weak.upgrade() else { return };
                this.wake_kick();
            }
        });
    }

    /// Our device key changed (endpoint rotation): contacts hold our tokens under the old key, so
    /// every one is handed over again, with the same token under the new key.
    pub fn wake_resend_all(self: &Arc<Self>) {
        {
            let mut s = self.shared.lock();
            let st = &mut s.state;
            let dids: Vec<String> = st.wake.iter().flat_map(|w| w.issued.iter().map(|e| e.did.clone())).collect();
            // Same token, new key: every device of every contact needs it again.
            for did in dids {
                st.wake_outbox.insert(did, BTreeSet::new());
            }
        }
        self.persist_in_background(self.shared.lock().epoch);
        self.wake_kick();
    }

    /// Runs the wake task, or asks the running one to look again. Cheap; call it after anything
    /// that changes who should hold a token (contact added or accepted, block, unblock, call
    /// permission, target).
    pub fn wake_kick(self: &Arc<Self>) {
        {
            let mut t = self.push.task.lock();
            if t.running {
                t.again = true;
                drop(t);
                self.push.nudge.notify_one();
                return;
            }
            t.running = true;
            t.again = false;
        }
        let this = self.clone();
        self.handle.spawn(async move { this.wake_loop().await });
    }

    async fn wake_loop(self: Arc<Self>) {
        let mut round = 0usize;
        let mut parked = BTreeSet::new();
        loop {
            match self.wake_round(&mut parked).await {
                Round::Progress => round = 0,
                Round::Retry => {
                    let pause = crate::wire::retry_pause(&RETRY_SECS, round);
                    round += 1;
                    tokio::select! {
                        _ = tokio::time::sleep(pause) => {}
                        _ = self.push.nudge.notified() => round = 0,
                    }
                }
                done @ (Round::Done | Round::Stop) => {
                    let mut t = self.push.task.lock();
                    if t.again && matches!(done, Round::Done) {
                        t.again = false;
                        round = 0;
                        // A new reason to look: contacts that turned us away may take us now.
                        parked.clear();
                        continue;
                    }
                    t.running = false;
                    return;
                }
            }
        }
    }

    /// One pass: forget what is gone, tell the relay about revokes, register what is missing,
    /// hand tokens to contacts.
    async fn wake_round(self: &Arc<Self>, parked: &mut BTreeSet<(String, [u8; 32])>) -> Round {
        let Ok(me) = self.me() else { return Round::Stop };
        let Ok(ep) = self.endpoint() else { return Round::Stop };
        let relay = self.wake_relay();
        let mut progress = false;
        let mut failed = false;

        // What is owed, read fresh.
        let (revokes, plan, dirty) = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return Round::Stop;
            }
            let st = &mut s.state;
            if st.wake.is_none() {
                return Round::Done;
            }
            let dirty = forget_gone(st) | code::retire_codes(st);
            let want = wanted(st);
            let Some(w) = st.wake.as_ref() else { return Round::Done };
            (w.take_revokes(), w.plan_register(&want, now_ms(), random_tag), dirty)
        };
        if dirty {
            self.persist_logged(me.epoch).await;
        }

        // Revokes first: a blocked contact's token must stop working as soon as we can say so.
        let mut revoked = Vec::new();
        for token in revokes {
            match relay.revoke(token.clone()).await {
                Ok(()) => revoked.push(token),
                Err(e) if e.token_dead() => revoked.push(token),
                Err(e) => {
                    tracing::info!("wake revoke failed: {e}");
                    failed = true;
                }
            }
        }
        if !revoked.is_empty() {
            {
                let mut s = self.shared.lock();
                if s.epoch != me.epoch {
                    return Round::Stop;
                }
                if let Some(w) = s.state.wake.as_mut() {
                    for t in &revoked {
                        w.revoked_ok(t);
                    }
                }
            }
            self.persist_logged(me.epoch).await;
            progress = true;
        }

        // Tokens for the contacts that lack one.
        if let Some((req, pending)) = plan {
            match relay.register(req).await {
                Ok(resp) => {
                    let handed = {
                        let mut s = self.shared.lock();
                        if s.epoch != me.epoch {
                            return Round::Stop;
                        }
                        let st = &mut s.state;
                        let handed = st.wake.as_mut().map(|w| w.apply_register(pending, resp)).unwrap_or_default();
                        // A new token: no device holds it yet.
                        for (did, _) in &handed {
                            st.wake_outbox.insert(did.clone(), BTreeSet::new());
                        }
                        handed.len()
                    };
                    self.persist_logged(me.epoch).await;
                    // An answer without our tags would plan the same request forever.
                    if handed == 0 {
                        tracing::info!("wake register failed: the relay answered without our tags");
                        failed = true;
                    } else {
                        tracing::info!("wake register ok: {handed} token(s)");
                        progress = true;
                    }
                }
                Err(e) => {
                    tracing::info!("wake register failed: {e}");
                    failed = true;
                }
            }
        }

        // Hand them over.
        let owed: Vec<(String, Vec<ContactDevice>, Issued)> = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return Round::Stop;
            }
            let st = &mut s.state;
            let mut owed = Vec::new();
            for (did, have) in st.wake_outbox.clone() {
                let issued = st.wake.as_ref().and_then(|w| w.issued_for(&did)).cloned();
                let devices = st.contacts.iter().find(|c| c.did == did).map(|c| c.devices.clone());
                match (issued, devices) {
                    (Some(i), Some(d)) if !d.is_empty() => {
                        // Every device of the contact needs its own copy.
                        if d.iter().all(|x| have.contains(&hex::encode(x.device))) {
                            st.wake_outbox.remove(&did);
                        } else {
                            let todo: Vec<ContactDevice> = d
                                .into_iter()
                                .filter(|x| !have.contains(&hex::encode(x.device)) && !parked.contains(&(did.clone(), x.device)))
                                .collect();
                            if !todo.is_empty() {
                                owed.push((did, todo, i));
                            }
                        }
                    }
                    // Nothing to hand over any more (re-planned on the next round if still due).
                    _ => {
                        st.wake_outbox.remove(&did);
                    }
                }
            }
            owed
        };
        for (did, devices, issued) in owed {
            let msg = Msg::WakeTokens { device: me.device, token: issued.token.clone(), exp_ms: issued.exp_ms, kinds: issued.kinds.bits() };
            for d in &devices {
                match self.deliver_wake_tokens(&ep, d, &msg).await {
                    Ok(true) => {
                        {
                            let mut s = self.shared.lock();
                            let st = &mut s.state;
                            let same = st.wake.as_ref().and_then(|w| w.issued_for(&did)).is_some_and(|i| i.token == issued.token);
                            if same && let Some(have) = st.wake_outbox.get_mut(&did) {
                                have.insert(hex::encode(d.device));
                                let all = st.contacts.iter().find(|c| c.did == did).is_none_or(|c| c.devices.iter().all(|x| have.contains(&hex::encode(x.device))));
                                if all {
                                    st.wake_outbox.remove(&did);
                                }
                            }
                        }
                        self.persist_logged(me.epoch).await;
                        tracing::info!("wake token handed to a device of {}", crate::short(&did));
                        progress = true;
                    }
                    // The device does not hold us as a contact: wait for a reason to ask again
                    // (a kick) instead of a retry loop.
                    Ok(false) => {
                        tracing::info!("wake token refused by a device of {}", crate::short(&did));
                        parked.insert((did.clone(), d.device));
                    }
                    Err(e) => {
                        tracing::info!("wake token to a device of {} not delivered: {e}", crate::short(&did));
                        failed = true;
                    }
                }
            }
        }

        if failed {
            Round::Retry
        } else if progress {
            Round::Progress
        } else {
            Round::Done
        }
    }

    /// `true`: taken; `false`: refused (`Reject`); `Err`: not reached or no sensible answer.
    async fn deliver_wake_tokens(&self, ep: &iroh::Endpoint, d: &ContactDevice, msg: &Msg) -> Result<bool, Error> {
        let conn = crate::wire::dial_call(ep, addr_for(&d.device, d.relay.as_deref())?).await?;
        match crate::wire::exchange(conn, msg).await? {
            Some(Msg::WakeTokensTaken) => Ok(true),
            Some(Msg::Reject { .. }) | Some(Msg::Notice { .. }) => Ok(false),
            other => Err(crate::not_taken("wake token", other.as_ref())),
        }
    }

    /// An incoming connection that opened with `WakeTokens`. Kept only when it comes from the
    /// very device it names (`remote`), a known device of a contact we have not blocked and who
    /// has not blocked us; everything else gets the generic refusal, nothing is held.
    pub async fn wake_tokens_incoming(
        self: &Arc<Self>,
        mut ctrl: Ctrl,
        remote: [u8; 32],
        device: [u8; 32],
        token: String,
        exp_ms: u64,
        kinds: u8,
    ) -> Result<(), Error> {
        let me = self.me()?;
        // `Err` is the reason class of a refusal.
        let held: Result<(), &str> = if device != remote {
            Err("not from the device it names")
        } else if let Some(kinds) = Kinds::from_bits(kinds) {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                Err("locked")
            } else {
                let st = &mut s.state;
                match st.contacts.iter().find(|c| c.has_device(&remote)).map(|c| c.did.clone()) {
                    None => Err("unknown device"),
                    Some(did) if !live_contact(st, &did) => Err("blocked contact"),
                    Some(did) if wake_mut(st).hold(&did, device, token, exp_ms, kinds, now_ms()) => Ok(()),
                    Some(_) => Err("token not acceptable"),
                }
            }
        } else {
            Err("unknown kinds")
        };
        if held.is_ok() {
            tracing::info!("wake token received and accepted");
            self.persist_in_background(me.epoch);
            ctrl.send(&Msg::WakeTokensTaken).await?;
        } else {
            tracing::info!("wake token received and refused: {}", held.err().unwrap_or("refused"));
            ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
        }
        ctrl.finish();
        Ok(())
    }
}

mod code;
mod ring;
pub use code::{CODE_REGISTER_TIMEOUT, HANDOVER_WAIT, apply_code_token, handover_in, hold_handover, with_handover};
#[cfg(test)]
mod backoff_tests;
#[cfg(test)]
mod code_tests;
#[cfg(test)]
mod dial_tests;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod ring_tests;
#[cfg(test)]
mod tests;
