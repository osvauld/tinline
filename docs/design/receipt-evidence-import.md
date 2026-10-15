# Atomic remote receipt/evidence import

Status: **implemented/tested storage prerequisite for roadmap M4**. No new wire
messages/capability, Node controller, live call hook or UI interface is installed.
This extends the [joined bridge](call-join-evidence-bridge.md) without selecting
recipient first-acceptance/removal/visibility policy.

## Receiver contract

`Ledger::apply_remote_with_evidence(update, supplied, admit)`:

1. Bounds/canonical-checks distinct content-addressed original SignedBlob sidecars.
2. Fork-stages native import with existing signature/scope/writer/structure and
   predecessor checks. Nothing is published by this stage.
3. Inspects the actual new native interval, including known IDs under new counters,
   using `ReceiptDoc::entries_since`. Fresh-ID count is not a frontier certificate.
4. Resolves the exact call proof references from supplied or already durable
   originals. Missing/corrupt proofs, duplicate/orphan sidecars and excessive
   combined reference count refuse without changing live/durable state.
5. Calls mandatory caller admission with native claims plus all resolved originals.
   `ImportClaim::already_stored` means **body already durable**, not historical
   authority, supporting-proof validation or permission to skip admission.
6. Atomically commits native update, VV/sequence, fresh indexes and originals.
   A commit failure publishes no progress. Known-ID/new-range advancement still
   commits, even with no new receipt rows.

The callback must verify protocol meaning, member/contact/device/source context,
first-versus-historical acceptance policy and authenticated source-session admission.
`verify_joined_entry` reuses the view's exact original CallStart/expected Join/Roster
correspondence checks; it does **not** choose current/historical authority or prove
that the receiving device itself joined a call. It validates the issuer's report.

Duplicate native responses still invoke the caller, including empty claim/proof
sets: a revoked source session must not bypass its gate merely because the receipt
body was already stored. Replays with sidecars match complete proof sets, avoiding
expansion into every unrelated event sharing a CallStart. No-op replay returns no
update and does not rewrite evidence/index/state.

Known claims with missing supporting originals are not silently upgraded/repaired
by this method. Explicit evidence recovery/migration needs its own transaction and
policy. Ordinary low-level reconstruction remains possible but cannot produce the
stronger proof-backed joined view when required originals are missing.

## Bounds and limitations

`MAX_IMPORT_EVIDENCE = calls::MAX_CALL_EVIDENCE = 16` is a **provisional per-import
sidecar AND distinct resolved-reference budget**, not a negotiated roster/frame or
retention limit. Each original envelope is at most 8,294 bytes; payload bytes total
at most 132,704 before list/digest/transport overhead. Native update remains under
its existing 12 MiB primitive ceiling. Central values: [limits](sync-receipt-limits.md).

Sender batching must respect this additional budget; bounded native operation
export alone does not guarantee a proof-complete admissible package.
[Proof-aware export packaging](receipt-evidence-packaging.md) now re-plans smaller
prefixes and includes exact originals. Negotiated transport and context deferral
remain unfinished. The new import guard also rejects fresh-update attempts to heal
an older dangling body implicitly; this document's 141-case result predates that
subsequent slice.
History scans, checkpoints, native verification and trial planning are not
constant-resource. Aggregate fairness/quota/CPU/disk limits remain unfinished.

## Six new tests

- Original issuer receipt/native update and exact protocol originals cross two
  storage hops with distinct replica writers, without relay re-signing.
- Missing, duplicate and unrelated supplied originals refuse; subsequent complete
  retry succeeds without prior frontier/index/evidence residue.
- Admission refusal and injected transaction failure leave no progress/originals;
  an empty duplicate response still invokes current peer admission.
- Valid receipt signature and correct evidence hashes do not make a forged protocol
  Roster valid; the explicit protocol verifier rejects it before persistence.
- A known dangling lower-level claim is not implicitly upgraded by a sidecar replay.
- Known-ID/new-native-range import checks originals and atomically persists VV-only
  progress; injected failure leaves the prior frontier. Uses the existing synthetic
  alternate-prefix fixture, **not a writer-fork safety assertion**.

First targeted queue run after five tests:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-call-proof-atomic-import -- cargo test --offline --locked \
  -p p2pcore --lib -- receipts:: --test-threads=1 --nocapture
```

**43 passed, 0 failed, 0 ignored, 275 filtered out**, 22.57s, warning-free.
After adding the sixth VV-only case, expanded owned regression:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-call-proof-import-owned-regression -- cargo test --offline --locked \
  -p p2pcore --lib -- receipts:: group::attestation:: group::doc:: \
  group::engine::durability:: group::own::contents::auth_tests:: \
  sync::engine::group_contents:: sync::engine::group_history:: \
  --test-threads=1 --nocapture
```

**141 passed, 0 failed, 0 ignored, 178 filtered out**, 84.59s, warning-free.
New replication tests are native/storage hops, not authenticated network sessions.
Call authority is synthetic; actual Node admission/lifecycle remains unconnected.
Prior authenticated host-proof wire tests are separate and do not establish new
recipient/call transport. No UI/media engine/device/phone changes.

Related: [runtime roadmap](receipt-runtime-roadmap.md),
[call owner handoff](call-receipt-core-ui-contract.md),
[ledger](call-receipt-ledger-slice.md).
