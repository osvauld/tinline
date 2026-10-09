# Rotation agent status / ownership board

Update through the coordinating session to avoid shared-worktree edits colliding.
This board is a handoff snapshot, not a running task service.

## Repository state at handoff

- Branch `endpoint-rotation-blocking`; base HEAD `6b1aaef`.
- Rotation tests/docs are uncommitted/untracked; preserve them.
- No subagents are currently claimed by this board.
- No production rotation implementation, stable API or derivation scheme selected.
- Core merge markers resolved via dev merge (35564bc); full p2pcore suite 121 passed (A1).

## Latest executed checks

| Command (from worktree) | Result |
|---|---|
| `python3 scripts/test_rotation_discovery_model.py` | 6 passed |
| `python3 scripts/test_rotation_network_model.py` | 10 passed |
| `python3 scripts/buildlock.py --who endpoint-rotation -- cargo test -p proto --test rotation_permit_model --test rotation_succession_model --test rotation_identity_boundary` | 33 passed: 18 permit/network/lifecycle + 10 earlier succession/network + 5 identity |

No full-core/workspace, actual transport or Android result is claimed. Documentation
handoff files were added afterward without changing those test sources.

## Task ownership

| Packet | State | Owner | Reserved files | Result / next gate |
|---|---|---|---|---|
| A baseline/audit | READY | unclaimed | none | reproduce tests, inspect inherited core failures |
| B history/permits | READY models | unclaimed | none | bounded current-session history and recovery proposals |
| C lifecycle/time | READY models | unclaimed | none | clock/configuration/storage failures |
| D author/group adapter | READY research | unclaimed | none | author/Loro/succession contract |
| E linked discovery | READY research | unclaimed | none | decision matrix; no scheme selected |
| F production proto | WAIT | unclaimed | none | approved model/policy/schema |
| G core/persistence | WAIT | unclaimed | none | F + baseline/migration/storage contracts |
| H Android/product | WAIT | unclaimed | none | G + lifecycle policy |

Shared model types, module registration, Cargo files, core/node.rs and planning
files are coordinator-owned until explicitly reserved for one agent. Standalone
new-file work is preferred during parallel investigation.

## Decision log

| Topic | Current state |
|---|---|
| Periodic configurable rotation, independent of blocking | user direction |
| Owner-replaceable device IDs | user direction; permanent-slot assumption superseded |
| Succession separate from delegated group rights | agreed integration boundary |
| Expired history vs live permission separation | candidate tested; final expiry/revocation policy still gated |
| Overdue resume creates one successor | candidate modeled, not frozen product policy |
| Atomic snapshot commit before announcement | required ordering; real storage guarantees not implemented |
| Per-device durable nonce high-water | test assumption, not recommended production API yet |
| Fork recovery / older-ancestor revocation | unresolved |
| Linked-device future endpoint calculation | requirement under investigation; no construction selected |
| Shared mnemonic hostile-device revocation | current limitation confirmed; no stronger guarantee |
| Author-key / Loro writer migration | unresolved |

## Wave 1 packet board (see rotation-agent-packets.md)

2026-10-10: branch now holds rotation work (01ee106), imported group-chat work (a3b8829) and
presence+dev (merge 35564bc). Baseline at 35564bc: Python 6 + 10; proto rotation 33; group-sim
24; `cargo check -p p2pcore --tests` ok. Subagents (Opus) run in isolated worktrees from this
branch and commit only owned files; coordinator merges.

| Packet | Batch | State | Owner | Owned files |
|---|---|---|---|---|
| A1 | 1 | READY | unclaimed | docs/design/rotation-baseline-report.md |
| B1 | 1 | READY | unclaimed | tests/rotation_history_paging.rs, rotation-history-policy-options.md |
| C1 | 1 | READY | unclaimed | tests/rotation_time_contracts.rs, rotation-time-contract.md |
| D1 | 1 | READY | unclaimed | rotation-author-key-trace.md |
| A2 | 2 | READY | unclaimed | rotation-lifecycle-hooks.md |
| B2 | 2 | READY | unclaimed | tests/rotation_history_adversarial.rs |
| C3 | 2 | READY | unclaimed | tests/rotation_challenge_contracts.rs, rotation-challenge-options.md |
| E1 | 2 | READY | unclaimed | rotation-linked-discovery-options.md, scripts/test_rotation_linked_discovery_options.py |
| B3 | 3 | READY | unclaimed | tests/rotation_fork_revocation.rs, rotation-fork-revocation-options.md |
| C2 | 3 | READY | unclaimed | tests/rotation_storage_contracts.rs, rotation-storage-failure-contract.md |
| D2 | 3 | WAIT D1 | unclaimed | rotation-author-group-adapter.md |
| G-A | 1 | READY | unclaimed | docs/group-sim-coverage-audit.md |
| G-B | 2 | READY | unclaimed | group-sim/src/policy_candidates.rs, docs/group-policy-options.md |
| G-C | 3 | WAIT C2 | unclaimed | group-sim/src/persistence.rs, docs/group-persistence-contract.md |
| G-D | later | WAIT B1,B3 + decisions | unclaimed | group-sim/src/reachability.rs |

## Per-agent result entries

Append reports here or link dedicated reports after coordinator review. Include
exact files, commands/results, assumptions, rejected alternatives and next step.
Do not mark a WAIT packet done based only on test fixtures.
