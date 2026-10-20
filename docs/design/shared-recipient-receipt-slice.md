# Shared recipient receipts: codec and native Loro primitive slice

Status: implemented/tested primitives in the isolated audit worktree, **not
runtime recipient delivery/read receipts or call history integration**.

Follow-on: [typed call claims and transactional ledger](call-receipt-ledger-slice.md)
adds call payloads and low-level sealed append/index transactions. Latest owned
regression is **127 passed**. The material below records the initial message-only
primitive stage; it does not describe live receipt issuance or transport.

## Sources and reuse

- `crates/receipts-core/src/lib.rs` (re-exported as `p2pcore::receipts`): canonical prototype v1 statements for both
  direct/group scopes and delivered/read kinds. DID signature binds conversation,
  UTC day, kind, issuer DID/device, sorted exact original-message/version digests
  and group metadata context. Direct scope has empty group context. Signature
  checking does not confer contact/member/device authority or establish coverage.
- `receipts/doc.rs`: dedicated grow-only binary `receipts` map. Full signed receipt
  digest is the key. Native writer must match issuer DID/device. Staged imports
  reject foreign scope/key/writer, nonbinary/deletion/other-container operations,
  missing predecessors and shallow snapshots without mutating the original.
- `signed_map.rs`: shared native binary-map operation inspection, also used by
  existing group message documents. Group record acceptance rules are unchanged.
- `group/doc/batching.rs`: existing whole-operation planner now has a native-doc
  entry point reused by receipts; no second causal planner, byte slicing or
  re-insertion/resigning at relay hops. Output bounds are not constant-work bounds.

Each signed batch names exact hashes, so a read receipt over A and C never marks B
read. Immutable bodies are not enlarged after signing. One batch can cover up to
256 originals; all codec/export budgets are indexed in
[the central limits reference](sync-receipt-limits.md).

`message_digest` hashes domain-separated length-delimited original signed bytes.
It is a low-level helper, **not an implemented canonical DM/group version extractor**.
Adapters must verify/canonicalize actual original content before using it. Group's
existing truncated record ID and DM's random message ID are not silently redefined.

## Explicit boundary: no runtime confirmation yet

`Receipt::sign_claim` signs a low-level claim. `ReceiptDoc::write` stages one local
operation. Neither checks a real message store/read event or persists an issued
acknowledgement. They must not be used as runtime delivery/read issuance until the
controller derives coverage from verified durable originals, freezes admission
context and persists before exposing/forwarding/indexing the result.

Native mutations/reconstruction require an explicit caller admission callback.
There is no built-in member/contact/device/removal policy; the callback cannot be
replaced by a signature-only acceptance rule. Already-accepted historical evidence
will need durable acceptance/provenance handling in the future store adapter.

Independent checkpoints preserve local writer identity. A future transactional
adapter must persist VV advancement even if a native update has no fresh receipt
IDs, and restore/reload on failure before returning a durable confirmation.
Snapshot tests are native byte reconstruction, **not receipt-store reopen/crash
validation**. No receipt namespaces/capabilities are allocated on the member/chat
wire by this slice. Its internal map name is not a negotiated document name.

The `#[allow(dead_code)]` module registration is intentional: no production caller,
UI projection, root summary, deletion/compaction or member/read network frames yet.
Host-storage proof persistence/own forwarding remains the separate existing path.

## Validation

Initial queued attempt timed out after 300 seconds waiting for shared admission.
Inspection found no surviving owned job; an admitted retry completed:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-recipient-loro-primitives-retry -- cargo test --offline --locked \
  -p p2pcore --lib -- receipts:: group::doc:: --test-threads=1 --nocapture
```

**39 passed, 0 failed, 0 ignored, 255 filtered out**, 42.88 seconds; warning-free.
The substring `receipts::` also selects the existing own-device group receipt
suite. Thirteen new cases comprise seven codec/hash tests and six actual native
Loro tests: canonical/domain/field binding, bounds/adversarial lengths, exact read
holes, two relay hops retaining original signatures/writers, policy rollback,
checkpoint restore, wrong structure/scope/writer, pending predecessor retry,
full-snapshot resume, concurrent issuers and indivisible-over-budget deferral.

Latest complete owned regression after the shared inspector/planner extraction:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-shared-receipt-owned-regression -- cargo test --offline --locked \
  -p p2pcore --lib -- receipts:: group::attestation:: group::doc:: \
  group::engine::durability:: group::own::contents::auth_tests:: \
  sync::engine::group_contents:: sync::engine::group_history:: \
  --test-threads=1 --nocapture
```

**116 passed, 0 failed, 0 ignored, 178 filtered out**, 169.66 seconds; warning-free.
Compared with the prior 96-case run this adds the thirteen recipient tests and
seven existing group-doc acceptance tests. Shared admission/Cargo jobs=2 used;
other agents' jobs/artifacts were not modified. Runtime varies under contention;
these timings are not capacity/energy measurements. No phone/UI/media changes.

## Next integration

1. Typed call/session receipt payload contract and ownership coordination, following
   [the call core/UI handoff](call-receipt-core-ui-contract.md); do not shoehorn call
   IDs into message hashes or reinterpret current JSON call history as signed proof.
2. Canonical persisted DM/group message-version extraction and read-event handling.
3. Explicit recipient/device/contact/member first-acceptance and historical policies;
   transactional store/provenance/indexing and failure/reopen tests.
4. Negotiated runtime exchange/status projection across groups, chats and authorized
   own replicas; only then shard summaries/rollover and retention decisions.

Related: [auditability design](message-receipt-auditability.md),
[host storage slice](host-storage-attestation-slice.md),
[limits](sync-receipt-limits.md).
