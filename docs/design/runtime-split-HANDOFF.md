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
- Remaining waves: W4 search-ix and W5 chat-runtime (chat + profile) — can go in parallel;
  then W6 group-runtime (unblocked, public group landed); W7 sync-runtime; W8 cleanup.

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
- Task manager: `--anywhere` by default; one release-test job on nixos at a time (7.6 GiB);
  no `sh -c` loops (one cargo command with many `--test` is fine); pipes need
  `bash -c 'set -eo pipefail; …'`; integration tests need `ulimit -n 65536`; Android
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

- `crates/core/tests/search.rs:581` `paging_across_tiers_with_a_tier_change_between_pages`
  fails 2/2 on plain dev dbcfd512 (jobs 4158, 4159): tie order for equal timestamps
  differs between test `(at,id)` reversed and the engine. Real bug, before release.
- `sync::engine::group_history::tests::actual_authenticated_two_node_archive_sync_crosses_page_and_batch_boundaries`
  times out ~1/3 at 90 s (also on dev). Proposal: 300 s at its `tests.rs:168`. Waiting for OK.

## Later

- W8: ~15 unused-import warnings from moved code; hooks impl uses `Inner::method(&self)`
  to avoid trait shadowing — collapse when `impl Inner` remnants go.
- Duplicate `MAX_HISTORY`; clippy in moved code; TAU error `cues.rs:89`.
- Worktrees to clean (show the user first): rt-w1, rt-w2, rt-w3, rt-base, split-wave*, ~40 old ones.
- Release after the split: user pushes → CI → Pixel 3a phone test (incl. MediaCodec moved in
  W1) → version bump (only the 12 workspace crates in Cargo.lock) → tag. iOS in or out: undecided.
- Rename p2pcore → tinline (post-release).
