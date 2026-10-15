# Endpoint rotation: test-first implementation plan

**New session/subagents:** start with `AGENTS.md` at the worktree root, then
[rotation-agent-handoff.md](rotation-agent-handoff.md),
[rotation-agent-tasks.md](rotation-agent-tasks.md), and
[rotation-agent-status.md](rotation-agent-status.md).

Status: test-first investigation. Initial discovery model implemented (6 tests
passing); production rotation is not implemented. See
[device audit](endpoint-rotation-device-audit.md) for current linking constraints.
Worktree: `.claude/worktree/endpoint-rotation-blocking`
Branch: `endpoint-rotation-blocking`

## 1. Agreed requirements

- Account DID is stable; transport EndpointIds are replaceable.
- Rotate automatically on a configurable interval, even with no blocks.
- Blocking does not trigger rotation. It immediately denies the verified DID in
  core and excludes it from future private address updates. At scheduled
  retirement, the previously known endpoint must no longer accept connections.
- We retain allowed contacts' addresses and initiate connections from the new
  endpoint, proving our established identity. No new invitation is required.
- Authenticated control traffic can carry address updates, including sessions
  used for presence/ephemeral data. No text message or call is required.
- No anonymity guarantee, peer-delegation chain, or Double Ratchet requirement.
- Personal blocks, group membership policy, and device revocation are distinct.
- Group chat/host integration is OUT OF SCOPE for this first implementation.

## 2. What “sequential keys” means

Initial proposal: generate independent random transport keys with established crypto primitives.
This is not frozen: linked-device offline prediction is now being investigated.
Any alternative must document sibling impersonation, retained discovery knowledge,
and recovery after discovery-secret replacement before adoption.
Do NOT derive the next private key from the current key, reveal future keys, or
invent a cryptographic ratchet. Sequentialness belongs to the authenticated
rotation history, not to a mathematical relationship between private keys.

Proposed signed record (final wire format follows tests):

```
version, owner_did, stable_device_slot, audience,
generation, predecessor_record_hash, endpoint_public_key,
issued_at, expires_at
```

The DID signs the complete domain-separated record. The slot identifies the
installation independently of its changing transport key. Audience prevents
future public/private cross-scope substitution. A device attestation binds the
new endpoint to the DID; a live iroh connection proves endpoint-key possession.
A forwarded record alone proves an owner's announcement, not live possession.

A predecessor hash authenticates the history relationship but is not enough to
prevent forks. A generation alone is also not enough to authorize an update.
Both owner authorization and persisted receiver ordering state are required.

Decision to validate in simulation: preserve strict history verification by
sending a bounded/paginated chain since the receiver's last accepted record.
If history is unavailable, use explicit fresh, authenticated resynchronization;
never silently reset the receiver's high-water mark. Do not assume contacts
receive every generation. A simpler owner-signed latest-state checkpoint is an
alternative to evaluate before freezing the wire format.

## 3. Invariants to specify before implementation

1. Only the expected DID may update its endpoint binding.
2. State is scoped by DID + stable device slot + audience.
3. Accepted generations never decrease across restart.
4. Exact duplicate records are idempotent; stale records cannot change state.
5. Same-generation conflicting records are detected, not resolved by arrival
   order. Proposed policy: retain the accepted record, flag conflict, require
   explicit recovery. Recovery after a compromised DID is outside this design.
6. A history link cannot splice records from another DID, slot, or audience.
7. A retired private transport key cannot authorize the next binding on its own.
8. An authorized new endpoint does not create a new contact/person/device slot.
9. A blocked DID receives no new private update and cannot initiate authorized
   operations, regardless of which attested endpoint it uses.
10. Rotation occurs without blocks, and blocking does not reset its deadline.
11. Wall-clock changes cannot roll back generation or extend old reachability
    indefinitely. Certificate validity and scheduler time are separate concerns.
12. Crash recovery never reuses a generation for a different key and never
    revives an endpoint already durably marked retired.
13. Address updates are bounded in size, verification work, and queued retries.
14. No unauthenticated presence packet mutates durable endpoint state.
15. Multi-device updates do not replace sibling devices' endpoints.

## 4. First deliverable: deterministic simulator and tests

Build a pure model before modifying real networking. Use a fake clock, seeded
randomness, in-memory peers, explicit durable snapshots, and a controllable
message queue. Model connect/authenticate, publish/update, delivery/drop/reorder,
block/unblock, tick, crash/restart, and retire. Separate volatile state from
persisted state so crashes have real meaning.

Use real existing signing/verification primitives in protocol tests; the model
must not treat every signature as valid. Later compare model expectations with
production protocol transitions. A simulator is evidence within its model, not
proof of Android scheduling or transport behavior.

### Ordered-history tests

- Accept generation 0 -> 1 -> 2 for the same identity/slot/audience.
- Exact replay changes nothing; older updates never restore retired addresses.
- Deliver 2 before 1; exercise the chosen gap/catch-up policy explicitly.
- Contact offline across many rotations catches up under bounded resources.
- Reject wrong signer, tampered key/epoch/hash/audience, forged identity, and
  a transport key that does not match its attestation.
- Reject missing/wrong predecessor links when chain verification is required.
- Detect a fork at one generation; do not silently pick the last arrival.
- Prove possession of the old endpoint key without the DID signing key cannot
  generate an accepted successor.
- Isolate slots and audiences; rotating one does not overwrite another.
- Reject unsupported versions and malformed/oversized records.

### Three-peer blocking scenario

Alice knows Bob and Carol. Alice blocks Carol between scheduled rotations.
Before retirement, core policy denies Carol despite knowledge of the old address.
At the deadline Alice rotates, reconnects/authenticates to Bob and sends the
signed update, but sends none to Carol. Bob recognizes the same DID and uses the
new endpoint. The old endpoint stops accepting connections. Carol cannot bypass
policy using another device or a leaked copy of the new address.

Also test no blocks, repeated blocks, unblock, block during update delivery,
existing session -> block, pending retries, and queued operations. Bytes already
sent before a block cannot be recalled. Unblock must not replay queued calls or
silently restore revoked grants.

### Time, persistence, and failure tests

- Configurable interval; changing it follows an explicit scheduling policy.
- Restart before/at/after deadline; Android resume after multiple missed periods.
- Wall clock forward/backward; generation remains monotonic.
- Inject a crash at each persistence/network transition.
- Key generation, storage, endpoint startup, and notification failures.
- Slow/offline recipients do not indefinitely delay old-endpoint retirement.
- Duplicate, dropped, reordered updates and retry exhaustion.
- Both peers rotate offline: report unreachable/recovery-needed explicitly;
  never claim identity proofs alone solve rendezvous.
- Property-based event sequences check all applicable invariants after every
  event; retain failing seeds and reduce traces for regression tests.

## 5. Decisions required before freezing protocol

Record outcomes in this file, with supporting test cases:

- Strict predecessor history vs signed latest-state checkpoints for catch-up.
- Generation allocation, stable slot migration, and clone/restore fork handling.
- New-device authorization: reuse existing device registry policy; do not let
  address rotation silently authorize an unknown installation.
- Rotation interval default/minimum, jitter, and interval-change behavior.
- Expiry/skew rules, and explicit recovery for long-offline contacts.
- Retirement semantics for active calls/transfers and bounded migration overlap.
  Initial proposed security rule: close old connections at retirement; call
  migration can be a later feature. Do not promise uninterrupted calls yet.
- Safe failure behavior if the new endpoint cannot start: fail closed at the
  old endpoint's deadline vs a visible degraded-state exception. No silent
  unlimited extension. Android cannot execute timers while suspended, so enforce
  overdue retirement before accepting operations on resume; measure OS behavior.
- Exactly which grants survive rotation (person grants normally do) versus
  which are device-bound. Blocking/revocation remains a separate policy.
- Whether removal and blocking remain distinct product actions.
- Block sync/concurrent unblock rules across linked devices: coordinate with
  existing account sync rather than inventing group-chat dependencies.

## 6. Implementation phases and exit gates

### Phase A — audit and baseline

Inspect proto, endpoint lifecycle, vault/store, device registry, chat/control
handshakes, account switching, and existing block enforcement. Record findings
and precise file/function integration points. The base currently contains
(RESOLVED 2026-10-10 via dev merge 35564bc; see rotation-baseline-report.md) committed conflict markers in `crates/core/src/node.rs` around lines 2717 and
2760. Resolve those in an isolated prerequisite change before claiming a passing
core baseline; do not mix unrelated cleanup into rotation changes.

Exit: documented baseline commands/results and ownership of pre-existing errors.

### Phase B — executable model and protocol tests

Implement Section 4 with fake time; agree Section 5 ordering decisions. Add
failing production-facing tests before implementing verification/state changes.
Do not put production networking behind a simulator-only success claim.

Exit: deterministic scenarios and property tests pass; sequence/fork/catch-up
semantics documented with concrete traces. Still no group-chat integration.

### Phase C — pure signed-update protocol

Add record types, domain separation, signing/verification, bounded decode,
ordering decisions, and explicit errors to the appropriate protocol module.
Reuse existing primitives. Verify live endpoint binding at the handshake layer.

Exit: malicious-input and ordering tests pass independently of iroh/Android.

### Phase D — persistence and rotation state machine

Introduce stable slot identity where needed, persist generation/current/pending
rotation and receiver high-water marks, and define crash-safe transitions.
Migration must retain DID, contacts, blocks, grants, and linked-device identity.
Keep new keys sealed with existing vault mechanisms; never log secrets.

Exit: persistence fault-injection, migration, and account-switch isolation tests.

### Phase E — real direct control integration

Carry updates over authenticated control sessions, not only call handshakes.
Audit presence/ephemeral entry points against the same policy. Authenticate the
expected DID when dialing, not simply any contact at a previously stored address.
Replace the correct slot's endpoint atomically, rather than appending forever.
Add retry bounds and cancel session/retry/transfer work when blocked.

Exit: direct two-/three-peer real transport tests with identity continuity and
no new contact invitations; stale updates cannot restore old dialing targets.

### Phase F — scheduling and Android lifecycle

Wire configurable interval to core; retire old transports and enforce overdue
rotation on unlock/resume. Normally one private endpoint, bounded overlap only
if adopted. Do not create an endpoint per friend. Defer separate public endpoint
and group-host distribution to a follow-up.

Exit: restart/resume tests, relay-only and direct-path tests, and real-device
measurements of idle battery, relay churn, rotation latency, and missed delivery.
Document untested hardware behavior rather than claiming simulator coverage.

## 7. Acceptance criteria for the first release

- Tests demonstrate authenticated, monotonic endpoint succession and recovery
  across reordering, missed updates, and restart under the chosen policy.
- Configured rotation runs independently of blocking.
- Allowed online contacts learn the replacement through authenticated control
  traffic without sending text or making a call.
- Blocked contacts get no subsequent private announcements, old endpoints are
  retired according to the documented policy, and leaked new addresses do not
  bypass DID authorization.
- No private endpoint leaks through existing DeviceList/call/chat distribution.
- Existing identity, contact, vault, device-linking and core tests remain passing
  (or pre-existing failures are explicitly documented).
- Offline mutual rotation is an explicit limitation until rendezvous is added.

## 8. Long-running task discipline

For each phase: update this document with files changed, decisions, exact test
commands/results, known failures, and next step. Keep changes small and separable
(protocol/model, persistence, integration, platform). Never report planned tests
as executed. Follow repository build locking for heavy builds. Do not modify
other agents' worktrees. Coordinate shared block/device-sync interfaces before
changing them; no group CRDT changes in this task.

Current progress: initial linking inventory and discovery-knowledge model are in
`endpoint-rotation-device-audit.md` and `design-models:scripts/test_rotation_discovery_model.py`.
Real-signature characterization tests are in
`crates/proto/tests/rotation_identity_boundary.rs`. They characterize current
attestation behavior, not a completed signed-succession protocol.

Signed-succession candidate model added in
`design-models:crates/proto/tests/rotation_succession_model.rs`. Command run:

```
python3 scripts/buildlock.py --who endpoint-rotation -- cargo test -p proto --test rotation_succession_model --test rotation_identity_boundary
```

Result: 10 tests passed (5 succession + 5 identity). The model uses real DID
signatures with stable slots, scoped predecessor hashes and strict contiguous
generations. Covers duplicates, stale replay, gaps/catch-up, serialized-state
round trips, wrong signer, tampering, scope substitution, same-generation forks,
wrong predecessors, and all six delivery permutations of three records.

Limits: candidate strict-history semantics, not a frozen wire format. Test-only
public deterministic endpoint seeds are fixtures, NOT a key-derivation scheme.
Serialization round trips do not prove atomic disk persistence. Fork detection
only compares against the locally accepted head; disconnected replicas can still
accept different owner-signed forks. No fork convergence, expiry, bounded history
paging, live transport, or crash fault injection has been implemented yet.

Multi-device behavioral model added in `design-models:scripts/test_rotation_network_model.py`.
Executed `python3 design-models:scripts/test_rotation_network_model.py`: 10 tests passed,
including 20 seeded schedules of 150 events each. Re-ran discovery model: 6 passed.
Each device has its own contact directory, policy, rotation schedule, sessions,
and reachability. Fixture: Alice phone/laptop, Bob phone/laptop, Carol phone.
Covers presence-only updates, sibling isolation, block/session cancellation,
alternate blocked devices, leaked addresses, overdue resume, explicit local block
sync, partitions/retry, duplicates, forged presence, unblock without replay, and
mutual offline rotation remaining unreachable. Invariants check monotonic local
bindings, symmetric authorized sessions, and bounded pending announcements.

Model limitations: authentication is an oracle, expiry and key derivation are not
modeled, packet delivery represents a live handshake (not store-and-forward), and
schedules use perfect monotonic time. Rotation generation is clock-derived in this
model only, not an allocation decision for production. Latest binding discovery
is modeled independently of strict signed-history verification; neither model
proves their integration. Sleep/resume is not crash persistence. No linked-device
prediction, transport relay behavior, history bounds, fork convergence, or trace
shrinking yet. Policy sync is explicit, not a globally shared block list.

Added signed queue composition in
`design-models:crates/proto/tests/support/rotation_network.rs`, included by the succession tests.
Five new scenarios use the exact strict-history verifier on delivery: independent
peer directories, wrong signer and receiver-side block checks, bounded history
pages/offline gaps, ideal atomic-commit crash boundaries, and cloned-device forks
accepted by different peers then detected on exchange. Crash tests retain the
actual random transport secret with the binding and assert that restart preserves
its public-key match; they do not exercise the vault or filesystem.

Validation command:
`python3 scripts/buildlock.py --who endpoint-rotation -- cargo test -p proto --test rotation_succession_model --test rotation_identity_boundary`
Initially queued behind another agent's workspace test; completed successfully:
15 passed (5 identity + 10 succession/network). Build log:
`/tmp/endpoint-rotation-signed-network.log`.

Scope limits: this Rust queue composes real signatures and address-book state but
does not yet integrate the Python topology/timer scheduler. Queue routing assumes
a delivery channel exists; it does not solve linked-device discovery. Page and
queue count limits are test policies, not a wire byte-limit implementation.
Fork exchange detects disagreement but deliberately leaves peers divergent;
recovery authority and fork convergence remain unresolved. Atomic durable commit
is an explicit assumption, not a proven storage implementation.

Coordination correction: `../group-chat-coordination-request.md` reports the
user requires owner-replaceable device slots/IDs. Our constant-slot test model is
provisional and must be generalized to bind previous/replacement IDs and isolate
sibling succession paths. Do not freeze permanently stable slots as a requirement.
Response and interface readiness: `../group-chat-coordination-response.md`.

Owner-replaceable permit model and direct queue integration are now in
`rotation_permit_model.rs` and `support/rotation_permit_network.rs`. Latest targeted
Rust run: 33 tests passed (18 permit/network/lifecycle, 10 earlier
succession/network, 5 identity); Python network/discovery: 10 + 6 passed.
Lifecycle model covers independent intervals, ideal atomic crash boundaries,
overdue resume, offline expiry/catch-up and bounded per-receiver challenges.
Details, exact command and remaining limits:
[permit-model progress](endpoint-rotation-permit-model.md). Expired history is not
an active route; separate group history verifies without private predecessor
records in the fixture. These are still test-only models, not production APIs.

Next: freeze crash ordering and fork recovery semantics, test queue saturation,
receiver crash persistence and old-fork evidence after further generations, then
connect the signed model to timed topology events. Prediction/recovery decisions
still precede a production key scheme. Test relay-only behavior in real iroh
rather than pretending this abstract packet queue measures it.
