# Host-signed storage attestation: crypto / persisted-state slice

Status: implemented in the isolated audit worktree. The initial crypto/verification
slice had **13 new Rust tests** and an **85-case owned regression**. Subsequent
atomic proof storage and negotiated own-device forwarding passed a **13-case
targeted run** (five existing codec cases plus eight new persistence/wire cases).
The latest owned regression including three additional guard/fault/reopen tests
passed **96 cases, 0 failed, 0 ignored, 185 filtered out**. No external group-host receipt acquisition,
member/read receipts, shard summaries, UI changes or direct-route suppression yet.

See [Message receipt auditability](message-receipt-auditability.md) for the selected
shared groups/chat receipt and independent shard-root/compaction design.

The user selected host-signed storage attestations and current verified hosting
authority on first acceptance; already accepted statements remain historical
observations. The initial slice implements the read-only first-acceptance check;
the follow-on acceptance path below persists original statements atomically.
Group regrant/epoch/succession policies are unchanged.

Current limits, units, defining constants and tuning status are listed centrally
in [Sync and storage-receipt limits](sync-receipt-limits.md). The signed-envelope
ceiling now derives exact codec overhead instead of adding padding.

## Signed statement (prototype, not a deployed wire contract)

`group/attestation.rs` uses canonical postcard (`codec::from_bytes_canonical`), dedicated signature
domain `tinline-group-storage-attestation-v1\0`, and a length-delimited body.
The DID-key Ed25519 signature binds:

- version 1, canonical group ID and meta/day document;
- host DID and its **group device ID**, not a calling endpoint;
- actual persisted document frontier, as sorted positive exclusive writer counters;
- the host's persisted metadata context, in the same canonical frontier form.

Metadata attestations require stored frontier == metadata context. Each frontier
is bounded to 4,096 writers; body to 128 KiB; enclosing signed bytes to body + 80 (two fixed-u64 Vec lengths
and the 64-byte Ed25519 signature).
Empty/negative/zero/duplicate/unsorted frontiers, unsupported versions, bad scope,
noncanonical/trailing/truncated encodings and malformed signatures are refused.
Decoder byte limits also cover adversarial encoded lengths. No wall-clock issuance
claim is added: a claimed metadata frontier does not prove when signing occurred.

`SignedStorageAttestation::open` checks signature/shape/scope only. It does NOT
prove hosting authority; a member's valid signature passes that cryptographic
check but fails the independent role check.

## Persisted issuance and first-acceptance checks

`GroupCore::issue_storage_attestation` holds the importer group lock through
loading persisted metadata/day rows and signing. It derives hosting authority from
original committed metadata records, not a mutable derived cache. It requires
membership, no removal, and the existing chat HostPermit + HostAccept authority for
the exact DID/group device. Call hosting is not implicitly chat hosting.

It reloads the actual accepted native operation document from sealed rows. Records
were signature-verified at ingress; load retains the existing verify-on-arrival
policy from `group-shards.md`. Persisted oplog structure/positions are checked.
A failed append cannot produce a statement covering the failed operation; a failed
metadata regrant cannot restore authority. No planned/sent/inventory VV is accepted
as the stored frontier. Unknown/empty documents cannot be attested. Closed cores
are refused, including a second check after planning/signing.

`GroupCore::verify_storage_attestation` uses receiver-local persisted metadata under
the same lock. Unknown/missing/behind metadata defers. Current exact hosting
authority and nonremoval are mandatory. A genuine signature with an old metadata
context cannot override known revocation/removal.

**This verifier is read-only.** Its result is not a durable accepted receipt and
can become stale after its lock is released. The follow-on acceptance adapter
rechecks authority/debt and commits provenance/indexing under the importer lock;
it does not blindly store a previously verified result after a race.

A signed statement is a host's storage attestation, not a remote disk audit.
Dishonest hosts can lie/erase data. It neither proves every member received/read
messages nor attachment bytes are retained. It provides no readiness/freshness
certificate or strong isolation of devices sharing account recovery secrets.

## Initial crypto / verification validation

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-attestation-owned-regression -- cargo test --offline --locked \
  -p p2pcore --lib -- group::attestation:: group::doc::batching:: \
  group::engine::durability:: group::own::contents::auth_tests:: \
  sync::engine::group_contents:: sync::engine::group_history:: \
  --test-threads=1 --nocapture
```

**85 passed**, 61.51 seconds. No ignored tests, 185 filtered out, no compilation
warnings. Includes existing native-Loro, sealed persistence, own-device wire and
lost-WANT/page/cache-Ack reconnect regressions. The new thirteen consist of five
pure cryptographic codec tests and eight actual group-engine/store cases:
independent host DID, two own replicas preserving opaque proof bytes, failed day
append/retry, failed metadata regrant, member-only signature, wrong host device,
forged mutable role cache, missing/behind metadata, revocation and removal.

These initial thirteen attestation tests do **not** use a negotiated receipt
transport, run a three-node receipt exchange, or reopen a newly persisted receipt
store. Subsequent wire/persistence coverage is described below.
Existing store clean-reopen tests do not establish process-kill/fsync/power-loss
proofs. Payload limits do not prove constant CPU/RAM: document reconstruction and
oplog traversal remain history-dependent. No Android/phone deployment.

An initial queued attempt timed out before compilation while another agent held
the build slot. Process inspection found no surviving owned job. An admitted
retry passed thirteen; its single owned unused-mut warning was removed before
the final warning-free 85-case run.

## Follow-on: atomic proof storage and own-device forwarding

`group/attestation/persistence.rs` / `group/store/receipts.rs` retain the original
signed bytes under a content digest and atomically project the host VV and clear
only covered metadata/day debt. New statements recheck current authority; known
originals remain historical and can reconcile subsequently imported covered loans.
Legacy unsigned Acks never populate the immutable proof ledger.

`sync/engine/group_receipts.rs` paginates these originals after explicit
`cap/group-receipts/v1` negotiation together with content/history capabilities.
Only one receipt pull is outstanding; cursor inspection is not an acceptance Ack.
Local document changes request a follow-up pass so receipt-before-content ordering
can reconcile later loans. Legacy message discriminants and Hello count bounds
are preserved. Limits and pacing are indexed in the central reference.

Validated command:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-receipt-durable-wire -- cargo test --offline --locked \
  -p p2pcore --lib -- group::attestation:: \
  group::engine::durability::receipt_persistence:: \
  sync::engine::group_receipts:: --test-threads=1 --nocapture
```

**13 passed, 0 failed, 0 ignored, 265 filtered out**, 6.07 seconds; warning-free.
Four new store cases cover atomic failure/retry, partial versus full debt coverage,
revocation/historical originals, metadata debt and exclusion of legacy Acks.
Four new own-wire/state cases cover three-marker negotiation/discriminants,
bounded refresh, fixed rescan deadlines and actual authenticated two-node proof
forwarding with content/debt reconciliation. This does not establish a three-node
host acquisition path, crash durability, receipt-store reopen or sustained load.

## Guard/fault/reopen validation and latest owned regression

Three additional actual-production-code cases are registered and passing:

- `durability/receipt_reopen.rs`: clean sealed-redb close/reopen after failed receipt
  commit, successful retry, exact original day/meta proofs, persisted projections,
  scope-specific debt clearing, exclusive proof pagination and idempotent replay.
- `group_receipts/guards.rs`: real localhost framing around the receipt handler;
  unnegotiated/obsolete responses, injected append failure and removed own device
  do not persist proofs or project a host confirmation. This handler fixture seeds
  the registry; authentication is tested separately in the actual self_run cases.
- `group_receipts/faults.rs`: actual authenticated receiver self_run with a
  synthetic fault peer; drop one request and one computed reply, verify stable
  retry IDs, ignore an obsolete response, deliver a duplicate, interrupt after
  durable acceptance and reconnect. Exactly one original proof/index remains;
  covered day debt clears. A content/history-only legacy session receives no
  receipt frames. These are application-message faults, not QUIC packet loss.

Targeted queued command:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-receipt-fault-reopen -- cargo test --offline --locked \
  -p p2pcore --lib -- group::engine::durability::receipt_reopen:: \
  sync::engine::group_receipts:: --test-threads=1 --nocapture
```

**7 passed, 0 failed, 0 ignored, 274 filtered out**, 20.65 seconds.

Latest complete owned regression:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-receipt-owned-regression -- cargo test --offline --locked \
  -p p2pcore --lib -- group::attestation:: group::doc::batching:: \
  group::engine::durability:: group::own::contents::auth_tests:: \
  sync::engine::group_contents:: sync::engine::group_history:: \
  sync::engine::group_receipts:: --test-threads=1 --nocapture
```

**96 passed, 0 failed, 0 ignored, 185 filtered out**, 92.20 seconds; warning-free.
Both commands used shared build admission with two Cargo jobs. No other agents'
processes or artifacts were modified. Clean reopen is not process-kill/fsync/
power-loss proof. This is not device-energy, global-resource or sustained-load
validation; direct routing and UI delivery semantics remain unchanged.

## Next work

1. External group-host receipt acquisition with explicit compatibility: group/2
   lacks a receipt capability field; no unconditional frames or day markers.
2. Broader multi-scope fault schedules, growing-archive fairness, external-host
   end-to-end receipt acquisition and crash/device-dependent validation.
3. Shared member/chat delivery/read receipts and eventually independently computed
   shard roots/receipt compaction, following the auditability design.
4. Status provenance review, scoped freshness/readiness and aggregate budgets
   before exclusive laptop routing. Direct remains enabled.

Sources: `group/attestation{.rs,/tests.rs}`,
`group/engine/durability/storage_attestations.rs`.
Related: [provenance decision](host-storage-receipt-decision.md),
[integration gates](laptop-forwarding-integration-gates.md).
