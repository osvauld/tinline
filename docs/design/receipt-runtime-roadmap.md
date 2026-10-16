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
pass, but no emulator/phone run yet. Groups: core step 1 only (see below), no display.

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
cursor moves and legacy ticks issue/project nothing. Group message receipts: **step 1
in progress** (core + in-process simulation only, see "Group message receipts" below). App displays: **not
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

## Group message receipts (step 1, branch `feature/group-receipts`, 2026-10-16)

Status: **core code written; tests written but NOT yet run** (the shared build disk hit
ENOSPC while compiling the test binary; see below). No wire frame, no app UI, no device run.
Nothing here is production integration: the multi-node cases are in-process simulations.

What exists (all in `crates/core`):

- **Format.** The shared signed `receipts::Receipt` (DID-signed, `Delivered`/`Read`), one
  ledger per `(Group(gid), UTC day of the post's day doc)`, covering exact
  `group_version(stored SignedRecord bytes)` digests. `metadata` = the issuer's meta-doc
  frontier when it accepted the post (delivered) or moved its read cursor (read), signed.
  One statement per member device per kind per pump (<=256 posts each); batching is the TODO.
- **Store seam.** `GroupStore::{append_shard_with, append_meta_with, put_meta_with}` add
  caller ops to the existing transactions. Ledgers and intents live in `group.redb`
  (`receipts/group/{gid}/{day}/…`, `receipt-runtime/messages/v1/…`); `GroupCore.receipts` is
  the one `MessageReceipts` controller of that store. Group intents use intent format 2
  (adds the frontier); DM intents keep format 1.
- **Issuance.** `apply_day` records a delivered intent for fresh visible posts of others in
  the transaction that stores them (not for batches our own device forwarded: that device
  issues); `apply_meta` does the same for pending posts a meta update made visible;
  `GroupCore::mark_read` (behind `group_mark_read`) moves a now **monotonic** cursor and records
  read intents for (old, new] in the same transaction (newest 4096). The pump runs in
  `group_after`, after `group_mark_read` and at core open.
- **Admission on ingest** (`group::member_receipts::check`, every device the same verdict in
  every delivery order): issuer DID admitted (member, not removed/left) as of the receipt's
  signed frontier; every covered post held, visible, not the issuer's own, its frontier inside
  the receipt frontier, and the issuer a member as of the post's frontier (late joiners cannot
  confirm earlier posts). Missing meta records/posts give a `pending: ` refusal (nothing
  stored; the next catch-up retries). Accepted statements are never re-judged or removed.
- **Aggregate** (`GroupCore::member_receipt_status` / `member_day_receipts` ->
  `GroupReceiptStatus { tick, eligible, delivered, read, members[{receipts, eligible}] }`):
  tick `Waiting` (one tick) -> `Delivered` when every eligible member has delivered or read ->
  `Read` when every eligible member read; `None` for posts of others. Nobody eligible stays
  `Waiting`. Per-member rows reuse `RecipientReceipts`/`ReceiptProof`. Rust-only DTO for now
  (UniFFI binding to be agreed with the UI lanes).
- **Eligibility (decision taken here, 2026-10-16; user may revise):** members who were
  members as of the post's signed frontier, excluding the author, AND are still members now.
  A member removed or gone after the post stops blocking the mark; its statements are kept and
  listed with `eligible = false`. A late joiner never counts for earlier posts.
- **Clock for "signed while admitted":** the receipt's signed meta-doc frontier (causal, U11
  style), never a wall clock. Proves: the signer claims it had seen exactly those records; a
  statement whose frontier includes the signer's removal/leave is refused; a statement can
  never cover a post whose author had seen the removal (post frontier must be inside the
  receipt frontier). Does NOT prove: that a removed member did not withhold meta updates to
  keep signing with a stale frontier for posts concurrent with its removal (same limit as
  posts). Device level: group core admits per DID (no group device revocation exists), so
  "device admitted" = "its DID admitted"; a hostile linked device holding the account key is
  not distinguishable (shared-mnemonic limit).

Wire (2026-10-16, user: "we dont need backward compatibility"): **`tinline/group/4`** =
`/3` plus `ReceiptHello { days: [(day, vv)] }`, `Receipts { day, update }`,
`ReceiptAck { day, vv }`, no capability gate, no compatibility with `/3` (different ALPN, an old
peer fails at connect). Scheduling (`engine/member_receipts.rs`): each session sends
`ReceiptHello` after its doc catch-up (recent held days + unacknowledged outbox days); the
answer is `Receipts` for what the peer lacks (bounded: 256 KiB / 64 ops per frame, 16 frames per
pass, 64 days per hello); every import is answered by `ReceiptAck` (authoritative VV, clears
the sender's outbox mark). New statements from our pump (`group_after`, `group_mark_read`,
core open) and fresh imported ones are pushed live to every other session of the group; a host
re-exports the ledger's original native ops, so every statement keeps its signer's bytes,
writer and counter. A frame refused as pending is not stored; its day is remembered on the
session and asked again (`ReceiptHello`) after our docs advance. A refused frame never closes
the session. Lost frames: recovered by the next ack/hello (reconnect = new session = hello).

Tests (written, **not yet run**: first ENOSPC, then the user paused all builds):
`group::member_receipts::tests` (3 pure cases) and
`group::engine::durability::group_receipts_sim` (10 cases over real in-process GroupCores:
waiting -> delivered -> read; partial; duplicate frames/posts/relays; reordered and lost
frames then catch-up; forged/unsigned/wrong-group/foreign-writer/non-member/own-post
refused; late joiner; removed member's late pre-removal statement accepted and post-removal
or stale-frontier ones refused, leave/removal not blocking; pending posts and unknown
frontiers; one device per account; reopen of intents/ledgers/kept statements; size table)
plus 3 cases over the group/4 frames (real `Sess` queues, postcard round trip, the handlers
`group_read` calls; members only via the host): host relay byte-identical to all members;
duplicate frames, a lost frame recovered by reconnect hello and outbox ack; a statement for a
post not yet held asked again after the post arrives. Not a network (iroh) test.

```sh
CARGO_BUILD_JOBS=2 python3 scripts/buildlock.py --who group-receipts -- \
  cargo test --offline --locked -p p2pcore --lib -- group::member_receipts:: \
  group::engine::durability::group_receipts_sim:: --test-threads=1 --nocapture
```
Then the existing owned regression (`receipts:: chat:: version:: group::` filters above) must
be re-run: the store seam and `apply_day`/`apply_meta` transactions changed.

Not done: real-network (iroh, two-node `Node`) receipt test, UniFFI binding, app ticks/
details, own-device replication of ledgers beyond the host relay, batching, shards. Imports
raise only `on_group_changed` (no per-message event yet).

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
- **Delayed evidence (2026-10-16):** a late receipt from a removed device/member is accepted
  if it was signed while admitted; for groups "while admitted" is judged against the
  receipt's signed meta frontier (see "Group message receipts"). Late joiners do not send
  receipts for posts from before they joined; no group-size cap for now.

## TODO (deferred by user)

- Group receipts: batch delivered/read confirmations (one signature per member per
  sync or range instead of per message), then shard-level confirmations (M7).
  Measure receipt traffic/storage in the simulation before choosing the batching.
  Measurement: `receipt_count_and_bytes_per_post_by_group_size` (3/10/50 members, one post,
  everyone delivers and reads) prints statements and signed/frame bytes per post. Model:
  2*(N-1) statements per post per N-member group (plus duplicates from extra devices).
  **Numbers pending** (test not yet run, ENOSPC). The `Receipt.messages` list already allows
  one statement to cover up to 256 posts, so a range/batch receipt needs no format change.

## Decisions still open

- ~~Delayed evidence~~: decided 2026-10-16 (accepted if signed while admitted; group rule
  in "Group message receipts"). Still open: per-device revocation for groups (none exists).
- Group eligibility for members removed after a post (implemented choice: they stop
  blocking; their statements stay listed) and the "nobody eligible" mark (stays one tick).
- Whether read receipts can be turned off (not asked yet).
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
