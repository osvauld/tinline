# Own-device paged history / native batch wire slice

Status: implemented in the isolated audit worktree, with **71 targeted Rust tests
passing**, followed by a nine-test pass after adding final send-time admission
rechecks. No direct-route suppression, route manager or attachment transport.

## Negotiation / compatibility

Both `cap/group-contents/v1` and `cap/group-history/v1` must appear with empty VVs
in the authenticated self/3 Hello. The second marker alone does not negotiate.
Two catalog slots are reserved beneath the existing `MAX_OWN_DOCS + 4` Hello
ceiling. Existing message discriminants (bincode then, postcard varint tags now) are preserved: the new variants
are appended and never sent to peers without history negotiation. Content-only
peers retain the existing whole-diff/recent-document path; peers without either
marker retain legacy DM/group-token behavior. Midstream history renegotiation is
refused; a new connection has fresh request state.

New frames (`crates/core/src/sync/frames.rs`):
- `GroupHistoryGet`: group, exclusive day cursor, request ID.
- `GroupHistoryPage`: echoed request/group/cursor, bounded days/VVs, next cursor,
  explicit scope deferral.
- `GroupWant`: canonical group document, actual local accepted VV, request ID.
- `GroupBatch`: echoed request/document, native operation update, planned covered
  VV, device batch signature, more/deferral flags.
- `GroupHistoryChanged`: advisory rescan notification for an already known scope.

No new ALPN; no portable host receipt is invented from these frames. Header VVs
and progress flags are authenticated-link advice, not author proofs or read states.
Original signed map records remain the content authority.

Limit values and defining constants are indexed in the central
[Sync and storage-receipt limits](sync-receipt-limits.md) reference.

## Dispatcher / bounded state

`sync/engine/group_history.rs` is dispatched by the real own-device self loop.
Group tokens create only an empty known context through existing bootstrap code.
Each known offered group is pulled metadata-first, then ascending day inventory.
The receiver reads its actual local frontier for every WANT; sender replies via
`GroupCore::own_content_export_batch` from persisted sealed rows. Source-side
planned/sent VVs never advance `peer.docs`.

One outstanding pull per connection; one inventory page resident. Limits:
- 512 tracked known group scopes (existing token-message limit).
- 64 days / 128 KiB day/VV payload per page.
- 64 KiB / 4,096 writer entries per frontier.
- 256 KiB encoded native update / 64 operations per batch.
- Minimum 25 ms between new requests; retry the same outstanding request after
  five seconds; idle inventory rescan after sixty seconds, with a fixed deadline.

Scope scheduling is FIFO/deduplicated and rotates at completed page boundaries.
Paused scopes retain only their cursor/metadata phase, not extra inventory pages.
A huge individual day can still hold the pull slot across many batches: per-batch
scope leases and sustained-load fairness are **not yet proven**. These scheduling
constants are implementation limits, not a battery-saving/capacity measurement.
A legitimate over-budget frontier/operation defers its document/scope rather than
certifying completion or permanently deleting data. More than 512 groups or
4,096 writers needs a further scalable inventory/frontier strategy; these are not
new persistent membership limits or a claim that all such history was synchronized.

Incoming responses must match the current session-local request ID and scope;
retired IDs are ignored without import/Ack. Invalid cursors, order, frame bounds
and signatures are refused. Current account epoch and nonremoved-own-device
admission are checked on requests, responses and pumping, and rechecked after
planning/import immediately before replies/receipts. Unknown change hints
cannot bootstrap a group. Existing group policies and DID/device distinctions
are unchanged.

A nonempty batch must import and persist, including borrowed obligations, before
an empty signed `Sync` returns the actual accepted VV as **local cache receipt**.
Planned frontier coverage is checked against the resulting local VV. Persistence
failure ends the session without receipt; reconnect retries from persisted state.
There is no history readiness boolean exposed by this slice: an empty page ends
an observed pass, not a stable snapshot, host storage or phone/user delivery.
Insertions behind a cursor and deferred/stale inventory entries require another pass.

## Actual validation

Final owned command (queued with two build jobs):

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-history-wire-final -- cargo test --offline --locked -p p2pcore --lib -- \
  group::doc::batching:: group::engine::durability:: \
  group::own::contents::auth_tests:: sync::engine::group_contents:: \
  sync::engine::group_history:: --test-threads=1 --nocapture
```

**71 passed, 0 failed, 0 ignored, 185 filtered out**, 86.85 seconds. Previous
combined run also passed 71 (59.74 seconds). After adding final preflight admission
rechecks, an owned nine-test attempt waited behind another agent's workspace check
and hit the outer 300-second timeout during execution. Inspection confirmed no
surviving owned process; a separately admitted retry completed:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-history-preflight-confirmed -- cargo test --offline --locked -p p2pcore --lib \
  sync::engine::group_history:: -- --test-threads=1 --nocapture
```

**9 passed, 0 failed, 0 ignored, 247 filtered out**, 25.97 seconds. This is the latest
owned source validation; the complete 71-case result precedes only those extra
send-time gate calls. Owned module formatting and `git diff --check` pass.
No broad workspace suite or Android/device deployment.

Nine new tests:
- Two actual nodes restored from a synthetic shared account, separate device
  secrets, admitted registries and real localhost iroh self_run handshakes. The
  receiver starts without the group. Bootstraps verified metadata, synchronizes
  **70 cold days** across the 64-day page boundary; the first day has **70 original
  operations**, crossing the 64-operation batch boundary. Verify native VVs and
  exact original record/proof/position equality for that large day.
- Handler guard test on a real stream with manually seeded authenticated registry
  fixtures: unnegotiated request, invalid cursor, retired response ID, forged batch,
  append failure and removed own device. No cache Ack for stale/refused/unpersisted
  import. This test does not itself run pairing; the two-node test covers self auth.
- Both exact markers required; old/new bincode tags; bounded/malformed/negative VVs;
  notifications cannot create/unboundedly duplicate scopes; FIFO requeue ordering;
  fixed idle rescan deadline; nonwrapping request IDs/retry-ID preservation.

Early validation was deliberately not counted as success: a 130-cold-day fixture
hit its initial 60-second deadline under concurrent shared load. A diagnostic rerun
showed advancing history but the outer 180-second queue-command timeout cut the
job before completion. Process inspection confirmed no surviving owned job (the
queue holder note was stale). The final bounded fixture uses 70 days, retaining
both required boundaries, with a 90-second internal deadline. This is correctness
validation, **not throughput benchmarking** or a claim that the old timing failure
proved protocol convergence under sustained load.

## Additional owned validation

The fault fixture `sync/engine/group_history/faults.rs` now runs the actual receiver
self_run against an authenticated fault-injecting peer. It suppresses one WANT,
one computed inventory reply and processing of the returned cache Ack, then closes
the link and reconnects. Actual persisted frontiers prevent a duplicate day pull;
a valid obsolete response carrying an additional original operation is ignored in
the new session. Original operation/proof equality, empty host-Ack indexes and
retained durable day debt are checked. These are application-message faults, not
QUIC packet-loss injection or a receiver process-kill restart.

The latest warning-free combined owned run is **85 passed, 0 failed, 0 ignored,
185 filtered out**, 61.51 seconds, including this new fault test and thirteen
[storage-attestation crypto/store tests](host-storage-attestation-slice.md).
Receipt forwarding remains unimplemented; direct remains enabled.

## Remaining gates

- Further lost-request/lost-Ack/reconnect schedules; obsolete callbacks with other
  ID/scope timings; growth behind cursors during transport; multiple groups/huge days under
  sustained load; document dependencies from multiple original writers on the wire.
  Lower-level real-Loro tests cover writer dependencies but do not replace these.
- Efficient retained-history reconstruction, repeated candidate encoding, source
  work quotas and stream-buffer accounting. Payload bounds do not prove constant
  process RAM or a global connection/byte/work scheduler.
- Authenticated admitted-host storage-receipt forwarding to mobile. Own cache VVs
  are not external host/contact/member/read acknowledgements.
- Fresh per-scope upstream readiness and explicit failure/bypass policy before any
  exclusive laptop routing. Direct remains enabled; blobs, calls and endpoint pooling
  are unchanged. Shared group feature parity/headless distribution remain separate work.

Sources: `sync/engine.rs`, `sync/engine/group_history{.rs,/tests.rs,/guards.rs}`,
`sync/engine/group_contents.rs`, `sync/frames.rs`, `group/own/contents.rs`.
Related: [bounded native exporter](bounded-loro-export.md),
[own content adapter](own-group-content-slice.md),
[integration gates](laptop-forwarding-integration-gates.md).
