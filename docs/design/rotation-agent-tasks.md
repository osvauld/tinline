# Rotation subagent task packets

Read `rotation-agent-handoff.md` first. These are assignable packets, not claims
that all work can start concurrently. A coordinator must reserve files and resolve
policy gates. Record ownership in `rotation-agent-status.md` before editing.

**Wave 1 is expanded into subagent-sized packets (A1–E1) in
[rotation-agent-packets.md](rotation-agent-packets.md). Assign from there.**

## Suggested next-session coordinator prompt

> Work in `/home/abe/p2p_chat/.claude/worktree/endpoint-rotation-blocking` on
> `endpoint-rotation-blocking`. Read AGENTS.md and the rotation handoff/task/status
> docs. Preserve the existing untracked work. Reproduce targeted tests, then
> assign disjoint Wave 1 packets. Keep group networking out of scope. Do not freeze
> unresolved security policy or invent a derivation scheme. Collect proposals and
> tests, integrate one shared-file change at a time, and report decisions needing
> the user before production implementation. No commits unless authorized.

## Common subagent completion report

```
Packet / owner:
Starting HEAD and relevant initial status:
Files owned and changed:
Requirement and candidate policy tested:
Implementation/model details:
Commands executed / pass-fail-pending / log paths:
What this does NOT prove:
Policy decisions needed:
Interface changes requested from coordinator:
Next step:
```

Never report a conceptual matrix as executable coverage. Keep tests for current
limitations clearly labeled (a passing test may demonstrate a missing guarantee).

## A — Baseline and dependency audit

**Start now.** Read-only by default; prerequisite repair only after coordinator approval.

Owned deliverable: `docs/design/rotation-baseline-report.md` (new).
Conditional edit ownership: `crates/core/src/node.rs` ONLY while explicitly reserved.
Do not change Cargo manifests/lockfile without coordinator agreement.

Tasks:
1. Record current HEAD/status. Reproduce existing proto and model tests with buildlock.
2. Inspect committed merge markers in node.rs; compare surrounding implementation
   and available history to determine intended behavior. Do not guess or discard
   either branch blindly. Propose a minimal isolated resolution.
3. Identify APIs/state keyed by EndpointId: registry, DeviceList, tombstones, links,
   invitations, grants, chat sessions/signatures, Loro writer IDs, self-sync, blobs.
4. Identify endpoint startup/shutdown ownership, account switching, vault lock/unlock
   and Android resume hooks. Record file/function references and ownership boundaries.
5. After approved repair, run appropriate core/workspace checks; record unrelated
   failures separately. No opportunistic broad cleanup.

Exit: exact baseline results, reviewed prerequisite repair if any, dependency map.
No claim of all-target readiness from proto-only success.

## B — Succession permit/history semantics

**Start now on model tests.** Production schema waits for decisions.

Owned deliverables:
- `crates/proto/tests/support/rotation_history_contracts.rs` (new candidate module).
- `docs/design/rotation-history-policy-options.md` (new).

Read current permit model, both signed queue models, courier token/proof code,
and the group integration contract. Do not modify existing shared model modules
without a reserved integration window. Coordinator adds module wiring/helpers.

Required scenarios:
1. More than PAGE successors: verify old pages through a session proving the
   newest endpoint key, without requiring possession of retired private keys.
2. Delivery order, dropped page, duplicate page, wrong predecessor, malformed/oversize
   page and queue saturation; accepted state stays unchanged on invalid page.
3. Missing retained history: explicit recovery-required outcome, no silent reset.
4. Fork observed at current generation AND older fork evidence after advancement.
   Detect ambiguity; do not silently choose a winning branch as "recovery".
5. Revocation of anchor/head/older ancestor after descendants accepted. Present
   candidate policies and consequences; do not invent retroactive authority.
6. Replay of an expired historical permit cannot authorize current operations.
7. Group audience cannot require private ancestors for verification.
8. Device ID collision/reuse against current and historical sibling paths.

Evaluate strict full history vs validated checkpoints. State byte/record/CPU/storage
bounds and where durable anti-rollback state lives. Distinguish successor legitimacy
from current permissions and from proof of historical creation time.

Exit: executable candidate cases, clear unresolved oracles, recommended interface
for bounded history plus current binding. Protocol promotion requires approval.

## C — Lifecycle/time/persistence models

**Start now on disjoint tests.** Read B's proposed shared-type changes before integrating.

Owned deliverables:
- `crates/proto/tests/support/rotation_time_contracts.rs` (new candidate module).
- `docs/design/rotation-time-storage-contract.md` (new).

Current lifecycle helper/module remains coordinator-owned unless explicitly handed
over. Do not edit another agent's network helpers concurrently.

Required scenarios:
1. Timer without blocks; block/unblock never resets deadline.
2. Configure interval shorter/longer near deadline; invalid/zero/overflow inputs.
   Propose explicit effective-time policy rather than accidentally picking one.
3. Sleep/resume and restart before/at/after deadline with several missed periods.
4. Wall-clock rollback/forward vs monotonic timers and signed validity; persisted
   generation never regresses. Distinguish process monotonic clock from reboot time.
5. Key generation, sealed commit, endpoint startup, announcement and shutdown fail
   independently. A failed operation must not silently extend old access forever.
6. Crash before/after each durable boundary; receiver updates persist key/generation
   and route atomically. No ACK/announcement before the required commit.
7. Challenge state capacity/expiry/restart, concurrent request ordering, nonce
   allocation failure/overflow, peer isolation. Evaluate session-bound random
   challenges rather than assuming per-request durable counters are desirable.
8. Active old connection at retirement: proposed terminate/migrate behavior and
   notification; do not claim uninterrupted calls without an actual design.

Exit: storage transition table, failure matrix, deterministic tests, proposed time
semantics. Models still do not prove fsync, power-loss or Android execution behavior.

## D — Key/author/group compatibility

**Start now as source review and contract proposals.** No group-worktree writes.

Owned deliverable: `docs/design/rotation-author-group-adapter.md` (new).
Optional fixture module: `crates/proto/tests/support/rotation_author_contracts.rs`
only after coordinator approves a test oracle/module registration.

Tasks:
1. Trace actual local transport secret use in attestations, chat/self-sync batch
   signatures, stored proofs and Loro peer IDs. Identify migrations needed.
2. Compare separate author-key identity vs author-key rotation with retained proofs.
   Explain clone/restore behavior, old signature verification and CRDT author ranges.
3. Align owner-replaceable IDs with group nomination succession, without sibling
   capability inheritance or an implicit physical-hardware continuity claim.
4. Define candidate adapters for verified succession, current routing, author-proof
   verification, group host assignment and verified-DID policy queries.
5. Specify audience-safe full proof exports/inventories, not only leaf filtering.
6. Coordinate expiry/history distinction and group assignment revocation with the
   group task. Group membership causal policy remains their gate, not ours to solve.

Exit: source-backed compatibility report, proposed shared fixtures and explicit
start/wait recommendations. Do not freeze group serialization/writer IDs here.

## E — Linked-device discovery/security decision

**Start now as research/modeling.** Does not block unrelated simulation tests, but
must be decided before selecting a production future-key scheme.

Owned deliverable: `docs/design/rotation-linked-discovery-options.md` (new).
Optional standalone tests: `scripts/test_rotation_linked_discovery_options.py`.
Do not modify existing Python model files in parallel.

Compare at least:
- Independent random endpoints + authenticated updates + explicit recovery route.
- Shared prediction secret/epochs, distinguishing deriving public reachability from
  giving siblings transport private keys.
- Rendezvous/address publication separate from transport-private-key derivation.

For each, model online/offline windows, both sides retiring endpoints, clock skew,
restore, clone, unlinked honest device, hostile retained mnemonic, leaked discovery
secret and epoch refresh excluding an offline allowed sibling. State what needs a
reachable peer or fresh secret distribution. Record audited existing guarantees.

Use established constructions/libraries as references only after inspecting their
actual APIs and assumptions. No homemade ratchet/derivation formula shipped under
this task; no claim that shared mnemonic permits strong per-device revocation.

Exit: decision matrix, counterexample traces, user-facing recovery limits and exact
question(s) for user approval. A recommendation is not a selected scheme.

## F — Production pure protocol (WAIT)

Start only after applicable history/expiry/fork/audience decisions and model review.
Coordinator allocates `crates/proto/src/rotation.rs`, module registration and tests.

- Define versioned canonical/exact-byte signed representation and domain separation.
- Validate owner, scope, previous/replacement IDs, generation, predecessor, validity,
  authorization and input bounds. Do not conflate signature verification with policy.
- Keep historical verification and current access/routing as distinct outputs/types.
- Tests call production API, fail before implementation, then pass; preserve independent
  oracle fixtures. Add arbitrary malformed input/size/depth work-bound tests.
- Separate live endpoint possession/session binding from owner succession signature.
- Re-run proto regressions; obtain group-interface review before declaring API stable.

Exit: tested pure API only; no accidental networking/vault side effects.

## G — Persistence/core control integration (WAIT)

Start after F plus core baseline/key migration/storage contracts. Split into serial
subtasks with assigned core files; do not give competing agents all of node.rs.

1. Sealed storage migration: keys, succession/order, registry and revocation, history
   cursors, timers; tests against temporary profiles and injected write failures.
2. Authenticated control updates independent of calls/UI; expected DID verification
   when dialing and safe private/group audience filtering.
3. Central verified-DID checks and cancellation for sessions/retries/transfers.
   Contact removal, group bans and device revocation remain separate policies.
4. Actual iroh rotation/startup/retirement with direct and relay-only integration
   tests, real three-/five-peer processes and restart recovery.

Exit: real end-to-end evidence, not only simulator parity. Track unlink/restore and
historical author-proof migration regressions explicitly.

## H — Android/product lifecycle (WAIT)

Start after working core API and agreed interval/retirement policy.

- Expose configuration without per-contact endpoints/services.
- Handle unlock, resume, foreground service, boot, network changes, process death.
- Retire overdue endpoints before authorized operations resume; document OS limits.
- Test emulator lifecycle and physical-device behavior separately; measure idle
  battery, relay churn, missed updates and rotation latency without fabricated numbers.
- UI states explain pending/unreachable/recovery-needed, no anonymity claims.

Exit: platform-specific acceptance results with hardware/test conditions disclosed.

## Integration review checklist (coordinator)

- Scope ownership respected; no unrelated main/group worktree modifications.
- Newly introduced policy is labeled proposed or explicitly approved.
- Current models' ideal-storage/transport assumptions remain visible.
- Rejection cannot corrupt peer maps, overwrite siblings or revive revoked/expired access.
- Every group-facing proof path excludes private endpoint disclosure.
- Full-history processing is bounded independently of delegation depth.
- Commands/results verified, including regression tests appropriate to changed files.
- Status board and relevant progress docs updated; unresolved gates remain visible.
