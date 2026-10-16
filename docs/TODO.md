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
