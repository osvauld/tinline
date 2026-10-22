# Runtime split: handoff (2026-10-22)

Plan and design: `docs/design/runtime-split.md` (waves table near line 498). This file is
the state at the end of the first integration session.

## Where things are

- Parent branch: `integrate/runtime-split`, worktree `.claude/worktrees/runtime-split`,
  HEAD `f7afbfde`. Clean, green. The user merges it to `dev`; do not merge to dev yourself.
- Merged so far:
  - W1 `refactor/rt-w1-video` (MediaCodec into `crates/video` feature `mediacodec`, new
    `crates/call-video`) — a8ebfaa6.
  - W2 `refactor/rt-w2-gcall` (`crates/gcall-net`, `GcallHost` trait) — 8023768b.
  - dev e98b63d6 (public group + images) — bc83d848; dev dbcfd512 (GroupDelivery::Failed) — fb7f387b.
  - W3 `refactor/rt-w3-node` (`crates/node-runtime`: NodeCtx, NodeHooks, OwnDocSignal,
    contacts, block, caps, devlist, rotation, wire/frames/rekey/store) — f7afbfde.
    p2pcore `Inner { ctx: Arc<NodeCtx>, .. }` derefs to NodeCtx; `impl NodeHooks for Inner`
    in `crates/core/src/node/hooks.rs`, installed (as Weak) in `node/api_identity.rs`.
  - W4 `refactor/rt-w4-search` (`crates/search-ix`: `SearchIx` index slot, write hooks and
    `skip_next_writes`; built on node-runtime; `PostDiff` moved to `group-core::state`), d2c0b6f8.
  - W5 `refactor/rt-w5-chat` (`crates/chat-runtime`: `ChatRt` with the chat engine and
    profiles; the `ChatSink`/`ProfileSink` traits have p2pcore adapters; the plain receipt
    records are in `receipts-core::status`; `dm_version` moved to `chat-core::doc`;
    `OwnDocSink::in_session` and `NodeCtx::is_own_device` are in node-runtime), 7d7d5cf9.
    p2pcore keeps `Inner { chat: Arc<ChatRt> }`. `Node::block_rt` maps
    `node_runtime::Error` on a blocking call, and `Inner::chat_open` reconciles search when
    `ChatRt::chat_open` returns true. `synth_dm` is behind chat-runtime's `testkit` feature,
    which p2pcore `dev-tools` turns on.
    W5 checks: workspace check (4181), lib tests (4183; p2pcore 162 + chat-runtime 7 = 169),
    set 1 (4184), set 2 (4196), bindings diff empty (4197), APK (4198).
  - W6 `refactor/rt-w6-group` (`crates/group-runtime`: `GroupRt` derefs to `NodeCtx` and
    holds the group engine, sessions, calls, invitations, requests, codes, own docs,
    attestation, calldir/calllog, images and member receipts; 54 files moved with `git mv`).
    p2pcore keeps `Inner { group: Arc<GroupRt> }`. Each runtime trait (`GroupSink`,
    `GroupRequestSink`, `GroupInvitationSink`, `GroupCallSink`) has a p2pcore `*EventsAdapter`.
    `GroupRt` is the `GcallHost` (installed as Weak in `api_identity.rs`); gcall errors map
    through `calls::from_gcall`, because the orphan rule blocks a `From` impl. Search
    reconcile for groups moved into `group-runtime/src/search.rs` (`RECONCILE_BATCH` and
    `NO_BG_RECONCILE` are in search-ix). `GROUPS_MARK` is in node-runtime `signal`, and
    `From<group_core::Error>` is in node-runtime. Durability tests build a bare runtime with
    `bare_rt` (NodeCtx over a temp dir and a quiet NodeSink), not p2pcore `Node`. The synth
    helpers (`SynthGroup`, `SynthRemote`) and test hooks are behind group-runtime `testkit`,
    which p2pcore `dev-tools` turns on. The `own/contents/auth_tests.rs` test, which needs
    `Node`, stays in p2pcore as `group/own_auth_tests.rs`.
    W6 checks: dev-tools workspace check; lib tests (4235; p2pcore 73 + group-runtime 89 +
    chat-runtime 7 + node-runtime 20 = 189); bindings diff empty (4236); bench+vp8 (4237);
    APK (4239); set 1 (4254, laptop `-j 6`); set 2 (4255; 11 targets, 99 passed).
    `sync-connection-audit` was already on dev (2b3a4d10) and was removed.
- Remaining waves: W7 sync-runtime, then W8 cleanup.

## Rules that held this session

- p2pcore is the only uniffi crate. Lower crates: no uniffi, own error types; map in
  `crates/core/src/error.rs`; FFI types re-declared with `#[uniffi::remote(..)]` in the
  module where they used to live. Check: `cargo tree -p <crate> -i uniffi` finds nothing.
- Generated Kotlin + Swift bindings must stay byte-identical. Baseline for the current
  integrate tree: `/home/abe/build-cache/offload/lead-runtime/4169/bindgen-base.tar`
  (identical to `lead-runtime-base/4096`). Bindings command (one `sh -c` step is fine):
  `cargo build -j 6 -p p2pcore --features dev-tools && rm -rf bindgen-base && cargo run -j 6 -p p2pcore --features dev-tools --bin uniffi-bindgen -- generate --library target/debug/libp2pcore.so --language kotlin --out-dir bindgen-base/kotlin && (same, --language swift --out-dir bindgen-base/swift) && tar cf bindgen-base.tar bindgen-base`
  with `--fetch bindgen-base.tar`; extract both into scratch dirs, `diff -r`; delete
  `bindgen-base*` from the worktree.
- Lock order (design doc): writing before shared; lifecycle; closing; live before gcall
  session; group.core held while the store opens. Hooks are called after data guards drop
  (except `in_group_call`, under `live`, as documented).
- Task manager: `--anywhere` by default; nixos has 16 GiB since 2026-10-22, so two release jobs are fine;
  no `sh -c` loops (one cargo command with many `--test` is fine); pipes need
  `bash -c 'set -eo pipefail; …'`; do not `| grep` inside a job (it buffers, so the log
  looks stuck); use `tasks logs ID --grep` afterwards. A `bash -c` job can land on the
  laptop with `-j 6`: pin heavy test jobs with `--peer nixos`. Integration tests need
  `ulimit -n 65536`; Android
  always `--peer nixos`. Lockfile refresh: `cargo metadata --offline --format-version 1 >/dev/null`.

## Acceptance per wave (what W3 ran)

1. `cargo check -j 6 --workspace --all-targets`, plus `--features dev-tools`, `bench`, `vp8`.
2. Lib tests of every touched crate; total count must match (W3: node-runtime 20 + p2pcore 169 = 189).
3. Release integration set 1 (calls, video_calls, gcall, group_call, lifecycle,
   endpoint_close, idle_close, contact_requests, blocking_acceptance,
   call_tokens_acceptance, device_list_characterization, device_list_push, devices,
   private_rotation, tickets, grant_characterization, restore, restore_characterization,
   restore_ticket, readd, accounts, link, link_expiry) and set 2 (chat_shards, fanout,
   golden_core, group_core, message_receipts, old_writer_delivery, profile_sync,
   public_group, search, vault, writer_id). Run each set as its own job.
4. Bindings diff (above). 5. APK `:app:assembleDebug` on `--peer nixos`.

Load flakes seen under a big set on nixos (pass alone, pass on dev): `lifecycle`
add_contact timeouts, `blocking_acceptance` <1 s assert, `devices` verified race,
`calls` glare/Busy, `group_receipts` own-sessions. Rerun the failed targets alone before
calling it a regression, and compare against `.claude/worktrees/rt-base` (detached dbcfd512).

Source-scan tests read paths: when code moves to a new crate, update them
(`grant_characterization.rs` now walks `crates/node-runtime/src` too).

## Open, needs the user

- Both earlier items were approved and fixed on branch `fix/test-flakes` (worktree
  `.claude/worktrees/test-flakes`, from dev), which the user merges:
  - `search.rs` paging test: the engine breaks equal-timestamp ties by a keyed hash of the
    id, and the test assumed id order. Not an engine bug. The test now sleeps 3 ms between
    old sends, so every `at` is distinct.
  - The group_history archive sync timeout went from 90 s to 300 s (passed in job 4203).

## Later

- W8: ~15 unused-import warnings from moved code; hooks impl uses `Inner::method(&self)`
  to avoid trait shadowing — collapse when `impl Inner` remnants go.
- Duplicate `MAX_HISTORY`; clippy in moved code; TAU error `cues.rs:89`.
- Worktrees: 17 merged ones were removed on 2026-10-22 (split-wave*, crate-split, gcall-*, etc.). The user
  removes `agent-aa449…`/`agent-ab21b…` (they need `--force`). About 40 with unmerged or uncommitted work wait for the user. rt-w1..w5 and rt-base
  go after the split lands.
- Release after the split: user pushes → CI → Pixel 3a phone test (incl. MediaCodec moved in
  W1) → version bump (only the 12 workspace crates in Cargo.lock) → tag. iOS in or out: undecided.
- Rename p2pcore → tinline (post-release).
