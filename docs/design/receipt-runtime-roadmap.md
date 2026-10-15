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
- Refactor integration remains deferred. Do not confuse isolated-worktree tests
  with integration against current `dev` or the newer refactor tree.

## Ownership / first handoff

Read-only inspection found active `groups/call-log` work in
`.claude/worktrees/agent-a53cd8bfa94191939`:

- Owner edits include `group/calls.rs`, `group/calllog.rs`, `group/store.rs`, module
  exports, Android and desktop UI. The new local `clog` view folds events and
  preserves one record across leave/rejoin. It is not portable signed evidence.
- Our lane: shared receipt codec/doc/store, bounded replication, supporting proof
  verification, evidence-backed projections and targeted tests/documentation.
- Do not build a second competing live call-log backend, edit their files, or
  allocate DTO/generated-binding names unilaterally. Existing changes in our
  older `group/store.rs` etc. still need an explicit integration reconciliation.
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

Gate: actual lifecycle -> admission -> durable originals/receipt -> view in a
core integration test, including failed persistence and retry. Synthetic protocol
claims or ClientEngine-only tests do not satisfy this gate.

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

Gate: source adapters and actual read-event tests, persistence failure/replay,
version substitution, one-device semantics and no receipt-of-receipt loops.

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

Gate: authenticated exchange with loss/retry/duplicates, obsolete response,
revoked device, missing proof, commit failure, reconnect/reopen and legacy peer;
all relays preserve the original issuer's evidence byte-for-byte.

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

## Decisions that cannot be guessed

- Recipient device/contact/member first acceptance, removal/regrant/epoch and
  delayed evidence rules; current host-attestation rules do not settle these.
- Call/read evidence visibility and participant-confirmation dissemination.
- Incomplete/missed semantics and historical authorization preservation.
- Required durable replicas, retention grace, shard finalization/revisions.
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
agreement, semantic transition/retry identity and live M1 wiring remain unfinished.
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
