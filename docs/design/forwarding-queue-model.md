# Bounded forwarding and partial handover model

Status: executable offline model only. No production protocol/schema change.
Source: `docs/archive/models/test_forwarding_queue_model.py`.

## Assumptions

Synthetic records carry immutable author, scope, intended destination, payload and
proof-token fields. Validation is a boolean oracle, not cryptography. Record IDs
are fixture counters, not production content hashes. Scope authorization is an
explicit allowlist. Authenticated stored-receipt origin is a test oracle consulting
the issuing replica's accepted receipt set, NOT a viable wire verification scheme.

One phone, one own desktop, one official host serving two groups, and two contacts
are modeled. Calls, blob transfers, membership transitions, stale-frontier validity,
blocking propagation and endpoint rotation remain outside this model.

## Queue and persistence behavior

- Every replica has a fixed accepted-record quota (32 in default fixtures).
- Full local storage explicitly rejects a write; it cannot report local success.
- Best-effort notification queue is bounded (two hints in default fixtures).
  Overflow drops notifications, not accepted records.
- Reconciliation scans accepted inventory rather than relying on surviving hints.
  Missing original records are transferred in bounded batches, round-robin across
  currently eligible scopes for each direction. Receipt batches also rotate by scope.
- A full receiving replica rejects the record without issuing a stored receipt.
  Sender retains its original and its pending state; increasing quota permits retry.
- Restart retains only designated durable records and receipts, losing hints.
  This is in-memory restart simulation, not crash/fsync/storage-transaction testing.
- Original authorship/proof tokens are unchanged across laptop forwarding.

Numbers are small fixture inputs, NOT production quotas. Accepted stores retain all
records; safe compaction/eviction is not solved here. Production storage must retain
unacknowledged writes and original verification evidence within explicit bounds.

## Receipts and handover

Local storage, laptop storage, and intended destination storage are distinct states.
A laptop stored receipt cannot clear a phone-authored write pending at an upstream
host/contact. Acknowledgement can be lost after successful receiver storage; retry
and reconciliation repair it without importing duplicate records.

For a group the fixture's intended destination is the official host: its receipt
means **host stored**, NEVER delivered to all members or read. For a DM it means
that particular modeled contact replica stored the record, not every device of the
person. No “all delivered” semantics follow.

Own-device receipt forwarding preserves the original issuer in the model. Before
production integration specify a scoped authenticated receipt representation or
explicitly document own-device-vouched receipt trust; do not invent host authority
or remove author proofs. Persist accepted data atomically before issuing a receipt.

Routing modes reuse the agreed behavior: Battery saver uses a reachable desktop;
if unavailable, direct sync; on return, partial reconciliation converges. Each step
has a bounded transfer size, so handover does not assume instant whole-history copy.
There are no measured failure deadlines, discovery guarantees or hysteresis rules.

## Tests

`python3 -B docs/archive/models/test_forwarding_queue_model.py`: **12 passed**, 0.315 s on
latest run; no ignored tests. Includes:

- Laptop acknowledgement distinct from upstream storage.
- Twelve-record notification overflow with single-record repair batches.
- Interruption after desktop storage but before acknowledgement, desktop restart,
  direct fallback and catch-up without changed author proof tokens.
- Receiver quota rejection, local admission failure and retry after quota increase.
- Persistence failure generates no storage receipt, followed by successful recovery.
- Twenty-four records from a hot group do not starve a second group's record with
  a one-record batch budget.
- Rejection of fabricated/substituted receipts and invalid records.
- One group's authorization cannot be inherited from another group.
- A slow/full phone does not stop independent desktop-to-contact reconciliation.
- Ten seeded schedules with twenty authored writes each, partial handovers/restarts,
  followed by bounded reconciliation: eventual exact-original convergence and receipts.

No real signatures, durable database, real networking, Android behavior or energy
claims are established. The slow-consumer case tests independent peer progress,
not fair scheduling of every document on a congested shared transport.

## Remaining gates

1. Extend stable-target round-robin record/receipt scheduling to byte budgets,
   topology churn, bounded receipt/dependency work and quota/backpressure responses.
2. Model network frame loss/reordering and unsolicited updates; cap memory before
   expensive verification rather than validating unbounded decoded input.
3. Audit actual group import/export validators and host receipts; write production
   acceptance tests before enabling group contents on own-device sync.
4. Define route handover readiness/timeout/backoff and acknowledgement retention
   across real process death, not merely in-memory object replacement.
