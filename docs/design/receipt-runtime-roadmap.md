# Receipt/runtime implementation roadmap

Status: **long-horizon staged implementation plan**. User authorized continuing
this work. This is not permission to commit/merge, change another owner's files,
install on a phone, erase data, or select open security/product policies.

## Baseline and honest progress accounting

- Shared signed delivered/read/call codecs, original-operation receipt documents
  and transactional sealed ledger have a prior owned regression of **127 passed**.
  Commands and limits: [ledger slice](call-receipt-ledger-slice.md).
- Group contents/history and own-device host-proof forwarding are tested slices.
  External host acquisition and an authenticated three-node chain remain missing.
- The [joined evidence bridge](call-join-evidence-bridge.md) in
  `receipts/{evidence,participation}.rs` and `receipts/store.rs` now checks original
  CallStart/expected outbound Join/Roster, commits exact originals with receipts
  atomically and derives joined observations. Eight new cases cover a real
  ClientEngine transition with synthetic authority, failure/retry, duplicate gate,
  clean sealed-redb reopen and missing/corrupt originals. Targeted **38 passed**;
  expanded owned regression **135 passed**, no failures/ignored tests, warning-free.
  Earlier admission-interrupted attempts did not count as passes.
- This bridge is **not connected to Node/live calls**. A signed matching roster
  alone does not establish this local device's actual lifecycle transition.
- The prior receipt/sync work was integrated and user-authorized for merge into
  `dev` at `385a1d649984bfc30b68cfe514499a89a2dbbe08`. That integration does not
  connect recipient receipts to live issuance, transport, or app status displays.

## Product completion requirement — real app receipt displays

**Required work, not an optional UI follow-up:** signed delivery/read receipts
must drive user-visible status in the shipped desktop and Android applications.
Codec, ledger, model, or transport tests alone do not complete this feature.

Current gap: DM ticks use peer version-vector acknowledgments; group Delivered
uses host acknowledgments/local hosting. Read cursors clear our own unread state
and sync between our devices; they are not recipient-signed read confirmations.
The new signed recipient ledger is not yet the source of these app projections.

Completion requires M3 runtime issuance and M4 authenticated replication **plus**:

- Wire durable, verified recipient evidence into the existing core status API and
  both app views, after agreeing DTO/binding and UI file ownership. Do not create
  a parallel demo-only status backend or retrospectively certify legacy ticks.
- Distinguish pending, host-stored, recipient-delivered, and recipient-read
  meanings. Exact presentation is a UI contract to agree; host storage must never
  masquerade as recipient delivery/read. Missing evidence stays unconfirmed.
- Scope each displayed confirmation to the exact message version and recipient;
  preserve read holes. One admitted recipient device suffices for account delivery;
  local mark-read or another device's cursor is not a remote recipient read claim.
- Make provenance inspectable from the message status/detail view: original signer,
  claimed coverage/version, and evidence kind. Distinguish legacy/local status from
  verified recipient evidence; a signature does not prove human attention.
- Validate the actual app path: persist message -> issue signed receipt -> exchange
  original evidence -> durably verify -> core event/projection -> visible status.
  Cover DM and group messaging, edits, linked devices, offline catch-up, duplicate
  replay, restart, bad/missing proofs, and storage failure. No stronger indicator
  may appear before the relevant evidence transaction commits.
- Record runnable integration tests and app-level demonstrations, not only models
  or screenshots backed by fake data. Desktop and Android app validation are
  separate deliverables; physical-device installation/testing still requires
  explicit authorization and the device lock.

**1:1 display progress (2026-10-15, branch).** Core `ReceiptTick` drives both apps' bubble
mark (clock / check = signed Delivered / double check = signed Read) and the message
menu shows signer and device; see [status display](message-receipt-ui-handoff.md).
Desktop demonstrated with real nodes (p2p-desktop vs p2p-peer, screenshots outside Git);
Android built (`scripts/build_android.py --abis x86_64 --debug-rust`) and its 133 unit tests
pass, but no emulator/phone run yet. Groups: not built.

This is a cross-M3/M4 product gate. Do not label signed delivery/read support
shipped, or its UI complete, until the real receipt-backed app path is demonstrated.

## Ownership / first handoff

Read-only inspection found active `groups/call-log` work in
`.claude/worktrees/agent-a53cd8bfa94191939`:

- Owner edits include `group/calls.rs`, `group/calllog.rs`, `group/store.rs`, module
  exports, Android and desktop UI. The new local `clog` view folds events and
  preserves one record across leave/rejoin. It is not portable signed evidence.
- Our lane: shared receipt codec/doc/store, bounded replication, supporting proof
  verification, evidence-backed projections and targeted tests/documentation.
- Do not build a second competing live call-log backend, edit their files, or
  allocate DTO/generated-binding names unilaterally. The older source/store
  changes have been reconciled in the merged integration; that is not a live-hook
  or generated-API agreement. At Joined-controller inspection the old call-log
  checkout was clean and its tip was an ancestor of our merged baseline.
- Ownership agreement and live-hook installation **have not happened yet**.
  The owner needs a handoff for the exact original accepted start, this device's
  current outbound Join and accepted host Roster, plus the confirmed local
  transition. UI-facing events currently discard some of these originals.
- Agree controller lifetime, source/admission lock order, post-commit notification,
  persistence-failure retry/outbox, DTO provenance and old-view compatibility.
  Do not hold media/state locks over unbounded storage work without review.

## Milestones and acceptance gates

### M0 — Stabilize the owned bridge (immediate)

1. Run queued targeted `receipts::` tests; fix compiler/test failures, then run
   the existing owned receipt/group/history regression. Record exact results.
2. Add clean sealed-redb reopen with byte-identical supporting originals;
   missing/corrupt evidence and mismatched scope/generation fail closed.
3. Test expected-Join replay, other linked-device Join, duplicate transitions,
   context/device changes, and before/after-commit failure boundaries.
4. Specify which guarantees are made by the bridge versus required caller locks
   and actual transition witness. Raw reference hashes are not evidence checks.
5. Register new byte/count/time limits in the central limits reference.

Gate: validated primitives and explicit hook contract, **not live call logging**.
No latest-draft regression success can be claimed before this gate passes.

### M1 — One real local group participation path

After ownership agreement:

1. Own one receipt-store/controller per backing namespace; initialize/close it
   with account/group lifecycle, not separate competing factories.
2. Capture accepted start, current local outbound Join and verified Roster at the
   real core transition; validate frozen membership/host/device context.
3. Commit originals and issuer receipt atomically. Only then expose receipt-backed
   history or advertise replication work. Legacy notification is not proof.
4. Preserve a failed event for bounded retry, without signing arbitrary UI events
   or losing it merely because the call engine already entered Joined.
5. Reconcile the owner's existing local call-log view with receipt-derived
   provenance; agree the API rather than duplicate the history engine.

Storage/controller prerequisite now implemented in
[`receipts/store/joins.rs`](../../crates/core/src/receipts/store/joins.rs):
[durable Joined controller](call-joined-controller.md). One factory shares its
controller; it freezes the first signed observation/originals, bounds pending
intents, rotates retries, and commits receipt/evidence/completion atomically.
Explicit source/device admission still runs on duplicates; old epochs are not
automatically rebound. Live event capture, retry scheduling and history/UI hooks
remain uninstalled. Latest expanded owned regression: **160 passed, 0 failed,
0 ignored, 216 filtered out**, including 15 new controller cases; 271.38 seconds.
Exact commands and limitations are recorded in that slice, not a live M1 pass.

Gate: actual lifecycle -> admission -> durable originals/receipt -> view in a
core integration test, including failed persistence and retry. Synthetic protocol
claims, controller/storage or ClientEngine-only tests do not satisfy this gate.

### M2 — Full lifecycle, direct calls and recovery

- Persist authentic observed start/offer before normal completion.
- Local leave/end, interruption, removal, declined/missed where justified, and
  observed verified host End remain separate statements.
- Cover reconnect, host generation/takeover, duplicate/out-of-order callbacks,
  concurrent linked devices and leave/rejoin without duplicate sessions.
- Use observed local segments/monotonic duration; do not sum overlapping devices
  or equate host lifetime with local active duration.
- Cross-midnight calls span receipt scopes but one stable session projection.
- Restart during participation yields explicitly incomplete/interrupted history,
  not a fabricated remote end. Persist the recovery context needed to do that.
- Add the direct-call source adapter without changing existing call/media engines;
  preserve legacy JSON provenance instead of retrospectively upgrading it.

Gate: live direct and group lifecycle tests, clean reopen/recovery, honest unknown
values and consistent group/unified history. Crash/power-loss tests are separate
from clean reopen and need explicit scope/measurement.

### M3 — Shared message delivery/read runtime

- Extract canonical original **persisted verified versions** from DM and group
  records; edits/deletes must not inherit another version's receipt.
- Issue delivered only after durable acceptance; one admitted device establishes
  account-level delivery, not all devices or attachment retention.
- Derive exact read coverage from real read events, retaining holes.
- Preserve original authorization/support material needed for first acceptance
  and historical audit; project status only after durable evidence acceptance.
- Reuse the shared ledger/bounded native exporter, with conversation-specific
  policy rather than a second message receipt transport/database stack.

- Connect the post-commit evidence projection to the existing core/app status
  contract; add message-status provenance inspection without treating host Acks
  or local unread cursors as recipient evidence.

**M3 progress (1:1 only, 2026-10-15, branch `feature/receipt-runtime`, not merged).**
[Runtime slice](message-receipt-runtime-slice.md): the chat engine records exact delivered
intents in the transaction that accepts contact batches/history snapshots, and exact read
intents in the `mark_read` cursor transaction; `MessageReceipts::pump` signs/commits them
with intent completion atomically; `chat_message_receipts` / `chat_day_receipts`
(additive UniFFI) project committed evidence per exact version (`dm_version`). Own-device
cursor moves and legacy ticks issue/project nothing. Group message receipts: **not
started** (needs a group-store transaction seam and member admission). App displays: **not
built**; [UI handoff](message-receipt-ui-handoff.md) because the UI files are owned by
active lanes. Validation (queued, `CARGO_BUILD_JOBS=2`, `--who receipt-runtime-m3`):

```sh
cargo test --offline --locked -p p2pcore --lib -- receipts::store::messages:: version:: --test-threads=1
```
**11 passed** (7 new controller cases with real DID-signed chat versions and sealed redb:
reopen, injected commit failure/retry, duplicate obligation, >256 split, one-device
account coverage, exact read holes, foreign issuer/session/call refusal, atomic outbox).

```sh
cargo test --offline --locked -p p2pcore --lib -- receipts:: chat:: version:: group::attestation:: \
  group::doc:: group::engine::durability:: sync::engine::group_contents:: \
  sync::engine::group_history:: --test-threads=1
```
**208 passed, 0 failed, 0 ignored, 175 filtered out**, 193.5 s, warning-free.

```sh
cargo test --offline --locked -p p2pcore --test readd --test grant_characterization \
  --test message_receipts -- --test-threads=1
```
All passed (readd 4, grant_characterization 7, message_receipts 2). `message_receipts`
runs two real nodes over iroh (network-dependent): recipient issues Delivered after
acceptance, the sender verifies it over chat/6 with a byte-identical receipt id, sender
mark-read does not count, recipient mark-read yields Read at the sender, an edit is a new
version that is re-delivered but not read; restart keeps the same statements. This is
the core path only, NOT a desktop/Android display demonstration.

Gate: source adapters and actual read-event tests, persistence failure/replay,
version substitution, one-device semantics and no receipt-of-receipt loops.
Product completion additionally requires the real desktop/Android receipt displays
and app-level acceptance path specified above; M4 supplies remote proof exchange.

### M4 — Negotiated recipient/call receipt replication

Storage prerequisite now implemented: [atomic remote evidence import](receipt-evidence-import.md).
Six new cases cover two storage hops, missing/orphan/forged originals, refusal,
commit failure, replay gates and VV-only advancement. Latest owned regression:
**141 passed**, no failures/ignored tests before the subsequent packaging slice.
[Proof-aware export packaging](receipt-evidence-packaging.md) now limits native
prefixes by their supporting-proof budget; its targeted 18 participation cases
pass. A fresh-update implicit historical-upgrade guard is included. First expanded
regression attempt waited out its 300-second admission timeout; not a test result.
After verifying no surviving owned job and a free queue slot, expanded retry:
**145 passed, 0 failed, 0 ignored, 178 filtered out**, 90.53s, warning-free.
This is not wire negotiation or Node integration; bounded transport remains unfinished.

- Agree capability/schema compatibility and legacy behavior before wire changes.
- Transfer original native operations plus required original supporting evidence;
  defer missing context, never publish a stronger status for dangling references.
- Bound pages, operations, encoded bytes, retries, pending context and aggregate
  scheduling. A new capability does not permit unbounded evidence buffering.
- Recheck session/device authority; distinguish first acceptance from already
  accepted historical originals according to the explicit selected policy.
- Exercise direct participants and authorized own-device replicas; public hosts
  do not gain private-call/DM access. Group dissemination follows chosen visibility.

**M4 progress (1:1 only).** `tinline/chat/6` adds `ReceiptHello`/`Receipts`/`ReceiptAck`
(original native ops, bounded frames, outbox marks completed by acks; import admits only
the conversation's two DIDs, Delivered/Read kinds, days we hold). **Wire bump needs the
user's OK before merge** (`/5` peers get 426, update without wipe). Not covered yet:
explicit loss/duplicate-frame and revoked/blocked-session wire tests, own-device receipt
replication, group exchange via hosts.

Gate: authenticated exchange with loss/retry/duplicates, obsolete response,
revoked device, missing proof, commit failure, reconnect/reopen and legacy peer;
all relays preserve the original issuer's evidence byte-for-byte. Then exercise
that exchange through the real app status displays under the product completion
requirement above. A passing wire test alone does not establish shipped receipts.

### M5 — External host receipts and scoped relay routing

This can advance independently of the call UI after file ownership agreement.

- Negotiate/acquire external host-signed storage statements; exercise an actual
  authenticated host -> laptop -> phone chain, not fixture issuance plus two nodes.
- Audit existing Delivered projections: unsigned cache/host transport Acks cannot
  become recipient delivery or read evidence.
- Implement per-scope readiness/freshness, durable covered debt and scoped fallback.
  Inventory EOF or a recent connection is not verified catch-up.
- Battery saver may return to a laptop path only after verified catch-up; Direct
  remains independently connected. Upstream-outage/bypass policy is still open.

Gate: topology/fault integration tests with host/privacy separation and no lost
upstream obligations. No exclusive laptop routing before this gate.

### M6 — Sustained load and distributable hosting

- Growing archives, huge days, many groups/writers/devices, aggregate queue/dial/
  connection/CPU/TX+RX/disk budgets and fairness, not just a 70-day happy path.
- Headless host/app distribution and official-group deployment, admission and
  endpoints without silently granting discovery/anonymity or hostless mesh.
- Group attachment support and resumable encrypted blob parity, keeping file bytes
  outside Loro; endpoint pooling only with separate admission/lifetime accounting.
- Measure Android battery/memory/network behavior under explicit device locks and
  separately authorized device sessions. Desktop models cannot establish energy.

Gate: documented workload/resource measurements, sustained fault/load tests and
operational packaging checks. Physical-device checks need explicit authorization.

### M7 — Compact audit history safely (later)

- Canonical shard manifests/revisions and independently computed content roots.
- Recipient-signed supersession for that recipient's exact delivery/read coverage;
  full-shard read only if every covered version was read.
- Preserve signatures, coverage/membership proof and historical authority material.
- Archive and test recovery/authorized replication before any deletion policy.
- Compatible receipt-document rollover for old/offline replicas; demonstrate real
  oplog/disk reclamation rather than assuming map deletion frees storage.

Gate: selected finalization/retention/replica policy and audit/recovery/rollover
acceptance tests. No root-only erasure or receipt deletion is currently authorized.

## Product decisions (user, 2026-10-15)

- **Delivery:** a receipt from any one admitted device of the recipient account
  counts as delivered for that account.
- **Removal:** receipts are never removed. Evidence signed while a device/member
  was admitted stays valid after later device removal or member removal; removal
  only stops new receipts from being accepted.
- **Visibility:** group members can see each other's read receipts and call
  receipts (participation) for that group.
- **Missed call:** a call that was rung but not picked up (no Joined) is missed.
- **Retention:** keep all receipt copies stored for now. Cleanup comes later via
  recipient-signed shard confirmations once shards exist (M7 and
  [message receipt auditability](message-receipt-auditability.md)), not by
  deleting individual message receipts.
- **Group receipts, first version (2026-10-15):** one signed receipt per message
  per member is acceptable ("messy is fine"). Optimize later — see TODO below.
- **Order of work:** every group-receipt step is built with deterministic tests and
  simulations first, then validated on real nodes/apps.

## TODO (deferred by user)

- Group receipts: batch delivered/read confirmations (one signature per member per
  sync or range instead of per message), then shard-level confirmations (M7).
  Measure receipt traffic/storage in the simulation before choosing the batching.

## Decisions still open

- Delayed evidence: whether a receipt signed by a since-removed device that only
  arrives after its removal is accepted (needs a signed-while-admitted rule).
- How many durable replicas are required; shard finalization/revision authority.
- Scoped freshness thresholds and upstream-outage/bypass behavior.

Record alternatives and tests at the relevant milestone; continue independent
mechanical work without selecting these defaults silently.

## Execution discipline

Each slice ends with owned-file formatting, diff/link checks, queued targeted
tests, exact result/limitations and a remaining-work update. Check buildlock status,
use `CARGO_BUILD_JOBS=2`, and inspect surviving owned processes after timeout before
retry. Never stop another owner's job or claim an admission timeout is a test pass.
Keep functional integration, authenticated transport, deterministic storage,
symbolic budgets and physical-device measurements separately labeled.

M0 progress: joined bridge validation/reopen complete for the documented eight
cases; concrete [owner hook handoff](call-join-evidence-bridge.md) recorded. Owner
agreement and live M1 wiring remain unfinished. The owned durable controller now
provides expected-Join-based transition identity and frozen retry state; its live
source lease, scheduling and history notification contract still need integration.
Do not treat handoff documentation as an agreement with the other session.

The user authorized continuing through these milestones without repeated approval
prompts. Advance available owned work; pause only for real ownership/security/product
or explicitly authorization-gated operations. This does not authorize background
execution after the harness stops, guessing policy, or claiming blocked goals done.

Next integration milestone: M1, not simultaneous speculative wire/UI/routing/schema
changes. Keep scoped compatibility/source tests progressing without touching owners.

References: [call handoff](call-receipt-core-ui-contract.md),
[auditability](message-receipt-auditability.md),
[limits](sync-receipt-limits.md),
[routing gates](laptop-forwarding-integration-gates.md).
