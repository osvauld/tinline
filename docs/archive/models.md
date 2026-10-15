# Design models moved out of the main tree (2026-10-14)

By user decision on 2026-10-14 the design simulations left the main tree. They are kept on the
local branch `design-models` (cut at 244df93) and on the tag `archive/pre-retire-2026-10-14`.
They no longer build or run in CI. The earlier `rf/retire-models` review had kept them because
each failed a coverage or decision check; that evidence is preserved below.

## Moved out
- `crates/group-sim` (whole crate, removed from workspace members).
- proto tests that exercise no real `proto` code: `rotation_challenge_contracts`,
  `rotation_fork_revocation`, `rotation_history_adversarial`, `rotation_history_paging`,
  `rotation_permit_model`, `rotation_storage_contracts`, `rotation_succession_model`,
  `rotation_time_contracts`, `rotation_visible_block`, and `support/{permit_base,
  permit_history,rotation_network,rotation_permit_lifecycle,rotation_permit_network}.rs`.
- Mixed files cut down to the real-code tests: `capability_tokens.rs` keeps
  `proto_verifier_accepts_single_capability_tokens`; `rotation_device_list_push.rs` keeps three
  tests of `sign/verify_device_list`, `newer` and `list_seq_in_bounds` (rewritten, model removed).
- `scripts/test_rotation_{discovery,network}_model.py`, `test_rotation_linked_discovery_options.py`.
- Kept: `gcall_capacity_bounds.rs`, `golden_wire.rs`, `rotation_identity_boundary.rs`, `support/fixtures.rs`.

## Restore
`git checkout design-models -- crates/group-sim` (or the other paths), then add
`"crates/group-sim"` back to `members` in the root `Cargo.toml`.

## Evidence that is now only on the branch
- group-sim `multi_host/group_call` (GC-S01..S17). Core `gcall` tests cover S01, S03, S04, S05,
  S06, S06b, S08, S13, S15, S16, S17. NOT covered in core: S02 (CallStart without a valid call
  permit), S07 (level-0 spoofer, dominance cap), S09 (rogue-host limitation), S10 (personal block,
  host learns nothing), S11 (U12 rotation deferral), S12 (member rotation), S13b (roster size),
  S14 (U7 endpoint audit). Its `ASSUMPTIONS` (G1-G10) were in `protocol.rs`; `group-call-spec.md`
  must carry them.
- group-sim `policy_candidates`: D03 (regrant/cascade), D04 (expiry), D05 (epochs) are open
  (`rotation-open-decisions.md` §6 Q5-Q9); core `group/state.rs` implements none.
- group-sim `host_relay`: catch-up gap policies (G-HISTORY), same-seq note fork keep-first vs
  freeze (G-FORK on endpoint notes), mnemonic clone with higher seq.
- proto succession model: no remaining test serializes and restores a receiver after a gap
  (`missing_history_requires_catchup_and_survives_serialized_restart`).
- Python discovery models: the E1 yes/no questions (`rotation-open-decisions.md` Q1) and
  `rotation-time-contract.md` (the rotation time contract) were argued with these.

## Decisions still open that these models were exploring
- D03 regrant/cascade, D04 grant expiry, D05 epochs (group policy).
- Rotation time contract (clock bounds, renewal windows).
- E1 linked-device discovery yes/no questions (U1 chose option I, details open).
- G-FORK: concurrent device-list edits (tie-break vs union vs freeze vs re-sign).
- G-HISTORY: catch-up gap policy; G-BLOCK: push exclusion while a block syncs; G-RENEWAL and
  G-CAPSHAPE for capability tokens.
- GC-S02/S07/S09-S12/S13b/S14 behaviour in the real group-call engine.
