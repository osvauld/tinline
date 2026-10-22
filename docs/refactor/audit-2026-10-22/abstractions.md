# Shared abstractions for the middle layer: peer sessions, doc replication, paged pulls

Worktree `/home/abe/p2p_chat/.claude/worktrees/rt-w8` (HEAD 88dfe367 plus uncommitted W8 edits, read as found). Read-only study. Paths are relative to `crates/`. Line numbers are from the files as read.

**Update from the lead applied:** no backward compatibility is needed. Wire formats, ALPN versions and stored data may change. Every proposal below states its format change and takes the cleaner design as the default. The `Node` UniFFI API is kept as the boundary; API options are in section 7.

## 0. Headline

| Candidate | Similarity | Verdict |
|---|---|---|
| 1. Peer sessions | **medium** (the plumbing is the same, the loops are not) | Worth it as a *kit* of shared pieces (framed stream, registry with the dialer tie-break, dialer with backoff, idle policy, admission), **not** as one `Handler` trait that owns every loop. Net about -150 to -250 lines, but one place for the race rules. |
| 2. Doc replication | **low as one loop, high in two sub-places** | One `Replicated` trait + loop does not fit chat/group. Two things do fit: (a) the **receipt exchange**, a near-verbatim copy in chat and group (high), and (b) sync-runtime's **`self_loop` doc routing**, which is the natural home of a doc registry (high, local to one crate). |
| 3. Paged pulls | **high between two copies, but only two copies exist** | `group_history.rs` and `group_receipts.rs` share about 70% of their request state machine. The other three named "copies" (chat receipts, group `member_receipts`) are **not** paged pulls; they are VV hello/push/ack exchanges (candidate 2a). Worth doing, small. |

Honest summary: the mechanism that is truly duplicated and cheap to share is **(2a) the receipt ledger exchange (about 280 lines in two copies)**, then **(3) the pull state machine (about 700 lines in two copies)**, then the **session kit (1)**. A full "rewrite the middle layer around shared abstractions" would shrink the five runtimes by perhaps 1,000 to 1,400 lines of about 33k (3 to 4%), not by a third. The larger simplification is in *consistency* (one set of timeouts, one tie-break, one hello shape) and in deleting accidental compatibility machinery (section 6).

---

## 1. Peer sessions

### 1.1 Table of the copies

"Chat" = `chat-runtime/src/chat/engine/session.rs` (+ `sync.rs` hello). "Group" = `group-runtime/src/engine/{dialing,accepting,sessions,lifecycle}.rs`. "Sync" = `sync-runtime/src/engine/{sessions,devices}.rs` (+ `link.rs`, a one-shot pairing protocol, not a session). "Calls/contacts" = `node-runtime/src/{call,outgoing,accept,run,lifecycle}.rs`.

| Aspect | Chat | Group | Sync (own device) | Calls / contact exchange |
|---|---|---|---|---|
| Endpoint | the private endpoint (node-runtime `serve_private`, `lifecycle.rs:179-257`) | its **own** group endpoint, own secret, own accept loop (`lifecycle.rs:224-250`, `accepting.rs:7-42`) | private endpoint | private endpoint |
| ALPN / version | `tinline/chat/7`, old ALPNs offered and refused: `version.rs:46`, `connect` `version.rs:106` | `GROUP_ALPN = tinline/group/5` (`wire.rs:...`), plain `ep.connect`, no old-ALPN list (`dialing.rs:168`) | `tinline/self/5` via `version::OWN` (`sessions.rs:31-36`) | `osvauld/p2p/1` via `version::CALL` (`wire.rs:~58`) |
| Dial timeout | `DIAL_TIMEOUT` 20 s (`node_runtime::wire`), per device in contact order (`session.rs:317-352`) | same 20 s (`dialing.rs:168`) | its own `DIAL_TIMEOUT` **10 s** (`engine.rs:~45`) | 20 s (`wire.rs:~20`, `call.rs:127`) |
| Address | `addr_for(device, relay)`, `list_before` (refresh contact device list first, `session.rs:334`) | `addr_for(host.endpoint, host.relay)` from a signed host address (`dialing.rs:166`) | `addr_for(device, relay)` from the registry (`sessions.rs:29`) | `addr_for`, `list_before` |
| Hello | stream opened by dialer; **`ChatMsg::Auth{attestation, grant, relay}`**, built by abusing `proto::call_hello(...)` with call_id `"chat"` and a `match ... _ => unreachable!()` (`session.rs:231-238`). Proves: device attestation = connection key, DID, a message token we issued (`chat_admit`), contact | `GroupMsg::Hello{auth: EndpointAuth, group, vv, days, invite, hosts, relay, presence}` / `Welcome` (`dialing.rs:181-196`). Proves: DID+device bound to the group endpoint key (`verify_endpoint`), then membership/invite/open policy on the *host* (`accepting.rs:109-150`) | `SelfMsg::Auth{attestation, relay, unlink}`, dialer first then acceptor (`sessions.rs:140-191`); `proto::accept_self_hello` (same DID, not tombstoned) | `Msg::CallHello` / `ContactHello` with grant/ticket |
| Hello timeout | `AUTH_TIMEOUT` (15 s) | `HELLO_TIMEOUT` 15 s (`engine.rs:82`) | `AUTH_TIMEOUT` 15 s | `HELLO_TIMEOUT`, `REPLY_TIMEOUT` 10 s |
| Admission | global `pending`/`reserved` semaphores before ALPN dispatch (`lifecycle.rs:196-208`), blocked DID gets a signed **Notice** instead of Auth (`session.rs:398-406`), `chat_may_dial` (contact and neither side blocks), message token with renewal | none at accept except per-ALPN dispatch and `HELLO_TIMEOUT`; verdict `Refuse/Removed/Welcome` by membership (`accepting.rs:109-150`); `Fetch`/`Moved` first frames | permit dropped at once (`lifecycle.rs:232-238`); tombstoned device told `Unlinked` (`sessions.rs:166-178`) | blocked → Notice, token check, availability, busy, glare yield |
| Frame codec | `ChatReader/ChatWriter` over `node_runtime::frames::FrameBuf`, label `"chat "`, `MAX_FRAME` 16 MiB (`chat/wire.rs:11-28`, `chat-core/wire.rs:11`) | `Reader/Writer` same `FrameBuf`, label `"group "`, `MAX_FRAME` 16 MiB (`wire.rs:~34`) | `FrameReader/FrameWriter`, label `""`, 16 MiB (`sync-runtime/frames.rs:9-34`) | `Ctrl` with `proto::{encode,decode}_frame` (different codec, `wire.rs:61-97`) |
| Main loop | one `tokio::select!` in `chat_run` (`session.rs:466-550`): heartbeat tick, idle tick, datagram, `reader.recv`, `cmds.recv`, `conn.closed` | **two tasks**: writer task draining `Out` channel, and a plain `loop { reader.recv }` in `group_read` (`sessions.rs:72-90`, `100-318`); plus a backfill task `group_days_sync` | one `select!` in `self_loop` (`sessions.rs:~330-555`): idle tick, `reader.recv`, two `sleep(peer.x.delay())` arms, `changes.changed()` | `select!` in `run_call` (`run.rs:~170-290`) with ring/watchdog/camera/encoder arms |
| Outbound queue | `mpsc<Cmd>` of intents (`Push(day)`, `Ack`, `Presence`...) executed by the loop | `mpsc<Out::Msg(GroupMsg)>` of **ready-made messages**, drained by the writer task | none; the loop reads `changes` watch and `dirty` set and encodes inline | `mpsc<Cmd>` |
| Idle close | app-level: no `ChatMsg` either way **and** `!chat_keep` (activity, pending work, live call), `idle_close()` 60 s, check every `idle_close/6` (`session.rs:481-489`) | **none**: sessions live until transport idle (20 s max idle, 5 s keep-alive, `lifecycle.rs:297-304`) or close | transport-level: QUIC stream byte counters unchanged **and** no pending change, same `idle_close()` (`sessions.rs:~290-310`), then `selfsync_settled` | n/a |
| Reconnect / backoff | `chat_kick`: loop 2 s doubling to 60 s, only while there is something pending; `dial_sem` 4 permits; `dialing` set per DID (`session.rs:280-309`) | tick every 2 s (`TICK`), backoff table `[1,2,5,15,30,60]` s per (group, host) in `core.backoff`, `dialing` set per (group, host) (`dialing.rs:8-65`) | `dial_round` every `sync_period()` 15 s or on signal; backoff `10 s << n` capped 300 s per device; only devices *owed* a sync (`synced` generation) (`devices.rs:28-80`) | none (user-initiated) |
| Session registry | `core.sessions: Mutex<HashMap<[u8;32] remote device, Arc<Session>>>`, `Session.id` counter; helper lookups by DID | `core.sessions: Mutex<HashMap<u64 id, Arc<Sess>>>`; lookups by `(gid, did, dev)` linear scan (`documents.rs:67`) | `selfsync.sessions: HashMap<[u8;32] device, SelfSlot{dialer, conn, acked, dirty}>` | `live.call` / `live.waiting` slots |
| Duplicate-session rule | both dialled at once: session with the **lower dialer device key survives** on both ends (`session.rs:445-455`); the old one gets `Cmd::Close` | none needed for member→host; hosts dialling hosts could duplicate (guarded only by `has_session` before dialling, `dialing.rs:~85`) | same lower-dialer-key rule (`sessions.rs:55-78`) | `yield_on_glare` (`run.rs:~137`) is the analogue for calls |
| Epoch / account switch | each message: `me.epoch != sess.epoch → Err(Locked)` (`sync.rs:~72`); `chat_close` closes all | `GroupCore.epoch`; `group_core()` replaces the core and `shut()`s the old one, which sends `Out::Close` to every session (`lifecycle.rs:14-33`, `documents.rs:100-110`) | `epoch` captured at `self_run`; `changes.changed()` arm returns when `shared.epoch != epoch`; each apply takes `epoch` | `epoch` on `Call` |
| Teardown | remove from map **only if `s.id` matches** (`session.rs:561-566`), `hist.take()`, presence update, finish writer, delayed `conn.close` 300 ms, redial after 2 s if pending | remove by id, abort backfill/writer tasks, `conn.close`, `on_group_changed`, `kick.notify_waiters` (`sessions.rs:76-90`) | `drop_session` compares `conn.stable_id()` (`sessions.rs:115-125`), `conn.close` | `end_call`, delayed close |
| On-close hooks | presence update, kick retry, search none | `group_presence_session_end`, event, kick | `selfsync_settled` (marks the generation synced) | record call log |

Evidence that the frame codec is already one thing: `node-runtime/src/frames.rs:1-54` is explicitly "shared by the chat, group and own-device sync wires"; the three wrappers differ only by message type, limit and an error label.

### 1.2 What is the same, what is parameters, what is protocol

**Identical code, copied three times (share verbatim):**
- Framed stream wrapper `Reader<M>/Writer<M>` (chat/wire.rs:11-28, group wire.rs, sync frames.rs:9-34): about 25 lines each, 75 total, becomes one generic type.
- Dialer tie-break registry: chat `session.rs:445-455` and sync `sessions.rs:55-78` are the same rule keyed by remote device key, with "remove only if mine" on teardown (chat by `id`, sync by `stable_id`). Group lacks the rule but needs the same "remove only if mine" (it does it by unique id).
- `dialing` set + per-peer backoff + "spawn dial, clear flag, update backoff" (chat `session.rs:280-309`, group `dialing.rs:38-65`, sync `devices.rs:48-77`).
- Accept path: `accepting.alpn()` with timeout, ALPN dispatch, `accept_bi`, first frame with timeout, close on error with a reason (chat `session.rs:379-408`, group `accepting.rs:7-60`, sync `sessions.rs:7-19`, node `lifecycle.rs:186-257`).
- Teardown: `finish` writer, wait `conn.closed()` bounded, then `conn.close(0, "bye")`, written five times with different waits (300 ms, 2 s, 3 s, 5 s, `LINGER`).
- Idle close arithmetic (`idle_close()`, `idle_check()` ticks) is shared as functions but each loop re-implements the "activity" notion.

**Same shape, differs by parameters (a config struct):**
dial timeout (20 s vs sync's 10 s: accidental), hello timeout 15 s, backoff schedule (three different schedules, same shape `fn(attempt) -> Duration`), idle policy (`Never | AppActivity(keep predicate) | TransportBytes`), who dials (chat: either, sync: either with tie-break, group: member→host only), registry key (device key vs `(gid, did, dev)`), per-ALPN permit.

**Genuinely protocol-specific (stays in the runtimes):**
- What Hello proves and what is admitted: chat's message token + block Notice; group's invite/open-join/removed verdicts and `Fetch`/`Moved` first frames; sync's tombstone + `Unlinked`.
- The main-loop *work*: chat's heartbeat/datagram presence, sync's catalogue and `dirty` marks, group's relay fan-out with `from` exclusion and the separate backfill task.
- On-close consequences (presence, `selfsync_settled`, kick).
- Calls: leg fan-out, ring timers, media; should **not** use the shared session type at all (a call is one stream with a state machine; `run_outgoing` is 300 lines of leg logic). One-shot exchanges (`exchange()`, tokens, notices, device lists) are request/reply and are already a good small helper (`wire.rs:54-62`). Leave node-runtime's call/contact code out of scope except the accept router.

So the honest similarity is **medium**: roughly 60% of the lines in these files are shared plumbing, but the plumbing is 15 distinct small pieces, not one loop.

### 1.3 Concurrency hazards the current copies handle (the design must keep each)

1. **Dial and accept of the same peer at once** (chat `session.rs:445-455`, sync `sessions.rs:55-78`): lower dialer device key wins on both sides; loser closes with "duplicate" (chat) or returns (sync: the new connection simply ends). Needs a registry that is *atomic* with respect to insertion (single mutex section; chat does it inside `core.sessions.lock()`).
2. **Teardown must not remove a newer session** (chat compares `id`, sync compares `stable_id`). Use a `SessionId(u64)` allocated by the registry.
3. **Dial in flight when a session appears** (chat `chat_kick` re-checks `chat_session` each loop iteration `session.rs:285-286`; group `group_targets` filters `has_session || dialing`; sync `dial_round` checks `sessions.contains_key`): the `Dialer` must re-check the registry right before connecting and again before registering.
4. **Epoch / account switch mid-session**: three different mechanisms (per-message check, core replacement + `shut`, captured epoch + watch). Unified rule: `Registry` is owned by something that is replaced per account epoch; `Session` carries `epoch`; `Protocol::on_frame` receives a `Ctx` whose `ctx.alive()` is false after the switch and the loop exits. Closing is `registry.close_all()`.
5. **Idle close vs in-flight work**: chat closes only if `!chat_keep` (pending days, wanted blobs, owed receipts, live call, local activity); sync closes only if QUIC bytes did not move *and* `!changes.has_changed()` (a change landed between check and close), then calls `selfsync_settled(generation)`; and *both* redial if work remains (`session.rs:572-585`). The design needs `Protocol::keep_open(&self) -> bool` evaluated on every idle tick and a mandatory `on_closed(reason)` that decides the redial. Do not move the "pending?" logic into the shared crate.
6. **Dial concurrency cap** (`dial_sem` 4 in chat); group/sync rely on set membership only. Keep an optional semaphore in `Dialer`.
7. **Blocked peers**: chat answers an *incoming* blocked DID with a signed Notice **before** its own Auth and an *outgoing* dial that receives a Notice instead of Auth treats it as a block (`session.rs:362-376`). This is protocol-specific; in the new design it is a `Protocol::admit` returning `Admit::Refuse(first_frame)`.
8. **Flood limits at the accept loop**: only node-runtime has them (`pending`/`reserved` permits released at different points per ALPN). Group has none (a stranger can hold `HELLO_TIMEOUT` of a task per connection). The shared `Acceptor` should apply the same two-tier permit to all ALPNs. That is a behaviour change for group (stricter), listed in risks.
9. **Rotation overlap**: a second, previous endpoint still accepts (`overlap_incoming`). Stays in node-runtime; the `Acceptor` is endpoint-agnostic (takes an `Accepting`).

### 1.4 Proposed crate `peer-session` (new, depends on iroh, tokio, parking_lot, cryptography::codec; no runtime types)

```rust
// ---- framing (replaces node-runtime/src/frames.rs wrappers x3, ~75 lines) -------------
pub struct FrameReader<M>(FrameBuf, PhantomData<M>);
pub struct FrameWriter<M>(SendStream, PhantomData<M>);
impl<M: DeserializeOwned> FrameReader<M> { pub async fn recv(&mut self) -> Result<Option<M>, Error>; } // cancel-safe
impl<M: Serialize>        FrameWriter<M> { pub async fn send(&mut self, m: &M) -> Result<(), Error>; pub fn finish(&mut self); }

// ---- policy, all the parameters that differ today -------------------------------------
pub struct Config {
    pub alpn: &'static [u8],
    pub dial_timeout: Duration,           // 20 s (sync's 10 s becomes 20 s)
    pub hello_timeout: Duration,          // 15 s
    pub max_frame: usize,                 // 16 MiB
    pub idle: IdlePolicy,                 // Never | AfterQuiet(Duration)   (both use transport+app activity, see below)
    pub backoff: Backoff,                 // fn(attempts) -> Duration; presets CHAT, GROUP, OWN
    pub dial_permits: usize,              // chat 4, others 8
    pub tiebreak: Tiebreak,               // LowerDialerKeyWins | None (group: roles are fixed)
}

// ---- what a protocol supplies ----------------------------------------------------------
pub trait Protocol: Send + Sync + 'static {
    type Msg: Serialize + DeserializeOwned + Send;     // the stream message enum
    type Key: Eq + Hash + Clone + Send;                // registry key: device key / (gid,did,dev)
    type Cmd: Send;                                    // intents from the runtime to one session
    type State: Send;                                  // per-session state owned by the loop

    /// Both sides call this after the stream is open. Dialer sends first when `dialing`.
    /// Returns who the peer is, or a refusal to send as the only frame.
    async fn handshake(&self, io: &mut Io<Self::Msg>, conn: &Connection, dialing: bool)
        -> Result<Established<Self::Key, Self::State>, Refusal<Self::Msg>>;
    async fn on_frame(&self, s: &mut Live<Self>, m: Self::Msg) -> Result<(), Error>;
    async fn on_cmd(&self, s: &mut Live<Self>, c: Self::Cmd) -> Result<(), Error>;
    /// Timers and wake-ups of this protocol (heartbeat, pump delay, change signal).
    /// Must be cancel-safe; default is `pending()`.
    async fn wake(&self, s: &mut Live<Self>) -> Result<(), Error> { std::future::pending().await }
    /// Evaluated on every idle tick: `true` keeps the session although quiet.
    fn keep_open(&self, s: &Live<Self>) -> bool { false }
    /// Always called once; decides redial, marks settled, presence, etc.
    fn on_closed(&self, s: &Established<Self::Key, Self::State>, why: CloseReason);
    /// Epoch guard: loop exits when false.
    fn alive(&self, s: &Established<Self::Key, Self::State>) -> bool;
}

pub struct Established<K, S> { pub key: K, pub peer_device: [u8; 32], pub epoch: u64, pub state: S }

// ---- the three machines -----------------------------------------------------------------
pub struct Registry<P: Protocol>;            // atomic insert with tie-break, remove-if-mine, close_all, get/iter
impl<P: Protocol> Registry<P> {
    pub fn insert(&self, key: P::Key, id: SessionId, dialer: [u8;32], tx: CmdTx<P::Cmd>, conn: &Connection) -> Insert; // Won | Lost
    pub fn remove_if_mine(&self, key: &P::Key, id: SessionId);
    pub fn cmd(&self, key: &P::Key, c: P::Cmd) -> bool;
    pub fn close_all(&self, reason: &'static str);
}
pub struct Dialer<P: Protocol> { /* Config, Registry, dialing: HashSet<Key>, backoff: HashMap<Key,(n,Instant)>, Semaphore */ }
impl<P: Protocol> Dialer<P> {
    /// Re-checks the registry, honours backoff and the in-flight set, then connects, handshakes,
    /// registers and spawns `run`. `addrs`: ordered candidates (chat: every device of the contact).
    pub fn ensure(self: &Arc<Self>, p: Arc<P>, key: P::Key, addrs: impl Fn() -> Vec<EndpointAddr> + Send + 'static);
    pub fn forget_backoff(&self, key: Option<&P::Key>);   // hurry(), forget_progress()
}
pub struct Acceptor<P: Protocol> { /* Config, Registry, two-tier permits */ }
impl<P: Protocol> Acceptor<P> {
    pub async fn serve(&self, p: Arc<P>, accepting: Accepting, permit: Permit) -> Result<(), Error>;
}
/// The loop itself: reader, command queue, protocol wake, idle tick, conn.closed, teardown
/// (`finish`, bounded wait, close, remove_if_mine, on_closed).
pub async fn run<P: Protocol>(p: Arc<P>, conn: Connection, io: Io<P::Msg>, est: Established<..>, rx: CmdRx<P::Cmd>);
```

Design notes:
- **Idle** unifies the two notions: the loop tracks both `frames seen` and QUIC `frame_tx/rx.stream` counters (sync's approach is strictly more accurate than chat's, because it counts bytes of a send in flight). The loop closes when `quiet >= idle_close() && !keep_open()`. This fixes a small inconsistency: chat resets `active` on `Cmd`s, sync on bytes.
- Datagrams (chat presence) are served by `wake`/an optional `on_datagram` hook; the loop `select!`s `conn.read_datagram()` only if `Config.datagrams`.
- Group's separate writer task is replaced by the generic loop writing inline; group's `Out::Msg` queue becomes `Cmd::Send(GroupMsg)`; the one thing the writer task buys (a slow `send` not blocking `reader.recv`) is real for group's relay fan-out. Keep an option `Config.split_writer: bool` and implement it as a spawned writer + queue when true. This is the main place the abstraction costs something.

### 1.5 How chat and sync implement it (sketch)

Chat (replaces `chat_kick`, `chat_dial`, `chat_handshake`, `handle_chat_*`, `chat_run`, about 330 of session.rs's 658 lines):

```rust
impl Protocol for ChatRt {                       // really `ChatProto(Arc<ChatRt>)`
    type Msg = ChatMsg;  type Key = [u8; 32];    // remote device
    type Cmd = Cmd;      type State = ChatSession; // did, profile_vv, peer_vv, hist, presence, receipt_vv
    async fn handshake(&self, io, conn, dialing) -> Result<Established<..>, Refusal<ChatMsg>> {
        let me = self.me()?;
        if dialing { io.send(&self.auth(&me, self.grant_for(conn.remote_id())?)).await?; }
        let first = io.recv_within(self.cfg.hello_timeout).await?;           // Notice => blocked
        let (did, hint, grant) = self.verify_auth(&me, &first, remote)?;
        if let Some(notice) = self.refusal_for(&did) { return Err(Refusal::send(ChatMsg::Notice { notice })); }
        self.chat_admit(&did, &grant)?;
        if !dialing { io.send(&self.auth(&me, self.grant_from(&did)?)).await?; }
        self.ctx.note_device(me.epoch, &did, remote, hint);
        Ok(Established { key: remote, peer_device: remote, epoch: me.epoch, state: ChatSession::new(did) })
    }
    async fn on_frame(&self, s, m) -> Result<()> { self.chat_handle(&s.core, &s.state, m, &mut s.io).await }   // existing fn
    async fn on_cmd(&self, s, c)   -> Result<()> { match c { Cmd::Push(d) => self.chat_push(..).await, ... } } // existing arms
    async fn wake(&self, s)        -> Result<()> { s.heartbeat.tick().await; self.presence_send(..); Ok(()) }
    fn keep_open(&self, s) -> bool { self.chat_keep(&s.core, &s.state.did) }                                   // existing fn
    fn on_closed(&self, s, why)    { self.presence_update(..); self.redial_if_pending(&s.state.did) }
    fn alive(&self, s) -> bool     { self.me().is_ok_and(|m| m.epoch == s.epoch) }
}
// chat_kick(did) becomes: self.dialer.ensure(proto, key_for(did), || contact_addrs(did))
```

Sync (replaces `self_dial`, `self_run` handshake, `register_session`, `drop_session`, `dial_round`'s backoff, and the idle arm; `self_loop` keeps its dispatch):

```rust
impl Protocol for SyncProto {
    type Msg = SelfMsg; type Key = [u8; 32]; type Cmd = (); type State = OwnSession; // acked, dirty, peer: PeerChat, sent docs
    async fn handshake(&self, io, conn, dialing) -> Result<..> {
        let me = self.me()?;
        if dialing { io.send(&self.my_auth(&me)).await?; }
        let SelfMsg::Auth { attestation, relay, unlink } = io.recv_within(HELLO).await? else { bad() };
        match proto::accept_self_hello(&attestation, me.id.did(), remote, &self.tombstones()) {
            Err(Tombstoned) if !dialing => return Err(Refusal::send_all([self.my_auth(&me), SelfMsg::Unlinked])),
            Err(_) => return Err(Refusal::protocol("not one of our devices")),
            Ok(_) => {}
        }
        if !dialing { io.send(&self.my_auth(&me)).await?; }
        if unlink && !dialing { return Err(self.take_unlinked(io).await); }
        self.note_own_device(me.epoch, &attestation, remote, relay);
        Ok(Established { key: remote, .., state: OwnSession::new(&self) })
    }
    async fn on_frame(&self, s, m) -> Result<()> { self.dispatch(s, m).await }     // today's big match, section 2.3
    async fn wake(&self, s) -> Result<()> {                                         // changes watch + receipts/history timers
        tokio::select! { _ = s.changes.changed() => self.on_changes(s).await,
                         _ = sleep(s.peer.receipts.delay()), if s.peer.receipts.enabled => s.peer.receipts.pump(..).await,
                         _ = sleep(s.peer.history.delay()),  if s.peer.history.enabled  => s.peer.history.pump(..).await }
    }
    fn keep_open(&self, s) -> bool { s.changes.has_changed().unwrap_or(true) }
    fn on_closed(&self, s, why)    { if why.is_idle_or_peer_closed() { self.selfsync_settled(s.key, &s.changes) } }
    fn alive(&self, s) -> bool     { self.shared.lock().epoch == s.epoch }
}
```

### 1.6 Size, risk, tests, formats

- **Replaced:** chat `session.rs` 280-585 (about 330 lines), group dial/accept/run plumbing (`dialing.rs` 8-65 + 156-170, `accepting.rs` 7-70, `sessions.rs` 1-95, `lifecycle.rs` bind/close about 100: about 330), sync `sessions.rs` 7-125 + `devices.rs` 48-77 + idle arm (about 230), node `lifecycle.rs:186-257` router (about 70, becomes a table), three frame wrappers (75), `version.rs` ALPN-refusal machinery (about 110 + tests, see section 6). About **1,150 lines**.
- **New:** `peer-session` about 550 (registry 90, dialer 130, acceptor 90, run loop 140, framing 40, config 60); per-runtime adapters chat about 130, sync about 90, group about 160. About **930**.
- **Net:** about **-200 lines**, but only if `version.rs` machinery goes (section 6). Without that, about -90. The real gain is one place for the six hazards above and removal of three diverging timeouts/backoffs.
- **Risks:** (1) group's relay fan-out depends on a non-blocking writer; a naive inline-write loop can stall a host on one slow member (keep `split_writer`). (2) Turning on the global permit pool for group changes flood behaviour. (3) `wake()` with `select!` over borrowed state is the usual cancel-safety trap (sync's `pump` awaits a `send`; a cancelled send could corrupt framing: today the `sleep` arms cancel only the *sleep*, and the body runs after the arm resolved, so it is safe; the trait must say "wake returns when something is due; it may not hold a send across a cancellation point"). (4) The `Established.epoch` check order relative to `register`.
- **Tests:** `core/tests/idle_close.rs` (320 lines, the spec for idle + redial), `devices.rs`, `lifecycle.rs`, `endpoint_close.rs`, `fanout.rs`, `chat_shards.rs`, `group_core.rs`, `group_g2.rs`, `public_group.rs`, `blocking_acceptance.rs`, `private_rotation.rs`, `link.rs`, `link_expiry.rs` (link untouched), `restore.rs`, `accounts.rs` (epoch switch). Runtime lib tests: `node-runtime` `version::tests`, `tests.rs`; `sync-runtime` `group_contents/tests.rs`; `core/src/sync/engine` wire/fault tests (`group_history`, `group_receipts` fault tests).
- **Format change:** none required for the kit. Defaults I propose with it (no back-compat): (a) a proper `SessionAuth { attestation, grant: Option<Grant>, relay, caps }` frame in `proto` for chat, replacing the `call_hello` abuse and its `unreachable!()` (`session.rs:231-238`); (b) the same `caps: Vec<String>` list in every Hello (see 6); (c) ALPN becomes `tinline/{chat,self,group}/1` and the old-ALPN list disappears.
- **Migration path:** (1) land `FrameReader<M>/Writer<M>` and replace the three wrappers (pure refactor, one commit each runtime). (2) `Registry` with tie-break, adopt in **sync** first (smallest, most isolated loop, with `idle_close.rs` and `devices.rs` as spec). (3) `Dialer`/backoff in sync, then chat. (4) `run()` loop in sync, then chat (chat has the most `select!` arms: datagram, heartbeat). (5) Group last, and only registry + dialer + acceptor + framing, **not** `run()` unless `split_writer` proved out. Running old and new side by side is possible per runtime because each runtime owns its ALPN; do not mix within one runtime.

---

## 2. Doc replication

### 2.1 Table of the copies

| | Chat DM (`chat-runtime/.../sync.rs`, `history.rs`, `local.rs`) | Group (`group-runtime/.../replication.rs`, `documents.rs`, `changes.rs`, `history.rs`) | Own-device sync (`sync-runtime/.../account.rs`, `push.rs`, `group_contents.rs`, `devices.rs`, `self_loop`) |
|---|---|---|---|
| Doc naming / scope | `dm/{pair}/{day}` (`doc_name`), one Loro doc per UTC day shard per conversation, plus `profile` doc; `chat_doc_day` checks the name belongs to this session's pair (`sync.rs:~158`) | `DocId::Meta` and `DocId::Day(day)` inside a group; the session is scoped to one group by the Hello | flat names: `acc`, `calls`, `profile`, `profile-cache`, chat `dm/...`, group `group/{gid}/{meta|day}` (`own/contents.rs`); `Hello.docs` lists names + VV |
| What is sent | `Hello{shards:[(day, vv)]}`, `Sync{doc, vv, update, sig}` (catch-up), `Push{doc, update, sig}` (live), `Ack{doc, vv}` | `Hello/Welcome{vv, days}` then `Update{doc, update}` / `Ack{doc, vv}` / `Days{after, days}` inventory | `Hello{docs}`, `Sync{doc, vv, update, sig}` (also the ack, with empty update), `Acks`, `Read`, `Groups`, `Invitations` |
| Update content | Loro export of **own-authored ops only** (`export_own_since`, `chat_send_own`), signed as a batch by the device key (`sign_batch`); receiver checks `signer == sess.device` | Loro export since peer VV of **all ops** (`export_since`, relay: `replication.rs push` uses `peer_vv`), **original signed record values**; no batch signature (each record carries its DID signature) | whole-doc `export_since(theirs)` for account/calls/profile; chat/group docs through the owning runtime's own export; batch signed by device key |
| Limits / paging | `MAX_HELLO_SHARDS`, recent `HELLO_DAYS` + pending days; history via `HistoryReq{before, limit<=14}` | `HELLO_DAYS` 7, `Days` pages of `DAYS_PAGE` 64 (inventory), then newest-first updates "a few a second" (`history.rs:109-229`) | `MAX_OWN_DOCS`, group docs offered via cap/group-history paged pulls (candidate 3); per-session `peer.docs` bounded with eviction (`group_contents.rs:35-58`) |
| Authorization | Per message: `Ctx{signer_did, now_ms, live, history}` into `shard.apply_remote` (live window 24 h, signer must be author, `Reject::{BadSignature, Pending, ...}`) | `admit(&GroupState, &[Entry], &[Entry])` closure per session: membership, removed-member upload window and "before removal" rule, admins-only, upload cut | the link itself (attested own device, tombstones); batch signature checked; group docs via `group_own_content_apply` re-check group admission |
| Apply / persist | `chat_receive` → `shard.apply_remote` → `chat_commit` (message index, file registry, conversation row, delivered receipt intent) → `chat_persist` (update row or snapshot + meta in one tx) → `selfsync_dirty` + search | `GroupCore::apply` → `apply_meta` / `apply_day` (`documents.rs:~210-500`): derive state, rejudge pending posts, store rows + meta + receipt intents atomically; persistence failure rolls back memory (`restore_meta`, `shards.remove`) | `apply_account_update`, `apply_calls_update`, profile/cache apply, `chat_own_apply`, `group_own_content_apply` |
| After apply | events (`on_message_added/changed`, `on_chat_changed`), ack to peer, `Cmd::Ack` to the contact's other sessions, blob downloads, receipts pump | `Changed` → `group_after`: push to other sessions (`from`), receipts pump, search, images, enforcement, host announce, own-device dirty marks, events (`changes.rs:7-80`) | `sync_bump`, `touch_device`, signal for other sessions (`dirty` set), ack frame |
| Conflict / rejection | `Reject::Pending` (ops build on lost ops of ours) → pull the day as a vouched snapshot (`chat_pull_day`); other rejects end the session | `Reject::Persistence` ends session; `Reject::Policy(ADMINS_ONLY)` refuses the batch, keeps session, member marks posts failed; any other policy → `Refused` + close | transient failure ends the session (so it retries); rejected chat batch only logged; group batch rejection ends session |
| Peer-knowledge tracking | `sess.peer_vv: HashMap<day, VV>`, merge on Ack | `Sess.peer_vv` (meta), `peer_days: HashMap<day, VV>`, `peer_has()` on Update/Ack | `sent`, `sent_calls`, `sent_profile`, `sent_cache` locals, `peer.docs`, `peer.acks`, `acked` |

### 2.2 Does one `Replicated` trait + one pull/push loop fit?

**Common core (true in all three):** each side knows "the peer holds VV `v` of doc `d`"; sends `export_since(v)` bounded; the receiver verifies, applies, answers its resulting VV; the sender merges that VV into its peer-knowledge. The bookkeeping for that last step is written five times (chat `peer_vv`, group `peer_has`, sync's four `sent_*` variables, `peer.docs`, `peer.acks`).

**Where one loop breaks:**
1. **Who triggers a send.** Chat triggers per-day via `Cmd::Push(day)`; group triggers by `Changed` fan-out to *all other sessions of the group* (`push(gid, from, days)`), including *relaying* a third party's ops, with ordering "meta before day" and an upload cut (`admit::upload_cut`); sync triggers by a `dirty` set and a global change watch. A generic loop cannot own triggering. It can only own "bring peer P up to date on doc D".
2. **What is exported.** Chat exports only ops of own writers (`export_own_since`), group exports everything held (relay). That is exactly "the original record bytes, never re-authored diffs": it is a property of the *doc's* `export`, and a Loro `export_since` already ships the raw ops. So the claim does not break the trait, provided the trait leaves `export` to the implementor and never rebuilds a doc from decoded entries. Group's cut export (`export_since_before(pv, &cut)`) means `export` needs an input beyond the VV (a "view of the peer": host/member).
3. **Authorization is per doc kind** with different inputs (`Ctx`, `admit` closure, device trust). The trait takes `apply(&self, from: &PeerCtx, doc, update) -> Result<Applied, Reject>`.
4. **The after-apply side effects** are the bulk of the code (chat_commit about 60 lines, group_after about 80, apply_meta/day about 250) and are not shared at all.
5. **Chat's live window** (`Sync` vs `Push`, `creates_outside`) and **Pending → snapshot pull** are chat-only.

So: **a generic push/pull loop that owns control flow does not truly fit.** What does fit is a *per-session, per-doc* helper plus a *uniform message vocabulary*:

```rust
// crate `doc-sync` (or module in `crdt`)
pub struct DocRef(pub String);                           // opaque to the helper: "dm/pair/day", "group/gid/meta", "acc"
pub trait Replicated: Send + Sync {
    type Peer;                                           // device key | (member did, hosting?) | ()
    fn local_vv(&self, doc: &DocRef) -> Option<VersionVector>;
    fn export(&self, peer: &Self::Peer, doc: &DocRef, since: &VersionVector, lim: BatchLimits)
        -> Result<Option<Batch>, Error>;                 // Batch { update, frontier, more }; raw ops, never re-authored
    fn apply(&self, peer: &Self::Peer, doc: &DocRef, update: &[u8])
        -> Result<Applied, Reject>;                      // Applied { vv_now, changed: bool, effects: Box<dyn FnOnce()> }
    fn doc_ok(&self, peer: &Self::Peer, doc: &DocRef) -> bool;   // naming/scope check (dm pair, group id, cap negotiated)
}
pub enum DocMsg { Have { docs: Vec<(DocRef, Vec<u8>)> }, Update { doc: DocRef, update: Vec<u8>, sig: Option<Vec<u8>> }, Ack { doc: DocRef, vv: Vec<u8> } }

/// Per-session tracker: what the peer holds, bounded, merged on ack, evicts cold docs.
pub struct PeerDocs { known: BoundedMap<DocRef, VersionVector> }
impl PeerDocs {
    pub fn on_have(&mut self, docs: &[(DocRef, Vec<u8>)]) -> Result<(), Error>;
    pub fn on_ack(&mut self, doc: &DocRef, vv: &[u8]) -> Result<(), Error>;      // merge, never replace
    pub fn on_sent(&mut self, doc: &DocRef, frontier: &VersionVector);
    pub async fn push<R: Replicated>(&mut self, r: &R, p: &R::Peer, doc: &DocRef, w: &mut impl Sink<DocMsg>, signer: Option<&Signer>) -> Result<(), Error>;
    pub async fn receive<R: Replicated>(&mut self, r: &R, p: &R::Peer, m: DocMsg, w: &mut impl Sink<DocMsg>) -> Result<(), Error>;
}
```
`receive` does: size/name check, signature check (if the protocol uses batch signatures), `apply`, run `effects`, `Ack{doc, vv_now}`, `on_sent`. `push` does `export`, sign, send, `on_sent`. The runtimes keep their triggers.

### 2.3 Where it pays: two concrete sub-places

**(a) Receipt ledger exchange: chat and group are near-verbatim copies (similarity high).** `chat-runtime/.../receipts.rs:208-323` (`chat_receipt_hello`, `_hello_in`, `_send`, `_receipts_in`, `_ack_in`) versus `group-runtime/.../member_receipts.rs:255-460` (`receipt_hello`, `on_receipt_hello`, `receipts_send`, `on_receipts`, `on_receipt_ack`, `receipts_push`, `receipts_rehello`). Same constants (`MAX_RECEIPT_DAYS`, `RECEIPT_FRAMES_PER_SEND`, `RECEIPT_BATCH`), same algorithm: hello lists ledger VV for recent days + outbox days; the answer is bounded `Receipts` frames until `!more`; each import answered by `ReceiptAck`; `acknowledged()` clears the outbox. Differences: group has an "off while the group is open" gate, a `receipt_waiting` set re-asked after docs advance (`pending` import), and **merges** the Ack into `receipt_vv` while chat **replaces** it (`chat_receipt_ack_in`: `sess.receipt_vv.lock().insert(day, vv)`; group's comment at `member_receipts.rs` (`on_receipt_ack`) explains why merging is right: "taking it as the whole truth sent every in-flight statement again"). So the chat copy has the resend-amplification defect the group copy already fixed. One shared implementation fixes it by construction.

Proposal: `receipts-core::exchange::ReceiptExchange<S: ReceiptScopeSource>` holding `receipt_vv` per day and the `waiting` set, with `hello()`, `on_hello()`, `send_day(sink)`, `on_receipts()`, `on_ack()`, `rehello()`, the source trait supplying `scope(day)`, `days_held(day)`, `off()`, `import(day, update)`, `export(day, theirs, lim)`, `outbox_days()`, `acknowledged(day, vv)`. The runtimes keep only the wire message mapping (`ChatMsg::Receipts` vs `GroupMsg::Receipts`) and "relay to other sessions" (group only).
  - Replaced: about 150 lines (chat) + 190 (group) = **340**; new shared about **170**, per-runtime adapters about 40 + 50. Net **about -80 lines**, plus the Ack-merge defect fixed once. Similarity **high**; risk **low-medium**.
  - Tests: `core/tests/message_receipts.rs` (183), `old_writer_delivery.rs`, `group_shards.rs`, `public_group.rs` (receipts off while open), `core/src/sync` receipts fault tests, lib tests `group-runtime member_receipts/tests.rs`, `engine/durability/group_receipts_sim.rs`, `chat-runtime chat/engine/tests.rs`.
  - Format: no change needed (frames already identical in shape). Option: one `DocMsg`-style `Receipts{scope, update}` instead of `day`.

**(b) sync-runtime `self_loop`: a doc registry instead of an if/else chain (similarity high, one crate).** `self_loop` (about 270 lines, `sessions.rs:~190-555`) routes by name: `DOC_NAME`, `CALLS_DOC`, `PROFILE_DOC`/`PROFILE_CACHE_DOC`, `group_documents::parse_doc`, chat `parse_doc`, with four `sent_*: Option<VersionVector>` locals and the same `apply → sign empty → send Sync{vv: now}` block repeated four times (`sessions.rs`: account at the first `if doc == DOC_NAME`, calls, profile/cache, group contents), and `push_changes(...)` called four times twice (`sessions.rs` after the match and in the `changes` arm). With `Replicated` implemented by `AccountDocs`, `CallsDocs`, `ProfileDocs`, `ChatDocs`, `GroupContentDocs` and one `PeerDocs`, `self_loop` becomes: `Have → peer.on_have`, `Sync → peer.receive(registry.route(&doc))`, plus the non-doc messages (`Read`, `Acks`, `Groups`, `Invitations`, `Unlinked`, history, receipts). I estimate `self_loop` + `push.rs` (about 270 + 220 = 490 lines) shrinks to about 260, plus about 160 new in the helper, **net -70 lines in sync-runtime**, but the main gain is that adding a doc means one `impl`, not five edits (the current code needed `MAX_OWN_DOCS + 4` hacks and "reserve three slots" in `group_contents::catalog`, see section 6).

### 2.4 Chat and group onto the helper

- **Chat**: `chat_receive` becomes `receive` with `apply` = `shard.apply_remote` + `chat_commit` + the event emission in `effects`; `chat_send_own` becomes `push` with `export` = `export_own_since`. The `Sync`/`Push` distinction stays as a `live: bool` on `Update` (or an enum `Delivery::{CatchUp, Live}` in the message). `Reject::Pending → chat_pull_day` stays in `apply`'s error mapping (`Reject::NeedSnapshot(day)`). Saves about 60 lines; gain is small.
- **Group**: `GroupCore::push` (about 80 lines) and `group_read`'s `Update`/`Ack` arm (about 80) map onto `push`/`receive`, but the fan-out and `Changed` pipeline stay. Saves about 60 lines; risk is highest (the order guarantees "meta first", the cut, `Reject::Policy(ADMINS_ONLY)` keeping the session). **Not recommended in the first pass.**

### 2.5 Summary for candidate 2

- Similarity: low as a single loop; **high** for 2a and 2b.
- Replaced: 2a about 340, 2b about 490, chat+group doc paths about 330 (optional). Shared new: about 170 (receipts) + 160 (`doc-sync`). Net: **about -150 (2a+2b), about -270 if chat/group doc paths follow**.
- Formats: none required. Optional unification of `Hello` doc lists (`Have`) and `Update`/`Ack` vocabulary across `ChatMsg`, `GroupMsg`, `SelfMsg` is a clean-up with no payoff until three runtimes share the helper.
- Migration: (1) receipt exchange in chat (the buggy copy) then group; (2) `doc-sync` in sync-runtime `self_loop` only; (3) chat's `chat_receive/send_own`; (4) group last or never.

---

## 3. Paged pulls

### 3.1 What exists (the premise is only partly right)

| Copy | Is it a paged pull? | Cursor | Messages | Timers |
|---|---|---|---|---|
| `sync-runtime/.../group_history.rs` (455) | yes, two-stage: page of day inventory, then `Want` batches per doc | day string (`after`/`next`), session-local `request` id | `GroupHistoryGet{request, group, after}` → `GroupHistoryPage{days, next, deferred}`; `GroupWant{request, doc, vv}` → `GroupBatch{update, sig, more, deferred}`; push-hint `GroupHistoryChanged` | `RETRY` 5 s resend, `RESCAN` 60 s, `next_issue` spacing 25 ms (`group_history.rs:~175`), one request in flight (`awaiting`) |
| `sync-runtime/.../group_receipts.rs` (240) | yes | hex digest of last proof (`digest(bytes)`), ordered | `GroupReceiptGet{request, group, after}` → `GroupReceiptPage{proofs, next}` | `RETRY` 5 s, `RESCAN` 60 s, `REQUEST_SPACING` 25 ms, one in flight (`waiting`) |
| `group-runtime/.../member_receipts.rs` + `src/member_receipts.rs` | **no**: VV hello/send/ack exchange of the ledger per day | none (VV per day) | `ReceiptHello`/`Receipts`/`ReceiptAck` | no timer; `receipt_waiting` re-hello when docs advance |
| `chat-runtime/.../receipts.rs` | **no**, same as above (candidate 2a) | none | same | none |
| `chat-runtime/.../history.rs` | one-shot request/response, not a state machine: `HistoryReq{before, limit<=14}` → `HistoryResp{days}` through a `oneshot` in `sess.hist`, 30 s timeout (`history.rs:59-110`, `158-183`) | `before: day` | 2 | none |
| `group-runtime/.../history.rs` `Days{after, days}` | push-style inventory pages, ended by an empty page; no request id, no waiting | day | 1 | `INVENTORY_WAIT`, pass wait 3 s / 300 s |

So there are **two** true copies, and they are the same program: both keep `scopes` (set), a work `queue` of `(group, cursor)` (`VecDeque<(String, Option<String>)>` vs `queued: VecDeque<String>` + `resumes` map), `serial` request id with overflow error, one `Waiting`/`Awaiting` with `sent: Instant`, `delay()` computing the next due time (`sent+RETRY` | `issue` | `scan+RESCAN`), `pump()` with the same three-step body (resend if overdue; honour spacing; rescan if idle; pop next; send; set waiting; set spacing), a `refresh`/`changed` re-enqueue that avoids resetting a pass in progress (`group_receipts.rs:62-73` vs `group_history.rs:~112-120`), and the same response validation (request id match, scope match, bound checks, cursor strictly monotone, `next == last item key`, `deferred` implies empty).

Differences: history has a second per-page phase (docs to fetch, each with `GroupWant`/`more` continuation, `covers()` skip, rotation of the current scope at page boundaries) and carries a `deferred` flag; receipts apply each proof of the page immediately and may re-enqueue the same group with `next` as cursor, with "Inspection advances, NOT acceptance" semantics.

### 3.2 Proposed `PagedPull` (module in `sync-runtime`, or `peer-session::pull`)

```rust
pub trait PullKind {
    type Scope: Ord + Clone;        // group id
    type Cursor: Ord + Clone;       // day, digest
    type Req: Clone;                // the frame to (re)send while waiting
    const RETRY: Duration;          // 5 s
    const RESCAN: Duration;         // 60 s
    const SPACING: Duration;        // 25 ms
    fn page_req(id: u64, s: &Self::Scope, after: Option<&Self::Cursor>) -> Self::Req;
}

pub struct PagedPull<K: PullKind> {
    pub enabled: bool,
    scopes: BTreeSet<K::Scope>, queue: VecDeque<(K::Scope, Option<K::Cursor>)>,
    dirty: BTreeSet<K::Scope>,                   // "refresh": finish the current pass first
    waiting: Option<Waiting<K>>, serial: u64, scan: Option<Instant>, next_issue: Option<Instant>,
}
pub enum Step<R> { Idle, Resend(R), Send(R) }
impl<K: PullKind> PagedPull<K> {
    pub fn add_scope(&mut self, s: K::Scope, cap: usize) -> bool;     // offer()
    pub fn refresh(&mut self, s: &K::Scope);                          // refresh()/changed()
    pub fn delay(&self) -> Duration;
    pub fn poll(&mut self, now: Instant) -> Result<Step<K::Req>, Error>;      // the body of pump()
    /// Validates a page response against the one in flight; Ok(None) = stale (ignore).
    pub fn on_page<I: Keyed<K::Cursor>>(&mut self, id: u64, s: &K::Scope, after: Option<&K::Cursor>,
        items: &[I], next: Option<&K::Cursor>, max_items: usize) -> Result<Option<Accepted<K>>, Error>;
    pub fn finish_page(&mut self, a: Accepted<K>);   // clears waiting; requeues (scope,next) or runs `dirty`
}
```
`Accepted` is a token proving the page validated (request id, scope, cursor monotone, `next == last key`, bounds), so the caller cannot forget a check. The send path (`own_group_admit` + `writer.send`) stays in the runtime's `pump`: `match pull.poll(now)? { Send(r)|Resend(r) => { admit; writer.send(&r).await? } Idle => {} }`.

`group_receipts.rs` rewritten on it (about 110 lines instead of 240):

```rust
pub struct ReceiptKind;
impl PullKind for ReceiptKind {
    type Scope = String; type Cursor = String; type Req = SelfMsg;
    const RETRY: Duration = Duration::from_secs(5); const RESCAN: Duration = Duration::from_secs(60); const SPACING: Duration = Duration::from_millis(25);
    fn page_req(request: u64, group: &String, after: Option<&String>) -> SelfMsg {
        SelfMsg::GroupReceiptGet { request, group: group.clone(), after: after.cloned() }
    }
}
pub type Receipts = PagedPull<ReceiptKind>;

pub async fn pump(r: &mut Receipts, inner: &Arc<SyncRt>, me: &Me, remote: &[u8; 32], w: &mut FrameWriter) -> Result<(), Error> {
    inner.group.own_group_admit(me.epoch, remote)?;
    match r.poll(Instant::now())? { Step::Idle => {}, Step::Send(m) | Step::Resend(m) => w.send(&m).await? }
    Ok(())
}
pub async fn handle(inner: &Arc<SyncRt>, me: &Me, remote: &[u8; 32], peer: &mut PeerChat, msg: SelfMsg, w: &mut FrameWriter) -> Result<(), Error> {
    if !peer.receipts.enabled { return Err(Error::Protocol("group receipts were not negotiated".into())); }
    inner.group.own_group_admit(me.epoch, remote)?;
    match msg {
        SelfMsg::GroupReceiptGet { request, group, after } => {           // serving side unchanged: ~20 lines
            validate_get(request, &group, after.as_deref())?;
            let proofs = inner.group.own_receipt_page(me.epoch, remote, &group, after.as_deref())?;
            let next = proofs.last().map(|(_, b)| digest(b));
            inner.group.own_group_admit(me.epoch, remote)?;
            w.send(&SelfMsg::GroupReceiptPage { request, group, after, proofs, next }).await?;
        }
        SelfMsg::GroupReceiptPage { request, group, after, proofs, next } => {
            let Some(page) = peer.receipts.on_page(request, &group, after.as_ref(), &proofs, next.as_ref(), PROOFS_PER_PAGE)? else { return Ok(()) };
            for (name, bytes) in proofs {                                  // per-page work: the only protocol-specific part
                accept_proof(inner, me, remote, &group, &name, &bytes)?;
            }
            inner.group.own_group_admit(me.epoch, remote)?;
            peer.receipts.finish_page(page);
        }
        _ => return Err(Error::Protocol("unexpected receipt message".into())),
    }
    Ok(())
}
```
`Keyed` for `(String, Vec<u8>)` yields `digest(bytes)`; `accept_proof` is the existing match on `AttestationError::{PendingMetadata, Authority} => {}`.

History on `PagedPull`: its second phase (docs to want, `GroupWant`/`more`) is a different request kind on the same wait/retry machinery. Model it as `Req = SelfMsg` already general in the sketch (the waiting entry holds any frame), keep `current: Option<Scope>`, `docs`, `continue_doc` in `History` as the "per-page job", and let `History::pump` call `poll` only for the page request. Do **not** force history's want-loop into the generic type. Expected: history 455 → about 320 lines.

### 3.3 Numbers, risks, tests, format

- **Replaced:** `group_receipts.rs` 240 + `group_history.rs` 455 = 695. **New:** shared `PagedPull` about 150, receipts about 110, history about 320 = **580**. Net **about -115**.
- **Risk:** low. Pure state machine, easily unit-tested with a fake clock (today `Instant::now()` is called inside; the new `poll(now)` makes it deterministic). Watch the `refresh` rule ("finish the existing pass before rescanning; hot scopes must not repeatedly reset the archive cursor") and the receipts-specific "inspection advances, not acceptance". `history` rotates the scope at page boundaries (`resumes`); keep it in `History`.
- **Tests:** `core/src/sync/engine/group_history/*` and `group_receipts/*` fault tests, `sync-runtime/src/engine/group_contents/tests.rs`, `core/tests/group_g2.rs`, `public_group.rs`, `devices.rs`, `message_receipts.rs`. Add a table test for `PagedPull` (stale response ignored, non-monotone cursor rejected, `next != last` rejected, refresh during pass, resend after `RETRY`, rescan after `RESCAN`).
- **Format change:** none needed. Proposed default (no back-compat): collapse `GroupHistoryGet/Page` and `GroupReceiptGet/Page` into `PullGet{kind, request, scope, after}` and `PullPage{kind, request, scope, after, items, next}`, drop `deferred`, and replace the fake capability docs (section 6). Saves about 20 enum-variant lines and the per-kind validators.
- **Migration:** receipts first (smaller, 240 lines), run the existing fault tests, then history. Both sides of the stream are the same code in the same crate, so no side-by-side is needed; gate by the capability negotiation until the generic messages land.

---

## 4. What the chat and group `member_receipts` "paged pull" actually shares

Candidate 3 asked to include them. They are not cursor-paged; they are the VV hello/send/ack exchange of the receipts ledger (section 2.3a). Group's `Days` is a push inventory in pages (`history.rs:132-142`), where the *page* is only a size bound and the waiting protocol is a polling loop (`INVENTORY_WAIT`, `300 s` pass wait). It could be expressed as `PagedPull` only after moving group to request/response pages (a protocol change: "peer pulls my inventory" instead of "I push it"), which is not worth it.

---

## 5. Recommended order

1. **Framed stream generic** (`FrameReader<M>/FrameWriter<M>`). 75 lines out, no risk. Do first; it unblocks everything.
2. **Receipt exchange shared (2a)** into `receipts-core`: highest similarity, fixes the chat Ack-replace defect, tests exist (`message_receipts.rs`).
3. **`PagedPull` (3)** in sync-runtime, receipts then history. Deterministic unit tests, low risk.
4. **Doc registry for `self_loop` (2b)** in sync-runtime, together with replacing the fake capability docs by a real `caps` field (section 6).
5. **Session kit (1)**: `Registry` + `Dialer` + `Acceptor` in **sync**, then **chat**; `run()` loop only after those two pass `idle_close.rs`. Group last, kit pieces only.
6. Stop there. `Replicated` for chat/group doc paths and a unified `Handler` loop for group are not worth their risk.

## 6. No-back-compat clean-ups that the shared design wants (default proposals)

These are format/storage changes; each deletes code.
1. **Delete the old-ALPN machinery**: `version.rs` `Versioned.old`, `old_alpns`, `superseded`, `reason`, `refuse`, the extra ALPNs in `bind_private` (`lifecycle.rs:158-160`), the `refuse` branch of the accept loop (`lifecycle.rs:213-222`), the `wipe` hint text, and `version::tests`. About 110 lines. ALPNs restart at `/1` (e.g. `tinline/chat/1`). `connect()` becomes `ep.connect(addr, alpn)`.
2. **A real `caps: Vec<String>` in every Hello** instead of capability markers smuggled as fake docs with an empty VV (`cap/group-history/v1` in `Hello.docs`, `supported()` x3, `catalog()` reserving "three slots", `MAX_OWN_DOCS + 4` in `sessions.rs`, "renegotiation" checks). About 60 lines and one class of bugs.
3. **`SessionAuth` frame** replacing chat's `call_hello` reuse (`session.rs:231-249`).
4. **One timeout set**: dial 20 s, hello 15 s everywhere (sync's 10 s dial is accidental).
5. **Generic `PullGet/PullPage`** (section 3.3) and `Have/Update/Ack` vocabulary (2.2) if the helpers land.
6. Storage: none required by these abstractions. (The earlier audits' storage items are independent.)

## 7. Optional `Node` API changes (listed separately, none required)

The abstractions are internal; `Node` stays identical. Options a cleaner design could take, with app-side impact:
- **None needed for sessions/replication/pulls.**
- Optional: expose `Node::peer_sessions() -> Vec<PeerSessionInfo{kind, peer, since, idle_for}>` (diagnostics, built from the three registries; today an ad hoc test hook `chat_doc_peers_for_test` etc.). Adds one UniFFI record + method; Kotlin/desktop/iOS call sites: none unless the debug screen wants it.
- Optional: remove the five `gcall_*` dead pass-throughs (glue audit); no app impact.
- If `group_set_backfill_paused` (unused in Kotlin per the glue audit) is dropped, the `backfill_paused` flag in `group_days_pass` goes: no app impact.

## 8. Overall verdict

The "shared abstractions" rewrite is worth it **selectively**. Three things are cheap and clearly good: the generic framed stream, the shared receipt exchange (it exposes a real behavioural divergence between chat and group), and the `PagedPull` state machine. A fourth, the doc registry in sync-runtime's `self_loop`, makes the one 270-line function readable and turns "add a doc" into one `impl`. The peer-session kit is useful as a set of small parts (registry with dialer tie-break, dialer with backoff, acceptor with flood limits, idle policy) that fix the real duplication and make the six concurrency rules explicit and testable once; it is not worth forcing chat, group and sync into one `Protocol` loop, because their loops differ (datagrams and heartbeats, a non-blocking relay writer, change-watch plus pump timers) and the work inside the loops is 80% protocol. A single generic `Replicated` push/pull loop does not fit: triggers, export rules (own ops versus relayed original records) and authorization differ per runtime, and the after-apply side effects dominate the code. Total effect: roughly **-550 to -850 net lines** across the five runtimes from the shared pieces (about 1,150 + 340 + 490 + 695 replaced, about 930 + 330 + 320 + 580 written), up to about **-1,400** if the compatibility machinery in section 6 is deleted and the chat/group doc paths follow. Out of about 33k lines that is 2 to 4%. The bigger payoff is behavioural: one tie-break, one timeout set, one idle rule, one ack-merge rule, one place to test each. Do not expect the middle layer to halve; the weight is in protocol-specific apply and after-apply logic, which no shared abstraction removes.
