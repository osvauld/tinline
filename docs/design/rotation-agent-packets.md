# Rotation Wave 1 — subagent-ready packets

Status: coordinator-authored assignment sheet (2026-10-09). Expands Wave 1 of
`rotation-agent-tasks.md` (packets A–E) into tasks small enough for one Sonnet
agent each. Packets F–H remain WAIT and are not expanded here.

Coordinator baseline at assignment time (HEAD `6b1aaef`, rotation work untracked):

```
python3 scripts/test_rotation_discovery_model.py   # 6 passed
python3 scripts/test_rotation_network_model.py     # 10 passed
python3 scripts/buildlock.py --who rotation-coord -- cargo test -p proto \
  --test rotation_identity_boundary --test rotation_succession_model --test rotation_permit_model
# 33 passed (5 + 18 + 10)
```

Group-chat sibling state (read 2026-10-09, `group-chat/docs/STATUS-group-chat.md`): branch
`group-chat` @6b1aaef, all uncommitted; test-only `crates/group-sim` (24 passed per their
status) consumes a MOCK `SuccessionAdapter::verified_path(&Nomination) -> Option<VerifiedSuccession>`
in `group-sim/src/reachability.rs`, whose contract is "full verified ancestry since the
nominated root, no omitted endpoint-bearing predecessors". Their next work is a
persistence/crash harness. They add `group-sim` to workspace `Cargo.toml`/`Cargo.lock`
(future merge touchpoint; we do not edit manifests).

Note: `dev` (2c6d231) already contains `0f36a29 core: resolve conflict markers left by
the rebase onto main`. This branch still has 4 marker blocks in `crates/core/src/node.rs`
(lines ~2717–2765). Bringing `dev` in is a coordinator/user branch decision, not a
subagent task.

---

## 0. Rules every packet inherits (paste into each agent prompt)

1. Worktree `/home/abe/p2p_chat/.claude/worktree/endpoint-rotation-blocking`, branch
   `endpoint-rotation-blocking`. Do not switch branches, reset, clean, stash, stage,
   commit, or touch any other worktree (sibling group-chat docs are READ-ONLY).
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
   python3 scripts/buildlock.py --who rotation-<PACKET> -- cargo test -p proto --test <your_target> 2>&1 | tee /tmp/rotation-<PACKET>.log
   ```
   Finish with one run that also includes `--test rotation_identity_boundary
   --test rotation_succession_model --test rotation_permit_model` (must stay 33 passed).
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
9. Stop and report (don't improvise) if: a required file is already modified by
   someone else, baseline tests fail before your change, or the task needs a
   shared-file edit.
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

Suggested batches (≤4 concurrent to keep buildlock queues short):
- Batch 1: A1, B1, C1, D1
- Batch 2: A2, B2, C3, E1
- Batch 3: B3, C2, D2
Coordinator reviews each batch, updates the status board, then launches the next.

---

## A1 — Core baseline & EndpointId dependency map

**Goal:** know exactly what in core breaks or must migrate when a device's transport key changes.

Read: handoff §2; `endpoint-rotation-device-audit.md`; `crates/core/src/{node.rs,store.rs,sync/*.rs}`;
`crates/proto/src/{lib.rs,link.rs}`; `docs/protocol.md`; `docs/design/{device-linking,multi-device}.md`.

Steps:
1. Record `git rev-parse --short HEAD`, `git status --short`.
2. Merge markers: list each block in `crates/core/src/node.rs` (line ranges, both sides
   summarized). Run `git show 0f36a29 -- crates/core/src/node.rs` and state whether that
   dev commit resolves exactly these blocks and how. Do NOT edit node.rs.
3. Run `python3 scripts/buildlock.py --who rotation-A1 -- cargo check -p core 2>&1 | tee /tmp/rotation-A1-check.log`
   and record the result (expected to fail on markers; record the first errors verbatim).
4. Build a table of every type/field/function/key that stores, signs with, indexes by or
   derives from a device transport public key / `EndpointId` / `device_secret`:
   registry (`OwnDevice`), `DeviceList`, tombstones, link grants, invitations, grants,
   contacts, blocks, chat sessions/batch signatures, self-sync, Loro peer IDs, blobs,
   relay hints, call routing, presence. Columns: `path:line` | what it keys/signs |
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

Read D1 output, group contract, `docs/group-chat-coordination-*.md`, sibling (read-only)
`group-chat/docs/{STATUS-group-chat.md,endpoint-rotation-coordination.md,group-chat.md}` and
`group-chat/crates/group-sim/src/{reachability.rs,authority.rs}`. Map our proposed adapters
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

## Coordinator checklist per returned packet

- `git status --short` shows only the packet's owned files added/changed.
- Re-run its target + baseline 33 through buildlock; numbers match the report.
- Every test has a KIND label; candidate-policy tests name a gate.
- Record result + open questions in `rotation-agent-status.md`; collect user questions.
