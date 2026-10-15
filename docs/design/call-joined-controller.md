# Durable group-call Joined controller

Status: **owned M1 integration prerequisite**, not installed into Node, live call
callbacks, existing call history, generated bindings, or either app UI. The
[product completion gate](receipt-runtime-roadmap.md#product-completion-requirement--real-app-receipt-displays)
remains unmet. No background retry task is started by this module.

## Implementation and ordering

`receipts/store/joins.rs` is owned by the existing `ReceiptStore` factory. Repeated
`join_controller()` calls share one live controller; it holds the factory, not an
independent sealed-store factory or a competing call-log database. The caller
supplies the factory's single live ledger for the required group/day. A ledger
from another factory is refused even if its scope happens to match.

1. At a **witnessed actual local Joined transition**, call `capture` with the
   account/device/source epoch, frozen observation time and metadata, exact
   accepted CallStart, expected outbound Join and accepted Roster, frozen authority,
   and explicit local admission. The shared participation verifier still checks
   original signatures and exact expected-Join correspondence. A roster alone
   does not witness a live transition.
2. Capture signs and persists one immutable intent containing the exact receipt
   body/originals before returning success. This intent is local work, not exported
   native progress or a UI confirmation. If capture fails or the queue is full,
   **the caller must retain the event**; this module cannot make a completely
   failed storage write durable or recover an event the caller discards.
3. The transition key binds issuer, scoped device and the original expected Join's
   evidence digest under `tinline-call-joined-transition-v1\0`. The first accepted
   observation time, metadata and roster win. Repeated callbacks/newer rosters do
   not produce another receipt. A new outbound Join nonce/attempt produces a new
   observation, not an extra call-history row selected by this controller.
4. `next_pending` selects at most one intent and rotates before retry. Failed
   attempts cannot indefinitely pin that cursor. It does not choose a timer,
   maximum retry lifetime, abandonment, eviction or outage policy.
5. `try_commit` checks originals, current call-role authority and mandatory local
   source/device admission again, even for completed replays. It retries the
   frozen signed body rather than constructing a new timestamp/version.
6. Native receipt update, frontier/sequence/index, exact original evidence,
   completion pointer and removal of the redundant pending copy share **one sealed
   transaction**. Failure leaves the original pending intent and native state
   unchanged. An already durable identical receipt can finish the intent in a
   sidecar-only transaction after admission and exact-original checks; it cannot
   heal missing historical evidence implicitly.
7. Only successful commit returns `ParticipationView`. `newly_committed` reports
   the first native insertion, not exactly-once notification delivery. If the
   process stops after commit but before notification, the existing history owner
   must reconcile from durable evidence. Completed replay is idempotent; its index
   alone is not a verified UI status.

Completion removes only a redundant local work item whose receipt and originals
are now durable. It does **not** delete receipt/audit evidence. Completion pointers
are retained; no retention/rollover policy is introduced.

## Source/lifecycle boundary and ownership

`JoinContext::source_epoch` is a caller's source label, not a durable boot identity,
cryptographic admission, or proof that a transition occurred. The admission callback
must check the actual source lease/account/device and expected Join, not merely a
matching epoch integer. A process counter may repeat after restart. Pending work
is not automatically rebound to a new epoch or authorized because it reopened;
recovery needs explicit admission. This controller does not settle historical
membership/removal/regrant rules.

Capture/completion serialize under the controller lock; callbacks must not reenter
it. Caller authority/source context must remain valid for the operation. Native
checkpoint/reconstruction/history inspection can scale with retained history:
**do not hold media or shared-state locks over these calls**. The actual frozen
source-lease/lock ordering needs agreement at the live hook.

The prior `groups/call-log` checkout is clean and its tip is an ancestor of this
worktree's merged baseline. That resolves the old unmerged-source situation; it
is **not** a live-hook/DTO ownership agreement. This slice changes only owned
receipt modules/tests/docs, leaving `group/calls*`, `group/calllog*`, `gcall/*`,
Node lifecycle and app/UI files untouched.

Exact next handoff request:

- Retain the accepted original start, actual outbound Join and accepted Roster at
  the real ClientEngine Joined transition, without putting private proof payloads
  into UI DTOs.
- Agree one backing-owned controller/ledger owner and frozen source/device lease
  across capture/commit; account stop/switch/restart must not authorize stale work.
- Preserve the only event on capture failure/full queue, and schedule bounded
  work outside media/state locks. Agree retry pacing rather than invent it here.
- Reconcile the existing history view after commit with explicit signed versus
  legacy/local provenance, including recovery after a lost notification. Allocate
  DTO/binding names with the owner, not in this storage prerequisite.
- Test authenticated live Node lifecycle -> original proofs -> durable intent ->
  atomic receipt -> existing history/event, including failure/retry/reopen. Then
  demonstrate the app path; controller tests do not satisfy that gate.

## Bounds and validation

Limits are registered in [the central reference](sync-receipt-limits.md). Pending
work is bounded, but completed pointers and native audit history grow with retained
observations. No constant-resource or power-loss claim is made.

The first controller-only run passed **14 tests**, 0 failed/ignored, 361 filtered
out, 24.10 seconds. A concurrent-callback case was then added; the latest expanded
owned regression includes all **15 new controller cases**:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who receipt-runtime-joined-controller -- cargo test --offline --locked \
  -p p2pcore --lib receipts::store::joins:: -- --test-threads=1

CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who receipt-runtime-joined-regression -- cargo test --offline --locked \
  -p p2pcore --lib -- receipts:: group::attestation:: group::doc:: \
  group::engine::durability:: group::own::contents::auth_tests:: \
  sync::engine::group_contents:: sync::engine::group_history:: --test-threads=1
```

Latest expanded result: **160 passed, 0 failed, 0 ignored, 216 filtered out**,
271.38 seconds test execution, no compiler warnings. Shared-queue admission wait
is not included in that test duration. Logs remain outside Git at
`/tmp/receipt-runtime-joined-{controller,regression}.log`.

New coverage: one factory/controller and one ledger/scope, actual ClientEngine
Joined fixture, frozen first time/originals across midnight and newer rosters,
new expected-Join identity, concurrent callbacks, capture and completion failure,
existing-native sidecar failure, exact-body retry, current admission on duplicates,
source/account/device/backing/scope refusal, role changes without history erasure,
full queue/backpressure/rotation, malformed/forged originals, no dangling-evidence
repair, and clean reopen of pending and completed states.

Controller tests use synthetic authority and original protocol statements,
including a fixture that drives the real ClientEngine Joined action. The expanded
regression also includes existing authenticated own-device sync tests, but neither
result exercises the new controller through live Node call hooks, existing history,
media, UI, physical devices, or negotiated recipient/call transport.
Clean sealed-redb reopen is not a process-kill/power-loss experiment. Owned rustfmt,
`git diff --check`, and relative-document-link checks pass.
