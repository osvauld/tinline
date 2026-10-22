# Glue code and idiom audit: p2pcore vs the runtime crates

Worktree `/home/abe/p2p_chat/.claude/worktrees/rt-w8`, HEAD 88dfe367 plus the uncommitted W8 edits already in the tree (read as found; nothing edited). Counts come from small scripts and are marked "heuristic" where they are. Paths below are relative to `crates/core/src` unless they start with a crate name.

## 0. Headline

- p2pcore is 8.4k non-test lines (12.9k with tests, benches and `sync/engine/*` fault tests). It is not one thin layer. It is two things:
  1. About 130 real forwarders (456 lines, `Node::x -> self.inner.<rt>.x`), plus about 50 more in the `_for_test` hooks (see section 1).
  2. About 1,500 lines of domain logic that never moved because it sits in `#[uniffi::export] impl Node` bodies and reaches into `shared.lock().state` and `core.groups.lock()`. Examples are `group_open_link`, `unlink_device`, `block`, the whole of search, and `group_invite_contacts`. This is the bigger problem: "thin wrapper" and "fat API" sit side by side, so the layering is inconsistent.
- `Inner` is now only a 5-field bundle with `Deref<Target = NodeCtx>`. Its remaining jobs are `impl NodeHooks` (108 lines of forwarding), `chat_open`/`commit_locked`, the search engine `impl Inner` (about 260 lines) and `push_mic`/`pull_speaker`. All of these can move out, and then `Inner` can be flattened away.
- UniFFI-fixed pieces that must stay: the `Node` method signatures, `*Events` traits, `#[uniffi::remote]` record declarations, `Error` (own type with `From` both ways, decided in W8) and `uniffi::setup_scaffolding!`.

Overall numbers:

| Item | Count |
|---|---|
| `self.inner.` accesses in core | about 346 (group 149, shared 58, chat 54, `.clone()` 48, me 26, sync 18) |
| `inner.group.group_core()` calls in core | 71 |
| `let inner = self.inner.clone(); block_on(async move ...)` | 48 (in this form) |
| `)??)` double-unwrap sites | 9 (`block_on` 45, `block_rt` 21) |
| `*EventsAdapter` | 10 impls, about 110 lines (node, chat, profile, group, group-calls, group-invitations, group-requests, link, contact-request, video) |
| `*_for_test` pub fns on `Node` | 91 across 16 files; 14 in `group/api/hooks.rs` are `#[uniffi::export]`, so shipped in release bindings (fixed, section 6) |
| glob "old path" re-exports (`pub use x::*`) | 27 |
| `use super::*` | 41 |
| `allow(unused_imports)` in core | 4 files |

## 1. Thin wrappers (layer by layer)

Layers today: `Node::x` (UniFFI) -> `self.inner.<rt>.x(..)` (Deref/field) -> runtime fn. The `Node -> Inner` hop is just a field access. So the only real "extra layer" is hooks, and the glue cost is `core` lookups plus the `block_on`/`Arc` ritual.

Heuristic forwarder counts (pub fns of at most 6 lines whose body is `self.inner.<chat|group|sync|search>...`, test hooks excluded from the 130):

| File | Forwarders / pub fns |
|---|---|
| `chat/api.rs` | 21/24 |
| `group/api/hooks.rs` | 18/21 (all `_for_test`) |
| `gcall/net/node.rs` | 13/14 |
| `sync/api.rs` | 13/24 |
| `group/api.rs` | 7/22 |
| `group/calls/node.rs` | 7/13 |
| `search/api.rs` | 7/14 |
| `group/requests.rs` | 5/9 |
| `group/api/images.rs` | 3/8 |
| `group/api/presence.rs` | 3/3 |
| `group/api/rotation.rs` | 3/4 |
| `group/invitations/node.rs` | 3/7 |
| `profile/mod.rs` | 2/10 |
| others | 1 to 2 each |

### Which layers can collapse, and which must stay

**Must stay** (the UniFFI boundary and error mapping): every `Node::x`, because it carries the doc comment, the `String` and `u64` shapes and the `Error` mapping. These one-liners are the honest cost of a stable foreign API. Do not try to remove them.

**Can collapse:**

1. **`let inner = self.inner.clone(); Ok(self.block_on(async move { inner.group.group_core()?; ... })??)`**. This appears in 48 places. Fix: a helper `Node::run<T>(&self, f: impl FnOnce(Arc<Inner>) -> impl Future<Output = Result<T, node_runtime::Error>>) -> Result<T, Error>` built on the existing `block_rt` (`node/mod.rs:107`). Then `group_invite`, `group_device_token`, `group_rotate_now`, `calls_ready`, `host_accept` and so on become `self.run(|i| async move { ... })`. It removes the clone line, the `Ok(..??)` wrap and the 9 `)??)`.
   - Size: about -100 lines, no behaviour change.
   - Risk: low. This is **P1**.
2. **`self.inner.group.group_core()?` followed by `self.inner.group.group_admin_check(&core, &gid)?`**. This pair (or `group_member_check`) precedes about 25 writes in `group/api.rs`, `hosts.rs`, `presence.rs` and `rotation.rs`. Fix: runtime methods `GroupRt::admin_core(&self, gid) -> Result<Arc<GroupCore>>` and `member_core(..)`, and `Node` calls `self.inner.group.admin_core(&gid)?`.
   - Size: about -50 lines.
   - Risk: low. **P1**.
3. **Test-hook files**. `group/api/hooks.rs` (165), `sync/api.rs` lines 238 to 400 (about 160 lines, mostly chat hooks living in the sync file: `forged_own_batch_for_test`, `chat_doc_peers_for_test`, `write_as_sibling_for_test` and others), `search/api.rs` 93 to 175, `node/testkit.rs` and `synth.rs`. They are pure pass-through to runtime fields, but needed by integration tests. The right move is to move each hook next to the thing it pokes, as a `testkit`-gated method on the runtime, and keep a one-line `Node` method only where tests call it. `sync/api.rs` is the wrong home for chat hooks.
   - Size: about 300 lines relocated (not removed).
   - Risk: low. **P2** (churn, no logic).
4. **`node/hooks.rs`** (108 lines, 22 methods; section 2.3 for the fix).

### Dead pass-throughs (verified by grep across `crates/` and `android/app/src`)

`gcall/net/node.rs` is plain Rust on `Node` (not UniFFI) and is used only by `core/tests/gcall.rs` and `calls.rs`. These five have no caller anywhere: `gcall_observe_end`, `gcall_set_mic_muted`, `gcall_mute_participant`, `gcall_push_mic`, `gcall_pull_speaker`. Delete them (about -20 lines, no risk; **P1**).

`gcall_loopback_addr` has one test user. The whole file is test-only API living in the library, so gate it `#[cfg(any(test, feature = "dev-tools"))]` or move it into `core/tests/support`.

`push_video_frame_ffi`, `push_video_frame_rgba`, `push_video_frame_yuyv` and `pull_video_frame_rgba` have no use by name outside their definition. They are probably used through the generated bindings or are leftovers, and the name grep does not prove it. Check against the Kotlin before deleting. UniFFI-exported functions with no non-definition mention in Rust or Kotlin (leave, bindings fixed): `group_message_receipts`, `set_group_rotation_policy`, `group_set_backfill_paused`, `group_call_hosting_enable`.

## 2. `Inner` and Deref-to-NodeCtx

### 2.1 What `Inner` is
`node/mod.rs:39`: `Inner { ctx, chat, group, sync, search }` plus `Deref<Target = NodeCtx>`. `Node { inner: Arc<Inner>, rt }`.

### 2.2 Deref hides which runtime owns state
- `self.inner.shared`, `.me()`, `.persist()`, `.handle`, `.sink`, `.video`, `.requests`, `.block`, `.accounts`, `.gate`, `.lifecycle` are all `NodeCtx` fields reached through Deref. That is 58 + 26 + 17 + more `self.inner.*` accesses. A reader cannot tell `self.inner.block.signed` (a `NodeCtx` field) from `self.inner.chat.xxx` (an engine) without knowing the struct.
- It is inconsistent. `self.inner.ctx.x` (29 uses: `caps.rs:42`, `block.rs:79,93,163`, `contact_req.rs:175,224`, `node/api_call.rs:9,18`, `lifecycle.rs:11,48`, `rotation.rs:13,20`, `api_identity.rs:88,97`) versus `self.inner.x` (about 250). The `ctx.` form is forced only because `Arc<NodeCtx>` methods (`start`, `add_contact`, `replace_account`) take `self: Arc<Self>` and auto-deref cannot turn `&Arc<Inner>` into `&Arc<NodeCtx>`. The design doc (`runtime-split.md:341`) already says Deref is a migration aid.
- Proposal:
  1. Add `Node::ctx(&self) -> &Arc<NodeCtx>`, `group(&self) -> &Arc<GroupRt>`, `chat()`, `sync()`.
  2. Replace `self.inner.shared` with `self.ctx().shared`, `self.inner.group.` with `self.group().`, and so on.
  3. Drop `Deref`.
  The call-site count is a few hundred, the compiler finds them all, and this is mechanical.
  - Size: about 350 call sites changed, 0 lines saved.
  - Risk: low. **P2**, because it is churn and best done last, after the logic moves in section 7.

### 2.3 `impl Inner`, glue versus leftovers

| Block | Lines | Verdict |
|---|---|---|
| `impl NodeHooks for Inner` (`node/hooks.rs`) | 108 | Almost all forwarding. 8 of 22 methods are `ChatRt::x(&self.chat)`. Real glue only where one hook touches two runtimes' internals: `group_unpark` (`self.group.core.lock()` + `parked.store`, which belongs in `GroupRt::unpark`), `selfsync_sessions_close` (`self.sync.selfsync.sessions.lock()` loop, which belongs in `SyncRt`), `group_rotation_policy` (belongs in `GroupRt`). |
| `Inner::chat_open` / `commit_locked` (`node/mod.rs:52-69`) | 20 | Genuine cross-runtime orchestration (chat store + search reconcile + `sync.acc_open` + persist). Keep, or make it `Node::commit_identity` itself. |
| `search/api.rs:207-462` `impl Inner` | 255 | A whole cross-runtime engine (`search`, `search_in`, reconcile, `search_hit`, `search_names`) living in a facade crate. See 2.4. |
| `node/api_call.rs:86` `push_mic`/`pull_speaker` | 12 | Fine. Two lines of dispatch between `NodeCtx` call audio and `group.gcall`. |

**Proposal for hooks (P2):** replace the single `NodeHooks` on `Inner` with narrow traits implemented by each runtime itself.
- `trait ChatHooks`, `GroupHooks`, `SyncHooks` live in node-runtime next to `NodeHooks`.
- `ChatRt`, `GroupRt` and `SyncRt` implement them on `Weak<Self>`.
- `NodeCtx` gets `install_chat_hooks(Weak<dyn ChatHooks>)` and so on.
- `hooks.rs` and the `Weak<Inner>` plumbing (`api_identity.rs:31`) disappear, and the runtimes stop being driven through p2pcore.

The cost is that node-runtime holds three `OnceLock<Weak<dyn _>>` instead of one. The trait is already object-safe (`BoxFuture`).
- Size: about -100 in core, +60 in the runtimes.
- Risk: medium. Ordering of awaits at start and stop is the thing the design doc warns about, so keep every call exactly where it is and only change the target type.
- Test: the lifecycle, idle_close and endpoint_close integration tests.

**Proposal for `Inner`:** once hooks and search move out, `Inner` becomes a plain struct of 5 `Arc`s with nothing left to implement. Flatten it into `Node { ctx, chat, group, sync, search, rt }`. `FRAME_PORTS: Weak<Inner>` (`video_call.rs:174`) becomes `Weak<Node>` or a small `Parts` struct. **P2/P3.**

### 2.4 Search engine in the wrong crate
`search/api.rs:207` (`search_reconcile`, `search_reconcile_dms`, `search`, `search_in`, `search_hit`, `search_names`) is about 255 lines of engine code. It needs `NodeCtx` (contacts, `me`), `ChatRt` (`chat_core`, `chat_load_shard`) and `GroupRt` (`groups`, `group_name_of`), which is why it did not fit `search-ix`.
- Proposal: a `SearchSource` trait in `search-ix` (conversation names, DM shard rows, group name/author lookup), implemented by thin adapters in p2pcore or by `ChatRt`/`GroupRt` themselves. The query and reconcile then live in `search-ix` and p2pcore keeps only the `Node::search` forwarders and the records.
- Also: `search` and `search_in` duplicate a 30-line scan loop (`search/api.rs:308-345` and `350-400`: `'scan: loop { scan.next(SCAN) ... looked >= budget ... }`). Share it through a closure for the per-row filter.
- Size: about 255 lines move, about -30 from dedupe.
- Risk: medium. Search has tests: `core/tests/search.rs` (790 lines), the bench bins, `search_*_for_test` hooks.
- **P2.**

## 3. Domain logic stuck in the facade (the real "glue")

These bodies are not wrappers. They mutate runtime state from p2pcore. This is the largest cleanup if "thin Node" is the goal; the cost is that tests need a runtime-level seam.

| Location | What | Lines |
|---|---|---|
| `group/api.rs:51` `group_messages` | cursor parse, paging across day docs, `core.groups.lock()` + shard walking (`Live { state, shards, .. }`) | about 50 |
| `group/api.rs:182` `group_open_link` | admin check, ensure, relay_ready, open record, stored/reuse link, write `meta.open_link` | about 35 |
| `group/api.rs:216` `group_set_open` | writes `Body::Open`, edits `live.meta`, `put_meta`, drops the code | about 30 |
| `group/api.rs:316-345` `group_flag`, `group_set_admin`, `group_leave`, `group_post` | each does `core.groups.lock().get(..)` role checks then `group_write` | 5 to 10 each |
| `group/api.rs:125` `group_create` | genesis + host permit + accept writes | about 20 |
| `group/invitations/node.rs:21` `group_invite_contacts` | eligibility rules over `shared.state.contacts` + `groups` | about 60 |
| `group/calls/node.rs` `group_call_join`/`leave` | peeks `calls.st.lock()`, `gcall.status()...roster.is_empty()` | about 30 |
| `sync/api.rs:146` `linked_devices`, `177` `rename_device`, `199` `unlink_device` | registry edits under `shared.lock()`, `selfsync.leaving.store(..)`, `remove_self_account`; "leaving" flag handled by hand | about 105 |
| `block.rs:54` `block`, `unblock`, `block_state`, `blocked_contacts` | edit `s.state.{blocked,sent_notices,notice_outbox}`, `revoke_all`, `retire_ticket` | about 100 |
| `caps.rs` `set_calls_allowed`, `can_call` | edit `contacts[].calls_allowed`, `token_outbox`, `cut_offs` | about 60 |
| `contact_req.rs:147-246` `peek_code`, `add_contact_by_code`, `contact_requests`, `sent_contact_requests`, `accept_contact_request` | ticket verification and `state` filtering | about 100 |
| `node/api_identity.rs:98-355` (`accounts`, `remove_account`, `set_device_label`, ...) | about 250 lines of account logic | about 250 |
| `search/api.rs:207+` | see 2.4 | 255 |

Proposed: node-runtime gets `NodeCtx::{block, unblock, block_state, blocked_contacts, set_calls_allowed, can_call, peek_code, add_contact_by_code, contact_requests, accept_request_flow, ...}`. group-runtime gets `GroupRt::{messages_page, open_link, set_open, flag, set_admin, leave, post, create, invite_contacts, call_join, call_leave}`. sync-runtime gets `SyncRt::{linked_devices, rename_device, unlink_device}`. `Node` then keeps only the `block_on` and error mapping. The facade drops by about 1,000 lines; the runtimes grow by the same. The runtimes already use `node_runtime::Error` (which p2pcore maps with `From`), so there is no new error plumbing.

- Size: about 1,000 lines move, about -150 from the helper effects.
- Risk: medium. Behaviour is unchanged, but the bodies currently use p2pcore-only types in places (`crate::chat::doc::day_of` is `chat_core`, `GroupSummary` is a `group_runtime::api` record, `GroupInvitePeek` is a UniFFI record, so check each). Do them one family per packet. Existing integration tests (`group_g2/g3`, `public_group`, `blocking_acceptance`, `contact_requests`, `link`, `devices`) cover every one of these.
- **P2 as a set, P1 for `sync/api.rs` and `block.rs`** since they are self-contained.

## 4. Adapters and sinks

Ten near-identical impls, each `impl <Runtime>Sink for XEventsAdapter(Arc<dyn XEvents>)` with one forwarding method per callback:
`chat/api.rs:145`, `profile/mod.rs:23`, `group/api/types.rs:242`, `group/calls/types.rs:111`, `group/invitations.rs:50`, `group/requests.rs:47`, `sync/api.rs:47`, `node/types.rs:133`, `contact_req.rs:120`, `video_call.rs:164`. About 110 lines.

A blanket impl is not possible: the orphan rule forbids `impl<T: ChatEvents> ChatSink for T` because `ChatSink` is a foreign trait and `T` is uncovered. A generic `Fwd<T: ?Sized>(Arc<T>)` still needs one method list per trait. UniFFI's `#[uniffi::export(with_foreign)]` cannot be put on the runtime trait without pulling `uniffi` into the runtimes (they were deliberately kept uniffi-free: `#[uniffi::remote]` is used for the records instead).

**Proposal (P1):** a declarative macro in p2pcore, e.g.
```rust
sink_adapter!(ChatEventsAdapter(ChatEvents) => chat_runtime::ChatSink {
    fn on_message_added(message: Message);
    fn on_presence_changed(peer_did: String, online: bool);
    ...
});
```
It takes the method list once (the same list that already appears in the `*Events` trait) and generates the struct plus the forwarding impl.
- Size: the 10 adapters go from about 110 lines to about 60 (macro body about 25 + one signature line per callback). It does not change bindings; the trait stays as written.
- Risk: low. Compile errors tell you immediately if a signature drifts.
- Note: this is a taste win, not a structural one. Do it only after packet A.

Also inconsistent: four setters poke runtime fields directly from p2pcore (`group/calls/node.rs:27` `*self.inner.group.calls.events.lock() = Some(..)`, `group/invitations/node.rs:14`, `group/requests.rs:58`, `contact_req.rs:135`) while chat/group/sync use `set_chat_sink`/`set_group_sink`/`set_link_sink`. Give the runtimes setters (`set_call_sink`, `set_invitation_sink`, `set_request_sink`) so the sink `Mutex` fields can be private. **P1**, about 10 lines.

## 5. Non-idiomatic Rust

### 5.1 `pub` everything
`group-runtime/src/lib.rs:15` says it outright: "Everything `pub` here beyond `GroupRt`'s constructor and the records is for p2pcore's facade and the sync engine, not a stable API". The result: `pub mod` for every module, 0 `pub(crate) fn` in any runtime crate (grep), and 205 + 109 + 203 + 69 + 12 pub fns.

By name, across `crates/` and `android/`, with no mention outside the defining crate: **273 pub items** (fns, consts, structs, enums, traits, types) are used only inside their own crate (group-runtime 101, node-runtime 91, chat-runtime 50, sync-runtime 31). Examples:
- `group-runtime/engine/member_receipts.rs`: `MAX_READ_POSTS`, `member_receipts_pump`, `receipts_send`, `receipts_push` and 11 more.
- `group-runtime/code.rs`: `MAX_CODES_PER_GROUP`, `CodeRt`, `check_fetched` and 10 more.
- `node-runtime/src/lib.rs`: `TICKET_TTL`, `DIAL_TOTAL`, `MAX_DEVICES`, `RING_TIMEOUT` and 9 more.
- `chat-runtime/chat/engine/receipts.rs`: `ClosedDays`, `read_versions`, `chat_read_intent`.
- `node-runtime/call.rs`: `SlotGuard`, `Leg`, `LegEvent`, `end_call`.
- `node-runtime/rotation.rs`: `RotationRt`, `complete_rotation`.
- `sync-runtime/engine/push.rs`: `push_changes`, `push_acks`, `push_groups` and others.

Another 31 are used outside only from tests or `synth` (`renew_grant`, `accept_device_list`, `SynthGroup`, `synth_group_*`, `VV_BYTES`...). Gate these on `testkit`.

Caveats: the scan is by name (a name shared with an unrelated item hides one; a type used in a pub signature must stay pub). The compiler arbitrates; the `unreachable_pub` lint is useless while all modules are `pub`, so first make internal `pub mod` into `mod` or `pub(crate) mod` and let rustc report.

Proposal: make modules private and re-export only the intended surface. Add the `#![warn(unreachable_pub)]` lint after that. It also exposes dead code, because `dead_code` then works on these crates (it is currently silent for all `pub` items; I found none by name with a single mention, but that is the name-level result).
- Size: mechanical, about 270 edits plus module visibility changes.
- Risk: low, compiler driven; the only breakage is in p2pcore tests that import runtime internals (the 31 above).
- **P2** (big diff, nothing semantic).

### 5.2 `self: &Arc<Self>` receivers
node-runtime 74, chat-runtime 79, group-runtime 94, sync-runtime 29: 276 in total. Heuristic (name-level fixpoint that looks for `self.clone()`, `Arc::clone(self)`, `self` passed as an argument, or a call to another method that needs `Arc`): about 95 of them need no `Arc` receiver (node 24, chat 11, group 46, sync 14). Examples: `group-runtime/engine/lifecycle.rs:13` `group_core`, `group-runtime/code.rs:123` `group_issue_code`, `group-runtime/invitations.rs:301` `deliver_letter`, `sync-runtime` `push_*`, `send_unlinked`.
- Why it matters: each `Arc<Self>` receiver forces callers to hold an `Arc` and spreads `.clone()`. In `NodeHooks for Inner` and in the `Node` API it is what forces `let inner = self.inner.clone();`.
- Proposal: switch candidates to `&self` in a compiler-driven pass. Where a method spawns a task, keep `self: &Arc<Self>` locally.
- Size: about 95 signature changes, some call-site simplifications.
- Risk: low. **P3**.
- Caution: the HEAD working tree already has about 60 modified files from the W8 pass; this pass collides with them if run in parallel.

### 5.3 `#[cfg(test)] pub use` shims and re-export tangles
- `sync/engine.rs`: `pub(crate) use sync_runtime::engine::*;` plus a `#[cfg(test)] #[allow(unused_imports)] use { ... 17 paths ... }` dump so the tests under `sync/engine/` can say `use super::*;`. Three more files repeat `#![allow(unused_imports)]` (`group_history.rs`, `group_receipts.rs`, `group_contents.rs`). Fix: each test file imports what it uses directly; delete the dump. About -30 lines of shim. **P1.**
- `group/mod.rs:11-12`: `#[cfg(test)] pub use group_runtime::{doc, own};` and `chat/mod.rs:9` `#[cfg(test)] pub use chat_runtime::chat::wire;`; `receipts/mod.rs:5` `#[cfg(test)] pub(crate) use receipts_core::*;`. Fix: the tests that need them import from the owning crate. **P1**, mechanical.
- 27 `pub use <runtime>::...::*;` "old path" shims (`caps.rs`, `block.rs`, `contact_req.rs`, `devlist.rs`, `store.rs`, `chat/store.rs`, `group/{calls,code,invitations,requests,calllog,member_receipts,rotation}.rs`, `profile/mod.rs`, `gcall.rs`, `sync/engine/*`, `node/mod.rs:34` `pub use node_runtime::*`, ...). Within core, `crate::group::...` is used 43 times, `crate::chat::` 23, `crate::node::` 37, `crate::store::` 9, `crate::block::` 6, `crate::sync::` 5. So after rewriting those imports to the real crate, the shim files that have no UniFFI declarations in them can be deleted outright (`devlist.rs`, `caps.rs`' glob, `store.rs`, `chat/store.rs`, `group/code.rs`, `group/rotation.rs`, `group/member_receipts.rs` if records are declared in the runtime, `gcall.rs`...). `node/mod.rs:34` `pub use node_runtime::*` is the worst: it dumps hundreds of names into `crate::node::`. Replace by explicit lists.
  - Size: about 150 import lines changed, about 12 files removed.
  - Risk: low. Pre-release, so no downstream expectations (tests in `core/tests` import only `p2pcore::{Node, Error, ...}`, 20-ish paths).
  - **P2.** Tests under `core/tests/support` use `p2pcore::gcall::*` (5 places), which must remain.

### 5.4 Stringly-typed IDs, bool params, big functions
- IDs: `gid: &str`/`group_id: &str` 161 occurrences and `did: &str|String` 169 in the four runtimes. There is no `Did`/`Gid` newtype anywhere (grep). Swapping `gid` and `did` compiles, and several fns take `(did: &str, gid: &str)` in one signature. The boundary types must stay `String`. An internal newtype (`struct Gid(Arc<str>)`) is a large change for modest value. **P3.**
- Bool params: 43 fns take a `bool` (two bools in `NodeCtx::log_refused(.., missed: bool, video: bool)` at `node-runtime/src/persist.rs:310`, `synth_group_flush_part(.., meta: bool, days: bool)`, `decline_reason(paused, busy, ..)`). Small (a 2-variant enum, or a flags struct, for the two-bool ones). **P3.**
- Long arg lists: 29 `allow(clippy::too_many_arguments)` in the workspace; in p2pcore `search_in` (9 args), `push_video_planes` (10). For search, a `Query { text, limit, cursor, budget }` struct fixes it as part of 2.4. **P3.**
- Huge functions (> 150 lines, runtime crates): `sync-runtime/src/engine/sessions.rs:228` `self_loop` (290 lines), `sync-runtime/src/engine/group_history.rs:245` `handle` (211), `group-runtime/src/engine/accepting.rs:47` `group_host_session` (171), `node-runtime/src/run.rs:146` `run_call` (168), `group-runtime/src/engine/sessions.rs:103` `group_read` (165), `node-runtime/src/outgoing.rs:154` `run_outgoing` (160). Plus the 100 to 150 band (`group_dial` 148, `handle_incoming_conn` 133, `add_contact_outcome` 129, `chat_run` 123, `incoming_call` 122, `pump` 118, `group_call_msg` 118, `group_core` 116).
  - `self_loop` (290 lines) is the only one that is really a problem. The usual shape is a `loop { match frame { ... } }` where each arm can be a method.
  - Risk: medium (these carry protocol ordering); each needs its own tests. **P3.**
- Huge files (> 1000 lines): only `group-runtime/src/engine/durability/group_receipts_sim.rs` (1,338, test-only, a simulation). In the runtimes, nothing else is over 800 (`images.rs` 800). Not a concern.
- `Option` + `is_some()` + `unwrap()`: I found about 1 pair in the runtimes. Not a pattern here. `unwrap()` count is 516 across runtime crates (mostly tests) and 13 `expect`; not part of this audit.
- `.map(|_| ())` 7 sites, `map_err(Error::from)` 32 sites in core where `?`/`Ok(x?)` would do: cosmetic. **P3.**
- `group-runtime/src/engine/durability.rs:4` `#![cfg_attr(not(test), allow(dead_code))]`: test fixtures compiled under `testkit`. Move the fixtures behind `cfg(any(test, feature = "testkit"))` and drop the allow. **P3.**

## 6. Things deliberately left alone
- `Node` method signatures, the `*Events` traits, `#[uniffi::remote]` records, `core_version`, `did_of_phrase`. Bindings fixed.
- The 14 `_for_test` fns in `group/api/hooks.rs` that are `#[uniffi::export]`: they ship in the release bindings. Hiding them changes the generated Kotlin, so only do that with explicit sign-off. The `#[cfg(any(test, feature = "dev-tools"))]` block at the end of that file shows the pattern for the rest.
- `p2pcore::Error` vs `node_runtime::Error`: kept distinct on purpose (W8 handoff note: a remote would pull transfer/call-video/audio/gcall-net into node-runtime). `map_err(Error::from)` is the price.

## 7. Recommended plan (independently testable packets)

Order matters because A and B shrink every later diff.

| Packet | Content | Size | Risk | Check |
|---|---|---|---|---|
| **A (P1)** | `Node::run(\|inner\| fut)` helper; `GroupRt::{admin_core, member_core}`; sink setters on `GroupRt`/`NodeCtx`; delete 5 dead `gcall_*` Node methods; delete the `sync/engine.rs` `cfg(test)` import dump and the 3 `allow(unused_imports)` lines, plus the `group/mod.rs` / `chat/mod.rs` / `receipts/mod.rs` cfg(test) shims | about -250 lines | low | `cargo clippy -p p2pcore`; tests `--lib group::`, `--test group_g2`, `--test gcall` |
| **B (P1)** | `sink_adapter!` macro for the 10 adapters | about -50 lines | low | `cargo check -p p2pcore`; the events integration tests (`--test calls`, `--test group_call`) |
| **C1 (P1)** | `sync/api.rs` -> `SyncRt::{linked_devices, rename_device, unlink_device}` and chat test hooks out of `sync/api.rs` | about 100 moved | low-med | `--test link`, `--test devices`, `--test lifecycle` |
| **C2 (P1/P2)** | `block.rs` / `caps.rs` / `contact_req.rs` / `api_identity.rs` logic -> node-runtime | about 500 moved | med | `--test blocking_acceptance`, `--test contact_requests`, `--test call_tokens_acceptance`, `--test accounts` |
| **C3 (P2)** | `group/api.rs`, `invitations/node.rs`, `calls/node.rs`, `requests.rs` logic -> group-runtime | about 400 moved | med | `--test group_g2`, `--test group_g3`, `--test public_group`, `--test group_call` |
| **D (P2)** | Split `NodeHooks` into per-runtime hook traits; delete `hooks.rs`; flatten `Inner` into `Node` once search is out | about -100 core, +60 runtime | med | `--test lifecycle`, `--test idle_close`, `--test endpoint_close`, `--test private_rotation` |
| **E (P2)** | Search engine -> `search-ix` with a `SearchSource` trait; share the scan loop between `search` and `search_in`; query struct | about 255 moved, -30 | med | `--test search`; bench bins compile with `--features bench` |
| **F (P2)** | Remove glob re-export shims, explicit imports; replace `pub use node_runtime::*` | about 150 lines changed, 12 files out | low | full `cargo check --all-targets -p p2pcore` |
| **G (P2/P3)** | Runtime visibility: private modules, `pub(crate)` for the 273 internal items, `testkit`-gate the 31 test-only; then `unreachable_pub` lint; `Arc<Self>` -> `&self` where the compiler allows (about 95 candidates) | about 400 edits | low | `cargo check` on all runtime crates and `--features dev-tools` |
| **H (P3)** | `Node::ctx()/group()/chat()/sync()` accessors replacing Deref; bool params to enums; newtype IDs; split `self_loop`; test-hook relocation | large | low-med | per item |

Run packets A and B first. They are small and make every later diff smaller. C1, C2 and C3 can go in parallel on different files (they share only `crates/core/src/lib.rs` re-exports). D depends on C finishing (so `Inner` has nothing left to implement), and G is best last because it changes pub-ness across all crates. Per the repo rules, build and test through `tasks run --who <agent> --anywhere -- cargo test -p p2pcore --test NAME` with named targets, and coordinate file ownership with the running W8 cleanup in this worktree (about 60 files are modified and uncommitted).

## 8. Method notes and limits
- The forwarder counts are heuristic (bodies of at most 6 lines containing `self.inner.<chat|group|sync|search>`; `block_rt(async move { x.f(..) })` counted). Roughly right to +-15%.
- The pub-visibility scan is name-based over `crates/**/*.rs`, `android/**/*.kt`, `ios/**/*.swift`. Name collisions between crates hide some internal-only items (undercount) and `pub` items needed by `pub` signatures are counted as internal (overcount); rustc settles both.
- The `Arc<Self>` candidates come from a text heuristic that does not follow trait dispatch or macro expansions; treat the 95 as a work-list for the compiler, not as a verified result.
- I did not run cargo or `tasks`. Everything is read-only analysis of the tree as found.
