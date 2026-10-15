# Typed call claims and transactional receipt ledger

Status: **implemented/tested low-level primitives**, not live call logging,
UI bindings, signed lifecycle issuance or negotiated receipt transport. Owned
regression: **127 passed, 0 failed, 0 ignored, 178 filtered out**.
Another session owns call UI/integration; no call engine/UI files were edited.

Subsequent [joined evidence bridge validation](call-join-evidence-bridge.md) adds
8 cases, atomic supporting-original storage and proof-backed observation reads.
Latest expanded owned regression: **135 passed, 0 failed, 0 ignored**. The 127-case
commands below remain the historical validation of this initial ledger slice;
real Node lifecycle hooks and negotiated recipient/call transport remain absent.

## Typed call payload

`receipts/calls.rs` adds `Kind::Call` with `Receipt::call: Option<CallEvent>`.
Calls cannot carry message hashes; delivered/read claims cannot carry call data.
All payload discriminators/fields are inside the original DID-signed body.
The issuer uses the existing `recipient` DID/device fields; for call observations
this means the observing account/device, not proof that anybody else participated.

This extends an **undeployed internal prototype v1 encoding**. It is not backward
compatibility with a previously deployed recipient receipt schema, migration of
host attestations, or an upgrade of old unsigned call-log rows.

Call events:

- `ObservedStart`, `Joined`, `Interrupted`, `ObservedHostEnd`, `Missed`, `Declined`;
- `ParticipationEnded { outcome }`, with Left, RemoteEnded, Removed, HostLost,
  Dropped, AnsweredElsewhere or Superseded.

Fields bind call/session ID, event, observed time, optional local active-segment
duration, applicable group host generation and sorted distinct supporting proof
digests. Group observations require a positive host generation; direct ones forbid
one. ObservedHostEnd is group-only. End/interruption can report local segment time;
other kinds cannot silently claim active duration. Observation UTC day must match
the receipt scope, including events on different days of the same session.

Start/join/host-end/missed/declined require supporting references. **References are
not verified evidence by themselves.** Runtime admission must check actual original
CallStart/Join/Roster/End or invitation evidence and retain it durably. A signed
Join request is not a successfully admitted local participation event. Local
ending and observed host ending remain distinct; neither proves all participants
observed the end or heard/saw media. Wall-clock claims are not trusted consensus
ordering, issuance time or revocation/freshness proof.

## Transactional sealed storage

`receipts/store.rs` accepts an existing `storage::Sealed` backing. No additional
vault/key scheme or duplicated per-receipt signed-body table is introduced.
Its **internal storage** namespace is:

```text
receipts/{dm|group}/{conversation_hash}/{UTC_day}/u/{sequence:020}
    accepted original native Loro update batch
.../state
    last committed storage sequence and encoded native VV
.../index/{original_signed_receipt_digest}
    first native writer/counter pointer (12 plaintext bytes before vault sealing)
```

These are not new negotiated wire document names. Index pointers are not another
signature or delivery/read/call UI projection. Original signed bytes remain in
the native updates; they can be fetched from the reconstructed document by digest.
No per-message status table, roster/duration projection or generated UI API exists.

`ReceiptStore` owns namespace leases: one live `Ledger` per scope in that factory;
share its ledger via Arc rather than reopen another mutable copy. Failed opens
release the lease. **One factory must own the namespace per backing store**;
creating independently managed factories over the same DB is unsupported. The
future Node controller must enforce this ownership and lock ordering with source
content/admission/lifecycle state.

Writes/imports work on an independent native checkpoint while holding the ledger
lock. A transaction commits update, sequence/VV state and fresh pointer indexes
with one sealed-store `apply`. Only after commit does the live doc/frontier change
and `Committed` return entries/update/frontier for later propagation. Any policy,
encoding or commit failure leaves the original live doc and durable state unchanged.

Duplicate bodies deduplicate. A genuinely new accepted native op range can advance
VV with no new receipt IDs; that advancement still commits. Existing index pointers
retain first positions. Open validates contiguous sequences, reconstructed frontier,
signatures/structure/caller admission and required index positions rather than
interpreting inconsistent rows as an empty/complete ledger.

This is append-native-updates storage, not full-snapshot rewrite on every event.
Reconstruction scans retained rows and verifies receipt history; checkpoints,
Loro import/planning and queries remain history-dependent. No total disk quota,
constant-resource claim, rollover or evidence-retention deletion is implemented.
Existing 500-call view trimming must not delete this ledger's evidence.

## Explicit remaining runtime gates

Low-level `Ledger::write` can durably store a signed claim, but does **not** derive
that claim from a real call lifecycle, durable original messages or a read event.
An explicit caller admission callback is mandatory. The runtime controller must:

- freeze/check verified participant/contact/member/device/source context and real
  observed lifecycle/content under appropriate locks;
- distinguish first acceptance from previously accepted historical evidence;
- persist/verify supporting originals; a dangling digest must never produce a
  stronger host/participant-provenance label;
- expose statuses/notifications only after successful transaction;
- integrate call events across midnight/reconnect/takeover without inventing global
  completion, duration or duplicate sessions;
- add explicit negotiated transport and source-device rechecks.

No runtime defaults silently resolve member revocation/regrant/late receipt policy,
read visibility, role authority, retired docs or artifact retention. The existing
host attestation authority/debt/forwarding path remains separate and unchanged.

## Tests and exact commands

Five new call-codec/native tests cover individual/group observations, signatures
binding all session fields, malformed/mixed payloads/day/generation/evidence,
local durations versus host end, and caller refusal of unconfirmed join claims.
Six new store tests cover atomic failure/retry and original/index retention,
receiver/policy failure, single-owner leases, real clean sealed-redb reopen for
call claims/read holes, VV-only advancement and persisted inconsistency rejection.

An initial queued attempt timed out before compilation; inspection found no
surviving owned job. The first admitted run had **29 passed, 1 failed**: the test
expected a same-value local Loro insert to create an op, but Loro optimized it
away. The fixture was corrected using the existing group test's synthetic alternate
writer prefix. This tests persistence of a known-ID/new-range edge; it does **not**
assert that writer forks are safe or change fork acceptance policy.

Passing targeted queued command:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-call-receipt-ledger-fixture-fix -- cargo test --offline --locked \
  -p p2pcore --lib -- receipts:: --test-threads=1 --nocapture
```

**30 passed, 0 failed, 0 ignored, 275 filtered out**, 17.55 seconds, warning-free.
`receipts::` also selects six existing own-device group receipt wire/state cases;
there are 24 shared receipt cases (prior 13 plus new 11).

Complete owned regression:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-call-ledger-owned-regression -- cargo test --offline --locked \
  -p p2pcore --lib -- receipts:: group::attestation:: group::doc:: \
  group::engine::durability:: group::own::contents::auth_tests:: \
  sync::engine::group_contents:: sync::engine::group_history:: \
  --test-threads=1 --nocapture
```

**127 passed, 0 failed, 0 ignored, 178 filtered out**, 80.34 seconds, warning-free.
Shared build admission and two Cargo jobs used. Clean reopen is not power-loss,
process-kill/fsync proof. Call tests use constructed claims, **not an actual
call lifecycle -> signing -> replicated history test**. No phone/UI/media changes.

Related: [core/UI handoff](call-receipt-core-ui-contract.md),
[initial shared primitive slice](shared-recipient-receipt-slice.md),
[auditability](message-receipt-auditability.md), [limits](sync-receipt-limits.md).
