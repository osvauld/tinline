# Production phase 1: fix shipped bugs on `dev`

Status: coordinator plan, 2026-10-10. Approved by the user to start ("yes lets go"). Merging into
`dev` still needs the user's explicit OK.
Merge gate (user, 2026-10-10): before merging into `dev`, the user tests a build of
`fixes-phase1` on their own devices (phone ZD22257P6G debug build + desktop), and groups are
simulated first.

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
| P8 | Sign chat messages with the DID key (U4 revised). Verify DID signatures; keep verifying existing device-key signatures for old history. Transition: dual-sign (device `sig` for un-upgraded peers + DID signature), new peers prefer the DID signature | U4 revised; user 2026-10-10 "each message is signed with the main signing key" | `chat/doc.rs` (`sign_msg` :171, sign sites :394 :424, verify :555), `chat/engine.rs`, `chat/own.rs` | 2 (after P1; P7 may run alongside) |
| P9 | Writer ID = hash(DID ‖ device value) as the Loro peer ID, set by the app and verified per post (U16); replaces the device-key-derived peer ID | U16 | `chat/doc.rs` peer ID, `sync/accdoc.rs`, own-device sync | 3 (after P8) |
| P4 | Push signed device list to contacts on every change; seq bound | U8; W2-D findings | `node.rs`, chat wire, `proto` device list; lower priority after P8 (no longer needed for chat verification, still for reachability) | 4 |
| P5 | Visible blocking: one signed notice on block + same reason on every later attempt (calls, chat, history fetch, ticket) | U5 revised, section 8b Q1 | `proto` (notice type), `node.rs`, `chat/engine.rs` | 5 |
| P6 | Separate `message` / `call` tokens; defaults; legacy migration with per-DID cut-off; compatibility with old peers | U13, U15 | `proto` grant, `node.rs`, `chat/engine.rs` | 6 (after P5) |
| UI | Android + desktop: "blocked" state and notice, "allow calls" toggle | U5 revised, U13 | apps | after P5/P6 core |

Batches are sequential where packets share `node.rs` / `chat/engine.rs` (P8, P9, P4, P5, P6).
Order changed 2026-10-10 (user): DID signing (P8) moved ahead of P4/P5/P6 because it removes the
cause of the restore problems (a restore changes the device key; the DID never changes).

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

Signing of other records by the device key (account doc, own-device sync, attestations; see
`rotation-author-key-trace.md` R3–R5) beyond chat messages: later packet.
Ticks regression and verified flag after restore (section 5 bugs 4, 5).

## Progress (coordinator)

On `fixes-phase1`: P2 `41b27cb`, P1 `cb65ee7`, P7 `eb8c6d3`, o1 test flip `4e05da3`, P8 `ce4be54`,
P9 `8b8b907`. Suite at `8b8b907`: core 160 + proto 46; one failure `link_expiry::an_expired_qr_is_refused`
(pre-existing timing flake: `link_new_scan` returned `Err("that code has expired")` directly
instead of reporting via the event; passes 3/3 alone; link code untouched by phase 1).

## Follow-ups found during phase 1

- P9b (recommended before release, P8 format not shipped yet so it can change freely):
  bind the writer value / writer ID into the DID-signed bytes so vouched history snapshots
  cannot put ops under a not-yet-seen sibling writer ID (P9 open question 1); and tie the device
  value to a stable install ID instead of the device key, so endpoint rotation does not change
  the writer ID (P9 Q3).
- DID-signature requirement cut-off (P8 Q1): coordinator lean = per contact, once that contact
  has sent DID-signed versions. Needs user decision.
- P10 (approved by user, after P9b): contact re-add divergence fix — a re-created conversation
  gets a fresh epoch in the writer ID so (writer, counter) pairs never repeat. Test first:
  remove, re-add and write the same day; the contact receives the new messages.
  Writer ID model clarified with user: writer ID = hash(domain ‖ DID ‖ device ID), device ID =
  stable random per-install ID (not a key), proven by the DID signature.
- (was) Pre-existing bug: removing a contact deletes the conversation but keeps peer/counter state;
  re-adding and writing the same day reuses (peer, counter) the contact already holds -> silent
  Loro divergence (P9 Q4). Needs a test + fix packet.
- Pre-existing: `chat_open` does not retry when `chat.redb` is still locked (P9 Q4).
- Possible product race: a crashed device may not reconnect for ~20 s when its stale self-sync
  session was dialled by the lower key (P7 finding 2).
- Flakes: `link_expiry::an_expired_qr_is_refused`; `link.rs` unlink pair (P7 finding 4).
- Docs: `docs/chat.md`, `docs/protocol.md` not updated for P8/P9.

## Design and acceptance tests (2026-10-10)

- UI design canvas (user asked for UI, not system diagrams): https://claude.ai/artifact/KS9HUDfQzRFX9tpayTt76a
  Android boards: Groups, Group info, Removal, Chat request, Blocking, Privacy (calls permission,
  address changes, lost contact); Desktop: group chat, Hosting, asked-to-host, contact panel.
  Choices made in the design and not yet decided by the user: declined chat requests are shown as
  "declined"; a blocked member's group messages show collapsed.
- R1a (`fix/r1a`): real-node acceptance tests for rotation and visible blocking, ignored until the
  features exist. R1b (`rot/r1b`): silent-block model rewritten to visible blocking; superseded
  group-sim tests removed or rewritten.
- Cleanup: 31 merged rot-*/fix-* worktrees removed (branches kept). `agent-*` worktrees belong to
  other sessions (some dirty) and were left alone.

## Phase 2 (overnight, 2026-10-10; user away until morning, "you go")

Integration branch `phase2` from `fixes-phase1` @ `bd5e1e9` (so the device-test build of
`fixes-phase1` stays unchanged). Common packet rules: `/home/abe/p2p_chat/.claude/worktrees/phase2-common.md`
(choices not covered by U1–U24 are made conservatively and marked PROVISIONAL for the user).

| Packet | Branch / worktree | Scope |
|---|---|---|
| P6 | `fix/p6` / `fix-p6` | separate message/call tokens, allow-calls API, "calls not allowed" reason |
| P10 | `fix/p10` / `fix-p10` | re-add divergence (writer, counter never repeat), chat_open retry, deterministic edit winner (flaky test) |
| G1 | `grp/core` / `grp-core` | group chat in core: records + signing, group endpoint, hosts, invite/join, post, multi-device, 3-flag removal, UniFFI API |
| UI-W | `ui/wire` / `ui-wire` (from `test/phase1-ui`) | apps use the real block API; no "remove contact"; block copy; desktop Groups nav |

Still not merged into `dev`; nothing pushed.

Phase 2 progress: P10 merged `c0c69a4`, P6 merged `d0df454`. Coordinator run on `phase2` @ `d0df454`
(`cargo test -p proto; cargo test -p p2pcore --no-fail-fast`): 281 passed, 0 failed, 16 ignored.
Started from `d0df454`: P4 (`fix/p4`, device-list push) and P10b (`fix/p10b`, undelivered own
messages after a writer-ID change are still pushed).
PROVISIONAL choices to confirm with the user: P10 delete beats edit, ties to higher writer ID;
wire bumped to chat/3, self/3, calls osvauld/p2p/1. P6 "calls not allowed" shown to any contact;
message token renews within 30 days of expiry; downgrade not announced; re-add keeps the previous
call permission; legacy v1 grants accepted as message+call.

Phase 2 progress (2026-10-11 early): P10b merged `618cdb7` + follow-up `84f4f04`; P4 merged `474924d`;
UI-W done on `ui/wire` (apps use the real block API, allow calls; desktop 57 tests); G1 group core
merged `f3b901b`. Coordinator run on `phase2` @ `f3b901b`: proto + p2pcore 343 passed, 0 failed,
16 ignored. Test build `test/phase2-ui` (= ui/wire + phase2 before G1) built, NOT installed (wire
bump chat/3, self/3, calls p2p/1 needs both devices updated and likely a wipe; user is device-testing
phase 1). Started: G-UI (`ui/groups-wire`, apps on the real group API), G2 (`grp/g2`: group store
loss, own-device group sync, U3 chat requests via host, U12 host rotation).
PROVISIONAL choices from G1 are listed in `docs/group-core.md` (creator's device first host; only
owner/admins invite, 7-day bearer invites; owner can't be flagged or leave; removal final; etc.).
