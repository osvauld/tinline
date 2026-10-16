# TODO

Open work after the 2026-10-16 refactor and rotation landing (dev 990c0d6). Detail on rotation is in
[rotation.md](rotation.md); the refactor plan is in [refactor/PLAN.md](refactor/PLAN.md).

## Verify what just landed
- [ ] Compile the Android app against dev. `Contact` gained `lost_touch`, and A3 changed five Kotlin
      files (`MainActivity.kt`, `P2pApp.kt`, `UnlockStore.kt`, `Unlock.kt`, `GroupsLive.kt`).
- [ ] Run the core integration tests not run on the rotation changes. Group rotation skipped
      group_call (all but the rotation test), group_g3, group_shards, video_calls, link_expiry,
      message_receipts, old_writer_delivery, profile_sync, readd, search, vault, writer_id and
      golden_core. 1:1 rotation skipped accounts, call_tokens_acceptance, chat_shards,
      endpoint_close, gcall, golden_core, grant_characterization, group_core, group_g2, group_g3,
      group_shards, lifecycle, message_receipts, old_writer_delivery, profile_sync, readd,
      restore_characterization, search, vault, video_calls and link_expiry. Skip
      `capacity_twelve_*` (fails on the old base too).

## Rotation in the apps (Android + desktop)
- [ ] "Lost touch: swap codes again" on the contact list and chat header, from `Contact.lost_touch`
      (refresh on `on_contacts_changed`), with QR share and scan.
- [ ] Settings: interval choice at or above `MIN_INTERVAL_MS` (1 day) through
      `group_rotation_policy` / `set_group_rotation_policy`. One setting drives both group and private
      endpoints. Show the next change from `private_endpoint_rotation_due()` and offer "change it now"
      (`private_rotate_now()`).
- [ ] QR screen re-renders after a rotation (`my_ticket()` returns the new address).
- [ ] Optional: neutral names for the policy API (it is still called `group_*`).

## Receipts (group receipts landed 2026-10-16, dev 12a5fec)
Detail in [design/receipt-runtime-roadmap.md](design/receipt-runtime-roadmap.md).
- [ ] Phone check on dev: group open no longer stalls (~0.75 s `Choreographer: Skipped` before the
      fix) and Privacy > Read receipts no longer changes height. If a stall remains, take a Perfetto
      trace of one group open.
- [ ] Re-run the Android unit tests and the full core integration tests on dev (only receipts/chat/
      group-receipts/tick/rotation ran after the last dev merge).
- [ ] Real two-device network test of `tinline/group/4` receipt frames (so far in-process only, plus
      three headless peers).
- [ ] Receipt size grows with group size (299 B at 3 members, 781 B at 50 per statement): find out
      why (likely the signed meta frontier) before choosing batching. Then batching, then shards (M7).
- [ ] Cache the group receipt status per (group, day) if group opens still feel slow (it verifies
      every signature per call).
- [ ] Contact device names: other people's devices show a short id ("dbebb7e4").
- [ ] Own-device receipt ledger sharing outside the group host relay.
- [ ] `p2p-peer chat-serve` group commands (gcreate/gjoin/gpost/greceipts…) sit uncommitted in the
      receipt-runtime worktree: commit as test tooling or drop.
- [ ] Decisions still open: per-device removal in groups; a proper group message details view (today a
      desktop tooltip and Android tap line); the not-delivered mark looks like a small smile at 12 px.
- [ ] Pre-existing: the chat jumps when the "X is offline" banner disappears.

## Whitepaper
- [ ] Group and 1:1 rotation are now built: change their tags from "designed" to "shipped". Add the
      verified-flag rule (it survives a rotation through old-key succession links; a new install or
      restore still resets it).

## Decisions still open
- [ ] Delete the storage-attestation runtime? Nothing issues one. Recommendation: delete.
- [ ] Remote unlink: lock the account instead of deleting its directory? Recommendation: lock.

## Next work
- [ ] Packet C: chats onto the shared Loro ledger (last of the refactor).
- [ ] Packet B remainder: gate the remaining `*_for_test` hooks (about 39 files) behind `dev-tools`.
- [ ] Hardening H1–H6: rav1d garbage-input SIGABRT repro, IP exposure before app auth test, lock
      poison helper, `capacity_twelve_talking_at_once` failure (fails even alone).
- [ ] Known failures: storage `golden_redb` (redb 2 fixture); clippy TAU error at
      `crates/core/src/cues.rs:89`.
- [ ] Flaky under load: `calls::hangup_while_dialing_is_immediate`,
      `blocking_acceptance::blocked_senders_messages_never_reach_the_blocker`.
- [ ] Rotation follow-ups: a dial in flight on a retired group endpoint can wait ~20 s; "lost touch"
      is one-sided and has no recovery through a shared group host yet.
- [ ] After the core: the Kotlin audit, then the iced desktop audit.
