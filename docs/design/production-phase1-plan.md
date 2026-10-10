# Production phase 1: fix shipped bugs on `dev`

Status: coordinator plan, 2026-10-10. Approved by the user to start ("yes lets go"). Merging into
`dev` still needs the user's explicit OK.

## Branches

- Integration branch `fixes-phase1`, created from `dev` @ `941df24`, worktree
  `/home/abe/p2p_chat/.claude/worktrees/fixes-phase1`. Only the coordinator merges into it (`--no-ff`).
- Each packet gets branch `fix/<id>` + worktree `/home/abe/p2p_chat/.claude/worktrees/fix-<id>`, created
  by the coordinator from `fixes-phase1`.
- Characterisation tests live on `endpoint-rotation-blocking` (not on `dev`). A packet brings the
  test file it needs with `git show endpoint-rotation-blocking:<path> > <path>` and turns the
  `// KIND: limitation` tests it fixes into `// KIND: invariant` tests asserting the correct
  outcome (keeping the scenario).
- Build: `CARGO_TARGET_DIR=/home/abe/p2p_chat/.claude/worktrees/fixes-phase1/target` (separate
  from the rotation target), always `python3 scripts/buildlock.py --who fix-<id> -- sh -c 'touch
  crates/<crate>/src/*.rs && cargo test ...'` (touch INSIDE the lock).
- Core runs use `--no-fail-fast`. Baseline on `dev`: run it first and record the numbers.

## Packets

| ID | Fix | Decision basis | Main files | Batch |
|---|---|---|---|---|
| P1 | Restore chat stall: accept history per message, not all-or-nothing; contact's sibling device too | U10 (section 12 Q1 b), section 5 bugs 1, 3 | `chat/engine.rs` (~1813 `apply_remote`), `chat/own.rs`, `chat/doc.rs` | 1 |
| P2 | Dead ticket after restore: drop/reissue the cached ticket when the device key changes | section 5 bug 2 | `node.rs` ticket (~1908, `st.ticket`) | 1 |
| P7 | Flaky `link.rs` self-probe test (`redeemed_nonces_union_and_strangers_cannot_open_self_sync`) | test hygiene | `crates/core/tests/link.rs` only (test fix; if a real race in core, report, don't fix) | 1 |
| P4 | Push signed device list to contacts on every change; seq bound | U8; W2-D findings | `node.rs`, chat wire, `proto` device list | 2 (after P1) |
| P5 | Visible blocking: one signed notice on block + same reason on every later attempt (calls, chat, history fetch, ticket) | U5 revised, section 8b Q1 | `proto` (notice type), `node.rs`, `chat/engine.rs` | 3 |
| P6 | Separate `message` / `call` tokens; defaults; legacy migration with per-DID cut-off; compatibility with old peers | U13, U15 | `proto` grant, `node.rs`, `chat/engine.rs` | 4 (after P5) |
| UI | Android + desktop: "blocked" state and notice, "allow calls" toggle | U5 revised, U13 | apps | after P5/P6 core |

Batches are sequential where packets share `node.rs` / `chat/engine.rs` (P4, P5, P6).

## Open details needing the user before their packet starts

- P1: under per-message acceptance, a pre-restore message signed by the old key is still
  individually rejected by a contact that never knew that key. Options: accept it is lost; the
  sender re-signs its own unacknowledged messages with the current key after restore; or the
  receiver keeps superseded keys of the same DID for verification only (section 12 Q1 a). P1
  reports what happens and does not choose.
- P4: seq bound value (lean: refuse a list whose seq exceeds receiver now + 1 day in ms).
- P5: notice wording (section 8b Q3), unblock behaviour (8b Q5).
- P6: message-token renewal (section 14 Q3), "calls not allowed" visibility (Q6).

## Not in phase 1

Content signed by DID + writer ID (U4 revised, U16): phase 2, the foundation for groups and rotation.
Ticks regression and verified flag after restore (section 5 bugs 4, 5).
