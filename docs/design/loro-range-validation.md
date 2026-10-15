# Native Loro range validation: acceptance fixtures

Status: **six real-Loro tests passed; targeted 46-test regression passed**. Production batching,
wire negotiation, routing and file support remain unchanged.
Source: `crates/core/src/group/engine/durability/loro_ranges.rs`.

The worktree's lockfile selects Loro **1.16.2**. Its native
`ExportMode::updates_in_range` takes `IdSpan(peer, start, end)` operation-ID ranges.
The test-only helper rebuilds a Loro export handle from a full native `GroupDoc`
snapshot, then exports the requested spans. It never inserts replacement records,
changes author/writer identities or signs an aggregate as host authority. This
helper is not a production byte-budget selection algorithm.

## Validated cases

1. Twenty-four original signed phone-writer records exported in three-operation
   native ranges, accepted on a sibling laptop, then forwarded from persisted rows
   to another own replica using that receiver's frontier. Compare exact record bytes,
   original DID/device, writer ID, counter and final persisted VV.
2. A later native range without its writer prefix fails without advancing durable
   state; prefix then retry converges; subsequent duplicate has no fresh change.
3. Two same-account device writers with real cross-writer Loro dependencies:
   A1 → B1 → A2. Out-of-order B1/A2 are rejected until both causal prefixes exist.
4. Inject append failure midway through ranged imports. In-memory/durable frontier
   must roll back; persisted exports cannot expose failed operations; retry preserves
   exact original records and writer positions.
5. Split one encoded native update into arbitrary byte halves: both must fail
   without a durable frontier; the intact operation-range export must succeed.
6. A valid signed range cannot be replayed into a different group.

These fixtures use the actual `GroupDoc` validation and `GroupCore` own-content
persistence/import/export adapter, not Python record oracles. They do not prove
pairing, real group transport admission, every fault timing, Android integration,
OS/power-loss durability, crypto isolation from a hostile shared-mnemonic device,
or that native range spans always fit a desired byte budget. A pending day import
can taint its cache; tests depend on existing core eviction/reload behavior, not
on Loro automatically rolling back an in-place import.

## Validation

Owned command (six tests passed):

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-loro-range-tests -- cargo test --offline --locked -p p2pcore --lib \
  group::engine::durability::loro_ranges:: -- --test-threads=1 --nocapture
```

The first admission attempt timed out before compilation after 180 seconds below
the queue's 4 GiB available-memory threshold. No owned waiter survived. The user
then explicitly authorized stopping the Android emulator, both Gradle daemons and
Kotlin daemon; Firefox was left untouched. All four exited and memory recovered.
The queue then admitted compilation (17.58 seconds) and this batch:
**6 passed, 0 failed, 0 ignored, 225 filtered out**, 10.90 seconds.

A separately admitted targeted regression batch also passed:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-loro-range-regression -- cargo test --offline --locked -p p2pcore --lib -- \
  group::engine::durability:: group::own::contents::auth_tests:: \
  sync::engine::group_contents:: --test-threads=1 --nocapture
```

**46 passed, 0 failed, 0 ignored, 185 filtered out**, 24.50 seconds. Includes the
six range fixtures, persisted forwarding/rollback, metadata obligations, archive
inventory, proof/admission guards and authenticated localhost self-reader checks.
Owned `rustfmt --check` and `git diff --check` passed.

Follow-up: a [byte-budgeted native selector](bounded-loro-export.md) and persisted
adapter are now implemented, with 16 new tests and a combined 62-test passing batch.
A subsequent [negotiated wire slice](own-history-wire-slice.md) now dispatches
receiver-frontier requests, pages and bounded batches, with a 71-test passing batch.
Host receipt forwarding and readiness remain next; no exclusive relay routing is enabled.
See [paged simulation findings](paged-sync-model.md).
