# Runtime split: moving the engines out of p2pcore

Status: design study, 2026-10-22. Nothing built. Read against `dev` at `fd626954` (the
crate-split merge). All paths are relative to `crates/core/src/` unless they say otherwise.

**Goal.** p2pcore (`crates/core`) becomes the uniffi facade plus the code that wires the
engines together. The runtimes move into plain-Rust crates. Rules:

- no uniffi in the lower crates;
- each lower crate has its own error type, mapped in p2pcore;
- p2pcore re-exports old paths so they keep working;
- no behaviour or wire change;
- the Kotlin and Swift bindings stay byte-identical;
- `crates/peer` and `crates/desktop` keep calling `Node` directly.

## 0. How the numbers were taken

- **Field use.** A regex counts `self.<field>` / `inner.<field>` (and `self.inner.<field>`)
  for every `Inner` field. It skips method calls and non-test files (`tests.rs`, `tests/`,
  `*_tests.rs`, `faults.rs`, `*_sim.rs`, `testkit.rs`).
- **Cross-area calls.** The script lists every function in an `impl Inner` block, finds which
  area defines it, then counts `self.f(`/`inner.f(` calls coming from other areas.
- **Accuracy.** Counts are within about ±10%. A few `inner.x` hits are local variables: the
  `voice: pending:7` hit is one, and is ignored.
- **Size of the engine and facade code.** `impl Inner` blocks cover **13.5k lines** in 58
  blocks. `impl Node` blocks (the facade) cover **3.5k lines**. Group engine 4.1k, chat 2.9k,
  node 2.4k, sync 1.6k, profile 0.7k, search 0.4k, gcall 0.3k, devlist/video/contact_req/caps/
  block about 0.2k each.
- **Test-only code.** `group/engine/durability/*` (3.2k lines) is `#[cfg(test)]`
  (`group/engine.rs:45`). It is not counted as runtime. Group runtime outside tests is about
  **9.2k lines**.

`Inner` is defined at `node/mod.rs:165-212` and built at `node/api_identity.rs:20-52`. The
engines are fields on it: `GroupRt` (`group/engine.rs:87`), `ChatCore`
(`chat/engine/mod.rs:67`), `SelfSync` (`sync/engine.rs:79`), `LinkState` (`sync/link.rs:53`),
`gcall::net::Service`, `BlockRt`, `ReqRt`, `CapsRt`, `DevlistRt`, `RotationRt`, `NodeVideo`
(`video_call.rs:179`). They are all reached through `impl Inner` methods spread over every
module.

## 1. Dependency map

### 1.1 Inner fields used, by area (non-test, approximate)

| Area (files) | Lines | `impl Inner` fns | Inner fields touched (count) |
|---|---:|---:|---|
| node core (`node/*` minus rotation) | 3.9k | 62 | shared 67, events 25, live 14, handle 14, accounts 13, lifecycle 5, gate 4, closing 3, gcall 3, writing 2, pending 2, group 2, requests/rotation/reserved/fail_writes 1 |
| rotation (`node/rotation.rs`) | 506 | 12 | shared 15, rotation 13, closing 2, events 2, handle/live/lifecycle/gate/selfsync 1 |
| block (`block.rs`) | 425 | 8 | shared 10, block 6, events 3, handle 1 |
| caps (`caps.rs`) | 388 | 9 | shared 12, events 3, caps 2, handle 1 |
| devlist (`devlist.rs`) | 296 | 7 | shared 8, devlist 2, handle 1 |
| contact_req (`contact_req.rs`) | 455 | 7 | shared 10, requests 6, handle 3, events 2, writing 1 |
| chat engine (`chat/*`) | 3.75k | 126 | shared 30, handle 7, chat 7, selfsync 2, chat_events 2, live 1 |
| profile (`profile/*`) | 1.05k | 23 | shared 19, profile_events 2, events 2, profile_downloads 2, handle 2, profile_fetches 1 |
| sync (`sync/engine*`, `sync/api.rs`) | 1.8k | 35 | shared 22, selfsync 19, handle 4, sync_tx 3, link_events 2, events 2, accounts 1 |
| sync-group (`sync/engine/group_{contents,history,receipts}`) | 1.1k | 0 (free fns taking `&Arc<Inner>`) | group 2 |
| link (`sync/link.rs`) | 528 | 18 | link 9, shared 7, handle 3, closing/gate/accounts 1 |
| group engine (`group/*` minus calls) | 8.0k | 106 | group 37, handle 17, shared 12, live 6, gcall 1 |
| group calls (`group/calls*`) | 1.2k | 23 | group 36, gcall 12, handle 1, shared 1 |
| gcall net (`gcall/net/*`) | 1.47k | 19 | gcall 23, handle 6 |
| video_call (`video_call*`) | 4.0k | 11 | video 9, handle 4, live 1 |
| search (`search/api.rs`) | 658 | 18 | shared 5, search 3, handle 3, search_skip 2, group 1 |
| receipts glue (`receipts/*`) | 408 | 0 | none (facade: `impl Node`) |
| cues, voice, avatar | 1.1k | 0 | none (pure, but they hold uniffi exports) |

Almost every engine reads `shared`, the account session (`Shared`, `node/mod.rs:112`), and
mostly its `state` (the persisted contact list), `endpoint` and `epoch`. Reads of
`shared.lock().X` outside `node/` are mostly `state` (about 25 in group/engine, 11 elsewhere),
then `endpoint`, `epoch`, `acct` and `profile`.

### 1.2 Cross-area calls (caller → callee: count, top methods)

Every area calls node core: `me()` (about 120 calls in total), `log()`, `endpoint()`,
`persist*`, `addr_for`, `now_ms`. That is the base layer. The calls that are not to node
core:

```
node     → chat 13 (chat_open/close/sessions_close/call_activity/started/close_now)
         → block 12, sync 8 (acc_open, sync_kick, sync_local, selfsync_started)
         → video 8 (video_enabled/offer/decided, remote_camera/codec)
         → group 6 (group_close, group_letter_incoming, group_started, group_invites_started)
         → devlist 5, caps 5, rotation 3, contact_req 3, gcall 3 (gcall_shutdown), profile 2, link 1
rotation → node 32, chat 3 (handle_chat_conn, chat_sessions_close, chat_kick_pending),
           group 2 (group_core, group_rotation_policy), devlist 2, sync 2 (sync_kick)
block    → node 15, chat 4 (chat_close_sessions, chat_kick, chat_pending)
caps     → node 14
devlist  → node 23, block 3, rotation 2 (touched), profile 1 (profile_for_new_contact)
contact_req → node 13, block 4, devlist 3, rotation 1
chat     → node 66, profile 13 (profile_pending/ack/download/apply_deletions/receive…),
           search 6 (search_dm/close/open/reconcile), block 3, caps 2, sync 1 (sync_bump), devlist 1
profile  → node 17, chat 7 (chat_hub 5, chat_kick, chat_profile_changed), sync 3 (sync_bump)
sync     → node 28, chat 20 (chat_core 7, chat_own_apply/docs/export/shard/writer),
           group 9 (group_own_*, own_group_admit), link 8, profile 5, devlist 3
sync-group → group 21 (own_group_admit 11, group_own_local_vv/history_context/page…), sync 1
link     → node 11, sync 7 (link_events 5, sync_kick, acc_open), devlist 1
group    → node 31, search 7 (search_group/forget/reconcile_groups), chat 5 (selfsync_dirty 3,
           chat_core 1 [facade, group/api.rs:358], is_own_device 1), block 3, group-calls 3, caps 1, gcall 1
group-calls → gcall 17 (gcall_status/join/host/end/observe/leave/set_cond), group 9, node 9
gcall    → node 4 (me, with_no_call, not_in_call), group 4 (group_endpoint 2, group_core/ensure in the facade)
search   → chat 5 (chat_core, load_shard), group 4 (group_core, group_name_of), node 4
video    → (none; it reads `self.video`, `self.live`, `handle`)
receipts → chat 4, group 1   (facade only)
synth    → group 7, chat 1, search 1, node 2  (dev-tools)
```

### 1.3 Cycles

1. **node ↔ everything.** Node core calls into every engine in four places:
   - the start sequence: `chat_started`, `selfsync_started`, `notices_started`,
     `welcomes_started`, `group_started`, `tokens_started`, `private_rotation_started().await`,
     `lists_started`, `group_invites_started` (`node/lifecycle.rs:178-189`);
   - routing incoming connections by ALPN (`node/lifecycle.rs:286-312`);
   - routing proto letters (`node/accept.rs:226-250`: welcome, notice, tokens, devices,
     group letter);
   - stop and close (`node/mod.rs:222-247`).

   Every engine calls back into node for `me`, `log`, `persist` and `endpoint`. This is the
   main cycle. Hooks break it (§3).
2. **chat ↔ profile.** Profile pictures ride on chat sessions and the chat blob hub. 13 calls
   go one way and 7 the other. `chat_hub` gates blobs with both `profile_may_serve` and
   `chat_may_serve` (`chat/engine/blobs.rs:15`). This is one runtime split across two
   modules.
3. **sync ↔ chat / group / profile.** Sync orchestrates the own-device docs of chat, group and
   profile (sync→chat 20, sync→group 30, sync→profile 5). The calls back are small:
   - chat→`sync_bump` once and profile→`sync_bump` 3 times;
   - group→`selfsync_dirty` 3 times. `selfsync_dirty` is defined in `chat/own.rs:73`; it
     writes into `selfsync.sessions` and then calls `sync_bump` (`sync/engine/account.rs:17`:
     `sync_tx.send_modify` plus `selfsync.kick.notify_one()`).
4. **search ↔ chat / group.** Chat and group push index rows (`search_dm`, `search_group`,
   `search/api.rs:296,310`). Search reconcile reads `chat_core` and `group_core`. The
   write side is a sink; the reconcile side sits above both.
5. **group ↔ gcall net.** Group-calls drives the gcall service (17 calls). Gcall uses
   `group_endpoint()` (`gcall/net/link.rs:14`, `gcall/net/node.rs:134,343`), and a `Session`
   keeps a `Weak<Inner>` (`gcall/net/session.rs:64`). `CallDirectory` is already a trait
   (`gcall/net/config.rs:39`, implemented by `group/calldir.rs:100`).
6. **node ↔ gcall through the call slot.** `with_no_call` holds `live` while gcall starts
   (`node/persist.rs:151`, used at `gcall/net/node.rs:139`). `outgoing.rs:60` checks
   `gcall.in_call()` under `live`. The lock order is **live → gcall session**.
7. **contacts cluster.** block, caps, devlist, contact_req and rotation all call node and each
   other: devlist→block/rotation, contact_req→block/devlist/rotation. They all mutate `State`
   through `persist*`. Treat them as one layer together with node core.

## 2. Leaf runtimes that can move now

### 2.1 Video call runtime → crates `video` + a new crate `call-video`

Today the video call code is split like this:

- **`video_call.rs` (593 lines).** Records (`VideoInfo`, `VideoFrame`), the `VideoEvents`
  foreign trait (`:173`), `NodeVideo` (`:179`), `Pipe` (`:218`), `CallVideo` (`:267`), the
  `impl Inner` glue (`:372-505`, `:531-593`) and the frame-port registry for JNI (`:511-530`).
- **`mediacodec.rs` (1626 lines).** It only uses `video::{EncoderConfig, Error, I420, Packet,
  Result, VideoDecoder, VideoEncoder}` (`mediacodec.rs:31`) plus `ndk-sys`. The one
  exception is `crate::Error` in the selftest wrapper (`:1624`).
- **`codecs.rs` (286 lines).** Encoder and decoder choice. It uses `video` and `mediacodec`.
- **`pipes.rs` (373), `path.rs` (51), and most of `live.rs`, `adapt.rs`, `tuning.rs`.** These
  are internals of `Pipe` and `CallVideo` plus `#[uniffi::remote(Record)]` /
  `#[uniffi::export] impl Node` blocks.

What the video code needs from `Inner`: `video` (9), `handle` (4) for spawning the send and
receive tasks in `video_decided` (`:406`), and `live.lock().call` (`:490`), which is the
current `Call`. `Call.video: CallVideo` is defined in node (`node/call.rs:51`). There are no
calls into other engines.

Plan:

- **V1a.** Move `mediacodec.rs` into `crates/video/src/mediacodec.rs` behind
  `video/mediacodec = ["dep:ndk-sys"]`. p2pcore keeps `mediacodec = ["video/mediacodec"]` so
  the Gradle flag stays the same. The selftest becomes a two-line `#[uniffi::export]` in
  p2pcore that maps `video::Error` to a string. About 1.6k lines move.
- **V1b.** New crate `crates/call-video` (deps: `video`, `video-call-core`, `proto`, `iroh`
  for `Connection`, `tokio`, `parking_lot`). It takes `CallVideo`, `Pipe`, `Timing`,
  `pipes.rs`, `codecs.rs`, `path.rs` and the non-export halves of `live`, `adapt` and
  `tuning`, plus `NodeVideo` with a plain sink in place of the uniffi trait:

```rust
// crates/call-video
pub trait VideoSink: Send + Sync { fn on_video_state(&self, info: VideoInfo); }
pub struct NodeVideo { enabled: AtomicBool, tuning: Mutex<VideoTuning>,
                       sink: Mutex<Option<Arc<dyn VideoSink>>>, dispatch: Mutex<Option<mpsc::Sender<..>>> }
impl CallVideo {
    pub fn decide(&self, handle: &Handle, conn: &Connection, ours: Option<VideoOffer>,
                  theirs: Option<VideoOffer>, nv: &NodeVideo) -> Decided;   // was Inner::video_decided
    pub fn pull_frame(&self) -> Option<RawFrame>;     // RawFrame { w, h, data: bytes::Bytes, seq }
    pub fn push_yuv_planes(&self, ..);
}
```

p2pcore keeps these parts:

- the `#[uniffi::export] impl Node` video API (about 330 lines);
- `VideoFrame` with its custom `Bytes` FFI type (`video_call/ffi_bytes.rs`), converted from
  `RawFrame` with a move and no copy;
- `#[uniffi::remote(Record)]` for `VideoInfo`, `VideoStats`, `VideoLive` and the rest, now
  defined in `call-video`. Remote records already work today: `video_call.rs:90`,
  `live.rs:26`, `tuning.rs:6`;
- `struct VideoEventsAdapter(Arc<dyn VideoEvents>)`, which implements `VideoSink`;
- `frame_port` and `video_jni.rs`.

Estimate: V1a about 1.6k lines and 3 files. V1b about 1.4k lines and 8 files. About 1.1k
lines stay in p2pcore.

Risks:

- the feature matrix (`vp8`, `mediacodec`, `target_os = "android"` with `ndk-sys`, desktop
  `vp8`) has to be reproduced;
- the `#[cfg(feature = "dev-tools")]` hooks in `mediacodec.rs` move with it;
- `pub(crate)` constants such as `MAX_PIXELS` are read from node.

Conflict with `feature/public-group`: none. `feature/ios-app` only changes `crate-type` in
`crates/core/Cargo.toml`. **Lowest risk; do it first.**

### 2.2 Group-call network service → new crate `gcall-net`

`gcall/net/*` is 1.47k lines of plain Rust; it already says it has no uniffi
(`gcall/net.rs:4`). It needs:

- `self.gcall` (its own `Service`);
- `handle`;
- `me()` (for the `Identity` only, `gcall/net/node.rs:98,133`);
- `group_endpoint()`;
- `with_no_call` / `not_in_call` (`node/persist.rs:142-160`);
- `now_ms`;
- `Error` variants `Busy`, `NotStarted`, `Timeout`, `Rejected`, `Protocol` and `Net`.

The `impl crate::Node` block (`gcall/net/node.rs:356-440`, about 84 lines) is plain Rust. It
is called by `tests/gcall.rs` and `tests/calls.rs` (`p2pcore::gcall::net::{GcallConfig,
GcallEvent, GcallEvents, StaticDirectory}`) and stays in p2pcore.

```rust
// crates/gcall-net
pub trait GcallHost: Send + Sync + 'static {
    fn identity(&self) -> Result<Identity, Error>;
    fn group_endpoint(&self) -> Option<Endpoint>;
    /// Runs `f` while no 1:1 call holds the slot (lock order: live → session), else Err(InCall).
    fn with_no_call(&self, f: &mut dyn FnMut() -> Result<(), Error>) -> Result<(), Error>;
}
pub struct GcallNet { svc: Service, handle: Handle, host: OnceLock<Weak<dyn GcallHost>> }
impl GcallNet {
    pub fn host(&self, cfg: GcallConfig) -> Result<(), Error>;       // was Inner::gcall_host
    pub fn join(..), leave(), end(), status(), push_mic(..), pull_speaker(), in_call(), busy(),
        cond(), configure_if_unset(..), observe_start(..), shutdown().await, incoming(Accepting).await …
}
pub enum Error { InCall, Busy, NotStarted, Timeout, Rejected(String), Protocol(String), Net(String) }
```

- `Session.inner: Weak<Inner>` becomes `Weak<dyn GcallHost>`.
- p2pcore implements `GcallHost for Inner` (about 20 lines) and installs it after building
  `Arc<Inner>`.
- `gcall_new_session` hands the session the `Weak`, as it does today.
- `with_no_call` takes a `&mut dyn FnMut` so the trait stays object-safe. The generic
  wrapper stays in node.

Estimate: about 1.35k lines move. 7 files move, and 8 files are edited: `group/calls*` (23
call sites become `self.gcall.x(..)` in place of `self.gcall_x(..)`), `node/call.rs:376,406`,
`node/outgoing.rs:60` and `node/persist.rs:250`.

Risk: low to medium.

- The lock order must hold: `with_no_call` runs the closure while `live` is locked.
- `public-group` does not touch `group/calls*` or `gcall/*`. It does touch
  `group/engine/lifecycle.rs`, but only 2 lines and not `group_endpoint`.

Can be done before public-group lands.

### 2.3 Not worth moving now

- **cues, voice, avatar, licences, recovery_key.** No `Inner` use, but they are uniffi
  surface. Moving their records would need `remote` declarations, and moving their
  objects/exports would change the bindings. Moving them to `present` would put them in
  `uniffi/present/present.kt` (`lib.rs:8`), which breaks "bindings identical". They are not
  runtime; leave them.
- **receipts glue (`receipts/*`, 408 lines).** Facade over chat and group plus the tick
  maths. It stays in p2pcore.

## 3. Node context design

### 3.1 `crates/node-runtime` contents

Node-runtime is the base every engine stands on. It holds the account session, the main
endpoint, the 1:1 call slot and the contacts cluster:

```rust
pub struct NodeCtx {
    pub handle: tokio::runtime::Handle,
    pub accounts: AccountDirs,
    pub gate: Mutex<()>,                       // create/commit/unlock
    pub writing: Mutex<()>,                    // state writes; before `shared`, never inside it
    pub shared: Mutex<Shared>,                 // Shared, Me, Acct move here unchanged
    pub live: Mutex<Live>,                     // 1:1 call slot (Call holds call_video::CallVideo)
    pub pending: Arc<Semaphore>, pub reserved: Arc<Semaphore>,
    pub lifecycle: Arc<tokio::sync::Mutex<()>>, pub closing: Arc<tokio::sync::Mutex<()>>,
    pub fail_writes: AtomicU32,
    pub sink: Arc<dyn NodeSink>,               // plain mirror of NodeEvents
    pub signal: OwnDocSignal,                  // sync_tx + kick + dirty sets (see 3.4)
    pub block: BlockRt, pub requests: ReqRt, pub caps: CapsRt, pub devlist: DevlistRt,
    pub rotation: RotationRt,
    pub video: call_video::NodeVideo,
    hooks: OnceLock<Weak<dyn NodeHooks>>,      // the engines above, installed by p2pcore
}
```

It also contains:

- `persist*`, `me()`, `endpoint()`, `log()`, `addr_for`, `now_ms`;
- `on_core_thread()` and the `USER_CALL` task-local (`node/util.rs:56-68`). These **must**
  live here so every crate sees the same task-local key;
- `wire::Ctrl` and `frames`;
- accept, call, outgoing, run, contacts, devices, unlock, rotation, block, caps, devlist and
  contact_req.

Estimate: about 6.5k lines (node 3.9k, rotation 0.5k, contacts cluster 1.6k, support 0.4k).

It depends on `call-video` and the model crates (`account-model`, `proto`, `identity`,
`storage`, `transfer` if needed). It does not depend on `gcall-net`; it gets `in_group_call`
from a hook instead.

### 3.2 How engines get the context

Each engine is its own struct holding `Arc<NodeCtx>`, and its methods move from
`impl Inner` to `impl ChatRt` / `impl GroupRt` / `impl SyncRt`. For a mechanical,
low-churn first move, give each engine struct `Deref<Target = NodeCtx>`:

```rust
pub struct ChatRt { ctx: Arc<NodeCtx>, core: Mutex<Option<Arc<ChatCore>>>,
                    sink: Mutex<Option<Arc<dyn ChatSink>>>, search: Arc<SearchIx>, profile: ProfileRt }
impl Deref for ChatRt { type Target = NodeCtx; fn deref(&self) -> &NodeCtx { &self.ctx } }
```

With that, `self.shared.lock()`, `self.me()?` and `self.log(..)` in the moved bodies compile
unchanged. The same trick works in p2pcore during the transition: `Inner { ctx:
Arc<NodeCtx>, chat, group, sync, gcall, search }` with `Deref<Target = NodeCtx>`, so the
engines still written as `impl Inner` keep compiling while node-runtime is extracted (W3)
and before they move themselves.

**Caveat.** There are 260 non-test methods with `self: &Arc<Self>` / `self: Arc<Self>`
receivers; 54 of them are in node and the contacts cluster. Auto-deref does not turn
`&Arc<Inner>` into `&Arc<NodeCtx>`, so calls such as `self.persist_in_background(epoch)` from
an `Arc<Inner>` need `self.ctx.persist_in_background(..)`. That is about 60 to 80 call sites.
The compiler finds them all.

Deref is a migration aid, not the end state. A later cleanup can replace it with explicit
`self.ctx`.

### 3.3 Breaking node → engines: one hooks trait, implemented in p2pcore

Every edge from node core up to an engine goes through one object-safe trait. p2pcore
implements it on `Inner` and installs it as a `Weak` right after `Arc::new(Inner{..})`
(`node/api_identity.rs:20`). Direct calls keep today's ordering and awaits. Do not use event
channels here: the start sequence awaits `private_rotation_started()` inline
(`node/lifecycle.rs:185`), and stop must flush gcall and close chat before the endpoint goes.
A channel would change that timing.

```rust
#[async_trait::async_trait]   // or boxed futures; ~6 async methods
pub trait NodeHooks: Send + Sync {
    // lifecycle (node/lifecycle.rs:178-189, node/persist.rs:250, node/mod.rs:222-247)
    fn started(&self, epoch: u64);                       // chat/selfsync/group/invites started
    async fn shutdown_calls(&self);                      // gcall_shutdown
    async fn close(&self);  fn close_now(&self);         // chat_close + group_close (+ _now)
    // incoming connections by ALPN (node/lifecycle.rs:286-312) and proto letters (accept.rs:250)
    async fn incoming(&self, alpn: &[u8], a: Accepting, permit: Option<OwnedSemaphorePermit>) -> Result<(), Error>;
    async fn group_letter(&self, ctrl: Ctrl, letter: proto::GroupLetter, remote: PublicKey, me: &Me) -> Result<(), Error>;
    // 1:1 call and rotation needs
    fn in_group_call(&self) -> bool;                     // outgoing.rs:60
    fn chat_call_activity(&self, did: &str);
    async fn chat_conn(&self, c: Connection);  fn chat_sessions_close(&self, did: &str);  fn chat_kick_pending(&self);
    fn group_rotation_policy(&self) -> Option<RotationPolicy>;
    fn profile_for_new_contact(&self, did: &str);        // devlist.rs
    fn link_cancel(&self);  fn acc_open(&self) -> Result<(), Error>;  fn sync_local(&self);
}
```

That is about 18 methods, matching the about 60 node→engine call sites counted in §1.2.

`NodeCtx::hooks()` returns `Option<Arc<dyn NodeHooks>>`. When the `Weak` is gone (the node is
dropping), each caller does what it would do with no engine: return or no-op.

### 3.4 Breaking edges between engines

Engines sit in this order (lowest first):

```
models (crdt, account-model, chat-core, group-core, search-core, receipts-core, gcall-core, transfer, video-call-core)
  └ call-video, gcall-net, search-ix
      └ node-runtime (NodeCtx, contacts cluster, 1:1 call, rotation)
          ├ chat-runtime (chat engine + profile pictures)        uses NodeCtx, SearchIx, signal
          └ group-runtime (engine, calls, invitations, requests)  uses NodeCtx, SearchIx, GcallNet, signal
              └ sync-runtime (self/3, link, group_{contents,history,receipts})  uses chat-runtime + group-runtime directly
                  └ p2pcore: Inner { ctx, chat, group, sync, gcall }, impl NodeHooks/GcallHost, adapters, uniffi facade, search reconcile, synth
```

How each edge from §1.3 is handled:

- **chat + profile: merge into one crate** (`chat-runtime`).
  - They form one runtime: the blob hub gate covers both (`chat/engine/blobs.rs:15`), and
    profile rides on chat sessions.
  - `ProfileEvents` gets a plain `ProfileSink` mirror.
  - `profile_downloads` and `profile_fetches` move into `ProfileRt`.
- **The calls back into sync** (`sync_bump` ×4, `selfsync_dirty` ×3, plus devlist and
  rotation calling `sync_kick`) go through a small signal object in `NodeCtx`:

```rust
pub struct OwnDocSignal { tx: watch::Sender<u64>, kick: Notify,
                          sessions: Mutex<Option<Weak<dyn DirtySink>>> }
impl OwnDocSignal {
    pub fn bump(&self) { self.tx.send_modify(|v| *v += 1); self.kick.notify_one(); }  // == sync_bump
    pub fn dirty(&self, name: &str) { if let Some(s) = self.sink() { s.mark_dirty(name) } self.bump() } // == selfsync_dirty
    pub fn kick(&self) { /* sync-runtime registers its backoff-clearing kick */ }
    pub fn subscribe(&self) -> watch::Receiver<u64>;
}
```

  `sync_tx` already lives on `Inner`, so moving it into `NodeCtx` keeps it in the same place.
  The per-session dirty sets stay in `SelfSync`, and `SelfSync` implements `DirtySink`.
  `selfsync_dirty` is synchronous today and stays synchronous.
- **sync → chat / group / profile: direct dependency, no traits.**
  - Sync is the orchestrator above them.
  - `sync-group` (`own_group_admit`, `group_own_*`) becomes plain calls into `GroupRt`'s
    public own-device API.
  - `chat_own_*` becomes calls into `ChatRt`.
  - `link` moves with sync: it uses `closing`, `gate` and `accounts` (all in `NodeCtx`) and
    `link_events`.
- **search: split in two.**
  - `search-ix` (low, about 250 lines) holds the index slot (`Inner.search`, `search_skip`)
    and the write hooks `search_dm`, `search_group`, `search_forget`, `search_open` and
    `search_close`. These use only `search-core`, `chat-core::MsgRec` and
    `group-core::PostDiff`.
  - Reconcile and the query facade (they read `chat_core` and `group_core`) stay in p2pcore
    or go into a thin `search-runtime` above chat and group.
- **group → chat (`chat_core().store.read_receipts()`).** Only the facade uses it
  (`group/api.rs:358`). It stays in p2pcore. `is_own_device` (`chat/own.rs:37`) is an
  account question and moves to `NodeCtx`.
- **group → block / caps (`is_block_from`, `take_notice`, `refusal_for`, `token_check`).**
  Direct calls to `NodeCtx` (the contacts cluster).
- **gcall ↔ group.** `GcallHost` is implemented in p2pcore over `Inner`, using
  `group.group_endpoint()` and `ctx.live`. Group-runtime depends on `gcall-net` directly.

### 3.5 uniffi callback traits

Each foreign trait gets a plain trait in the crate that emits the events. p2pcore holds a
one-line adapter for each, and the setter (`set_chat_events`, `chat/api.rs:142`) wraps the
foreign object:

```rust
// chat-runtime
pub trait ChatSink: Send + Sync { fn on_message_added(&self, m: Message); /* … same 6 … */ }
// p2pcore
struct ChatEventsAdapter(Arc<dyn ChatEvents>);
impl chat_runtime::ChatSink for ChatEventsAdapter { fn on_message_added(&self, m: Message) { self.0.on_message_added(m) } … }
```

| Foreign trait | Defined at | Plain trait | Crate |
|---|---|---|---|
| `NodeEvents` | `node/types.rs:7` | `NodeSink` | node-runtime |
| `ChatEvents` | `chat/api.rs:121` | `ChatSink` | chat-runtime |
| `ProfileEvents` | `profile/mod.rs:31` | `ProfileSink` | chat-runtime |
| `LinkEvents` | `sync/api.rs:12` | `LinkSink` | sync-runtime |
| `VideoEvents` | `video_call.rs:173` | `VideoSink` | call-video |
| `GroupEvents`, `GroupRequestEvents`, `GroupInvitationEvents`, `GroupCallEvents` | `group/api/types.rs:163`, `group/requests.rs:59`, `group/invitations.rs:86`, `group/calls/types.rs:43` | `Group*Sink` | group-runtime |
| `ContactRequestEvents` | `contact_req.rs:122` | `ContactRequestSink` | node-runtime |

The record types these callbacks carry (`Message`, `Chat`, `DeliveryState`, `CallInfo`,
`CallState`, `NodeStatus`, the `Group*` records…) move to the runtime crates.

- p2pcore redeclares each one with `#[uniffi::remote(Record)]` or `#[uniffi::remote(Enum)]`.
  Precedents: `chat/store.rs:8`, `group/calllog.rs:23`, `group/rotation.rs:9`.
- The compiler checks the redeclaration against the real type, and the generated bindings do
  not change.
- Where a record holds a custom FFI type (`VideoFrame.data: Bytes`), convert at the facade
  instead.

### 3.6 Errors

The existing pattern stays: each crate has its own `Error`, and p2pcore adds a `From` impl
(today `error.rs:48-136` maps 11 crates). New crates name their variants after the p2pcore
variants they map to: `InCall`, `Busy`, `NotStarted`, `Locked`, `NotFound`, `Timeout`,
`Rejected(String)`, `Protocol(String)`, `Net(String)`, `Io(String)`, so each `From` is one
arm per variant with the same `Display` text.

Engines return errors to each other. Those errors should cross crate boundaries **without
being re-wrapped**: an `InCall` must not turn into `Protocol("in call")`.

- Simplest: chat-, group- and sync-runtime reuse `node_runtime::Error`, the full set of
  runtime variants.
- p2pcore maps that one type, and the uniffi `Error` (`#[uniffi(flat_error)]`,
  `error.rs:1-3`) stays where it is.
- Alternative: make `node_runtime::Error` the uniffi error through `#[uniffi::remote(Error)]`.
  Check that uniffi 0.32 accepts `flat_error` on a remote error, and diff the bindings,
  before choosing it.

## 4. Wave order

| Wave | What | Lines moved | Files touched | Conflict: public-group / ios-app | Risk |
|---|---|---:|---:|---|---|
| W1 | V1a mediacodec → `video`; V1b `call-video` crate | ~3.0k | ~14 | none / Cargo.toml only (`crate-type` + `staticlib`) | low |
| W2 | `gcall-net` crate, `GcallHost` | ~1.35k | ~15 | none (no `group/calls*` or `gcall/*` in its diff) / none | low–med |
| W3 | `node-runtime`: `NodeCtx`, `Shared/Me/Acct/Live/Call`, persist, accept/call/run/outgoing, contacts cluster, rotation, `NodeHooks`, `OwnDocSignal`; `Inner` derefs to `NodeCtx` | ~6.5k | ~35 (node/*, block, caps, devlist, contact_req, wire, frames, store glue, plus small `self.ctx.` fixes everywhere) | **medium**: Deref keeps group/engine bodies unchanged, but `lib.rs`, `group/engine.rs` and `group/engine/lifecycle.rs` are in both diffs. **Do after public-group lands.** | med |
| W4 | `search-ix` (index slot + write hooks) | ~0.25k | ~6 | low | low |
| W5 | `chat-runtime` (chat engine + profile), `ChatSink`/`ProfileSink`, records remote | ~4.0k | ~25 | none / none | med |
| W6 | `group-runtime` (engine, calls, invitations, requests, code, own, attestation, calldir, member_receipts) | ~8k + ~1.3k from public-group | ~45 | **must follow public-group** (presence.rs, admit.rs, api/hooks.rs) | high (size) |
| W7 | `sync-runtime` (self/3, account, devices, push, sessions, link, sync-group) | ~3.5k | ~20 | small: public-group touches only sync test files | med |
| W8 | Cleanup: drop Deref where cheap, collapse `impl Inner` remnants, re-export audit | – | many | – | low |

**Order:** W1 and W2 now, in parallel; they share no files except `crates/core/Cargo.toml`.
Status (2026-10-22): W1, W2 and W3 are merged into `integrate/runtime-split`. So is W4
(d2c0b6f8): `SearchIx` builds on `node-runtime`, and `PostDiff` moved to `group-core`. W5
(7d7d5cf9) is in progress: `ChatRt` holds the chat engine and profiles, the plain receipt
records are in `receipts-core::status`, `OwnDocSink::in_session` was added, and
`ChatRt::chat_open` returns whether it opened the store so that p2pcore reconciles search.
W6 is next. See `runtime-split-HANDOFF.md`.

After public-group merges: W3, then W4 and W5 in parallel, then W6, then W7. W3 is the
dependency for everything after it.

Line counts above are non-test runtime code. Unit tests move with their modules (W6 about
3.2k test-only `durability`, W7 about 1.5k test and fault files).

**p2pcore at the end.** About 7–9k non-test lines, down from about 35k:

| Part | Lines |
|---|---:|
| `impl Node` facade | ~3.5k |
| uniffi record/enum definitions and `remote` redeclarations | ~1.5–2k |
| composition (`Inner`, `NodeHooks` and `GcallHost` impls, sink adapters, error mapping) | ~0.8k |
| search reconcile and query facade | ~0.4k |
| receipts glue and ticks | ~0.4k |
| uniffi leaves (voice, cues, avatar, licences, recovery_key, version) | ~1.4k |
| `video_jni` | ~0.1k |
| `synth` (dev-tools) | ~0.2k |
| bins (search-bench, year-bench, uniffi-bindgen) | ~1.1k |

## 5. Risks and open questions

1. **Lock ordering across crates.** There is no lock-order doc. The invariants are only
   comments, and they need to become crate docs (and debug asserts where cheap):
   - `writing` before `shared`, never inside it (`node/mod.rs:172-173`);
   - the `lifecycle` tokio mutex serialises start, stop, lock, set_passphrase and rotation
     (`node/rotation.rs:7`, `node/api_identity.rs:326`);
   - `closing` is held across the endpoint close;
   - **live → gcall session** (`gcall/net/node.rs:138`, `node/outgoing.rs:55-61`);
   - `group.core` is held while opening the store, under the epoch read
     (`group/engine/lifecycle.rs:13-21`);
   - the `parked` flag is read and written under `core`'s lock (`group/engine.rs:109-112`).

   Hooks run with no `NodeCtx` lock held. Audit every `hooks()` call site for this in W3.
2. **Tokio runtime ownership.** `Node` owns the `Runtime` (`node/mod.rs:217-220`) and
   shuts it down in `Drop` (`:222-247`) after `group_close` and `chat_close`.
   - Lower crates must only use `ctx.handle` and must never build a runtime.
   - `block_on`/`USER_CALL` (`node/mod.rs:254-265`) stays in p2pcore. The task-local itself
     must live in node-runtime: `user_call()` is read by group code.
   - Engine tasks today hold `Arc<Inner>`. After the split they hold their own
     `Arc<ChatRt>`/`Arc<NodeCtx>`, and the hooks are a `Weak`. Check that nothing relied on
     a task keeping *all* engines alive. `Node::drop` runs `close_now()` on chat and group
     explicitly, so this is probably fine, but test it with `tests/endpoint_close.rs`,
     `tests/lifecycle.rs` and `tests/idle_close.rs`.
3. **Bindings identical.** Before W1, generate Kotlin and Swift with `uniffi-bindgen`
   (`src/bin/uniffi-bindgen.rs`) and keep the output as a baseline. Every wave diffs against
   it; any change fails the wave. `remote` redeclarations must keep field order and doc
   comments where bindgen emits them. Doc comments are part of the generated Kotlin, so
   check whether remote items carry them.
4. **Test helpers.**
   - `synth.rs` (`pub mod`, dev-tools), `node/testkit.rs`, `chat/engine/synth.rs` and
     `group/engine/synth.rs` call into several engines. Each runtime crate needs a `testkit`
     feature exposing its synth methods. p2pcore's `dev-tools` forwards to all of them, as it
     already does for `group-core/dev-tools` and `account-model/testkit` (`Cargo.toml:31`).
   - The dev-tools test knobs on `GroupRt` (`group/engine.rs:99-103`) move with it.
   - Integration tests (`crates/core/tests/*`, `tests/support/*`, about 1k lines) use only
     `Node` and the re-exported paths, so they stay in p2pcore unchanged.
   - Unit tests that build a whole `Node` to poke engine internals cannot move into an
     engine crate without a `NodeCtx` test builder. Either keep them in p2pcore as
     composition tests (they need `pub` testkit accessors) or add `NodeCtx::for_test`.
     Decide per wave. Two sync test files and 7 of the 34 unit-test files build a `Node`.
5. **Bench bins.** `search-bench` and `year-bench` (`Cargo.toml:15-23`, feature `bench`) drive
   synthetic peers through `Node`. They stay in p2pcore; check they still build under
   `--features bench` after each wave.
6. **`crates/peer` and `crates/desktop`.** Both link p2pcore as a lib (peer with
   `dev-tools`) and only use `Node` and re-exports (`p2pcore::avatar`, `p2pcore::cues`,
   `VideoFrame`, `LinkEvents`, `TransferState`). public-group adds `peer/src/group_host.rs`;
   recheck its imports after W6.
7. **Visibility.** Today engines share `pub(crate)` internals: `GroupCore` fields
   (`group/engine.rs:171-201`), `ChatCore` (`chat/engine/mod.rs:67-97`), `SelfSync` (all
   `pub`). Moving them makes these crate APIs. To avoid freezing them, mark them
   `#[doc(hidden)] pub` and add a lint or doc saying they are not a stable API.
8. **Build cost.** More crates mean more parallel compilation, but p2pcore's cdylib link is
   unchanged. iOS `staticlib` (feature/ios-app) is not affected. Check with
   `cargo check -p p2pcore` per wave, plus the Android `assembleDebug` through `tasks`.
9. **Open: does rotation belong in node-runtime or in p2pcore?** It sequences endpoint, chat,
   group, devlist and sync under the lifecycle lock (`node/rotation.rs`). In node-runtime it
   needs 4 hooks; in p2pcore, devlist and contact_req need a hook back to it (`touched`,
   `endpoint_of`). This doc proposes node-runtime; either works.
10. **Open: profile as part of chat-runtime or its own crate behind a `ChatHub` trait.** This
    doc proposes merging, which needs no new traits.

### Acceptance checks per wave

- `cargo check -p p2pcore`, `--features dev-tools`, `--features bench`, the `mediacodec`
  feature for Android, and `vp8`.
- The integration tests touched by the wave's area, run with named targets (for example W2:
  `--test gcall --test calls --test group_call`).
- The bindings diff is empty.
- `cargo tree -p <new crate> -i uniffi` is empty.
- No public `p2pcore::` path used by `crates/peer`, `crates/desktop` or `crates/core/tests`
  changes. Those crates compile without edits.
