# Paged frontier reconciliation with shared message/file budgets

Status: **offline candidate model only**. No production routing/format change.
Source: `scripts/test_paged_sync_model.py`.

## Scope

Two explicit hops: group host ↔ personal sync device ↔ phone. All three retain
accepted original operations. This small model complements, rather than replaces,
the multi-user [host topology](host-topology-model.md) and
[host fault/budget](host-fault-budget-model.md) models.

The app adapter exports native Loro operations. This model does **not** run Loro:
operations are immutable `(document, original writer, counter, synthetic proof,
attachment reference)` fixtures. Only a writer's contiguous predecessor dependency
is modeled. No real signatures, account secrets, Loro cross-writer dependencies,
serialized range boundaries, encryption, storage transactions or power measurements
are exercised. All links are assumed authenticated/admitted; no membership security
policy is introduced by this model. Fixtures originate on the host; that does not
make the host an authority or cover every concurrent member-writing case.

## Exchange

- Sender advertises small sorted document/frontier pages. Source mutation revisions
  travel with pages. A changed revision during a pass restarts the scan, repairing
  insertions behind the cursor. The empty page ends an observed pass, not delivery.
- Page control traffic is rate limited and retried. No sender examines a destination's
  actual operation store to select a missing range.
- Receiver sends explicit `want(document, local frontier)` based on its own inventory
  and received advertisement. Sender responds with one whole symbolic operation
  beyond that frontier. Receiver validates the original and requires its predecessor
  before storing; missing dependency means no Ack.
- Acks advance a bounded advisory peer-frontier cache. Eviction can cause resend but
  must not determine durable delivery or erase pending data. The receiver's explicit
  WANT remains the progress cursor even when the sender evicts a document hint.
- Session generations scope pages, update requests, operations, Acks and blob frames.
  Queued callbacks from a retired generation cannot mutate the new session's state.
- Attachment requests use locally accepted original references and local verified
  byte prefixes. Source answers only from its available referenced prefix. Blob
  progress is independent of the evictable message-frontier cache.
- Incoming pages, requests, operations, file chunks and outgoing responses all share
  a per-device aggregate round-byte budget. Queue slots count unprocessed inbound
  packets as well as newly generated replies, preventing callback-induced overflow.
  Incoming callbacks cannot recursively deliver replies in the same round.
- Message/control scheduling precedes file requests, with rotating link direction
  and file selection. This shows finite fixture progress, not fair service under every
  sustained workload. File chunk retry is idempotent; a lost blob Ack can cause resend.

Byte sizes, five-round inventory pacing and 32-byte chunks are symbolic fixture
inputs, not production frame sizes, deadlines, bandwidth or quotas. Inventory
revision and page-cursor control are shared model protocol state; request/response
wire encoding and cursor authentication remain future work. The model sorts a source's
complete in-memory document list and retains discovered catalog entries; it does not
prove bounded archive-index CPU/RAM. The production storage foundation uses paged
`keys_after`; its full transport integration is unfinished.

`caught_up` checks an observed complete pass and advertised frontiers against local
storage. Its observations can become stale after a source write. It is deliberately
**not used to suppress direct routing** and is not a production readiness certificate.
Messages caught up does not establish attachment availability or host/member receipts.

## Findings from failing fixtures

1. Blindly pushing the first missing operation after a small peer-VV cache evicts an
   archive document can livelock: operation one is resent repeatedly, while operation
   two never arrives. Explicit receiver frontier WANTs repair the tested case.
2. File progress tied to the sender retaining the message's Ack frontier can stall
   after that frontier is evicted, even when the message was stored. File WANTs must
   carry their own progress derived from the receiver's original reference/prefix.
3. Continuous inventory/control traffic can consume a shared relay budget and starve
   the second hop. Pacing and rotating service are required; arbitrary retry timing
   can phase-lock with scheduling. Fixture pacing is not a selected production policy.
4. Mixing inbound iteration and outbound callback replies initially discarded Acks
   in the model. Detached incoming batches and reserved queue slots fix that bookkeeping
   error. These findings concern **model implementations/candidate designs**, not a
   claim that the same bugs have been reproduced in production.

## Owned validation

```sh
python3 -B scripts/test_paged_sync_model.py
```

**10 passed, 0 failed, 0 skipped**, approximately 0.4 seconds; tiny offline run,
no build/device session. `git diff --check` passed.

Coverage: twelve archive documents/two operations each crossing both hops with a
four-document advisory cache; insertion behind a cursor; lost page/Ack/WANT repair;
old-generation callbacks; missing predecessor/no Ack; disconnected-phone catch-up;
empty inventory pass vs actual storage; 8 KiB file plus new messages and quota recovery;
combined per-device byte and eight-packet bounds while new documents are appended.
All convergence checks assume faults/writes eventually stop and sufficient storage
becomes available. No global capacity or battery-saving claim follows.

## Remaining before implementation

- Real Loro range-export/import fixtures with multiple original writers, cross-writer
  causal dependencies, signed metadata ordering and indivisible oversized operations.
- Wire negotiation for pages/WANTs, bounded decoded inventories, source-scoped admission,
  dependency/verification work quotas and protected cursor/session generations.
- Couple this stale-inventory model to multi-user host connection/dial/work budgets.
  It currently budgets bytes/queues but does not model its own pool lifecycle.
- Define a freshness/readiness handshake across both hops, including upstream outages,
  restart, writes during catch-up and allowed direct bypass. Neither EOF nor inventory
  equality silently settles the policy.
- Compare lazy attachment WANTs against eager copying and sustained message/file load;
  include host cache quota, explicit file admission failure and corrupt range retries.

No database-file copying, phone install, commit or production route enablement occurs.
