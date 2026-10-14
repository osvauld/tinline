# Superseded-model review (2026-10-14)

Retirement pass `rf/retire-models`. Safety tag: `archive/pre-retire-2026-10-14`. Nothing was
deleted: each candidate failed its coverage or decision check. Restore command if a later pass
deletes them: `git checkout archive/pre-retire-2026-10-14 -- <paths>`.

## group-sim `multi_host/group_call` (4.2k lines) - KEPT
Models G6 group calls through one call host (GC-S01..S17). Core `gcall` tests cover S01, S03, S04,
S05, S06, S06b, S08, S13, S15 (`gcall/tests/failover.rs`, `tests/group_call.rs`), S16, S17
(`crates/core/src/gcall/tests/*`, `rank/tests.rs`). Not covered in core: S02 (CallStart without a
valid call permit), S07 (level-0 spoofer, dominance cap), S09 rogue-host limitation, S10 (personal
block, host learns nothing), S11 (U12 rotation deferral), S12 (member rotation), S13b (roster size),
S14 (U7 endpoint audit). Its `ASSUMPTIONS` (G1-G10) stay in `protocol.rs`; move them to
`group-call-spec.md` before any deletion.

## group-sim `policy_candidates` (1.8k) - KEPT
D01/D02 are decided (U11 signed frontier; `rotation-open-decisions.md`). D03 (regrant/cascade),
D04 (expiry), D05 (epochs) are still open in `rotation-open-decisions.md` §6 Q5-Q9, and core
`group/state.rs` implements none of them. `docs/group-policy-options.md` status line updated.

## group-sim `host_relay` (840) - KEPT
The README table covers the five tests R1b removed. The six remaining tests are rewritten, not
removed. Unreplicated elsewhere: catch-up gap policies (G-HISTORY), same-seq note fork keep-first vs
freeze (G-FORK on endpoint notes), mnemonic clone with higher seq. Others are partly covered by
`host_only`/`multi_host`.

## proto `rotation_succession_model.rs` + `support/rotation_network.rs` (389) - KEPT, header added
Crash boundaries and gap catch-up are covered by `support/rotation_permit_lifecycle.rs`
(`crash_at_each_rotation_boundary_resumes_the_committed_key_and_generation`),
`rotation_history_adversarial.rs` and `rotation_history_paging.rs`. No permit test serializes and
restores a receiver after a gap (`missing_history_requires_catchup_and_survives_serialized_restart`).

## `scripts/test_rotation_{discovery,network}_model.py`, `..._linked_discovery_options.py` (1065) - KEPT
Not required by `regress.py` or CI, but cited as evidence by `rotation-time-contract.md`,
`endpoint-rotation-*.md` and `rotation-linked-discovery-options.md`. U1 decided discovery option I,
but the E1 yes/no questions (`rotation-open-decisions.md` Q1) and the time contract remain open.
