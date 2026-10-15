# Group-call joined evidence bridge: implementation and owner handoff

Status: **tested core-client/protocol/storage bridge**, not installed into live
Node callbacks, negotiated transport, UI or the call-log owner's backend.
This advances [roadmap M0](receipt-runtime-roadmap.md); M1 remains gated on owner
coordination and a real Node/controller integration test. The subsequent
[durable Joined controller](call-joined-controller.md) freezes retry identity and
commits intent completion with receipts; it is also not installed into live hooks.

## Implementation

- `receipts/evidence.rs` retains canonical exact SignedBlob envelopes, with a
  separate length-prefixed BLAKE3 evidence domain and bounded decoder. Envelope
  decoding is not protocol authorization; the participation bridge opens and
  verifies the actual group-call protocol statements separately.
- `receipts/participation.rs::persist_join` verifies original signed CallStart,
  this device's **expected outbound Join**, and host-signed Roster. Group, call ID,
  issuer and host generation must match. The roster must include that exact Join's
  DID/issued time/nonce/signature, not another linked device's Join for the account.
- Current CallAuthority checks cover the host permit, issuer/starter membership
  and every listed roster member. The caller must hold/freeze the appropriate
  source and authority context; this is not a new membership/removal policy.
- `Ledger::write_with_evidence` checks the exact distinct referenced originals and
  commits native update, sequence/VV, original-receipt pointer and evidence rows
  with one sealed-store transaction. Evidence uses `.../evidence/{digest}` beside
  the ledger's updates. Only successful commit changes live state/returns progress.
- Its local admission callback runs even for an identical receipt replay. The
  lower-level ReceiptDoc write optimizes duplicates away; bypassing the live
  device/session gate on that path was found and corrected in this bridge.
- `joined_views` is a proof-backed observation reader, **not another call-history
  database**. It checks stored exact originals and receipt correspondence. Missing,
  corrupt or substituted originals cannot yield a confirmed participation view.
  It does not erase accepted observations when present authority later changes.

The current helper records **Joined only**. It does not supply start/end/outbox
state, durations, global attendance, source epoch tracking or Node lifetime hooks.
A local participation receipt does not prove anybody heard media or that another
member entered the call. Caller-supplied observation time/context are not consensus.

## Concrete live-hook handoff (ownership agreement still required)

The prior `groups/call-log` lane controls `group/calls.rs`, `group/calllog.rs`,
store integration and UI. At controller inspection its checkout was clean and its
tip was included in our merged baseline; no live-hook/DTO handoff agreement was
recorded. No files in that owner's checkout were modified.
There is no agreed generated DTO or allocated receipt wire contract.

The current source loses originals on the route to UI-facing events:

1. `gcall/client.rs::join_frame` creates the exact outbound signed Join. Capture it
   as the expected Join for that local attempt, including redial/takeover changes.
2. `ClientEngine::on_roster` verifies a signed Roster and emits Joined; retain the
   accepted original at this point alongside the corresponding accepted start.
3. `gcall/net.rs` maps ClientAction to GcallEvent, but Joined/roster notifications
   do not currently carry all original signed data needed by the bridge.
4. `group/calls.rs::group_call_event` maps those into the owner's local log/events.
   Agree a core-only evidence path rather than putting large private originals in
   UI DTOs or asking UI state to attest that admission happened.

Required inputs/ordering for that path:

- Actual **local confirmed Joined transition**, not Join sent, an arbitrary roster
  fetch, UI tap or repeated RosterChanged notification. The current helper cannot
  independently witness the live transition; the integration must provide it.
- Frozen account/device/group epoch and local admission, verified metadata/native
  context, accepted start/generation, expected outbound Join, accepted Roster and
  locally observed time. No endpoint notes or media frames enter this ledger.
- One ReceiptStore factory per backing namespace, one live ledger per scope, and
  agreed lifecycle/lock ordering. Do not instantiate unrelated factories in callbacks.
- A bounded retry/outbox/controller path when persistence fails after the engine
  has changed state. Retry the same observation/originals rather than generate new
  timestamps/signatures from each retry. Do not lose the only Joined event.
- Post-commit provenance notification to the existing view. Legacy local call-log
  persistence remains distinguishable; existing rows cannot become signed history
  merely because a new bridge exists.

The helper deduplicates **identical bodies**, not every possible repeat of a
semantic transition at a different timestamp. The controller must define/stably
persist transition identity before retry/rejoin handling can be called complete.

The subsequent [atomic remote import slice](receipt-evidence-import.md) adds
strict native-operation/original-proof acceptance with explicit caller admission;
latest owned regression is 141 passing cases. Wire/Node integration remains absent.

Receiving/forwarding operations without supporting originals can reconstruct a
low-level receipt ledger; it cannot produce the stronger joined view. Future
negotiated sync needs atomic evidence acceptance/context deferral, not reference-only
projection. Supporting proof retention and rollover remain separate future work.

## Validation and limitations

Eight new cases:

- Real ClientEngine outbound Join -> accepted host Roster -> actual Joined action
  -> signed receipt/atomic originals -> participation view.
- Failed commit leaves no evidence, frontier or projected observation; retry works.
- Another genuine Join of the same account cannot stand for the expected attempt.
- Revoked synthetic roles, local-device refusal and forged original are rejected.
- Dangling references cannot produce a proof-backed view.
- Identical replay still invokes the current local gate and preserves history.
- Clean sealed-redb close/reopen preserves exact original/receipt bytes, frontier,
  view and idempotence.
- Missing/corrupt stored originals after reopen fail closed; duplicate issuance
  cannot silently repair them outside an explicit transaction/recovery policy.

Targeted queued command (CARGO_BUILD_JOBS=2):

```sh
python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-call-join-evidence-m0 -- cargo test --offline --locked \
  -p p2pcore --lib -- receipts:: --test-threads=1 --nocapture
```

**38 passed, 0 failed, 0 ignored, 275 filtered out**, 20.60s test execution,
warning-free. The filter includes six existing own-device host-proof sync cases.
Admission wait preceded compilation; no bypass or concurrent duplicate owned run.

Expanded queued command (CARGO_BUILD_JOBS=2):

```sh
python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-call-join-evidence-owned-regression -- cargo test --offline --locked \
  -p p2pcore --lib -- receipts:: group::attestation:: group::doc:: \
  group::engine::durability:: group::own::contents::auth_tests:: \
  sync::engine::group_contents:: sync::engine::group_history:: \
  --test-threads=1 --nocapture
```

**135 passed, 0 failed, 0 ignored, 178 filtered out**, 88.38s, warning-free.

Call-role/device metadata in new tests is synthetic. Real ClientEngine protocol
handling is exercised, but not an authenticated live group call or Node/UI pipeline.
The client initializes its audio sender; no media frames, WebRTC processing,
recording, phone session or device installation is exercised. Clean reopen is not
process-kill/power-loss proof. Earlier authenticated host-proof wire tests remain
separate from recipient/call receipt transport, which is not implemented.

Next: complete the owner agreement and controller/retry identity contract, then
wire/test M1. Independently continue nonconflicting source/evidence validation;
never silently claim live call history or route readiness from these primitive tests.

Related: [core/UI contract](call-receipt-core-ui-contract.md),
[ledger slice](call-receipt-ledger-slice.md), [limits](sync-receipt-limits.md).
