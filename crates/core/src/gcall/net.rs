//! IO wiring for group calls (GA-P4): runs [`HostEngine`] and [`ClientEngine`] on real iroh
//! connections on `tinline/gcall/1` (§1.1).
//!
//! **Unstable.** This is the core-side API that group chat and the UI will call later; it is
//! plain Rust (no uniffi) and may change. Membership, permits, group endpoints and host
//! addresses all come through [`CallDirectory`], because real group membership is not in
//! production yet. Today a node dials and accepts from its device endpoint; the group endpoint
//! (U2) replaces it when group chat lands.
//!
//! Shape:
//!
//! - One [`Session`] per node at a time: the call this device is in and/or hosts. It holds both
//!   engines behind one lock. Engine actions are executed synchronously under that lock
//!   (control frames go to a per-connection writer task, datagrams go out at once), so the
//!   host's own user is routed through [`ConnId::LOCAL`] without a network hop: a `LOCAL`
//!   control frame or datagram from one engine is fed straight into the other.
//! - Each remote connection has one reader task (control stream + datagrams) and one writer
//!   task (control stream, then the close).
//! - A 20 ms ticker per session drives `HostEngine::tick` / `ClientEngine::tick` and a
//!   level-only keepalive. Audio itself is driven by the app's audio threads, exactly like
//!   1:1 calls: [`Node::push_mic`](crate::Node::push_mic) / `pull_speaker` fall through to the
//!   group call when no 1:1 call is active (or use [`crate::Node::gcall_push_mic`] directly).
//! - Close codes ([`close_code`]): a client that sees [`CLOSE_ANSWERED_ELSEWHERE`] ends with
//!   "answered elsewhere"; any other close while joined starts the §5 redial.
//! - Takeover (§5): a node whose `host_ref` is in the `backups` of a call it knows (the call
//!   its own client is in, or one fed through [`crate::Node::gcall_observe_start`]) builds a
//!   `HostEngine::takeover` on the first `Join{host_gen + 1}`, from the network or from its
//!   own redialing client. If that first Join is refused the engine is dropped again.
//! - Glare and split (§3.1, §5): every CallStart this node learns of goes on a per-group
//!   [`CallBoard`]; when the call hosted here lost, it ends with `End{superseded, next_host:
//!   winner}` and its members follow the winner (at once if they know its CallStart, else when
//!   it arrives, within [`FOLLOW_WINDOW`]).
//! - One call at a time: while this device's user is in a group call a 1:1 call is refused
//!   (incoming: Busy), and a group call cannot be started or joined during a 1:1 call.
//!   `Node::stop` and `Node::lock` leave the call and, when hosting, send `End{host_leaving,
//!   next_host}` before the endpoint closes.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use identity::Identity;
use iroh::endpoint::{Accepting, Connection, ConnectionError, RecvStream, SendStream};
use iroh::EndpointAddr;
use parking_lot::Mutex;
use proto::SignedBlob;
use proto::gcall::{self as w, CallStart, GCALL_ALPN, GroupCallMsg};
use tokio::sync::mpsc;

use super::*;
use crate::Error;
use crate::node::Inner;

/// QUIC application close codes on `tinline/gcall/1`. 0 is a plain goodbye (left, link
/// replaced). The rest mirror [`CloseReason`]; `REFUSED` is a Join that was not accepted.
pub const CLOSE_BYE: u32 = 0;
pub const CLOSE_ENDED: u32 = 0x10;
pub const CLOSE_ANSWERED_ELSEWHERE: u32 = 0x11;
pub const CLOSE_REMOVED: u32 = 0x12;
pub const CLOSE_DROPPED: u32 = 0x13;
pub const CLOSE_REFUSED: u32 = 0x14;

/// Connect + first control frame, per attempt. The engine's own `ANSWER_TIMEOUT_MS` covers a
/// host that connects but never answers.
const DIAL_TIMEOUT: Duration = Duration::from_millis(ANSWER_TIMEOUT_MS);
/// How long a closing writer waits for the peer to have read its last frames.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(1);
/// After the control stream ends, how long to wait for the peer's close (and its code).
const CLOSE_WAIT: Duration = Duration::from_secs(2);
/// Level byte of the idle keepalive: silence (127 = -127 dBov).
const SILENT_LEVEL: u8 = 127;
/// How long a `Superseded` waits for the winner's CallStart to arrive through the relay.
pub const FOLLOW_WINDOW: Duration = Duration::from_secs(30);

pub fn close_code(r: CloseReason) -> u32 {
    match r {
        CloseReason::Ended => CLOSE_ENDED,
        CloseReason::AnsweredElsewhere => CLOSE_ANSWERED_ELSEWHERE,
        CloseReason::Removed => CLOSE_REMOVED,
        CloseReason::Dropped => CLOSE_DROPPED,
    }
}

/// What the wiring needs to know about groups: the [`CallAuthority`] checks plus where a call
/// host can be reached. Group chat implements it over its derived state
/// (`group::calldir::GroupCallDir`, GA-I1); [`StaticDirectory`] is the fixed one for tests.
pub trait CallDirectory: CallAuthority + Send + Sync {
    /// The transport address of `host`'s group endpoint, if known.
    fn host_addr(&self, host: &HostRef) -> Option<EndpointAddr>;
    /// This device's call-host ref in `gid`, when [`GcallConfig::host_ref`] is not set (a
    /// device may host calls of several groups, each under its own permit).
    fn my_host_ref(&self, _gid: &str) -> Option<HostRef> {
        None
    }
    /// This device's DID-signed group endpoint note for a Join, when [`GcallConfig::note`] is
    /// not set.
    fn my_note(&self) -> Option<SignedBlob> {
        None
    }
}

/// A [`CallDirectory`] over a [`StaticAuthority`] and a fixed host address book; for tests and
/// until group chat provides the real one. Both halves may be changed while a call runs.
#[derive(Default)]
pub struct StaticDirectory {
    pub auth: Mutex<StaticAuthority>,
    pub hosts: Mutex<BTreeMap<HostRef, EndpointAddr>>,
}

impl CallAuthority for StaticDirectory {
    fn is_member(&self, gid: &str, did: &str) -> bool {
        self.auth.lock().is_member(gid, did)
    }
    fn host_permit_ok(&self, gid: &str, host: &HostRef) -> bool {
        self.auth.lock().host_permit_ok(gid, host)
    }
    fn endpoint_ok(&self, gid: &str, did: &str, note: Option<&SignedBlob>, endpoint: &[u8; 32]) -> bool {
        self.auth.lock().endpoint_ok(gid, did, note, endpoint)
    }
}

impl CallDirectory for StaticDirectory {
    fn host_addr(&self, host: &HostRef) -> Option<EndpointAddr> {
        self.hosts.lock().get(host).cloned()
    }
}

/// Events out of the group-call layer. Delivered on core threads, outside any lock.
#[derive(Debug, Clone, PartialEq)]
pub enum GcallEvent {
    /// This device is in the Roster of `host` at `host_gen` (also after a redial).
    Joined { call_id: String, host_gen: u32, slot: u8, host: HostRef },
    /// slot → DID.
    RosterChanged { call_id: String, host_gen: u32, roster: BTreeMap<u8, String> },
    /// Host lost or handing over; redialing the backups ("Reconnecting…").
    Reconnecting { call_id: String },
    /// The call lost a glare or split; the call hosted by `by` is joined automatically when
    /// its CallStart is known (`gcall_observe_start`).
    Superseded { call_id: String, by: HostRef },
    /// This device's participation ended.
    Ended { call_id: String, why: ClientEnd },
    /// This device now hosts `call_id` at `host_gen` (a start or a takeover). `start` is the
    /// signed CallStart to announce through the chat hosts (G1).
    HostingStarted { call_id: String, host_gen: u32, start: SignedBlob },
    /// This device stopped hosting. `end` is the signed End (relay it when `reason` is `Ended`).
    HostingEnded { call_id: String, reason: EndReason, next_host: Option<HostRef>, end: Option<SignedBlob> },
    Notice(HostNotice),
}

pub trait GcallEvents: Send + Sync {
    fn on_gcall_event(&self, ev: GcallEvent);
}

/// Set once per node (`gcall_configure`) before hosting or joining.
#[derive(Clone)]
pub struct GcallConfig {
    pub dir: Arc<dyn CallDirectory>,
    pub events: Arc<dyn GcallEvents>,
    /// This device's call-host ref, if it holds a `host_call` permit and has hosting enabled.
    /// Needed to host, and to take over as a backup.
    pub host_ref: Option<HostRef>,
    /// Local hosting conditions (§2.4).
    pub cond: Cond,
    /// This device's DID-signed group endpoint note, sent with every Join (G2).
    pub note: Option<SignedBlob>,
}

impl GcallConfig {
    /// This device's host ref for `gid`: the configured one, else the directory's.
    pub fn host_ref_for(&self, gid: &str) -> Option<HostRef> {
        self.host_ref.clone().or_else(|| self.dir.my_host_ref(gid))
    }

    fn note_now(&self) -> Option<SignedBlob> {
        self.note.clone().or_else(|| self.dir.my_note())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientStatus {
    pub state: ClientState,
    pub host: HostRef,
    pub host_gen: u32,
    pub my_slot: Option<u8>,
    pub roster: BTreeMap<u8, String>,
    /// Slots the host forwards (`Active`).
    pub active: BTreeSet<u8>,
    /// Downstream datagrams accepted per roster slot.
    pub heard: BTreeMap<u8, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostingStatus {
    pub host_gen: u32,
    pub k: usize,
    pub epoch: u32,
    pub roster: BTreeMap<u8, String>,
    /// The forwarded set S (plus the G6-D2 provisional slot).
    pub forwarded: BTreeSet<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcallStatus {
    pub call_id: String,
    pub client: Option<ClientStatus>,
    pub hosting: Option<HostingStatus>,
}

// ---- node-level service -----------------------------------------------------------------

/// Lives in `Inner`. At most one live session.
#[derive(Default)]
pub(crate) struct Service {
    cfg: Mutex<Option<Arc<GcallConfig>>>,
    /// The latest `gcall_set_cond`, also before any configuration.
    cond: Mutex<Option<Cond>>,
    cur: Mutex<Option<Arc<Session>>>,
    known: Known,
}

/// What this node knows about calls beyond its own session.
#[derive(Default)]
struct Calls {
    /// CallStarts seen through the chat relay (or the API), highest generation per call_id;
    /// what a backup that is not in the call takes over from, and what a `Superseded` follows.
    starts: BTreeMap<String, CallStart>,
    /// gid → board (glare and split settling for the call hosted here).
    boards: BTreeMap<String, CallBoard>,
    /// A `Superseded` whose winner's CallStart is not known yet: (winner, gid, when).
    follow: Option<(HostRef, String, Instant)>,
}

type Known = Arc<Mutex<Calls>>;

impl Service {
    fn live(&self) -> Option<Arc<Session>> {
        self.cur.lock().clone().filter(|s| !s.finished())
    }

    /// This device's user is in a group call (or one is being set up): 1:1 calls are refused.
    /// Hosting listen-only does not count.
    pub(crate) fn in_call(&self) -> bool {
        self.live().is_some_and(|s| {
            let st = s.st.lock();
            st.client.as_ref().map_or(st.host.is_none(), |c| !is_ended(c))
        })
    }

    pub(crate) fn push_mic(&self, pcm: &[i16]) {
        if let Some(s) = self.live() {
            s.push_mic(pcm);
        }
    }

    pub(crate) fn pull_speaker(&self) -> Vec<i16> {
        match self.live() {
            Some(s) => s.pull_speaker(),
            None => vec![0; audio::FRAME],
        }
    }

    fn known_start(&self, call_id: &str) -> Option<CallStart> {
        self.known.lock().starts.get(call_id).cloned()
    }

    /// A session is live (in a call, or hosting one).
    pub(crate) fn busy(&self) -> bool {
        self.live().is_some()
    }

    /// This device's user is in a group call (joining, joined or reconnecting): what the apps'
    /// audio threads follow. Hosting listen-only does not count.
    pub(crate) fn own_in_call(&self) -> bool {
        self.live().is_some_and(|s| s.st.lock().client.as_ref().is_some_and(|c| !is_ended(c)))
    }

    /// The conditions the platform reported (`gcall_set_cond`, or an explicit `gcall_configure`),
    /// None until then. Call-host status publishing waits for them: a device that never said
    /// what it is must not advertise as a desktop on mains (GA-A1).
    pub(crate) fn cond_known(&self) -> Option<Cond> {
        *self.cond.lock()
    }

    /// The current local hosting conditions.
    pub(crate) fn cond(&self) -> Cond {
        let set = *self.cond.lock();
        set.or_else(|| self.cfg.lock().as_ref().map(|c| c.cond)).unwrap_or_else(Cond::desktop)
    }

    /// Configures unless something (a test, the app) already did. True if this configured.
    pub(crate) fn configure_if_unset(&self, mut cfg: GcallConfig) -> bool {
        let mut slot = self.cfg.lock();
        if slot.is_some() {
            return false;
        }
        if let Some(c) = *self.cond.lock() {
            cfg.cond = c;
        }
        *slot = Some(Arc::new(cfg));
        true
    }
}

// ---- session ------------------------------------------------------------------------------

struct Peer {
    conn: Connection,
    tx: mpsc::UnboundedSender<Out>,
}

impl Peer {
    fn close(self, code: u32) {
        if self.tx.send(Out::Close(code)).is_err() {
            self.conn.close(code.into(), b"");
        }
    }
}

enum Out {
    Frame(Vec<u8>),
    Close(u32),
}

struct HostSide {
    eng: HostEngine,
    conns: BTreeMap<ConnId, Peer>,
    /// A takeover engine waiting for its first valid Join; dropped if that Join is refused.
    taking: bool,
}

enum Link {
    None,
    Local,
    Remote { seq: u64, peer: Option<Peer>, queued: Vec<Vec<u8>> },
}

struct St {
    host: Option<HostSide>,
    client: Option<ClientEngine>,
    link: Link,
    /// Host addresses given with `gcall_join`, before the directory.
    addrs: BTreeMap<HostRef, EndpointAddr>,
    mic: Vec<i16>,
    mic_muted: bool,
    last_up: u64,
    events: Vec<GcallEvent>,
    next_conn: u64,
    next_seq: u64,
    /// The winner named by a `Superseded`, joined when the client ends.
    follow: Option<HostRef>,
    /// Current local conditions (§2.4); a takeover starts with these.
    cond: Cond,
}

enum Work {
    H(Vec<HostAction>),
    C(Vec<ClientAction>),
}

pub(crate) struct Session {
    call_id: String,
    id: Identity,
    cfg: Arc<GcallConfig>,
    /// Calls run on the group endpoint (U2/U7), looked up at each dial: it may rotate (U12).
    inner: std::sync::Weak<Inner>,
    handle: tokio::runtime::Handle,
    /// Session clock: Unix ms at creation plus monotonic elapsed time.
    t0: Instant,
    t0_unix: u64,
    known: Known,
    st: Mutex<St>,
}

fn is_ended(c: &ClientEngine) -> bool {
    matches!(c.state(), ClientState::Ended(_))
}

impl Session {
    fn new(call_id: &str, id: Identity, cfg: Arc<GcallConfig>, inner: std::sync::Weak<Inner>, handle: tokio::runtime::Handle, known: Known) -> Arc<Self> {
        let cond = cfg.cond;
        Arc::new(Session {
            call_id: call_id.into(),
            id,
            cfg,
            inner,
            handle,
            t0: Instant::now(),
            t0_unix: crate::node::now_ms(),
            known,
            st: Mutex::new(St {
                host: None,
                client: None,
                link: Link::None,
                addrs: BTreeMap::new(),
                mic: vec![],
                mic_muted: false,
                last_up: 0,
                events: vec![],
                next_conn: 1,
                next_seq: 1,
                follow: None,
                cond,
            }),
        })
    }

    fn now(&self) -> u64 {
        self.t0_unix + self.t0.elapsed().as_millis() as u64
    }

    fn auth(&self) -> &dyn CallAuthority {
        &*self.cfg.dir
    }

    fn finished(&self) -> bool {
        let st = self.st.lock();
        st.host.is_none() && st.client.as_ref().is_none_or(is_ended)
    }

    /// Runs `f` under the lock, executes every engine action it queued (and their
    /// consequences), then delivers the events outside the lock.
    fn with<R>(self: &Arc<Self>, f: impl FnOnce(&Arc<Self>, &mut St, &mut VecDeque<Work>) -> R) -> R {
        let (r, events) = {
            let mut st = self.st.lock();
            let mut q = VecDeque::new();
            let r = f(self, &mut st, &mut q);
            while let Some(work) = q.pop_front() {
                match work {
                    Work::H(acts) => {
                        for a in acts {
                            self.host_act(&mut st, a, &mut q);
                        }
                    }
                    Work::C(acts) => {
                        for a in acts {
                            self.client_act(&mut st, a, &mut q);
                        }
                    }
                }
            }
            (r, std::mem::take(&mut st.events))
        };
        for e in events {
            self.cfg.events.on_gcall_event(e);
        }
        r
    }

    fn spawn_ticker(self: &Arc<Self>) {
        let s = self.clone();
        self.handle.spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_millis(FRAME_MS));
            iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                iv.tick().await;
                s.with(|s, st, q| s.tick(st, q));
                if s.finished() {
                    s.with(|s, st, _| s.drop_link(st));
                    break;
                }
            }
        });
    }

    fn tick(self: &Arc<Self>, st: &mut St, q: &mut VecDeque<Work>) {
        let now = self.now();
        if let Some(h) = st.host.as_mut() {
            q.push_back(Work::H(h.eng.tick(now, self.auth())));
        }
        if let Some(c) = st.client.as_mut() {
            q.push_back(Work::C(c.tick(now, self.auth())));
            // An app that is not pushing audio would be dropped after MEMBER_DROP_MS: keep
            // the gated sender's level-only datagram going.
            let joined = c.state() == ClientState::Joined;
            if joined && matches!(st.link, Link::Remote { .. }) && now >= st.last_up + LEVEL_ONLY_MS {
                q.push_back(Work::C(vec![ClientAction::Datagram(w::up_level(SILENT_LEVEL).to_vec())]));
            }
        }
    }

    fn push_mic(self: &Arc<Self>, pcm: &[i16]) {
        self.with(|_, st, q| {
            if st.client.as_ref().is_none_or(is_ended) {
                st.mic.clear();
                return;
            }
            st.mic.extend_from_slice(pcm);
            let mut frame = [0i16; audio::FRAME];
            while st.mic.len() >= audio::FRAME {
                if st.mic_muted {
                    frame = [0; audio::FRAME];
                } else {
                    frame.copy_from_slice(&st.mic[..audio::FRAME]);
                }
                st.mic.drain(..audio::FRAME);
                let acts = st.client.as_mut().expect("checked above").on_mic(&frame);
                q.push_back(Work::C(acts));
            }
        });
    }

    fn pull_speaker(&self) -> Vec<i16> {
        let mut out = [0i16; audio::FRAME];
        let now = self.now();
        if let Some(c) = self.st.lock().client.as_mut() {
            c.mix(&mut out, now);
        }
        out.to_vec()
    }

    fn status(&self) -> GcallStatus {
        let st = self.st.lock();
        let call_id = match (&st.client, &st.host) {
            (Some(c), _) => c.call_start().call_id.clone(),
            (None, Some(h)) => h.eng.call_start().call_id.clone(),
            _ => self.call_id.clone(),
        };
        GcallStatus {
            call_id,
            client: st.client.as_ref().map(|c| ClientStatus {
                state: c.state(),
                host: c.host().clone(),
                host_gen: c.host_gen(),
                my_slot: c.my_slot(),
                roster: c.roster().clone(),
                active: c.active().1.clone(),
                heard: c.roster().keys().map(|s| (*s, c.heard(*s))).collect(),
            }),
            hosting: st.host.as_ref().map(|h| HostingStatus {
                host_gen: h.eng.call_start().host_gen,
                k: h.eng.k(),
                epoch: h.eng.epoch(),
                roster: h.eng.roster(),
                forwarded: h.eng.active().1.clone(),
            }),
        }
    }

    /// Closes the client's current link (a remote one with a goodbye after its queued frames).
    fn drop_link(&self, st: &mut St) {
        if let Link::Remote { peer: Some(p), .. } = std::mem::replace(&mut st.link, Link::None) {
            p.close(CLOSE_BYE);
        }
    }

    /// §5: become `prev`'s next-generation host, if this device is one of its backups.
    fn take_over(&self, st: &mut St, prev: &CallStart) -> Result<(), Refusal> {
        if st.host.is_some() {
            return Err(Refusal::Refused);
        }
        let me = self.cfg.host_ref_for(&prev.gid).ok_or(Refusal::Refused)?;
        let own = st.client.as_ref().is_some_and(|c| !is_ended(c) && c.call_start().call_id == prev.call_id);
        let eng = HostEngine::takeover(self.id.clone(), prev, me, st.cond, own, self.now())?;
        let s = eng.call_start();
        st.events.push(GcallEvent::HostingStarted { call_id: s.call_id.clone(), host_gen: s.host_gen, start: eng.start_blob().clone() });
        tracing::info!("gcall: taking over {} at gen {}", s.call_id, s.host_gen);
        st.host = Some(HostSide { eng, conns: BTreeMap::new(), taking: true });
        Ok(())
    }

    /// §3.1 glare / §5 split: if the call hosted here lost to a call on the board, end it with
    /// `End{superseded, next_host: winner}`.
    fn sweep(self: &Arc<Self>) {
        self.with(|s, st, q| {
            let Some(h) = st.host.as_mut() else { return };
            let mine = h.eng.call_start().clone();
            let winner = {
                let mut k = s.known.lock();
                let b = k.boards.entry(mine.gid.clone()).or_insert_with(|| CallBoard::new(&mine.gid, s.cfg.host_ref_for(&mine.gid)));
                let _ = b.accept_start(h.eng.start_blob(), s.auth());
                b.superseded(&mine)
            };
            if let Some(w) = winner {
                tracing::info!("gcall: {} gen {} superseded", mine.call_id, mine.host_gen);
                q.push_back(Work::H(h.eng.supersede(w)));
            }
        });
    }

    fn host_act(self: &Arc<Self>, st: &mut St, a: HostAction, q: &mut VecDeque<Work>) {
        let now = self.now();
        let local_up = matches!(st.link, Link::Local);
        match a {
            HostAction::Control { conn: ConnId::LOCAL, frame } => {
                let (Some(c), true) = (st.client.as_mut(), local_up) else { return };
                let Ok(Some((msg, _))) = w::decode_control(&frame) else { return };
                if let Ok(acts) = c.on_control(msg, now, &*self.cfg.dir) {
                    q.push_back(Work::C(acts));
                }
            }
            HostAction::Control { conn, frame } => {
                if let Some(p) = st.host.as_ref().and_then(|h| h.conns.get(&conn)) {
                    let _ = p.tx.send(Out::Frame(frame));
                }
            }
            HostAction::Datagram { conn: ConnId::LOCAL, bytes } => {
                if let (Some(c), true) = (st.client.as_mut(), local_up) {
                    c.on_datagram(&bytes, now);
                }
            }
            HostAction::Datagram { conn, bytes } => {
                if let Some(p) = st.host.as_ref().and_then(|h| h.conns.get(&conn)) {
                    let _ = p.conn.send_datagram(Bytes::from(bytes));
                }
            }
            HostAction::Close { conn: ConnId::LOCAL, reason } => {
                if local_up {
                    st.link = Link::None;
                    if let Some(c) = st.client.as_mut() {
                        q.push_back(Work::C(c.on_closed(reason == CloseReason::AnsweredElsewhere, now, &*self.cfg.dir)));
                    }
                }
            }
            HostAction::Close { conn, reason } => {
                if let Some(p) = st.host.as_mut().and_then(|h| h.conns.remove(&conn)) {
                    p.close(close_code(reason));
                }
            }
            HostAction::Notice(n) => st.events.push(GcallEvent::Notice(n)),
            HostAction::Ended { reason, next_host, end } => {
                if let Some(h) = st.host.take() {
                    for (_, p) in h.conns {
                        p.close(CLOSE_ENDED);
                    }
                    let s = h.eng.call_start();
                    tracing::info!("gcall: stopped hosting {} gen {}: {reason:?}", s.call_id, s.host_gen);
                    if let Some(e) = end.as_ref().filter(|_| reason == EndReason::Ended)
                        && let Some(b) = self.known.lock().boards.get_mut(&s.gid)
                    {
                        let _ = b.accept_end(e, self.auth());
                    }
                    st.events.push(GcallEvent::HostingEnded { call_id: s.call_id.clone(), reason, next_host, end });
                }
            }
        }
    }

    fn client_act(self: &Arc<Self>, st: &mut St, a: ClientAction, q: &mut VecDeque<Work>) {
        let now = self.now();
        let auth = &*self.cfg.dir;
        let Some(c) = st.client.as_mut() else { return };
        match a {
            ClientAction::Dial { host } => {
                let (hgen, prev) = (c.host_gen(), c.call_start().clone());
                if hgen > prev.host_gen {
                    st.events.push(GcallEvent::Reconnecting { call_id: prev.call_id.clone() });
                }
                self.drop_link(st);
                if self.cfg.host_ref_for(&prev.gid).as_ref() == Some(&host) {
                    // Our own host engine, through ConnId::LOCAL; as a backup, take over first.
                    let have = st.host.as_ref().is_some_and(|h| {
                        let s = h.eng.call_start();
                        s.call_id == prev.call_id && s.host_gen == hgen
                    });
                    let ok = have || (hgen == prev.host_gen + 1 && self.take_over(st, &prev).is_ok());
                    let c = st.client.as_mut().expect("present");
                    c.set_local(ok);
                    if ok {
                        st.link = Link::Local;
                    } else {
                        q.push_back(Work::C(c.dial_failed(now, auth)));
                    }
                    return;
                }
                let addr = st.addrs.get(&host).cloned().or_else(|| self.cfg.dir.host_addr(&host));
                let c = st.client.as_mut().expect("present");
                c.set_local(false);
                match addr {
                    None => q.push_back(Work::C(c.dial_failed(now, auth))),
                    Some(addr) => {
                        let seq = st.next_seq;
                        st.next_seq += 1;
                        st.link = Link::Remote { seq, peer: None, queued: vec![] };
                        let s = self.clone();
                        self.handle.spawn(async move { s.dial(seq, addr).await });
                    }
                }
            }
            ClientAction::Control(frame) => match &mut st.link {
                Link::Local => {
                    let Ok(Some((msg, _))) = w::decode_control(&frame) else { return };
                    let Some(h) = st.host.as_mut() else { return };
                    match h.eng.on_control(ConnId::LOCAL, msg, now, auth) {
                        Ok(acts) => {
                            h.taking = false;
                            q.push_back(Work::H(acts));
                        }
                        Err(e) => {
                            tracing::debug!("gcall: own Join/Leave refused: {e:?}");
                            if h.taking {
                                st.host = None;
                            }
                        }
                    }
                }
                Link::Remote { peer: Some(p), .. } => {
                    let _ = p.tx.send(Out::Frame(frame));
                }
                Link::Remote { peer: None, queued, .. } => queued.push(frame),
                Link::None => {}
            },
            ClientAction::Datagram(bytes) => {
                st.last_up = now;
                match &st.link {
                    Link::Local => {
                        if let Some(h) = st.host.as_mut()
                            && let Ok(acts) = h.eng.on_datagram(ConnId::LOCAL, &bytes, now)
                        {
                            q.push_back(Work::H(acts));
                        }
                    }
                    Link::Remote { peer: Some(p), .. } => {
                        let _ = p.conn.send_datagram(Bytes::from(bytes));
                    }
                    _ => {}
                }
            }
            ClientAction::Joined { slot } => {
                let s = c.call_start();
                st.events.push(GcallEvent::Joined { call_id: s.call_id.clone(), host_gen: c.host_gen(), slot, host: c.host().clone() });
            }
            ClientAction::RosterChanged => {
                let s = c.call_start();
                st.events.push(GcallEvent::RosterChanged { call_id: s.call_id.clone(), host_gen: c.host_gen(), roster: c.roster().clone() });
            }
            ClientAction::Superseded { by } => {
                st.events.push(GcallEvent::Superseded { call_id: c.call_start().call_id.clone(), by: by.clone() });
                st.follow = Some(by);
            }
            ClientAction::Ended(why) => {
                let (call_id, gid) = (c.call_start().call_id.clone(), c.call_start().gid.clone());
                st.events.push(GcallEvent::Ended { call_id, why });
                self.drop_link(st);
                st.mic.clear();
                // §3.1 glare / §5 split: join the winner if its CallStart is known.
                let by = st.follow.take().filter(|_| why == ClientEnd::Superseded);
                let next = by.and_then(|by| {
                    let mut k = self.known.lock();
                    let found = k.starts.values().find(|k| k.host_ref == by && k.gid == gid).cloned();
                    if found.is_none() {
                        // Its CallStart is still on its way through the relay.
                        k.follow = Some((by, gid.clone(), Instant::now()));
                    }
                    found
                });
                if let Some(start) = next
                    && let Ok((c, acts)) = ClientEngine::new(self.id.clone(), start, self.cfg.note_now(), now, auth)
                {
                    st.client = Some(c);
                    q.push_back(Work::C(acts));
                }
            }
        }
    }

    // ---- remote link (participant side) --------------------------------------------------

    fn current(st: &St, seq: u64) -> bool {
        matches!(st.link, Link::Remote { seq: s, .. } if s == seq)
    }

    async fn dial(self: Arc<Self>, seq: u64, addr: EndpointAddr) {
        let connected = async {
            let ep = self.inner.upgrade().and_then(|i| i.group_endpoint()).ok_or(Error::NotStarted)?;
            let conn = ep.connect(addr, GCALL_ALPN).await.map_err(Error::net)?;
            let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
            Ok::<_, Error>((conn, send, recv))
        };
        let (conn, send, mut recv) = match tokio::time::timeout(DIAL_TIMEOUT, connected).await {
            Ok(Ok(x)) => x,
            other => {
                tracing::debug!("gcall: dial failed: {:?}", other.map(|r| r.map(|_| ())));
                self.with(|s, st, q| {
                    if Self::current(st, seq) {
                        st.link = Link::None;
                        if let Some(c) = st.client.as_mut() {
                            q.push_back(Work::C(c.dial_failed(s.now(), &*s.cfg.dir)));
                        }
                    }
                });
                return;
            }
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let installed = self.with(|_, st, _| {
            let Link::Remote { seq: s, peer, queued } = &mut st.link else { return false };
            if *s != seq {
                return false;
            }
            for f in queued.drain(..) {
                let _ = tx.send(Out::Frame(f));
            }
            *peer = Some(Peer { conn: conn.clone(), tx: tx.clone() });
            true
        });
        if !installed {
            conn.close(CLOSE_BYE.into(), b"");
            return;
        }
        self.handle.spawn(writer(conn.clone(), send, rx));
        drop(tx);
        let mut buf = vec![];
        loop {
            tokio::select! {
                f = read_frame(&mut recv, &mut buf) => match f {
                    Some(msg) => self.with(|s, st, q| {
                        if Self::current(st, seq)
                            && let Some(c) = st.client.as_mut()
                            && let Ok(acts) = c.on_control(msg, s.now(), &*s.cfg.dir)
                        {
                            q.push_back(Work::C(acts));
                        }
                    }),
                    None => break,
                },
                d = conn.read_datagram() => match d {
                    Ok(d) => {
                        let now = self.now();
                        let mut st = self.st.lock();
                        if Self::current(&st, seq)
                            && let Some(c) = st.client.as_mut()
                        {
                            c.on_datagram(&d, now);
                        }
                    }
                    Err(_) => break,
                },
            }
        }
        let why = tokio::time::timeout(CLOSE_WAIT, conn.closed()).await;
        let elsewhere = matches!(&why, Ok(ConnectionError::ApplicationClosed(c)) if c.error_code == CLOSE_ANSWERED_ELSEWHERE.into());
        tracing::debug!("gcall: host link closed: {why:?}");
        self.with(|s, st, q| {
            if Self::current(st, seq) {
                st.link = Link::None;
                if let Some(c) = st.client.as_mut() {
                    q.push_back(Work::C(c.on_closed(elsewhere, s.now(), &*s.cfg.dir)));
                }
            }
        });
        conn.close(CLOSE_BYE.into(), b"");
    }

    // ---- host side -------------------------------------------------------------------------

    /// Serves one accepted participant connection whose first frame was `first`.
    async fn serve(self: Arc<Self>, conn: Connection, send: SendStream, mut recv: RecvStream, mut buf: Vec<u8>, first: GroupCallMsg) {
        let remote: [u8; 32] = *conn.remote_id().as_bytes();
        let (tx, rx) = mpsc::unbounded_channel();
        let cid = self.with(|_, st, _| {
            let h = st.host.as_mut()?;
            let cid = ConnId(st.next_conn);
            st.next_conn += 1;
            h.eng.connected(cid, remote);
            h.conns.insert(cid, Peer { conn: conn.clone(), tx: tx.clone() });
            Some(cid)
        });
        let Some(cid) = cid else {
            conn.close(CLOSE_REFUSED.into(), b"");
            return;
        };
        self.handle.spawn(writer(conn.clone(), send, rx));
        drop(tx);
        self.host_control(cid, first);
        loop {
            tokio::select! {
                f = read_frame(&mut recv, &mut buf) => match f {
                    Some(msg) => self.host_control(cid, msg),
                    None => break,
                },
                d = conn.read_datagram() => match d {
                    Ok(d) => self.with(|s, st, q| {
                        if let Some(h) = st.host.as_mut()
                            && h.conns.contains_key(&cid)
                            && let Ok(acts) = h.eng.on_datagram(cid, &d, s.now())
                        {
                            q.push_back(Work::H(acts));
                        }
                    }),
                    Err(_) => break,
                },
            }
        }
        self.with(|_, st, q| {
            if let Some(h) = st.host.as_mut() {
                if let Some(p) = h.conns.remove(&cid) {
                    p.close(CLOSE_BYE);
                }
                q.push_back(Work::H(h.eng.disconnected(cid)));
            }
        });
    }

    fn host_control(self: &Arc<Self>, cid: ConnId, msg: GroupCallMsg) {
        self.with(|s, st, q| {
            let Some(h) = st.host.as_mut() else { return };
            if !h.conns.contains_key(&cid) {
                return;
            }
            match h.eng.on_control(cid, msg, s.now(), &*s.cfg.dir) {
                Ok(acts) => {
                    h.taking = false;
                    q.push_back(Work::H(acts));
                }
                Err(e) => {
                    tracing::debug!("gcall: control refused: {e:?}");
                    if h.taking {
                        for (_, p) in std::mem::take(&mut h.conns) {
                            p.close(CLOSE_REFUSED);
                        }
                        st.host = None;
                        return;
                    }
                    // A connection without a participant has nothing more to say: let its
                    // client move on to the next candidate now instead of at its timeout.
                    let joined = h.eng.roster().values().any(|d| h.eng.conn_of(d) == Some(cid));
                    if !joined && let Some(p) = h.conns.remove(&cid) {
                        p.close(CLOSE_REFUSED);
                    }
                }
            }
        });
    }
}

/// Writes control frames in order; on `Close` finishes the stream, waits (bounded) until the
/// peer has read it, then closes the connection with the code.
async fn writer(conn: Connection, mut send: SendStream, mut rx: mpsc::UnboundedReceiver<Out>) {
    let mut code = CLOSE_BYE;
    while let Some(out) = rx.recv().await {
        match out {
            Out::Frame(f) => {
                if send.write_all(&f).await.is_err() {
                    return;
                }
            }
            Out::Close(c) => {
                code = c;
                break;
            }
        }
    }
    let _ = send.finish();
    let _ = tokio::time::timeout(FLUSH_TIMEOUT, send.stopped()).await;
    conn.close(code.into(), b"");
}

/// The next control frame; `None` at end of stream, on a read error or a malformed frame.
async fn read_frame(recv: &mut RecvStream, buf: &mut Vec<u8>) -> Option<GroupCallMsg> {
    loop {
        match w::decode_control(buf) {
            Ok(Some((m, n))) => {
                buf.drain(..n);
                return Some(m);
            }
            Ok(None) => {}
            Err(_) => return None,
        }
        let mut tmp = [0u8; 2048];
        match recv.read(&mut tmp).await {
            Ok(Some(n)) => buf.extend_from_slice(&tmp[..n]),
            _ => return None,
        }
    }
}

// ---- node hooks -------------------------------------------------------------------------

impl Inner {
    /// `Node::stop` / `Node::lock` / an account switch, before the endpoints close: leave the call and, when hosting, end it with
    /// `End{host_leaving, next_host}`; waits (bounded) until those frames are out.
    pub(crate) async fn gcall_shutdown(&self) {
        let svc = &self.gcall;
        if let Some(s) = svc.live() {
            let mut conns = vec![];
            {
                let st = s.st.lock();
                if let Some(h) = &st.host {
                    conns.extend(h.conns.values().map(|p| p.conn.clone()));
                }
                if let Link::Remote { peer: Some(p), .. } = &st.link {
                    conns.push(p.conn.clone());
                }
            }
            s.with(|s, st, q| {
                if let Some(c) = st.client.as_mut() {
                    q.push_back(Work::C(c.leave(s.now())));
                }
            });
            s.with(|s, st, q| {
                if let Some(h) = st.host.as_mut() {
                    let next = h.eng.next_backup(s.auth());
                    q.push_back(Work::H(h.eng.end(EndReason::HostLeaving, next)));
                }
            });
            let deadline = tokio::time::Instant::now() + FLUSH_TIMEOUT + Duration::from_millis(200);
            for c in conns {
                let _ = tokio::time::timeout_at(deadline, c.closed()).await;
            }
        }
        *svc.cur.lock() = None;
        *svc.known.lock() = Calls::default();
    }

    /// A `tinline/gcall/1` connection (called from the accept loop in `node.rs`). The first
    /// control frame must be a Join for the call this device hosts, or one it may take over.
    pub(crate) async fn handle_gcall_incoming(self: Arc<Self>, accepting: Accepting) -> Result<(), Error> {
        let conn = tokio::time::timeout(DIAL_TIMEOUT, accepting).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?;
        let first = async {
            let (send, mut recv) = conn.accept_bi().await.map_err(Error::net)?;
            let mut buf = vec![];
            let msg = read_frame(&mut recv, &mut buf).await.ok_or_else(|| Error::Protocol("no frame".into()))?;
            Ok::<_, Error>((send, recv, buf, msg))
        };
        let (send, recv, buf, msg) = match tokio::time::timeout(DIAL_TIMEOUT, first).await {
            Ok(Ok(x)) => x,
            Ok(Err(e)) => {
                conn.close(CLOSE_REFUSED.into(), b"");
                return Err(e);
            }
            Err(_) => {
                conn.close(CLOSE_REFUSED.into(), b"");
                return Err(Error::Timeout);
            }
        };
        let join = match &msg {
            GroupCallMsg::Join { join, .. } => w::open_join(join).ok(),
            _ => None,
        };
        let session = match join {
            Some(j) => self.gcall_session_for(&j),
            None => None,
        };
        let Some(s) = session else {
            conn.close(CLOSE_REFUSED.into(), b"");
            return Err(Error::Rejected("no such call here".into()));
        };
        s.serve(conn, send, recv, buf, msg).await;
        Ok(())
    }

    /// The session that serves `j`: the call hosted here, or a takeover (§5) when this device
    /// is a backup of the previous generation.
    fn gcall_session_for(self: &Arc<Self>, j: &w::Join) -> Option<Arc<Session>> {
        let svc = &self.gcall;
        if let Some(s) = svc.live() {
            let known = svc.known_start(&j.call_id);
            let ok = s.with(|s, st, _| {
                if let Some(h) = &st.host {
                    return h.eng.call_start().call_id == j.call_id;
                }
                let prev = st.client.as_ref().map(|c| c.call_start().clone()).filter(|p| p.call_id == j.call_id).or(known);
                prev.is_some_and(|p| p.host_gen + 1 == j.host_gen && s.take_over(st, &p).is_ok())
            });
            return ok.then_some(s);
        }
        // Listen-only takeover by a backup that was not in the call.
        let prev = svc.known_start(&j.call_id).filter(|p| p.host_gen + 1 == j.host_gen)?;
        let cfg = svc.cfg.lock().clone()?;
        let me = self.me().ok()?;
        let s = Session::new(&j.call_id, me.id.clone(), cfg, Arc::downgrade(self), self.handle.clone(), svc.known.clone());
        {
            let mut cur = svc.cur.lock();
            if cur.as_ref().is_some_and(|c| !c.finished()) {
                return None;
            }
            *cur = Some(s.clone());
        }
        if !s.with(|s, st, _| s.take_over(st, &prev).is_ok()) {
            return None;
        }
        s.spawn_ticker();
        Some(s)
    }
}

/// The group-call service on `Inner` (the group layer, `group::calls`, drives it from core
/// threads); `Node` wraps it below.
impl Inner {
    /// Sets the directory, event sink, own host ref and conditions. Applies to sessions
    /// started afterwards.
    pub(crate) fn gcall_configure(&self, cfg: GcallConfig) {
        // Explicit conditions count as reported (`Service::cond_known`).
        *self.gcall.cond.lock() = Some(cfg.cond);
        *self.gcall.cfg.lock() = Some(Arc::new(cfg));
    }

    pub(crate) fn gcall_cfg(&self) -> Result<Arc<GcallConfig>, Error> {
        self.gcall.cfg.lock().clone().ok_or_else(|| Error::Protocol("group calls not configured".into()))
    }

    /// A new session, unless a group call or a 1:1 call is going on.
    fn gcall_new_session(self: &Arc<Self>, call_id: &str) -> Result<Arc<Session>, Error> {
        let cfg = self.gcall_cfg()?;
        let me = self.me()?;
        if self.group_endpoint().is_none() {
            return Err(Error::NotStarted);
        }
        let s = Session::new(call_id, me.id.clone(), cfg, Arc::downgrade(self), self.handle.clone(), self.gcall.known.clone());
        // Under the 1:1 slot's lock, so that no 1:1 call begins meanwhile (it checks `in_call`).
        self.with_no_call(|| {
            let mut cur = self.gcall.cur.lock();
            if cur.as_ref().is_some_and(|c| !c.finished()) {
                return Err(Error::Busy);
            }
            *cur = Some(s.clone());
            Ok(())
        })?;
        Ok(s)
    }

    /// New local hosting conditions (§2.4: battery, charging, thermal, metered), from the
    /// platform. A call hosted here degrades as the policy says (K = 1 and no new Joins, or
    /// `End{host_leaving, next_host}`); later takeovers start with them.
    pub(crate) fn gcall_set_cond(self: &Arc<Self>, cond: Cond) {
        *self.gcall.cond.lock() = Some(cond);
        if let Some(c) = self.gcall.cfg.lock().as_mut() {
            let mut n = (**c).clone();
            n.cond = cond;
            *c = Arc::new(n);
        }
        if let Some(s) = self.gcall.live() {
            s.with(|s, st, q| {
                st.cond = cond;
                if let Some(h) = st.host.as_mut() {
                    q.push_back(Work::H(h.eng.set_cond(cond, s.auth())));
                }
            });
        }
    }

    /// A CallStart relayed through the chat hosts: remembered so that this device can take
    /// over as a backup, or follow a `Superseded`. Verified (signature, permit, starter).
    /// When it beats the call hosted here (glare, split), that call ends `superseded`.
    pub(crate) fn gcall_observe_start(self: &Arc<Self>, start: &SignedBlob) -> Result<(), Error> {
        let cfg = self.gcall_cfg()?;
        let s = w::open_start(start).map_err(|e| Error::Protocol(e.to_string()))?;
        if !cfg.dir.host_permit_ok(&s.gid, &s.host_ref) || !cfg.dir.is_member(&s.gid, &s.starter_did) {
            return Err(Error::Rejected("call host not permitted".into()));
        }
        let follow = {
            let mut k = self.gcall.known.lock();
            let b = k.boards.entry(s.gid.clone()).or_insert_with(|| CallBoard::new(&s.gid, cfg.host_ref_for(&s.gid)));
            let _ = b.accept_start(start, &*cfg.dir);
            let follow = k.follow.as_ref().is_some_and(|(by, gid, t)| *by == s.host_ref && *gid == s.gid && t.elapsed() < FOLLOW_WINDOW);
            if follow {
                k.follow = None;
            }
            if k.starts.get(&s.call_id).is_none_or(|have| have.host_gen < s.host_gen) {
                k.starts.insert(s.call_id.clone(), s);
            }
            follow
        };
        if let Some(cur) = self.gcall.live() {
            cur.sweep();
        }
        if follow && let Err(e) = self.gcall_join(start, None) {
            tracing::debug!("gcall: following the glare winner failed: {e}");
        }
        Ok(())
    }

    /// A relayed `End{ended}`: the call leaves the group's board (it no longer wins a glare).
    pub(crate) fn gcall_observe_end(&self, end: &SignedBlob) -> Result<(), Error> {
        let cfg = self.gcall_cfg()?;
        let e = w::open_end(end).map_err(|e| Error::Protocol(e.to_string()))?;
        let mut k = self.gcall.known.lock();
        let b = k.boards.entry(e.gid.clone()).or_insert_with(|| CallBoard::new(&e.gid, cfg.host_ref_for(&e.gid)));
        b.accept_end(end, &*cfg.dir).map_err(|r| Error::Rejected(format!("{r:?}")))
    }

    /// Host a call. `cfg.host_ref` must be this device's configured host ref. With
    /// `own_user_in` this device's user joins at once through `ConnId::LOCAL`. Returns the
    /// signed CallStart to announce (also delivered as `HostingStarted`).
    pub(crate) fn gcall_host(self: &Arc<Self>, cfg: HostConfig) -> Result<SignedBlob, Error> {
        let gc = self.gcall_cfg()?;
        if gc.host_ref_for(&cfg.gid).as_ref() != Some(&cfg.host_ref) {
            return Err(Error::Protocol("host_ref is not this device's".into()));
        }
        let own = cfg.own_user_in;
        let s = self.gcall_new_session(&cfg.call_id)?;
        let res = s.with(|s, st, q| {
            let now = s.now();
            let eng = HostEngine::new(s.id.clone(), cfg, now).map_err(|e| Error::Rejected(format!("{e:?}")))?;
            let blob = eng.start_blob().clone();
            let start = eng.call_start().clone();
            st.events.push(GcallEvent::HostingStarted { call_id: start.call_id.clone(), host_gen: start.host_gen, start: blob.clone() });
            st.host = Some(HostSide { eng, conns: BTreeMap::new(), taking: false });
            if own {
                let (c, acts) = ClientEngine::new(s.id.clone(), start, s.cfg.note_now(), now, &*s.cfg.dir).map_err(|e| Error::Rejected(format!("{e:?}")))?;
                st.client = Some(c);
                q.push_back(Work::C(acts));
            }
            Ok(blob)
        });
        match res {
            Ok(_) => s.spawn_ticker(),
            Err(_) => *self.gcall.cur.lock() = None,
        }
        res
    }

    /// Join the call announced by `start`. `host_addr` is where its host can be reached (else
    /// the directory is asked). Progress arrives as `GcallEvent`s.
    pub(crate) fn gcall_join(self: &Arc<Self>, start: &SignedBlob, host_addr: Option<EndpointAddr>) -> Result<(), Error> {
        let gc = self.gcall_cfg()?;
        let s = w::open_start(start).map_err(|e| Error::Protocol(e.to_string()))?;
        self.gcall_observe_start(start)?;
        // Joining the call this device already hosts listen-only: add the user to it.
        if let Some(cur) = self.gcall.live() {
            self.not_in_call()?;
            return cur.with(|cur, st, q| {
                let hosts_it = st.host.as_ref().is_some_and(|h| h.eng.call_start().call_id == s.call_id);
                if !hosts_it || st.client.as_ref().is_some_and(|c| !is_ended(c)) {
                    return Err(Error::Busy);
                }
                let start = st.host.as_ref().expect("checked").eng.call_start().clone();
                let (c, acts) = ClientEngine::new(cur.id.clone(), start, gc.note_now(), cur.now(), &*gc.dir).map_err(|e| Error::Rejected(format!("{e:?}")))?;
                st.client = Some(c);
                q.push_back(Work::C(acts));
                Ok(())
            });
        }
        let sess = self.gcall_new_session(&s.call_id)?;
        let host = s.host_ref.clone();
        let res = sess.with(|sess, st, q| {
            if let Some(a) = host_addr {
                st.addrs.insert(host, a);
            }
            let (c, acts) = ClientEngine::new(sess.id.clone(), s, gc.note_now(), sess.now(), &*gc.dir).map_err(|e| Error::Rejected(format!("{e:?}")))?;
            st.client = Some(c);
            q.push_back(Work::C(acts));
            Ok(())
        });
        match res {
            Ok(()) => sess.spawn_ticker(),
            Err(_) => *self.gcall.cur.lock() = None,
        }
        res
    }

    /// Leave the call (signed Leave). A desktop host keeps hosting listen-only; a phone host
    /// ends the call with `host_leaving` (§2.4, done by the engine).
    pub(crate) fn gcall_leave(self: &Arc<Self>) {
        if let Some(s) = self.gcall.live() {
            s.with(|s, st, q| {
                if let Some(c) = st.client.as_mut() {
                    q.push_back(Work::C(c.leave(s.now())));
                }
            });
        }
    }

    /// As host: end the call for everyone (`End{ended}`).
    pub(crate) fn gcall_end(self: &Arc<Self>) {
        if let Some(s) = self.gcall.live() {
            s.with(|_, st, q| {
                if let Some(h) = st.host.as_mut() {
                    q.push_back(Work::H(h.eng.end(EndReason::Ended, None)));
                }
            });
        }
    }

    /// Mute this device's microphone: silence is sent instead, so the sender gate closes and
    /// only level-only datagrams go out.
    pub(crate) fn gcall_set_mic_muted(&self, muted: bool) {
        if let Some(s) = self.gcall.live() {
            s.st.lock().mic_muted = muted;
        }
    }

    /// Local mute of one participant (§3.4, §6); the host is not told.
    pub(crate) fn gcall_mute_participant(&self, did: &str, muted: bool) {
        if let Some(s) = self.gcall.live()
            && let Some(c) = s.st.lock().client.as_mut()
        {
            c.set_muted(did, muted);
        }
    }

    /// Microphone PCM, 48 kHz mono, any length (what `Node::push_mic` does when no 1:1 call
    /// is active).
    pub(crate) fn gcall_push_mic(&self, pcm: &[i16]) {
        self.gcall.push_mic(pcm);
    }

    /// The next 20 ms of the group-call mix.
    pub(crate) fn gcall_pull_speaker(&self) -> Vec<i16> {
        self.gcall.pull_speaker()
    }

    pub(crate) fn gcall_status(&self) -> Option<GcallStatus> {
        self.gcall.cur.lock().clone().map(|s| s.status())
    }

    /// This node's group endpoint on loopback (direct, no relay). It must be up.
    pub(crate) fn gcall_loopback_addr(&self) -> Result<EndpointAddr, Error> {
        let ep = self.group_endpoint().ok_or(Error::NotStarted)?;
        let mut a = EndpointAddr::new(ep.id());
        for s in ep.bound_sockets() {
            let ip: std::net::IpAddr = if s.is_ipv4() { std::net::Ipv4Addr::LOCALHOST.into() } else { std::net::Ipv6Addr::LOCALHOST.into() };
            a = a.with_ip_addr(std::net::SocketAddr::new(ip, s.port()));
        }
        Ok(a)
    }
}

/// Group calls on a [`crate::Node`]. **Unstable** plain Rust; the app API is the group layer's
/// UniFFI calls (`group::calls`). Calls run on the group endpoint (U2/U7): the calls that start
/// a session bring it up first.
impl crate::Node {
    /// Sets the directory, event sink, own host ref and conditions. Applies to sessions
    /// started afterwards. Group chat configures itself (`GroupCallDir`) unless this was called.
    pub fn gcall_configure(&self, cfg: GcallConfig) {
        self.inner.gcall_configure(cfg)
    }

    /// The group endpoint, bound (otherwise it is bound only once the account has a group).
    fn gcall_ensure_endpoint(&self) -> Result<(), Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            inner.group_ensure(&core).await.map(|_| ())
        })?
    }

    /// New local hosting conditions (§2.4: battery, charging, thermal, metered), from the
    /// platform. A call hosted here degrades as the policy says (K = 1 and no new Joins, or
    /// `End{host_leaving, next_host}`); later takeovers and the advertised `CallHostStatus`
    /// use them.
    pub fn gcall_set_cond(&self, cond: Cond) {
        self.inner.gcall_set_cond(cond)
    }

    /// A CallStart relayed through the chat hosts (`Inner::gcall_observe_start`).
    pub fn gcall_observe_start(&self, start: &SignedBlob) -> Result<(), Error> {
        self.inner.gcall_observe_start(start)
    }

    /// A relayed `End{ended}`: the call leaves the group's board.
    pub fn gcall_observe_end(&self, end: &SignedBlob) -> Result<(), Error> {
        self.inner.gcall_observe_end(end)
    }

    /// Host a call (`Inner::gcall_host`).
    pub fn gcall_host(&self, cfg: HostConfig) -> Result<SignedBlob, Error> {
        self.gcall_ensure_endpoint()?;
        self.inner.gcall_host(cfg)
    }

    /// Join the call announced by `start` (`Inner::gcall_join`).
    pub fn gcall_join(&self, start: &SignedBlob, host_addr: Option<EndpointAddr>) -> Result<(), Error> {
        self.gcall_ensure_endpoint()?;
        self.inner.gcall_join(start, host_addr)
    }

    pub fn gcall_leave(&self) {
        self.inner.gcall_leave()
    }

    pub fn gcall_end(&self) {
        self.inner.gcall_end()
    }

    pub fn gcall_set_mic_muted(&self, muted: bool) {
        self.inner.gcall_set_mic_muted(muted)
    }

    pub fn gcall_mute_participant(&self, did: &str, muted: bool) {
        self.inner.gcall_mute_participant(did, muted)
    }

    pub fn gcall_push_mic(&self, pcm: &[i16]) {
        self.inner.gcall_push_mic(pcm)
    }

    pub fn gcall_pull_speaker(&self) -> Vec<i16> {
        self.inner.gcall_pull_speaker()
    }

    pub fn gcall_status(&self) -> Option<GcallStatus> {
        self.inner.gcall_status()
    }

    /// This node's group endpoint on loopback (direct, no relay), bound if needed: for tests
    /// and same-machine hosts.
    pub fn gcall_loopback_addr(&self) -> Result<EndpointAddr, Error> {
        self.gcall_ensure_endpoint()?;
        self.inner.gcall_loopback_addr()
    }
}
