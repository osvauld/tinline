# Group persistence and crash contract (packet G-C, S01–S07)

Status: proposal — not approved

Scope: the durability boundary of a group replica (host or member) that ingests original signed
envelopes, what a crash or step failure leaves behind, what a `StoredAck` means, and how restart,
corruption, eviction, pending queues and export behave. Cases are from
`docs/group-chat-tests.md:97-110` (§4). The vocabulary matches C2
(`docs/archive/rotation/rotation-storage-failure-contract.md` §4, lines 110-114).

Executable model: `design-models:crates/group-sim/src/persistence.rs` (15 tests), wired in through one
`#[path]`+`mod` pair in `design-models:crates/group-sim/src/tests.rs`. It reuses the existing fixtures
read-only (`signed_fixture`, `Author`, `Replica::{receive, valid_proof, dependencies_ready}`).

**Not modelled:** fsync, flush ordering, power loss, torn or partial writes, redb transactions,
vault sealing/AEAD, the split between `account.json` and `account.redb`, disk-full, Android process
death and real sockets. A "durable write" is one ideal atomic struct assignment
(`Host::write`). A crash drops every volatile field (`Host::crash`, or an early return in
`Host::ingest` that does not restore `Volatile`). Passing these process-crash tests is **not**
evidence of filesystem or power-loss durability (`docs/group-chat-tests.md:109-110`). S03's "flush
behaviour" is therefore only: *the ack is emitted after the atomic write returns*. Whether a real
store flushes/fsyncs before returning is a G-STORAGE question, not answered here.

## 1. Verified in source and tests

| Fact | Source |
|---|---|
| Design pipeline: validate → scratch replica → atomically persist proofs/state/indexes → acknowledge persistence → fan out | `docs/group-chat.md:170-178` |
| `StoredAck` = accepted persisted envelope IDs/VV, not enqueued bytes | `docs/group-chat.md:221` |
| Push is best-effort; reconciliation repairs gaps | `docs/group-chat.md:222,236` |
| A dishonest host can ack and later discard; eviction must not promise durability | `docs/group-chat.md:247-248,253-254` |
| `QuotaExceeded` is a distinct outcome from `Invalid` | `docs/group-chat.md:185-187` |
| The existing `Replica` keeps everything in memory, with no durable/volatile split | `design-models:crates/group-sim/src/tests.rs:159-167` |
| The existing `Replica::apply` scratch-imports and then mutates in place, so a scratch must be forked to separate import from commit | `design-models:crates/group-sim/src/tests.rs:259,330-350` |
| Pending queue bound `MAX_PENDING = 16` | `design-models:crates/group-sim/src/tests.rs:9,236-238` |

## 2. Model (`persistence.rs`)

- **Durable** (`struct Durable`): `originals` (id → exact encoded author-signed envelope),
  `index` ("peer:sequence" → id), `snapshot` (Loro snapshot, **a cache only**), `pending` (only
  under `PersistBounded`), `evicted` (id → "peer:sequence" manifest), `writes` (oracle counter).
- **Volatile** (`struct Volatile`): the in-memory `Replica`, which includes the volatile pending
  queue. A scratch fork exists only while one ingest is running.
- **Oracle** (`struct Net`): every `StoredAck` is recorded as `(id, backed)`, where `backed` =
  "id was in Durable when the ack left". `pushes` = best-effort fan-out to one subscriber.
- **Sender**: keeps persisted originals until it sees a `StoredAck`, and retries the exact bytes.
- **Open (restart)** rebuilds the replica *only* from `originals`, re-verifying every proof
  through the normal `Replica::receive`, in dependency order. Index and snapshot are compared
  against the rebuilt state and are never trusted alone. Rehydration emits no ack and no fan-out.

## 3. Pipeline and crash table (ordering `CommitThenAck`)

| # | Step | Durable write | Crash after this step → recovery |
|---|---|---|---|
| 1 | Validate (proof, duplicate, dependencies, quota, eviction manifest) | none (except a pending write under `PersistBounded`) | no ack; Durable byte-identical; sender retries |
| 2 | ScratchImport (fork + `receive`, promotes ready pending) | none | same as 1 |
| 3 | **Commit** (durability boundary) | ONE write: originals + index + snapshot (+ pending) | update retained; no ack yet; sender retry → `Duplicate` re-ack with **no write** |
| 4 | StoredAck | none | acked update survives (S03); fan-out lost |
| 5 | FanOut | none (push) | subscriber repaired by reconciliation from persisted originals |

A `Fail(step)` (error, process keeps running) at 1–3 discards the scratch and leaves Durable
byte-identical. At 4 there is no ack, so the sender's retry produces a re-ack. At 5 there is no
push, so reconciliation repairs it.

Counterexample ordering, kept as a limitation: **AckBeforeCommit** (C2 §3c analogue). A crash
between StoredAck and Commit acknowledges an update that the restart loses. The sender has
already dropped its copy, so nothing resends it.

## 4. Invariants checked

| ID | Invariant | Oracle |
|---|---|---|
| P1 | Ack only after commit (I7; C2 I3) | every `Net.acks` entry has `backed == true`, and the id is in Durable at the end |
| P2 | No partial state before the boundary | Durable `==` pre-ingest Durable (whole struct, including `writes`); the recovered `View` (doc value, VV, accepted, pending) equals the populated pre-update view |
| P3 | Exactly one commit per update; idempotent retry | final `writes` equals the fault-free control; a duplicate retry leaves Durable byte-identical |
| P4 | Recovery equals the fault-free control | `canonical(Durable)` (originals, index, pending ids, evicted, decoded snapshot value + VV) equals the control. Snapshot bytes are not compared (`group-chat-tests.md:33`) |
| P5 | Delivery repaired without duplicate events | subscriber `fanout.len() == 3`; a late duplicate push → `Duplicate` |
| P6 | Only verified originals are served (S04) | everything exported is byte-equal to an original and is accepted by an independent replica |
| P7 | Pending never becomes implicitly accepted (S06) | the accepted set after restart equals the accepted set before the crash; the queue stays ≤ `MAX_PENDING` |
| P8 | Export = stored originals, never a synthesized diff (S07) | exported bytes ∈ `Durable.originals`; synthesized diffs are rejected and the peer `View` is unchanged |

## 5. Tests (`cargo test -p group-sim (design-models branch) persistence`)

| Test | KIND | Case |
|---|---|---|
| `every_crash_or_failure_point_converges_to_fault_free_control_with_one_commit` (5 steps × {CrashBefore, CrashAfter, Fail} = 15 runs) | invariant | S01–S03 |
| `s01_crash_or_failure_before_commit_sends_no_ack_and_recovers_no_partial_state` (8 faults) | invariant | S01 |
| `s02_commit_then_crash_or_failure_before_ack_or_fanout_retains_and_retries_idempotently` (6 faults) | invariant | S02 |
| `s03_stored_ack_then_immediate_forced_restart_keeps_acknowledged_original` (3 faults) | invariant | S03 |
| `s03_ack_before_commit_ordering_acknowledges_an_update_that_restart_loses` | limitation | S03 |
| `s04_no_open_policy_serves_corrupted_proof_snapshot_or_index_as_valid` (2 policies × 6 corruptions) | invariant | S04 |
| `s04_fail_closed_policy_refuses_to_open_and_leaves_corrupted_store_untouched` | candidate-policy S04 | S04 |
| `s04_reconstruct_policy_quarantines_and_rebuilds_from_verified_proofs` | candidate-policy S04 | S04 |
| `s05_quota_and_shard_eviction_report_truthful_availability_and_keep_identity` | invariant | S05 |
| `s05_earlier_stored_ack_does_not_guarantee_retention_after_eviction` | limitation | S05 |
| `s06_restart_with_pending_queue_never_accepts_implicitly_and_stays_bounded` (both policies) | invariant | S06 |
| `s06_volatile_pending_queue_is_lost_on_restart_and_recovered_by_sender_retry` | candidate-policy S06 | S06 |
| `s06_persisted_pending_queue_is_reverified_and_rerequested_after_restart` | candidate-policy S06 | S06 |
| `s07_export_after_overlapping_batch_sends_only_missing_byte_identical_originals` | invariant | S07 |
| `s07_synthesized_diff_from_host_state_is_rejected_and_leaves_peer_unchanged` | invariant | S07 |

Non-vacuity guards:
- every faulted run asserts that the fault was actually reached (`expect_injected`);
- baselines are populated: 2 accepted envelopes before the update under test (3 for S04/S07);
- S04 has a clean-reopen positive control;
- S05 shows that the equivocating envelope is otherwise valid, because a fresh replica accepts it, so the eviction manifest is the single cause of rejection;
- in S07, each synthesized input differs from a valid one in one respect: (i) a re-labelled `update` makes the signature fail; (ii) a member-signed diff carrying other authors' ops passes `valid_proof` and dependency readiness and then fails the peer-range check;
- the limitation test has a no-crash positive control.

### Corruptions (S04)

`ProofSignature` (a flipped signature byte; the id still matches), `RecordSwapped` (another valid
record stored under this id), `Undecodable`, `SnapshotBytes` (garbage), `StaleSnapshot` (a valid
but older snapshot), `IndexPointsElsewhere`. A corrupted `a1` also makes its dependent `a2`
unverifiable: `a2` is demoted to pending and is never accepted.

### Mutation sanity check (temporary, not committed; separate target dir)

| Mutation | Result |
|---|---|
| default ordering becomes StoredAck → Commit → FanOut | 6 tests fail (sweep, S01, S02, S03 invariant, S04 reconstruct, S05 limitation) |
| `OpenReport::is_clean` always true (open never detects corruption) | 2 fail (both S04 candidate tests) |
| eviction-manifest equivocation check removed | 1 fails (S05 invariant) |
| `r.fanout.clear()` in `open` removed | **0 fail**. The host's internal `Replica.fanout` is not the fan-out channel (that is `Net.pushes`), so this line is cosmetic |

The S04 invariant test still passes when `is_clean` is forced to true. Serving reads only the
re-verified replica, so it never depends on the clean flag. Detection is covered by the
candidate tests.

## 6. Candidate policies (open gates; nothing selected)

### S04: opening a store that fails re-verification

| Option | Behaviour (tested) | Cost |
|---|---|---|
| FailClosed | refuses to open. Serves nothing, ingests nothing (`Down`), sends no ack. The corrupted store is left byte-identical as evidence | the replica is offline until an operator repairs or resyncs it. No resync path is modelled |
| Reconstruct | quarantines bad records and demotes their dependents to pending. Rebuilds index and snapshot from verified proofs in ONE write. `missing_dependencies()` names the ids to re-request. The rebuilt store reopens clean under FailClosed | a corrupted record silently becomes "missing", and acks previously given for it are no longer backed (cf. S05 limitation) |

Both options re-verify every proof on open. The cost of that at real history sizes is not
measured. A sealed vault (AEAD) would detect some corruption earlier, but that is not modelled.

### S06: is the pending-dependency queue durable?

| Option | Behaviour (tested) | Cost |
|---|---|---|
| Volatile | no durable write for pending. After a restart the queue is empty and the host has nothing to re-request. Recovery relies on the sender retrying unacked originals | more resend traffic; a sender that has gone offline delays repair |
| PersistBounded | each new pending envelope costs one durable write (≤ `MAX_PENDING`; overflow → `Limit` and is not persisted). Records are re-verified on open, still not accepted, and the host names the missing dependency itself. A corrupted pending record is quarantined, never promoted, and recovered from the sender's original | durable write amplification; an attacker can fill the persisted queue up to its bound |

### S05: eviction (tested shape, not a product decision)

Whole-shard eviction keeps the signed genesis/grants and an (id → "peer:sequence") manifest.
Availability then reports `Evicted`, not `Stored` or `Unknown`, and this survives restart. A
dependent of an evicted op is `Pending` and names the evicted id. An equivocating envelope with an
evicted key is `Invalid`. A quota refusal is `Quota`: no ack and Durable unchanged. Refilling
from an original restores `Stored`. **Limitation:** an earlier `StoredAck` is no retention
guarantee. After the sender drops its copy and the host evicts, no replica in the model can serve
it.

## 7. What this does NOT prove

- Real durability: no fsync, torn writes, power loss, redb/vault transactions, cross-file
  atomicity or disk-full. A crash happens only between whole steps, never inside a write.
- Multiple shards and per-shard quotas. The pending promotion that runs inside one ingest can
  exceed the quota by the number of promoted envelopes; this is not bounded in the model.
- Real anti-entropy. `reconcile` is complete-set selection (as in `scheduler.rs`), not a
  bandwidth-efficient protocol. There is one subscriber and one sender.
- Malicious hosts. The host here is honest. A lying `StoredAck` is out of scope (N08).
- Resync after FailClosed, operator UI, and cost of re-verification at scale.
- Recipient delivery/read receipts (I8/R07). Only host storage acks are modelled.

## 8. Questions for the user (G-STORAGE)

1. **S04:** FailClosed or Reconstruct when re-verification fails on open? Under Reconstruct,
   must previously acked-but-quarantined ids be surfaced to their senders?
2. **S06:** Persist the pending queue (bounded, write amplification) or keep it volatile and
   rely on sender retry and reconciliation?
3. **S03:** For the real store, does a `StoredAck` require fsync of the group store before
   sending, and does the same answer apply to the sealed vault?
4. **S05:** Is whole-shard eviction with an identity manifest the right granularity? Does
   availability need to be advertised to peers, or only answered on request?
5. Must the host re-verify every stored proof on each open, or can it rely on the vault AEAD
   plus an integrity check over the index and snapshot?

## 9. Interface notes for the coordinator

- Only shared edit: `design-models:crates/group-sim/src/tests.rs`, the added `#[path = "persistence.rs"] mod
  persistence;`, placed next to `reachability`.
- `persistence.rs` defines local `View`/`view()` and `fork()` helpers because they were not
  exposed at base 9e6bbb5. After the G-H merge they could be replaced by G-H's `snapshot()` /
  `assert_rejected_unchanged`.
- Shared `CARGO_TARGET_DIR`: one run during development executed another worktree's 35-test
  `group-sim` binary, even though only `persistence.rs` had been touched. Mutation runs therefore
  used `CARGO_TARGET_DIR=/tmp/claude-1000/target-group-gc`. Final runs passed 39/39 both there and
  in the shared target after `touch design-models:crates/group-sim/src/*.rs`. Each run showed `Compiling
  group-sim (.../rot-gc/...)` and listed the 15 `tests::persistence::*` tests.
