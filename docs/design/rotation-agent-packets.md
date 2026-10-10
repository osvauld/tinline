# Rotation Wave 1 — subagent-ready packets

Status: coordinator-authored assignment sheet (2026-10-09). Expands Wave 1 of
`rotation-agent-tasks.md` (packets A–E) into tasks small enough for one Opus
agent each. Packets F–H remain WAIT and are not expanded here.

Branch state (2026-10-10): `endpoint-rotation-blocking` @`35564bc` contains the rotation
models (`01ee106`), the group-chat work imported from the `group-chat` worktree (`a3b8829`;
that worktree is now a stale duplicate), and `presence` + `dev` merged in (node.rs conflict
markers resolved by dev). This branch is the single home for rotation AND group-chat work.

Coordinator baseline at `35564bc`:

```
python3 scripts/test_rotation_discovery_model.py   # 6 passed
python3 scripts/test_rotation_network_model.py     # 10 passed
python3 scripts/buildlock.py --who rotation-coord -- cargo test -p proto \
  --test rotation_identity_boundary --test rotation_succession_model --test rotation_permit_model
# 33 passed (5 + 18 + 10)
python3 scripts/buildlock.py --who rotation-coord -- cargo test -p group-sim   # 24 passed
python3 scripts/buildlock.py --who rotation-coord -- cargo check -p p2pcore --tests   # ok
```
(The core crate's package name is `p2pcore`.)

Group-chat state: see `docs/STATUS-group-chat.md`. Test-only `crates/group-sim` consumes a MOCK
`SuccessionAdapter::verified_path(&Nomination) -> Option<VerifiedSuccession>` in
`crates/group-sim/src/reachability.rs`, whose contract is "full verified ancestry since the
nominated root, no omitted endpoint-bearing predecessors". Group policy gates D01–D06 and
persistence scenarios S01–S07 are in `docs/group-chat-tests.md`.

---

## 0. Rules every packet inherits (paste into each agent prompt)

1. You run in your own isolated git worktree created from `endpoint-rotation-blocking`.
   First run `git log --oneline -1 35564bc` and `git merge-base --is-ancestor 35564bc HEAD`;
   if 35564bc is not in your history, STOP and report. Do not touch any other worktree
   (`/home/abe/p2p_chat/.claude/worktree/*` are READ-ONLY, including the stale `group-chat`).
   No rebase/reset/push. When done, commit ONLY your owned files on your worktree branch
   with explicit paths (never `git add .`/`-A`), message prefixed `[<PACKET>]` and ending
   `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. The coordinator merges.
2. Create/edit ONLY the files listed under "Owns". Every other file is read-only,
   including existing `crates/proto/tests/**`, `scripts/test_rotation_*.py`,
   `Cargo.toml`/`Cargo.lock`, `crates/proto/src/**`, all status/plan/handoff docs.
   Need a shared change? Put it under "Interface changes requested" in the report.
3. Rust model tests go in a NEW top-level file `crates/proto/tests/<name>.rs` (cargo
   auto-discovers it as its own test target). It must be self-contained: copy any
   fixture you need (e.g. `Claims`/`Permit`/`View` from `rotation_permit_model.rs`)
   into your file with a comment `// Copied from rotation_permit_model.rs @6b1aaef;
   coordinator consolidates later.` Do not `#[path]`-include existing support modules.
   Use only existing proto deps: `identity`, `ed25519-dalek`, `sha2`, `serde`,
   `serde_json`, `rand`. Use real signatures; never a "signature always valid" stub.
4. Build/test ONLY through buildlock and ONLY your own target plus the baseline:
   ```
   python3 scripts/buildlock.py --who rotation-<PACKET> -- cargo test -p proto --test <your_target> 2>&1 | tee /tmp/claude-1000/rotation-<PACKET>.log
   ```
   Finish with one run that also includes `--test rotation_identity_boundary
   --test rotation_succession_model --test rotation_permit_model` (must stay 33 passed).
   group-sim packets: `cargo test -p group-sim` instead (baseline 24 passed).
   Use log paths under `/tmp/claude-1000/` (e.g. `/tmp/claude-1000/rotation-<PACKET>.log`).
   Never run workspace-wide `cargo test`/`cargo fmt`; `rustfmt <your file>` only.
5. Fake time (`u64` ticks), deterministic seeds, in-memory state only. No iroh,
   no filesystem, no real clock, no network.
6. Label every test with a leading comment, exactly one of:
   `// KIND: invariant` (must hold for any acceptable policy),
   `// KIND: candidate-policy <GATE-ID>` (encodes one option of an open gate),
   `// KIND: limitation` (passes by demonstrating a missing guarantee).
   Rejection tests must assert the accepted state is byte-for-byte unchanged
   (compare a clone/serialization before and after), not only that an error returned.
7. Policy gates (G-DISCOVERY, G-AUTHORITY, G-FORK, G-REVOCATION, G-HISTORY, G-TIME,
   G-RENEWAL, G-KEYS, G-RETIREMENT, G-BLOCK, G-STORAGE — see handoff §3) are NOT yours
   to decide. Where an oracle is unresolved: model ≥2 options as separate
   `candidate-policy` tests or a parameter, and list the question in the report.
   Never invent a key-derivation/ratchet scheme. Never claim shared-mnemonic revocation.
8. Docs you own: state at the top `Status: proposal — not approved`. Cite source as
   `path:line`. Separate "verified in source/tests" from "proposed".
9. Stop and report (don't improvise) if: an owned file already exists, baseline
   tests fail before your change, or the task needs a shared-file edit.
10. Final message = the completion report template in `rotation-agent-tasks.md`
    ("Common subagent completion report"), plus the exact test names you added
    with their KIND label, and the last `test result:` line(s) verbatim.

---

## Packet overview and scheduling

| ID | Title | Type | Owns (new files) | Depends on | Max size |
|---|---|---|---|---|---|
| A1 | Core baseline & EndpointId dependency map | read-only audit | `docs/design/rotation-baseline-report.md` | — | doc |
| A2 | Endpoint lifecycle hook map | read-only audit | `docs/design/rotation-lifecycle-hooks.md` | — | doc |
| B1 | Multi-page history + live possession of newest key | Rust model | `crates/proto/tests/rotation_history_paging.rs`, `docs/design/rotation-history-policy-options.md` | — | ~12 tests |
| B2 | Adversarial delivery, replay, ID collisions | Rust model | `crates/proto/tests/rotation_history_adversarial.rs` | — | ~12 tests |
| B3 | Forks and older-ancestor revocation | Rust model + options | `crates/proto/tests/rotation_fork_revocation.rs`, `docs/design/rotation-fork-revocation-options.md` | — | ~10 tests |
| C1 | Timer, interval config, clocks | Rust model + contract | `crates/proto/tests/rotation_time_contracts.rs`, `docs/design/rotation-time-contract.md` | — | ~12 tests |
| C2 | Storage boundaries and failure matrix | Rust model + contract | `crates/proto/tests/rotation_storage_contracts.rs`, `docs/design/rotation-storage-failure-contract.md` | A2 helpful, not required | ~12 tests |
| C3 | Challenge / possession session options | Rust model + options | `crates/proto/tests/rotation_challenge_contracts.rs`, `docs/design/rotation-challenge-options.md` | — | ~8 tests |
| D1 | Transport-key use trace (author/Loro/proofs) | read-only audit | `docs/design/rotation-author-key-trace.md` | — | doc |
| D2 | Author/group adapter contract | doc | `docs/design/rotation-author-group-adapter.md` | D1 | doc |
| E1 | Linked-device discovery decision matrix | research + Python | `docs/design/rotation-linked-discovery-options.md`, `scripts/test_rotation_linked_discovery_options.py` | — | doc + ~8 tests |

Suggested batches (≤5 concurrent; buildlock serializes builds):
- Batch 1: A1, B1, C1, D1, G-A
- Batch 2: A2, B2, C3, E1, G-B
- Batch 3: B3, C2, D2, G-C, G-H, K1
- Later: G-B after G-H (shares tests.rs); G-D after G-HISTORY/G-FORK decisions
(2026-10-10: actual dispatch is rolling — packets start as soon as dependencies merge, ≤5 live.)
Coordinator reviews each batch, updates the status board, then launches the next.

---

## A1 — Core baseline & EndpointId dependency map

**Goal:** know exactly what in core breaks or must migrate when a device's transport key changes.

Read: handoff §2; `endpoint-rotation-device-audit.md`; `crates/core/src/{node.rs,store.rs,sync/*.rs}`;
`crates/proto/src/{lib.rs,link.rs}`; `docs/protocol.md`; `docs/design/{device-linking,multi-device}.md`.

Steps:
1. Record `git rev-parse --short HEAD`, `git status --short`.
2. Merge markers are resolved (dev `0f36a29`, merged at `35564bc`); just confirm
   `grep -c '^<<<<<<<' crates/core/src/node.rs` is 0.
3. Run `python3 scripts/buildlock.py --who rotation-A1 -- cargo test -p p2pcore 2>&1 | tee /tmp/claude-1000/rotation-A1-core.log`
   and record pass/fail counts per test target and any failure names verbatim. Classify
   failures as pre-existing (do not fix). This is the first full-core baseline claim.
4. Build a table of every type/field/function/key that stores, signs with, indexes by or
   derives from a device transport public key / `EndpointId` / `device_secret`:
   registry (`OwnDevice`), `DeviceList`, tombstones, link grants, invitations, grants,
   contacts, blocks, chat sessions/batch signatures, self-sync incl. new `chat/own.rs`
   (34g own-device chat sync), Loro peer IDs, blobs, relay hints, call routing,
   presence (new `chat/presence.rs`, EphemeralStore). Columns: `path:line` | what it keys/signs |
   persisted? (where) | effect if key rotates | migration needed (yes/no/unknown).
5. List where contact removal implies block (path:line) and every block check site.

Owns: `docs/design/rotation-baseline-report.md`.
Acceptance: table covers every `grep -rn "device_secret\|EndpointId\|NodeId\|PublicKey" crates/core/src`
hit (or explicitly marks hits as irrelevant); merge-marker comparison is concrete; check result recorded.
Not in scope: fixing anything, proposing protocol.

## A2 — Endpoint lifecycle hook map

**Goal:** know where a rotation scheduler would start/stop endpoints and where overdue retirement must run.

Read: `crates/core/src/node.rs` (endpoint creation, accept loop, shutdown), account switching,
vault lock/unlock, desktop app crate, Android bridge/service code (find via `grep -rn "Endpoint::builder\|bind(\|shutdown\|unlock\|on_resume\|onResume\|BOOT_COMPLETED"`).

Deliver a doc with: (1) sequence of endpoint startup → accept → shutdown with `path:line`;
(2) who owns the endpoint object (one per account? per install?) and how account switch
replaces it; (3) vault lock/unlock and where secrets become available; (4) Android
foreground service, boot, resume, network-change hooks and which core calls they make;
(5) every in-flight thing tied to an endpoint (calls, chat sessions, transfers, retries)
and how it is cancelled today; (6) candidate insertion points for "rotate now",
"retire old", "enforce overdue on resume" — labeled proposal.

Owns: `docs/design/rotation-lifecycle-hooks.md`. Read-only otherwise. No builds required.

## B1 — Multi-page history with live possession of the newest key

**Goal:** resolve the known failure: possession was bound to the last endpoint of a page,
which may be retired when catch-up spans several pages.

Read: `rotation_permit_model.rs` (Claims/Permit/View/page/live), `support/rotation_permit_network.rs`,
`endpoint-rotation-permit-model.md`, group contract item 1–2.

Model: copy Claims/Permit/View. Separate two operations:
`verify_history_page(view, page) -> Result<View>` (signatures/predecessors/scope only, no
possession) and `bind_live(view, newest_permit, challenge, proof, now) -> Result<LiveBinding>`
(possession of newest endpoint key only).

Required tests:
1. 3+ pages (e.g. 13 successors, PAGE=4): all intermediate keys discarded/retired; receiver
   accepts history then live binding proves only the newest key. (invariant)
2. Live proof by a retired intermediate key is rejected; state unchanged. (invariant)
3. History verified but no live proof → historical knowledge advances, no dialable route. (invariant)
4. Live proof for a key not at the verified head (head not yet reached) rejected. (invariant)
5. Pages interrupted mid-way, resumed later from the receiver's cursor. (invariant)
6. Strict full history vs owner-signed checkpoint: implement BOTH as candidate-policy G-HISTORY
   tests (checkpoint = DID-signed record of `{owner, audience, device path anchor, generation,
   head id}`); show what each accepts/rejects for the same scenario, including a receiver whose
   cursor predates the checkpoint.
7. Group compatibility: the group mock requires FULL ancestry from the nominated root.
   State, with a test, whether the checkpoint option can satisfy that contract (e.g. a
   checkpoint signed per audience that the group side accepts as the root) or would require
   the group contract to change. Do not change the group contract; report it.
8. Work bounds: count signature verifications and records retained per receiver for N=1000
   rotations under each option; assert computed bounds (not timing).

Doc (`rotation-history-policy-options.md`): strict vs checkpoint table — bytes/records/CPU per
catch-up, storage per receiver, anti-rollback state location, what "historical legitimacy" vs
"current permission" vs "creation time" each proves; open questions for G-HISTORY.

## B2 — Adversarial delivery, replay and ID collisions

**Goal:** invalid input never changes accepted state; bounded queues; explicit recovery outcome.

Read: same as B1.

Required tests (each asserts unchanged state on rejection):
1. Dropped middle page, duplicate page, page reordering (all permutations of 3 pages). (invariant)
2. Wrong predecessor; splice across owner, audience, and device path. (invariant)
3. Malformed: truncated signature, unknown version, oversized claims (define a test byte cap
   constant and mark it test-only), page longer than PAGE. (invariant + candidate-policy G-HISTORY for the cap)
4. Queue saturation: more pending pages than the per-sender bound → bounded, oldest/newest
   drop policy shown as two candidate-policy variants; no unbounded growth. (candidate-policy)
5. Receiver missing retained history (cursor unknown/pruned) → explicit `RecoveryRequired`
   outcome; high-water mark not reset. (invariant)
6. Replayed expired historical permit cannot create a live binding or route. (invariant)
7. Group-audience verification never requires private ancestors; private permit presented
   on group path rejected. (invariant)
8. Replacement-ID collision: new ID equal to a sibling's current ID, a sibling's historical ID,
   own historical ID → each rejected or flagged; list which require registry-level state
   (`limitation` where path-local checks cannot see it).

No doc owned; put analysis in the completion report.

## B3 — Forks and older-ancestor revocation

**Goal:** detect, never silently resolve, forks; lay out revocation options.

Read: B1 inputs + `support/rotation_network.rs` (clone fork exchange), group contract item 3.

Required tests:
1. Same-generation fork arriving at one receiver → conflict flagged, first-accepted kept, live
   use of either branch per candidate policy (two variants: freeze path / keep head). (candidate-policy G-FORK)
2. Fork evidence for an OLDER generation arriving after the receiver advanced past it → detected. (invariant: detected; handling = candidate-policy)
3. Two receivers accept different branches; exchanging evidence makes both flag; neither
   silently switches. (invariant)
4. Last-arrival-wins is demonstrably rejected. (invariant)
5. Revocation of anchor / head / older intermediate after descendants accepted: implement
   ≥2 candidate policies (a) cascade invalidates descendants, require fresh owner-signed
   recovery; (b) revocation only blocks that record's endpoint, descendants stand. Show
   concrete traces of what each permits for an attacker holding the revoked device's key
   AND the shared mnemonic (expect: (b) cannot exclude them; state it). (candidate-policy G-REVOCATION)
6. Revoked assignment (host-rights fixture) not restored by any succession. (invariant)

Doc (`rotation-fork-revocation-options.md`): option table, attacker-capability matrix
(old transport key only / mnemonic / both), what requires G-AUTHORITY change, questions for user.

## C1 — Timer, interval configuration and clocks

**Goal:** explicit time semantics proposal with deterministic tests.

Model: a `Scheduler` with persisted `{generation, last_rotation_at, interval, next_deadline}`
and two fake clocks: `monotonic` (resets on reboot) and `wall` (can jump).

Required tests:
1. No blocks → rotates at each deadline; block/unblock between never moves deadline. (invariant)
2. Interval change shorter/longer near deadline: implement ≥2 candidate effective-time policies
   (apply-from-now vs apply-from-last-rotation, with min-floor); invalid inputs 0, below minimum,
   overflow rejected without state change. (candidate-policy G-TIME; invariant for invalid input)
3. Sleep/resume and restart before/at/after deadline with k missed periods: exactly one
   successor on resume, next deadline = resume + interval (existing candidate) vs aligned
   grid alternative. (candidate-policy G-TIME)
4. Wall clock rollback by > interval and forward jump: generation never decreases; old
   endpoint never extended indefinitely; signed validity uses wall time, scheduling uses
   persisted elapsed accounting — show what breaks if scheduler uses wall time only. (invariant + limitation)
5. Reboot (monotonic reset) handled from persisted wall timestamps with a skew clamp. (candidate-policy)
6. Permit validity vs rotation interval: rotation failing for > validity → the old binding
   expires; show resulting unreachability explicitly. (candidate-policy G-RENEWAL)

Doc (`rotation-time-contract.md`): proposed semantics table, every parameter marked test-only,
questions for user (default/min interval, jitter, overdue policy, skew tolerance).

## C2 — Storage boundaries and failure matrix

**Goal:** a storage transition table that guarantees no generation reuse, no revival, no ACK before commit.

Model: explicit `Durable` vs `Volatile` structs; a fault injector that can crash or fail at
each named step: `GenerateKey`, `SealCommit`, `StartEndpoint`, `Announce(peer)`, `RetireOld`,
`ShutdownOld`; receiver steps `Verify`, `Commit`, `Ack`.

Required tests:
1. Crash at every sender step (loop over steps) → restart state satisfies: no generation used
   for two different keys; retired never revived; old not extended past deadline. (invariant)
2. Each step *fails* (not crash): defined outcome per step; candidate policies for StartEndpoint
   failure: fail-closed at deadline vs visible degraded with bounded grace. (candidate-policy G-RETIREMENT/G-STORAGE)
3. Receiver crash before/after Commit; ACK only after Commit. (invariant)
4. Two-phase "pending next key" persisted before announce; restart resumes same pending key. (candidate-policy)
5. Active old connection at retirement: terminate vs bounded overlap variants; assert no
   uninterrupted-call claim. (candidate-policy G-RETIREMENT)

Align vocabulary with the group task's planned persistence harness (durability boundary,
"ack only after commit", idempotent retry from persisted originals — see their STATUS doc
"Next work"); note shared terms in the doc so the two harnesses can later share fixtures.

Doc (`rotation-storage-failure-contract.md`): transition table (state → step → durable writes →
crash outcome), failure matrix, explicit note that fsync/power-loss/vault are not modeled.

## C3 — Challenge / possession session options

**Goal:** decide whether live possession needs durable nonces at all.

Required tests comparing two candidate-policy G-STORAGE designs:
(a) existing per-receiver durable nonce high-water + volatile outstanding set (8 / TTL 5);
(b) session-bound: proof signs a transcript containing a receiver-chosen random challenge and
both endpoint keys, valid only within one session, nothing persisted.
For each: replay across sessions, replay after receiver restart, capacity exhaustion by many
concurrent requests, peer isolation (one peer's flood doesn't block another), nonce overflow
(variant a). Doc compares disk writes per update, failure modes, and what a real QUIC/TLS
channel binding would provide (cite iroh/quinn API names only after reading them in
`~/.cargo/registry` sources; otherwise say "unverified").

## D1 — Transport-key use trace

Read-only. Trace every use of the local transport secret/public key beyond networking:
attestations, chat/self-sync batch signatures, stored proofs, Loro peer IDs (find derivation
code), blob/transfer auth. For each: `path:line`, what verifiers later need (old pubkey? DID?),
what breaks if the key rotates (old signatures, CRDT peer ranges, dedupe), and data at rest
that embeds the key. Output `rotation-author-key-trace.md`. No proposals beyond a short
"migration questions" list for D2.

## D2 — Author/group adapter contract (after D1)

Read D1 output, group contract, `docs/group-chat-coordination-*.md`, now in this branch:
`docs/{STATUS-group-chat.md,endpoint-rotation-coordination.md,group-chat.md}` and
`crates/group-sim/src/{reachability.rs,authority.rs}`. Map our proposed adapters
onto their existing mock `SuccessionAdapter`/`VerifiedSuccession`/`BindingEvidence`/
`Nomination` types: what they can keep, what fields are missing (expiry vs `retire_at`,
generation, permit id, revocation), and the full-ancestry vs checkpoint question from B1.
Compare (i) separate long-lived author key per device path vs (ii) author key rotates with
transport key + retained proof chain: clone/restore behavior, old signature verification,
Loro writer IDs, storage cost. Define candidate adapter signatures (Rust-like pseudocode,
NOT added to code) for: verified succession, current route, author-proof verification, host
assignment check, verified-DID policy query; audience-safe export. Mark which need group-agent
agreement. Output `rotation-author-group-adapter.md`. No group-worktree writes.

## E1 — Linked-device discovery decision matrix

Compare: (1) independent random endpoints + authenticated updates + explicit recovery route;
(2) shared prediction secret / epochs (public reachability only, never sibling transport
private keys); (3) rendezvous/address publication (relay mailbox or similar) separate from keys.
For each: online/offline windows, both sides retiring, clock skew, restore, clone, honest
unlinked device, hostile retained mnemonic, leaked discovery secret, epoch refresh excluding an
offline allowed sibling; what needs a reachable peer or fresh secret distribution.
Python file: deterministic symbolic model (no real crypto, no derivation formula) with
counterexample traces as tests. Only cite external constructions after reading their actual
docs; mark otherwise "unverified". End with exact yes/no questions for the user.
Do not edit existing `scripts/test_rotation_*.py`.

---

## Group-chat packets (work lives in `crates/group-sim`, docs in `docs/`)

Shared rules above apply; Rust tests are NEW modules under `crates/group-sim/src/`. Adding a
module requires one `mod` line in `crates/group-sim/src/tests.rs` (that is how `reachability`
is wired, with `#[path]` + `use super::*`); that `#[path]`+`mod` pair is the only allowed edit outside
owned files, and it must be reported. Read first: `docs/STATUS-group-chat.md`,
`docs/group-chat.md`, `docs/group-chat-tests.md`, `docs/group-rotation-integration-contract.md`,
`crates/group-sim/README.md`.

### G-A — Group-sim coverage audit (read-only + test run)

Run `cargo test -p group-sim` through buildlock. For each claim in `crates/group-sim/README.md`
and `docs/STATUS-group-chat.md` "Executable coverage", name the test(s) that cover it and
whether the test actually asserts it (negative controls present? unchanged-state checks?).
Map every conceptual case ID in `docs/group-chat-tests.md` to: executable test / partially /
none. List weak or vacuous tests. Owns: `docs/group-sim-coverage-audit.md`.

### G-B — Causal policy candidates D01–D05

Owns: `crates/group-sim/src/policy_candidates.rs`, `docs/group-policy-options.md`.
For each of D01–D05 implement ≥2 candidate oracles as `candidate-policy D0x` tests over small
traces with all delivery orders, reusing existing fixture builders (read-only). Must show for
each option: order-independence holds or not, what a removed member signing against a stale
frontier can achieve, partition behavior. Do not pick a winner; doc gives the comparison
table and exact questions for the user. D06 is B3's (rotation) — reference, don't duplicate.

### G-C — Persistence/crash harness S01–S07 (after C2 vocabulary exists; batch 3)

Owns: `crates/group-sim/src/persistence.rs`, `docs/group-persistence-contract.md`.
Explicit `Durable`/`Volatile` replica state and a crash/fail injector at: validate, scratch
import, commit, StoredAck, fan-out. Implement S01–S07 as tests. Use the same step/term names as
C2's `rotation-storage-failure-contract.md` where they overlap; state that real fsync/vault
durability is not modeled.

### G-H — Group-sim test hardening (from G-A audit §5; before G-B)

Owns edits to existing `crates/group-sim/src/{tests.rs,scheduler.rs,reachability.rs,authority.rs}`
and `crates/group-sim/README.md` (sole owner of these files while running). Apply the G-A §5
hardening table: non-vacuity guards (accepted-count/len assertions) and omitted-dependency
negative control on `seeded_reordered_loss_and_retry_schedules_converge`; a seeded schedule that
runs past `BINDING_LIFETIME` so retirement actually happens; isolated single-cause rejection tests
(attestation-to-author binding, `delegable=false`, issuer-must-be-parent-holder, cross-group with
post-state check); a reusable before/after replica snapshot helper (doc, VV, accepted, pending,
fanout) used by rejection tests on POPULATED replicas; make queue limits reachable and asserted;
make `connect` report a reason so routing-failure tests can tell why; mock path install checks the
whole path (reject `[nominated, sibling]`). Do not change semantics silently: if hardening exposes a
real bug, keep a failing-then-fixed pair and report it. Re-scope README claims to what is asserted.
Run `cargo test -p group-sim`; report old 24 → new count.

### G-D — Real succession verifier behind the group adapter (WAIT for B1 + B3 + user decisions)

Replace `MockRotation` usage with an adapter over the rotation verifier fixture, keeping the
mock for contrast. Not schedulable until G-HISTORY (full ancestry vs checkpoint) and G-FORK
are decided.

---

## K1 — Restore key-change characterization tests (core; current-product relevance)

A1 (O1–O5) and D1 (§7) found that restore-over-existing ALREADY changes the device key today, with
untested consequences. Owns NEW `crates/core/tests/rotation_restore_characterization.rs` only.
Read `crates/core/tests/restore.rs` and its helpers first and reuse them (read-only). Write tests
that characterise current behaviour after a restore that changes the device key, each labelled
`// KIND: limitation` if it demonstrates a defect or `// KIND: invariant` if it holds:
1. `ticket()` after the key change: is the cached ticket redeemable? (A1 O1, `node.rs:1910`)
2. A message sent but unacknowledged before restore: is it delivered afterwards? (O3)
3. Delivery ticks for pre-restore messages after restore. (O4)
4. A contact's OTHER device verifying our pre-restore history via relay/history path. (O2, D1 §2)
5. Contact's `verified` flag after our restore. (O6)
Do NOT fix production code; a limitation test asserts today's (wrong) outcome and names the
expected-correct outcome in a comment. Run `cargo test -p p2pcore --test rotation_restore_characterization`
through buildlock, then full `cargo test -p p2pcore` (baseline 121 passed).

---

## Wave 2 — host relay, chat requests, silent blocking (user decisions U1–U6)

Source of truth: `docs/design/rotation-open-decisions.md` section 0 (U1–U6). Still simulation
and characterisation only: no production code. Section 0 rules apply with these changes:
- Base commit is the Wave 2 coordinator commit (given in the prompt), not 35564bc.
- group-sim packets: create a NEW file `crates/group-sim/src/<name>.rs` and add exactly one
  `#[path = "<name>.rs"] mod <name>;` pair at the end of the existing list in
  `crates/group-sim/src/tests.rs` (that one edit is allowed). Reuse `super::*` fixtures
  (Author, Replica, envelopes) read-only. Baseline `cargo test -p group-sim` = 73 passed.
- Notes/requests are signed with real Ed25519 via the existing identity fixtures. A
  "rotation note" here is a sim stand-in for a permit: DID-signed (owner, audience, endpoint,
  monotonic seq). Do not build the real succession verifier (that is G-D, still blocked).
- "Host" = the group's relay replica as already used in `tests.rs`/`reachability.rs`.

### W2-R — Rotation notes relayed by the group host (U1, U2)
Owns `crates/group-sim/src/host_relay.rs` (+ mod line). Tests (~10):
1. Member rotates its group endpoint; host relays the note; online members update; an
   offline member catches up from the host on return.
2. Audience separation: no private-audience endpoint ever appears in host state or in any
   group-relayed message (scan serialized host state + every relayed byte).
3. Two members both rotated while apart recover each other's current group endpoint via the host.
4. Host rotates: online members get it directly; an offline member recovers it from another
   member forwarding the host's DID-signed note (verified, not trusted blindly).
5. Forged, stale (lower seq), replayed, wrong-owner and private-audience notes are rejected
   with host and member state byte-for-byte unchanged.
6. A member blocked personally by another member still receives that member's group notes
   (block does not change group membership).
7. Limitation: host and all forwarding members unreachable -> no recovery without re-share.

### W2-Q — 1:1 chat request through the host (U3, U5)
Owns `crates/group-sim/src/chat_request.rs` (+ mod line). May add dev-deps
`x25519-dalek = { version = "2", features = ["static_secrets"] }` and `aes-gcm = "0.10"` to
`crates/group-sim/Cargo.toml` (+ resulting Cargo.lock lines for group-sim only; both crates
already in the lockfile). Each member publishes a DID-signed X25519 request key in the group.
Tests (~12):
1. Request is E2E-encrypted to the target; host sees only routing metadata. Assert the
   requester's and target's private endpoints never appear in host state or relayed bytes.
2. Accept: both sides obtain each other's private endpoint, bound to the right DID (signed
   inside the ciphertext); afterwards direct chat needs no host.
3. Host tampering (bit flip, swapped ciphertext between targets, re-addressed request)
   is detected; state unchanged.
4. Replayed request/acceptance rejected; an acceptance cannot be redirected to a third party.
5. Host drops the request -> requester sees `NoAnswer`, never `Refused`.
6. Silent block: a blocked requester's request is dropped; what the requester observes is
   identical to an offline/non-responding target (compare the observation values).
7. candidate-policy G-BLOCK: explicit decline vs silent decline (both modelled).
8. Rate limit per (requester, target); over-limit requests dropped, state unchanged.
9. Request key rotated/stale: request to an old key is rejected or answered per a
   candidate policy; never decrypted with a retired key after its retirement.

### W2-B — Silent blocking on the private audience (U5) + current-product characterisation
Owns NEW `crates/proto/tests/rotation_silent_block.rs` and NEW
`crates/core/tests/block_characterization.rs`. proto part (~10, self-contained per rule 3):
1. Blocked contact is excluded from the next private rotation note; all others receive it.
2. Until that rotation the blocked contact can still reach the old endpoint at the network
   level but its traffic is dropped; it observes the same as "offline" (compare observations).
3. Blocking never triggers a rotation (schedule unchanged by block events).
4. Block list syncs to the owner's linked devices; a sibling device also excludes the
   blocked contact from its notes; ordering race (sibling rotates before sync) shown as a
   limitation with the exposure window measured.
5. Block during a rotation grace window: blocked contact does not get the new endpoint.
6. candidate-policy G-BLOCK: unblock re-includes at next rotation vs sends current endpoint
   immediately.
7. A blocked contact sharing a group learns only the group endpoint, never the private one.
core part: characterise TODAY's behaviour (no production edits), reusing helpers from
`crates/core/tests/restore.rs`/`rotation_restore_characterization.rs` read-only: what a
blocked contact observes on message and call attempts (explicit `Reject`, see
`crates/core/src/node.rs:280`) vs an offline peer. Label `limitation` where it differs from U5.
Run `cargo test -p p2pcore --test block_characterization` then full `cargo test -p p2pcore`
(baseline 129 passed) and `cargo test -p proto` (baseline 166 passed).

### W2-G — Personal block inside groups (U5, U6)
Owns `crates/group-sim/src/block_in_group.rs` (+ mod line). Tests (~8):
1. Personal block leaves group membership, permits and convergence unchanged for everyone
   (state of all non-blocking replicas byte-identical with and without the block).
2. candidate-policy G-BLOCK: blocker's local view of the blocked member's group messages —
   (a) shown, (b) hidden, (c) collapsed placeholder. Hidden content still converges and is
   relayed (the blocker's replica must not become a censoring relay unless policy says so;
   model both as candidates if the blocker is the host).
3. Blocked member cannot learn the blocker's private endpoint through group traffic.
4. Block by an admin/owner does not revoke or alter the blocked member's grants.
5. Blocked member's chat-request path is out of scope here (W2-Q) — reference only.

---

### W2-H — Host-only group model with DID-signed content (U4 revised, U7)
Owns NEW `crates/group-sim/src/host_only.rs` (+ mod line) and NEW `docs/group-host-only-model.md`.
Builds a parallel model next to the existing fixtures (which encode the superseded
device-key-signed, mesh-sync design) without editing them. Tests (~12):
1. Envelope = content signed by the DID key + random per-device writer ID (not derived from any
   key) used as Loro peer ID and per-device seq. Two own devices writing concurrently converge.
2. Scan every byte any member receives: no device key, transport key or endpoint of any other
   member appears (positive control: the host's own endpoint is known to members).
3. Re-run W2-G test 10's leak scenario under this model: no private endpoint leak.
4. Topology: members only talk to the host; a member-to-member delivery attempt is impossible
   (no address) — show by construction.
5. Member rotation note goes to host only; host routes to the new endpoint; old one retired.
6. Host rotation reaches members (direct + catch-up on return).
7. Host offline: group stalls (limitation), writes queue locally, converge on host return.
8. Forged DID signature, wrong group, replayed envelope, writer-ID collision across two DIDs
   and within one DID: rejected/handled with state unchanged (collision handling may be a
   candidate-policy G-FORK).
9. Chat request via host still works with DID-signed content (reference W2-Q construction;
   copy what's needed).
Doc: list every existing group-sim test that encodes a superseded assumption (mesh sync,
device-key signing, member-held endpoints), with file:line — do not edit them.

---

### W2-M — Multi-host group model (host permits, host reconnect) + superseded-test relabel
Owns NEW `crates/group-sim/src/multi_host.rs` (+ mod line), NEW `docs/group-multi-host-model.md`,
and label/comment-only edits (no logic changes) to the silent-block tests listed in
`docs/group-host-only-model.md` §4 (`chat_request.rs`, `block_in_group.rs`). Builds on
`host_only.rs` (copy with provenance). Decisions: U4 revised, U5 revised, U7, U9, section 9
Q5/Q6 (multiple hosts; host reconnects members after its rotation; host details). Tests (~14):
1. Host permit: owner/admin appoints a host via the grant/delegation chain; a host without a
   valid permit, with a revoked permit, or appointed by a non-admin is refused by members.
2. Host need not be an admin: host can relay but cannot grant/remove (state unchanged).
3. Host is one device of a user: a sibling device of the same DID is NOT a host.
4. Member posts via host H1 reach members on H2 (host-to-host sync); two hosts partitioned
   then healed converge.
5. Member rotation note goes to every host; one host relays to another that missed it.
6. Host rotates and itself reconnects members at their current endpoints (no overlap window).
7. Host and member both rotated while apart: recovered via another host (invariant with ≥2
   hosts); with one host -> candidate-policy G-TIME: member keeps old endpoint until host ack
   vs no retention (re-share).
8. Writer-ID collision with two hosts seeing claims in different orders: first-claim-at-host
   diverges (limitation) vs deterministic tie-break, e.g. lowest signed claim (candidate G-FORK).
9. Host removal by an admin: removed host's relays refused by members afterwards.
10. U9 removal applied by every honest host; a rogue host still serving the removed member
    is a limitation (no group-content encryption).
11. Visible block/removal notice (U5 revised): removed member receives a signed notice.
Relabel the silent-block tests as `// KIND: candidate-policy G-BLOCK` with a comment
"rejected option per U5 revised" — no behaviour change; test count unchanged.

---

### W2-D — Device-list push to contacts (U8)
Owns NEW `crates/proto/tests/rotation_device_list_push.rs` and NEW
`crates/core/tests/device_list_characterization.rs`. proto model (~10, self-contained, rule 3),
using the real `proto::sign_device_list` / `verify_device_list` where possible:
1. Device added / unlinked / restored / rotated -> new list with higher seq pushed to every
   unblocked contact; contacts accept and replace; blocked contact gets nothing.
2. Contact offline: receives the latest list on reconnect (whoever reaches whom first).
3. Stale/lower seq, forged, wrong-DID, replayed lists rejected with state unchanged.
4. Restore case (K1 stall): contact with the old list accepts messages from the new device
   key only after the pushed list arrives; show the stall is gone with push.
5. Two own devices edit the list concurrently (same seq, different contents): candidate-policy
   G-FORK options (higher-seq-wins re-sign, union merge, freeze) — model, don't decide.
6. Limitation: any phrase holder can sign a list that drops honest devices.
7. Visible block (U5 revised): blocked contact gets the signed block notice, not the list.
core part: characterise today — the list travels only in call hello/accept
(`crates/core/src/node.rs:2510`, `:2754`); adding a device updates no contact until a call;
chat to a contact holding an old list after our restore (reuse K1 helpers read-only).
Run `-p proto --test rotation_device_list_push`, `-p p2pcore --test device_list_characterization`,
then full `-p proto` (180) and `-p p2pcore` (134).

### W2-F — Removal by admin flags (U9) + group-level rotation on host revocation
Owns NEW `crates/group-sim/src/flags.rs` (+ mod line) and NEW `docs/group-flag-removal-model.md`.
Build on `multi_host.rs` (copy with provenance). Tests (~14):
1. 3 flags from 3 distinct admins remove the member and revoke the grant; same admin flagging
   3 times does not (candidate-policy G-AUTHORITY: distinct vs any — model both).
2. Flags in every delivery order (and across two hosts) reach the same outcome.
3. Forged flag, flag by non-admin, flag by a host-only (non-admin) device: rejected, unchanged.
4. Fewer than 3 admins: candidate policies (owner alone / threshold = admin count / no removal).
5. Owner cannot be flagged/removed (candidate) vs can.
6. Flag withdrawal before threshold (candidate) and flag expiry (candidate).
7. After removal: every honest host refuses the member's posts and drops its endpoint; the
   removed member gets a signed visible notice (U5 revised).
8. Posts around the 3rd flag: reuse D01/D02 candidates (signed frontier vs removal wins).
9. Rejoin after removal by a new invite (candidate allowed / forbidden).
10. Host revocation + group-level rotation (section 10 Q8 decided): a legitimate host sends the
    signed revocation to all members; all members and remaining hosts rotate group endpoints and
    send notes only to permitted hosts; the revoked host then reaches nobody and receives no new
    content (invariant). Limitations: content it already held; a member not yet reached keeps
    posting to the rogue host until reached; an undetected rogue host is not covered.
11. Admins flagging other admins (candidate).

---

### W2-C — Separate message / call capability tokens (U13)
Owns NEW `crates/proto/tests/capability_tokens.rs` and NEW
`crates/core/tests/grant_characterization.rs`. proto model (~12, self-contained, rule 3; may
call the real `proto::issue_grant` / `verify_grant` for today's `call` cap):
1. Separate `message` and `call` grants (or cap set) issued by our DID; the issuer is the only
   verifier; holder/issuer/cap/expiry/revocation checks; forged/wrong-holder/wrong-cap/expired/
   revoked refused with state unchanged.
2. Defaults: ticket/QR contact -> message+call; group chat-request contact -> message only;
   "allow calls" upgrade issues the call token; downgrade revokes it.
3. Message-only contact: message accepted, call refused (with a visible reason), no ring.
4. Block revokes both and sends the signed visible notice (U5 revised); refusal immediate.
5. Migration: existing `call` grant is treated as message+call.
6. Linked devices: a grant issued on one own device is honoured by siblings (revocation set
   sync) — model the sync window as a limitation.
7. Renewal: today `call` grants renew on answered calls (`node.rs:2753`); model message-grant
   renewal candidates (on message activity vs fixed TTL) as candidate-policy G-RENEWAL.
core part: characterise today's grant behaviour — what a contact needs to call and to message
now, whether messaging checks a grant at all, revocation handling, renewal on answer.
Run `-p proto --test capability_tokens`, `-p p2pcore --test grant_characterization`, then full
`-p proto` (197) and `-p p2pcore --no-fail-fast` (141).

### W2-U — Announced, delayed host rotation after member removal (U11, U12)
Owns NEW `crates/group-sim/src/removal_rotation.rs` (+ mod line). Build on `flags.rs` /
`multi_host.rs` (copy with provenance). Tests (~10):
1. On removal every host refuses the removed DID's NEW posts and calls immediately.
2. Hosts send a signed "rotating soon" announcement to remaining members only; rotate after
   the delay (fixture: 2 days in ticks); reconnect members themselves; removed member then
   reaches nobody.
3. U11: posts whose signed frontier predates removal are accepted until the hosts rotate,
   refused after (unreachable); a backdated post after rotation cannot be delivered.
4. Count cap candidate on pre-removal uploads (candidate-policy G-HISTORY) vs rotation-only.
5. Several removals inside one delay window -> one rotation (batching) vs one per removal.
6. Member offline during the whole delay: reconnected by the host afterwards.
7. Forged/replayed announcement rejected, state unchanged; announcement never reaches the
   removed member.
8. Limitation: during the delay the removed member still knows live host addresses (can
   connect, gets refused).

---

## Coordinator checklist per returned packet

- Agent's worktree branch commit touches only the packet's owned files (+ allowed mod line).
- Re-run its target + baseline 33 through buildlock; numbers match the report.
- Every test has a KIND label; candidate-policy tests name a gate.
- Record result + open questions in `rotation-agent-status.md`; collect user questions.
