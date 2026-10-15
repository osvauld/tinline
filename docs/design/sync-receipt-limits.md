# Sync and storage-receipt limits: single reference

Scope: the own-device group history / host-attestation work in
`feature/sync-connection-audit`. Includes nearby existing content/transport bounds
that constrain this work. **Not a complete inventory of every app/audio/video limit.**
Refactor integration is pending; source paths below refer to this audit worktree.

Code constants are the implementation source of truth. Update this reference in
the same change whenever one changes. KiB = 1,024 bytes; MiB = 1,048,576 bytes.
A limit's presence does not prove constant memory, adequate throughput or battery
savings. In particular, persisted-document reconstruction still scales with history.

## Classification

- **Fixed format/algorithm:** changing it requires a codec/algorithm/version change,
  not tuning a performance knob.
- **Existing content rule:** already validated for signed content; changing it can
  affect compatibility/acceptance and needs separate review.
- **Provisional budget/pacing:** conservative implementation choice, not a measured
  capacity, membership policy, retention window or readiness/failure certificate.

## Cryptographic receipt codec

All constants below live in [`group/attestation.rs`](../../crates/core/src/group/attestation.rs).

| Item | Value | Kind / reason |
|---|---:|---|
| Statement version | 1 | Fixed prototype schema, not a deployed host wire version |
| `SIGNATURE_BYTES` | 64 bytes | Fixed Ed25519 signature width |
| `BINCODE_VEC_LENGTH_BYTES` | 8 bytes | Fixed-integer bincode Vec length prefix |
| `MAX_BODY` | 128 KiB | Provisional signed-body allocation/decoding budget |
| `MAX_SIGNED` | `MAX_BODY + 2 × 8 + 64` = 131,152 bytes | Derived EXACT envelope ceiling: body Vec and signature Vec; no padding guess |
| `MAX_WRITERS` | 4,096 per frontier | Provisional serialization/verification budget; applies separately to stored and metadata frontiers |
| Host DID length | 200 bytes | Codec shape bound, consistent with group record identity fields |
| Group ID | 64 lowercase hex characters | Canonical existing group identifier encoding |
| Frontiers | positive, strictly writer-sorted, no duplicate writers | Canonical encoding rule, not a tunable budget |

The signed envelope ceiling is not the size of the enclosing self/3 message. Its
request, group, cursor, document and list framing add bytes under the outer frame
ceiling. A valid signature proves the host's statement, not an independent disk audit.

## Shared recipient primitives (unfinished prototype)

These are code budgets in the new `receipts.rs` / `receipts/doc.rs` prototype,
not deployed member/chat limits. The primitive codec/native Loro tests and the
expanded owned regression pass; see [validation](shared-recipient-receipt-slice.md).

| Item | Value | Defining location | Kind / behavior |
|---|---:|---|---|
| `MAX_MESSAGES` | 256 exact digests/claim | `receipts.rs` | Provisional signature coalescing budget; sorted distinct hashes, no inferred read holes |
| Message/receipt digest | 32 bytes | same | BLAKE3 with distinct domains and length prefixes; actual original DM/group version extraction still pending |
| `MAX_CONTEXT_WRITERS` | 1,024 | same | Provisional group metadata-context budget; direct claims require empty context |
| `MAX_BODY` | 32 KiB | same | Provisional encoded claim ceiling |
| `SIGNATURE_BYTES` / `VEC_LENGTH_BYTES` | 64 / 8 bytes | same | Ed25519 / fixed-integer bincode widths |
| `MAX_SIGNED` | 32 KiB + 80 = 32,848 bytes | same | Derived exact signed envelope ceiling |
| `MAX_UPDATE_BYTES` | exporter `MAX_EXPORT_BYTES` = 12 MiB | same | Shared native import ceiling; not total document retention |
| Export call budgets | caller-selected under existing exporter ceilings | `receipts/doc.rs` -> `group/doc/batching.rs` | Reuses whole-operation planner; no constant-resource promise |

Caller admission is mandatory for native mutations but has no built-in runtime
membership/read/coverage policy yet. Signatures alone do not advance UI. Snapshot
reconstruction rechecks history/signatures and admits the reconstructed set; shallow
snapshots are refused. Large archives need a tested rollover/retention strategy.
Typed call claims are now an undeployed prototype; limits live in
`receipts/calls.rs` and remain separate from participant/session policy:

| Item | Value | Constant / behavior |
|---|---:|---|
| Call ID byte cap | 128 | `MAX_CALL_ID_BYTES`, provisional; actual native protocol/session ID checked by caller admission |
| Supporting proof references/event | 16 digests | `MAX_CALL_EVIDENCE`, provisional per-event allocation budget, NOT roster size |
| Observation time | u64 milliseconds, convertible to i64 and matching scope UTC day | Signed wall-clock observation, NOT trusted ordering/freshness |
| Local active segment | optional u64 milliseconds | End/interruption only; NOT global call lifetime |
| Group host generation | positive u32; absent for direct calls | Payload scope rule, not host authorization |
| Original receipt pointer index | 12 plaintext bytes (u64 writer + i32 counter) | `receipts/store.rs::Position`; vault AEAD/path overhead is additional |
| Draft original protocol payload cap | 2 × `proto::gcall::MAX_CONTROL` = 8,192 bytes | `receipts/evidence.rs::Evidence::from_blob`; provisional storage codec budget, NOT a larger control-frame allowance |
| Protocol signature text | 86 bytes | `receipts/evidence.rs::SIG_TEXT_BYTES`; fixed unpadded base64url Ed25519 encoding |
| Draft original protocol evidence envelope | 8,294 bytes | `receipts/evidence.rs::MAX_PROTOCOL_EVIDENCE_BYTES`; exact maximum under the provisional payload cap: 8,192 + two 8-byte String prefixes + 86 |
| Draft joined proof set | Exactly 3 distinct originals | `receipts/participation.rs`; CallStart + expected outbound Join + host Roster; internal bridge requirement, NOT negotiated wire schema |
| Remote evidence import budget | 16 supplied originals AND 16 distinct resolved references | `receipts/store.rs::MAX_IMPORT_EVIDENCE = calls::MAX_CALL_EVIDENCE`; provisional per-import bound, not total retention; `store/bundles.rs::export_with_evidence` re-plans smaller native prefixes to honor it |
| Remote evidence payload total | At most 16 × 8,294 = 132,704 bytes | Derived envelope payload cap; list/digest/transport framing and native update bytes are additional |

`receipts/store.rs` appends native update batches, sequence/VV state and tiny first
position indexes atomically using an existing sealed backing. One factory owns
that DB namespace, enforcing one live ledger per scope. Reopen scans retained
history; no total byte quota, per-session evidence retention cap or rollover yet.
See [call/ledger validation and boundaries](call-receipt-ledger-slice.md).

## Receipt transfer / storage

| Item | Value | Defining location | Kind / behavior |
|---|---:|---|---|
| `PROOFS_PER_PAGE` | 1 | `group/attestation/persistence.rs` | Provisional byte/work/fairness budget; shared by store-page adapter and wire bounds |
| `MAX_PROOF` | `attestation::MAX_SIGNED` | `sync/engine/group_receipts.rs` | Derived alias, not a second independent cap |
| Outstanding receipt pull | 1 per self session | `group_receipts::Receipts::waiting` | Provisional session-state bound; separate from the history pull slot |
| `REQUEST_SPACING` | 25 ms | `sync/engine/group_receipts.rs` | Provisional minimum spacing between new requests |
| `RETRY` | 5 seconds | same | Provisional lost-reply retry; same request ID/cursor |
| `RESCAN` | 60 seconds | same | Provisional periodic inventory pass; local document changes also request a rescan |
| Request IDs | checked nonwrapping u64 | same, `serial` | Correlation rule; retired responses are ignored; IDs are session-local |
| Proof cursor/digest | 64 lowercase hex characters | `attestation::persistence::{digest,valid_digest}` | BLAKE3 over ORIGINAL signed bytes; exclusive lexicographic cursor |
| Stored original proofs | retained, content-addressed | `group/store/receipts.rs`, `proof/{group}/{digest}` | No retention/total disk quota implemented; grows with accepted unique attestations |

Cursor advancement means **inventory inspected**, not receipt accepted. Missing
metadata or current hosting authority does not produce a confirmation. Later
passes reconsider those rows. Finishing a page/pass is not a readiness certificate.
Local-change refreshes finish an active pass before restarting, rather than
repeatedly resetting a hot archive cursor.

A receipt page and a native-history batch can be outstanding on the same session.
These per-path bounds are **not an aggregate byte/work/connection scheduler**.
There is no automatic byte quota/eviction for persisted receipts yet; a future
quota must defer safely rather than silently erase audit evidence or user data.

## Own-device native history

| Item | Value | Defining location | Kind / behavior |
|---|---:|---|---|
| `PAGE_DAYS` | 64 days/page | `sync/engine/group_history.rs` | Provisional page budget |
| `PAGE_BYTES` | 128 KiB | same | Provisional day-name/frontier payload budget |
| `VV_BYTES` | 64 KiB | same | Provisional encoded frontier budget |
| Frontier writer count | 4,096 | same, `vv()` | Provisional decode/state budget |
| `BATCH_BYTES` | 256 KiB | same | Provisional actual encoded native update budget |
| Operations/batch | 64 | same, `BatchLimits` call | Provisional whole-operation work budget |
| New request spacing | 25 ms | same, `History::pump` | Provisional pacing |
| `RETRY` / `RESCAN` | 5 s / 60 s | same | Provisional retry / periodic rescan |
| Outstanding history pull | 1 per self session | same, `History::awaiting` | Provisional state bound |
| `HISTORY_PAGE_DAYS` / page bytes | 64 / 128 KiB | `group/own/contents.rs` | Persisted inventory adapter bounds; must stay consistent with wire budget |

Too-large operations/frontiers defer, not split encoded native bytes or certify
completion. Hash/day insertions behind a cursor require another pass. Page rotation
improves scope fairness; a huge individual day can still hold the history slot
across multiple batches. Sustained-load fairness is not proven.

## Native exporter and legacy content transfer

| Item | Value | Defining location | Kind |
|---|---:|---|---|
| `MAX_EXPORT_BYTES` | 12 MiB/call | `group/doc/batching.rs` | Provisional exporter ceiling; wire caller uses the smaller 256 KiB budget |
| `MAX_EXPORT_OPS` | 256/call | same | Provisional exporter ceiling; wire caller uses 64 |
| `MAX_EXPORT_WRITERS` | 4,096/call | same | Provisional planning budget |
| `MAX_BATCH` | 12 MiB | `group/own/contents.rs` | Existing contents adapter bound |
| `MAX_OWN_BATCH` | 12 MiB | `chat/own.rs` | Existing DM own-device bound |

Whole native operations retain original author/writer proofs. Bounds on output do
not imply constant exporter work: trial encoding and document/VV copies allocate
history-dependent data and may temporarily exceed a candidate's output budget.

## Catalogs, groups and outer transport

| Item | Value | Defining location | Kind / behavior |
|---|---:|---|---|
| `MAX_OWN_DOCS` | 4,096 | `chat/own.rs` | Existing own-message/catalog/advisory-cache budget, not total retained documents |
| Fixed Hello docs | up to 4 | `sync/engine.rs` | Account, calls, profile, profile cache |
| Capability slots | 3 | `sync/engine/group_contents.rs::catalog` | Reserve contents/history/receipt markers; data catalog uses `MAX_OWN_DOCS - 3` |
| Hello entries | at most `MAX_OWN_DOCS + 4` | `sync/engine.rs` | Existing legacy-compatible outer bound |
| `MAX_OWN_GROUPS` | 512 | `group/own.rs` | Existing own-token message and tracked-scope budget; NOT a newly imposed membership limit |
| `MAX_FRAME` | 16 MiB | `sync/frames.rs`; `group/wire.rs` | Existing bincode frame-body ceiling, excluding u32 frame-length prefix |
| `HELLO_DAYS` | 7 recent days | `group/wire.rs` | Existing initial inventory window, NOT a retention limit |
| `DAYS_PAGE` | 64 days | same | Existing direct-host day inventory budget |
| `MAX_SHARDS_CACHED` | 12 days/group | `group/shard.rs` | Existing LRU cache budget, NOT disk retention |

Receipt frames are sent only after all three exact empty-VV capability markers
negotiate. group/2 host receipt acquisition is still a separate unfinished slice;
old unsigned group Acks are never upgraded into invented signed receipts.

## Existing signed group content constraints

Defined in `group/record.rs`; these are **existing content validation rules**, not
new receipt tunables:

| Constant | Value | Applies to |
|---|---:|---|
| `MAX_RECORD` | 64 KiB | Whole stored signed record |
| `MAX_TEXT` | 20,000 bytes | Post text (`String::len`, not character count) |
| `MAX_NAME` | 100 bytes | Group/member display names |
| `MAX_SEEN` | 1,024 writers | Original record's signed metadata `seen` frontier |

Thus the receipt/history 4,096-writer budget does not override the separate
1,024-writer record-creation/acceptance rule. Scaling these together needs a
separate compatible frontier/content strategy.

## Existing call-history view limits (not signed evidence retention)

`store.rs::MAX_HISTORY` and `sync/callsdoc.rs::MAX_HISTORY` each currently keep
**500** call view records. `CallsDoc` merges JSON `CallRecord` views; it does not
carry portable original participant signatures. These are legacy UI/view budgets,
not permission to discard signed audit evidence. The future core/UI integration
contract is [Call receipts and history](call-receipt-core-ui-contract.md).

## What validation is still needed

Provisional values must be tested against many groups/writers, huge days, archive
growth, slow peers, lost replies/reconnects and actual device memory/energy. Safety
boundary tests do not select optimal timers or sustainable throughput. Network
and phone measurements remain separate from deterministic offline validation.

Related: [typed call/ledger slice](call-receipt-ledger-slice.md),
[shared recipient primitive slice](shared-recipient-receipt-slice.md),
[shared receipt/shard auditability](message-receipt-auditability.md),
[call receipt core/UI contract](call-receipt-core-ui-contract.md),
[attestation slice](host-storage-attestation-slice.md),
[history wire slice](own-history-wire-slice.md),
[bounded exporter](bounded-loro-export.md).
