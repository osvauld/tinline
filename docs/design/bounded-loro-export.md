# Bounded native Loro exporter (group map documents)

Status: **implemented planning/storage adapter**, now consumed by the follow-up
[negotiated history wire slice](own-history-wire-slice.md).
Sixteen exporter tests passed; this slice's original targeted batch: **62 passed**.
The follow-up combined wire/export batch passed **71**.
This follows [native range validation](loro-range-validation.md) and the
[paged simulation](paged-sync-model.md), without changing routing or admission.

## Owned implementation

- `crates/core/src/group/doc/batching.rs`: `GroupDoc::export_batch`,
  `BatchLimits`, `ExportBatch`, `BatchError`.
- `crates/core/src/group/doc/batching/tests.rs`: thirteen actual-Loro tests.
- `crates/core/src/group/own/contents.rs`: new planning-only
  `GroupCore::own_content_export_batch`, sharing a persisted-doc loader with the
  unchanged legacy whole-diff exporter.
- `crates/core/src/group/engine/durability/bounded_exports.rs`: three real
  persisted-adapter tests; shares original-proof fixture helpers with `loro_ranges`.

Documents remain grow-only maps of record ID → signed binary record. Different
writers normally insert different keys; batching is about bounded transfer and
causal prerequisites, not last-writer-wins replacement or message deletion.

## Selection

1. Reject zero/excessive limits, tainted sources, negative receiver counters and
   writer-budget overflow. Project the peer VV onto source-known history. A peer
   may legitimately hold additional operations absent from this source.
2. Verify that this common prefix is causally closed using Loro's VV/frontier
   conversion. This is a structural check, **not proof of peer storage**: a peer
   can still lie about a well-formed frontier.
3. Maintain one missing-operation head per writer in a min-heap, ordered by native
   Lamport timestamp then writer ID. Prerequisites have earlier timestamps; check
   the chosen change's direct dependencies against the covered prefix as well.
4. Add exactly one whole map operation, coalesce selected contiguous spans per
   writer, and encode using `ExportMode::updates_in_range`. Offset Lamport timestamps
   within multi-operation changes are handled explicitly; a change/commit need not
   fit entirely in one batch.
5. Keep only candidates whose actual encoded payload fits the byte limit, stopping
   at the operation limit or first too-large candidate. Never split encoded bytes,
   re-insert map values, copy the database or sign a replacement aggregate.

Result includes the update, selected operation count, covered source-known VV and
`more` relative to the source VV captured at planning start. **The covered VV is not
the complete source VV, a receiver Ack, a host receipt or a readiness certificate.**
It omits receiver-only operations; after import, a receiver may have a larger actual
VV. Future requests must use the receiver's actual accepted frontier.

If even the first missing operation cannot fit, return explicit
`BatchError::Oversized { required, limit }` without changing either replica. Once
some operations fit, return that useful prefix instead. Selection is conservative,
not maximum packing: it does not skip a large operation to prefer a later independent
writer. Another scope must remain serviceable when one scope defers.

## Limits and resource caveats

Per-call ceilings: 12 MiB accepted output, 256 selected operations, 4,096 writer
entries in each input VV. Callers choose lower budgets (fixtures use 4 KiB/3 operations).
These are **planner admission limits, not group membership/retention limits**.
More writers require an explicit future scalable planning strategy; do not turn
this helper's deferral into a claim that a large group is caught up.

Candidate encoding is performed incrementally, potentially once per selected
operation, and can allocate one over-limit candidate before rejecting it. Group
record inserts are indivisible. Persisted document reconstruction, VV cloning,
causal closure and Loro encoding still have history-dependent CPU/RAM costs. This
is not a constant-memory streaming implementation and no sustained capacity or
Android energy claim follows. The native format's compressed size need not be
monotonic; stopping at the first too-large prefix is safe but not optimal packing.

The store adapter reloads only accepted sealed rows, never a possibly tainted live
cache. Persistence failure therefore cannot leak failed operation bytes or a higher
frontier to the next own replica. Existing group-scoping guards and self-device
admission remain separate. These unit/storage fixtures do not perform network
pairing or establish current membership/transport permission by themselves.

## Validation

First twelve planner cases passed; a second batch with grouped-change and persisted
adapter coverage passed all sixteen. The final combined owned regression was:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-bounded-loro-regression -- cargo test --offline --locked -p p2pcore --lib -- \
  group::doc::batching:: group::engine::durability:: \
  group::own::contents::auth_tests:: sync::engine::group_contents:: \
  --test-threads=1 --nocapture
```

**62 passed, 0 failed, 0 ignored, 185 filtered out**, 24.08 seconds. Compilation
and tests used shared queue admission with `CARGO_BUILD_JOBS=2`. Owned-file formatting
and `git diff --check` passed. Broad workspace, production multi-node range
transport, device tests and performance benchmarks were not run.

New coverage: eighty-record history under a small byte cap; operation count vs
partial advertised VV; one nine-insert native change split over five batches;
three deterministic concurrent/dependency schedules with three writer IDs;
cross-writer A1 → B1 → A2; duplicate import; snapshot/reload interrupted transfer;
indivisible oversize/exact-cap retry; impossible/negative VV; receiver-only history;
invalid limits/writer budget; tainted source; metadata maps; persisted two-hop
forwarding preserving every original proof/position; failed append, no downstream
advancement, then retry; document/group guards. Existing durability/admission and
authenticated localhost self-reader tests also passed in the combined batch.

## Next integration gates

1. Negotiate paged history/WANT support explicitly; retain legacy self-sync behavior.
2. Wire bounded document pages and receiver-frontier requests to this adapter. Never
   promote the planned frontier into advisory peer knowledge before a received Ack.
3. Bound queues, decoded request sizes and per-scope work. Check epoch/device and
   group-scoped admission for every dispatched request; isolate scoped deferrals.
4. Exercise actual authenticated multi-node range transfer with dropped requests,
   lost Acks/restarts, simultaneous writes and metadata/day dependencies. Day Loro
   causality and signed metadata-frontier visibility are different dependencies.
5. Forward authenticated host-storage receipts and establish fresh per-scope readiness
   before considering mobile direct-route suppression. EOF, `more == false`, local
   inventory agreement and own-device storage are not host/member delivery receipts.

No route policy, endpoint pooling, phone install, retention policy or authority
change is part of this slice. The subsequent [history wire slice](own-history-wire-slice.md)
implements negotiated pages/WANTs/native batches; host receipt/readiness gates remain.
