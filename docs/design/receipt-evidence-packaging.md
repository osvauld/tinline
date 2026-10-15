# Proof-aware receipt export packaging

Status: **implemented, targeted and expanded owned regression passed**. Internal storage/export API, not
negotiated wire transport or live call logging. Extends
[atomic receipt/evidence import](receipt-evidence-import.md).

## Sender behavior

`Ledger::export_with_evidence(theirs, limits)` in `receipts/store/bundles.rs` holds
one scope's ledger lock while reusing the existing whole-operation native planner.
It inspects only that planned native interval and gathers the exact original
protocol digests referenced by its call entries. Distinct originals are deduplicated
and fetched from sealed storage with existing canonical/hash checks.

If the native interval needs more than `MAX_IMPORT_EVIDENCE` (16) distinct proofs,
it **re-plans a smaller causal prefix**, halving the actual selected operation count.
It does not trim encoded updates, omit evidence, re-author operations, re-sign a
relay statement or advertise the larger frontier. Existing native byte/op/writer
bounds, causal closure and indivisible-oversize deferral remain in force.

The returned `EvidenceBatch` contains a native `ExportBatch` plus its supporting
originals. Its frontier remains advisory, never a receiver acknowledgement or route
readiness. Missing/corrupt required originals return an error, not an apparently
complete proof-less batch. Native byte budget and sidecar envelope/count budget
are separate; no combined negotiated frame format is assigned yet.

Repeated native planning, JSON inspection and retained-history reconstruction
remain history-dependent. This is not a constant-resource exporter or an aggregate
fairness/quota scheduler. Lowering the per-page proof budget trades throughput for
smaller packages; the current value is provisional, not device-capacity evidence.

## Additional receiver guard

Newly supplied proofs must not implicitly upgrade an older dangling body that shares
those originals. Import now identifies that affected known body even when a fresh
native update advances VV, and refuses the implicit recovery. This closes the
fresh-update variant of the existing duplicate-replay guard. Explicit recovery and
historical authorization policy remain separate work, not defaulted by this API.

## Four new tests and latest validation

- Six independent writers' joined observations require eighteen unique originals;
  the sender reduces a native batch to proof-complete prefixes within the sixteen
  proof budget. Strict receiver import converges to identical originals/views/VV.
- Missing or corrupt stored originals refuse export without source-frontier change.
- Empty source returns no batch; a one-byte indivisible native budget defers with
  the existing explicit oversize error, without returning a claimed frontier.
- A fresh native update cannot heal an older dangling body sharing its supplied
  originals. Neither fresh claim, originals nor VV progress is committed.

Queued targeted command:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-proof-aware-export -- cargo test --offline --locked \
  -p p2pcore --lib -- receipts::participation:: --test-threads=1 --nocapture
```

**18 passed, 0 failed, 0 ignored, 305 filtered out**, 10.04s, warning-free.
Includes the prior fourteen participation/import cases and these four new ones.
Admission/package-cache waiting preceded execution; no queue bypass.

The first expanded-regression attempt (`--who sync-proof-packaging-owned-regression`)
timed out after 300 seconds waiting for the Android build's compile slot. No owned
build/test process survived inspection. That attempt is **not a test result**;
the prior completed expanded regression was 141 passed before this slice.

After inspecting that no owned job survived and the compile slot became free,
the queued retry completed:

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-proof-packaging-owned-retry -- cargo test --offline --locked \
  -p p2pcore --lib -- receipts:: group::attestation:: group::doc:: \
  group::engine::durability:: group::own::contents::auth_tests:: \
  sync::engine::group_contents:: sync::engine::group_history:: \
  --test-threads=1 --nocapture
```

**145 passed, 0 failed, 0 ignored, 178 filtered out**, 90.53s, warning-free.

These are deterministic native/storage/component tests with synthetic call-role
context. They do not establish new authenticated transport, Node issuance, UI
integration, real membership policy or physical-device energy/capacity. No other
owner's lifecycle/UI files were edited.

Next available steps: source/admission controller and negotiated bounded package
transport, coordinated with the existing call-log
owner before shared live hook/API changes. See [roadmap](receipt-runtime-roadmap.md).
