# Retired tests and dead code (2026-10-14)

Everything below is restorable from the git tag `archive/pre-retire-2026-10-14`
(`git show archive/pre-retire-2026-10-14:<path>`).

## Duplicate core integration tests (merged)

Merged only where the removed test's assertions were a subset of the kept one's.

- Removed `unlinking_a_device_reaches_the_contact_without_a_call`
  (`crates/core/tests/device_list_characterization.rs`) -> covered by the same-named test in
  `crates/core/tests/device_list_push.rs`.
  - B registers a sibling, A dials it: kept (`A learns B2`), stronger (learned by push, no call needed).
  - B unlinks it, A stops dialling it: kept (`A drops B2`, then `a.dials == [b.key]`).
  - "B's own list dropped B2": folded into the kept test (verified own list no longer names B2).
  - Only difference dropped: the old test taught A the list through an answered call first.
- Removed `ticket_contacts_can_message_and_call_each_other`
  (`crates/core/tests/grant_characterization.rs`) -> covered by `ticket_contacts_can_chat_and_call_both_ways`
  (`crates/core/tests/call_tokens_acceptance.rs`).
  - Both directions: message delivered -> `message_arrives` (holds + delivered).
  - Both directions: call rings, caller hangs up -> `rings`.
  - Kept test also checks calls_allowed / can_call / two distinct tokens.
- Removed `o1_restore_reissues_the_ticket_for_the_new_device_key`
  (`crates/core/tests/rotation_restore_characterization.rs`) -> covered by
  `after_restore_my_ticket_names_the_current_device_key_and_redeems` (`crates/core/tests/restore_ticket.rs`).
  - New ticket differs from the old one: kept. Names the DID (`iss`) and the new device key: kept.
  - "old key != new key": folded into the kept test (`assert_ne!(old_dev, new_dev)`).

Considered and kept (different setup or unique assertions): the other device_list pairs
(adding/blocked/merge-refusals/max-seq/restore: different layer or setup), fanout.rs device-list tests (cap,
verified reset, persistence), grant `block_takes_both_tokens_back...` (raw wire peer) and
`calls_are_refused_per_contact...` (third contact, global availability), old_writer_delivery and writer_id
(different scenarios from the o3 tests), and blocking_acceptance
`blocked_caller_never_rings_and_ends_quickly` vs `blocked_caller_ends_at_once_with_the_blocked_reason`
(the latter calls `ignores_notices(&b)` first, so the setups differ).

## rotation_acceptance placeholders

14 `#[ignore]` stub tests and their `mod harness` stubs moved to
`docs/design/rotation-acceptance-checklist.md`; bodies are in the tag.

## search benchmark

`crates/core/src/search/bench.rs` moved to `crates/core/src/bin/search-bench/bench.rs` (git rename); the
binary entry is `bin/search-bench/main.rs`. `lib.rs` lost `search_bench` / `search_bench_live` and gained
`#[doc(hidden)] pub mod search_bench_support` re-exporting `DocRow, Mark, Mode, PHASES, SearchIndex, text`.

## gcall dead items

Deleted `HostEngine::is_limited`, `HostEngine::lower_priority`, `CallBoard::previous` (no callers).
Marked `#[cfg(test)]`: `HostEngine::dropped`, `CallBoard::hosted_by`, `rank::pick`.

## Scripts

Deleted `scripts/measure_idle.py` and `scripts/record_play_demo.py` (no references outside themselves).
