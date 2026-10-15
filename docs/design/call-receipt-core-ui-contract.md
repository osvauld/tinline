# Call receipts and history: core / UI handoff

Status: **user-selected scope and proposed integration contract**, not an
implemented UI API/wire schema. [Typed call claims and transactional ledger](call-receipt-ledger-slice.md)
are now tested low-level primitives; runtime hooks/projections remain unfinished. Individual and group call logs belong to the shared
signed-receipt audit layer. Another session owns UI implementation; this audit
worktree owns shared receipt primitives/documentation. Do not overwrite UI or
parallel group-call engine work. Refactor integration remains deferred.

Long-horizon milestones, ownership handoff and validation gates are tracked in
[the runtime roadmap](receipt-runtime-roadmap.md). The latest supporting-evidence
bridge is [tested at core-client/storage level](call-join-evidence-bridge.md),
not a live Node hook or a new UI backend.

## Existing implementation versus target

In this checkout:

- `store::CallRecord` is an individual-call view with `call_id`, peer DID/name,
  direction, start time (unix seconds), duration, end reason, direct-path hint and
  missed flag. It has no original receipt signature or group subject.
- `store::History` / `sync::CallsDoc` retain/merge at most 500 view records. The
  `calls` Loro document stores JSON view rows and chooses a deterministic ranked
  account of each call, not separately verifiable participant evidence.
- Group-call protocol already signs statements such as CallStart, Join, Leave,
  Roster and End. Keep/reuse those original proofs where their exact semantics
  fit; do not replace them with a claim that an arbitrary cached row was signed.
- `gcall::net::GcallEvent` exposes Joined, RosterChanged, Reconnecting, Superseded,
  Ended, HostingStarted and HostingEnded. These are candidate core integration
  hooks, not a completed history persistence path.
- Shared `receipts` primitives now cover message delivered/read and typed call
  observations, with a low-level transactional sealed ledger. The latest expanded
  owned regression passes 127 cases; [implementation and validation](call-receipt-ledger-slice.md).
  Actual lifecycle issuance, original supporting-proof/admission checks, Node
  storage hooks, negotiated sync and UI projection remain unfinished. No new
  call-log backend is connected to the live call engine yet.

Target: one receipt-backed call-history model for individual conversations and
stable group IDs, with explicit legacy provenance. Do not put a group ID in a
`peer_did` field or duplicate existing call lifecycle/media engines.

## Evidence and scope

A call receipt binds the conversation/group, authentic call/session ID, issuer
DID and scoped installation/device ID, typed observed event and any exact
supporting original protocol-proof digests. Group host generation belongs in
applicable evidence so takeover/host migration does not become a second call.
The envelope must distinguish call events from delivered/read message receipts;
signing a naked session ID or reinterpreting one payload kind as another is unsafe.

Typed event semantics to implement:

| Claim | What it establishes | What it does NOT establish |
|---|---|---|
| Observed offer/start | Issuer observed that exact authentic offer/start | Everybody was notified or somebody answered |
| Local participation joined | Issuer reports its device entered the call after successful admission | Every roster entry actually joined/listened |
| Local participation ended | Issuer reports its device stopped participating, with its observed reason | The whole group call ended |
| Local participation interrupted | Issuer observed disconnect/unavailable continuation | Host/global end or a known remote duration |
| Observed host end | Issuer observed that exact verified host End; retain its original | Every recipient observed it |
| Not joined / missed / declined | Local account/device outcome for an observed offer, under explicit rules | Offline accounts were notified or cryptographic proof of human intent |

Host CallStart/End and participant observations have different authorities.
A signed Join request alone is not a successful-admission receipt. An authenticated
roster may describe what the host reports; participant-signed confirmations remain
separate. Preserve host/participant originals when presenting stronger provenance.

No call-media frames or endpoint addresses belong in call receipt payloads.
This records session events, not audio/video recordings or proof that media was
heard/seen. Group participant identities and read/history visibility must follow
conversation privacy policy, not implicit public-host discovery.

## Core lifecycle and durability requirements

1. Persist observed authentic offer/start context when first known. Do not wait
   until normal termination to create every trace of a call.
2. After successful local join, derive/sign an immutable typed receipt from the
   actual core lifecycle and supporting verified context, not a UI assertion.
3. Persist the receipt/original evidence and view projection atomically before
   notifying receipt-backed history or forwarding it. Failure cannot expose an
   unpersisted signed confirmation as completed/auditable history.
4. Preserve reconnect/takeover segments under the same subject/session ID. Sign
   only observed transitions; deduplicate retransmitted originals by their digest.
5. On normal leave/end, persist this device's outcome. An app restart during an
   active session yields an incomplete/interrupted local record until supported
   evidence resolves it; do not invent remote end times or successful completion.
6. Sync original receipt operations and required evidence, then rebuild the view.
   Acceptance checks issuer identity, device/conversation admission and supporting
   proof coverage. A personal relay forwards originals unchanged, not its own
   signature standing in for another participant.

Wall-clock times are signed observations, not consensus ordering or proof of
issuance before revocation. Local active duration should use observed participation
segments/monotonic measurement where available, not global host lifetime. The
exact payload schema, event sequence/predecessor representation, missing-event
handling and admission/removal policies need explicit tests before freezing them.

## Proposed UI projection contract — not generated bindings yet

The UI should consume a **derived view**, separate from immutable original
receipts. The following are requirements for the future core DTO, not existing
Rust/UniFFI field names:

- Subject: individual conversation identity OR group ID; optional display-name
  snapshot for presentation only.
- Stable call/session ID; type/media and direction where actually known.
- Local observed start, first successful join and last observed participation end,
  with unknown values represented as unknown, not fake zero timestamps.
- Local active duration/segments and current final/incomplete outcome.
- Group host generations/reconnect transitions where relevant; local participation
  state does not imply the group call's global state.
- Provenance: legacy local view, valid issuer-signed observation, valid host proof,
  participant confirmation, or pending context. UI labels must not promote one
  into another.
- Optional observed versus independently confirmed participant information, kept
  distinguishable and subject to visibility/admission rules.
- Stable paging/order keys and immutable evidence IDs for later audit/details.

Group history must be available in both the relevant group and the unified call
log. Aggregate this account's linked-device observations without double-counting
concurrent participation intervals or claiming every device joined. Different
participants' receipts are evidence rows, not duplicate call-log sessions.

No concrete API is promised by this document. Coordinate DTO/binding names with
the UI owner and the group-call core owner before modifying shared interfaces.
The UI session can implement presentation/state handling without pretending the
receipt-backed backend is already wired or tested.

## Loro/storage/sync reuse

Use typed immutable signed values in the shared receipt machinery, with exact
conversation/session scope and appropriate receipt documents/periods. Event days
can cross midnight; the history projection groups by session, not merely UTC day.
Do not let call receipts enter the message set that delivery/read receipts cover;
there must be no acknowledgements of acknowledgements.

Original signed protocol evidence referenced by digest must remain available to
verify claims; a dangling hash reference is not sufficient audit material.
Individual call evidence stays with participants and authorized personal replicas.
Authorized group hosts retain group evidence according to the selected visibility
policy; hosting is not permission to collect private individual-call history.

Separate view-cache trimming from evidence retention. The existing 500-call limit
is a legacy view budget, **not authorization to erase signed call evidence**.
Do not silently convert older JSON rows into independently verified participant
receipts. Optional newly signed account observations about legacy rows must be
explicitly marked as retrospective, not original event proof.

No negotiated call-receipt wire extension exists yet. Reuse bounded native export
and import, but require explicit capabilities; legacy peers retain their old view
semantics. Do not overload existing message day names or silently change ALPN.

## Acceptance tests and ownership boundaries

Core-owned tests must cover individual/group scope substitution, forged issuer,
wrong session/host generation, join-request-versus-confirmed-join distinction,
reconnect/takeover without duplicate calls, duplicate/out-of-order evidence,
partial observations, persistence failure, incomplete-call reopen, original-proof
forwarding, revoked devices and legacy view migration/negotiation.

UI-owned validation covers unified/group call-list presentation, stable paging,
unknown/incomplete/missed states, distinction between local and global completion,
provenance labels and no false participant/duration claims. Receipt signing and
role checks belong in core, not the UI.

No phone installation, media-engine change, UI edit, call-log migration or signed
call receipt runtime integration has been performed. Prototype payload fields are
not frozen/generated UI bindings; coordinate the actual handoff before integration.

Related: [shared auditability](message-receipt-auditability.md),
[limits](sync-receipt-limits.md), [protocol](../protocol.md),
[group core](../group-core.md).
