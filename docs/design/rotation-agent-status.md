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
| A1 | 1 | MERGED | rot/a1 56e774b; p2pcore 121 passed; O1–O8 findings | docs/design/rotation-baseline-report.md |
| B1 | 1 | MERGED | rot/b1 02792b5; 12 tests | tests/rotation_history_paging.rs, rotation-history-policy-options.md |
| C1 | 1 | MERGED | rot/c1 f2ca695; 19 tests | tests/rotation_time_contracts.rs, rotation-time-contract.md |
| D1 | 1 | MERGED | rot/d1 502d90f; doc | rotation-author-key-trace.md |
| A2 | 2 | MERGED | rot/a2 d348b11; doc | rotation-lifecycle-hooks.md |
| B2 | 2 | MERGED | rot/b2 ba3fdea; 14 tests (analysis in rotation-open-decisions Q5) | tests/rotation_history_adversarial.rs |
| C3 | 2 | MERGED | rot/c3 dc7157d; 14 tests | tests/rotation_challenge_contracts.rs, rotation-challenge-options.md |
| E1 | 2 | MERGED | rot/e1 fd7b917; 14 python tests | rotation-linked-discovery-options.md, scripts/test_rotation_linked_discovery_options.py |
| B3 | 3 | MERGED | rot/b3 49e40b2; 15 tests | tests/rotation_fork_revocation.rs, rotation-fork-revocation-options.md |
| C2 | 3 | MERGED | rot/c2 73945dc; 13 tests | tests/rotation_storage_contracts.rs, rotation-storage-failure-contract.md |
| D2 | 3 | MERGED | rot/d2 f248eb9; doc | rotation-author-group-adapter.md |
| G-A | 1 | MERGED | rot/ga f8bca01; audit doc | docs/group-sim-coverage-audit.md |
| G-B | 2 | MERGED | rot/gb b3ac944; +23 tests, group-sim now 73 | group-sim/src/policy_candidates.rs, docs/group-policy-options.md |
| G-C | 3 | MERGED | rot/gc fa8357b; +15 tests, group-sim now 50 | group-sim/src/persistence.rs, docs/group-persistence-contract.md |
| G-H | 3 | MERGED | rot/gh a60e8f8; group-sim 24→35, 2 sim bugs fixed | group-sim sources + README |
| K1 | 3 | MERGED | rot/k1 77ab375; 8 core tests, confirmed restore bugs | crates/core/tests/rotation_restore_characterization.rs |
| G-D | later | WAIT B1,B3 + decisions | unclaimed | group-sim/src/reachability.rs |

Wave 1 complete 2026-10-10 at `7ff55f0`. Final regression: `cargo test -p proto` 166 passed
(46 unit + 120 rotation model), `cargo test -p p2pcore` 129 passed (121 + 8 K1), group-sim 73,
Python 6 + 10 + 14. Log `/tmp/claude-1000/rotation-coord-final.log`.

Coordinator verification after each merge: owned-files-only diff, reran the packet's tests in
the coordinator checkout (counts above). Note: shared CARGO_TARGET_DIR can run stale binaries
from other worktrees — `touch` the crate sources before verifying group-sim.

User decisions are collected in `rotation-open-decisions.md`.

Wave 2 regression at 198c1f1: proto 180, p2pcore 134, group-sim 113 — all passing. Log `/tmp/claude-1000/rotation-coord-w2.log`.

## Wave 2 packet board (user decisions U1–U6, 2026-10-10)

| Packet | State | Branch / worktree | Owned files |
|---|---|---|---|
| W2-R | MERGED fcdb335, +11 | rot/w2r `.claude/worktrees/rot-w2r` | group-sim/src/host_relay.rs (+ mod line) |
| W2-Q | MERGED c954d56, +18 | rot/w2q `.claude/worktrees/rot-w2q` | group-sim/src/chat_request.rs (+ mod line, group-sim dev-deps) |
| W2-B | MERGED 020a91e, +14 proto +5 core | rot/w2b `.claude/worktrees/rot-w2b` | proto/tests/rotation_silent_block.rs, core/tests/block_characterization.rs |
| W2-D | MERGED d3d6467, +17 proto +7 core | rot/w2d `.claude/worktrees/rot-w2d` | proto/tests/rotation_device_list_push.rs, core/tests/device_list_characterization.rs |
| W2-F | MERGED 9d0fbd2, +16 | rot/w2f `.claude/worktrees/rot-w2f` | group-sim/src/flags.rs (+ mod line), docs/group-flag-removal-model.md |
| W2-M | MERGED eb21b91, +18 | rot/w2m `.claude/worktrees/rot-w2m` | group-sim/src/multi_host.rs (+ mod line), docs/group-multi-host-model.md, KIND relabels |
| W2-H | MERGED 77793b1, +16 | rot/w2h `.claude/worktrees/rot-w2h` | group-sim/src/host_only.rs (+ mod line), docs/group-host-only-model.md |
| W2-G | MERGED 380cd87, +11 | rot/w2g `.claude/worktrees/rot-w2g` | group-sim/src/block_in_group.rs (+ mod line) |

## Per-agent result entries

Append reports here or link dedicated reports after coordinator review. Include
exact files, commands/results, assumptions, rejected alternatives and next step.
Do not mark a WAIT packet done based only on test fixtures.
