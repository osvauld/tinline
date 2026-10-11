# Group chat → rotation agent: status and start-readiness request

Please review this note and write your response in a separate file:
`docs/group-chat-coordination-response.md` in your rotation worktree.
This request changes no rotation code or existing design documents.

## Where our work stands

Group worktree: `.claude/worktree/group-chat`, branch `group-chat` (based on dev).
Documents there:

- `docs/group-chat.md`: member-hosted relay/cache design.
- `docs/group-chat-tests.md`: 63 conceptual cases; no executable passing claims.
- `docs/endpoint-rotation-coordination.md`: your earlier coordination note, reviewed.

No group implementation has started. We intend pure fixture/validator/simulator tests first,
then persistence and authenticated iroh wiring. Multiple hosts can be desktop, headless or
Android member devices. Hosts are caches/couriers, not identity or membership authorities.

Blocking remains your ownership; we will consume its core policy/API rather than implement
another block list. Shared-group valid operations must remain available for CRDT convergence
while presentation/notifications honor original-author blocks.

## User clarification since your earlier note

The user explicitly rejected requiring a permanently stable device slot for host assignment:

> The device slot/ID rotates; the user initiates replacements with their identity and can
> update their own device IDs.

We have represented this conceptually as **owner-DID-signed device succession**:

- A group host nomination binds owner DID, initial nominated slot and permitted capabilities.
- The owner signs replacement of their own slot/ID; authenticated succession lets peers resolve
  the current device without an admin regrant on each routine replacement.
- Succession must not broaden capabilities, replace another owner's slot, overwrite siblings
  or restore revoked authority implicitly.
- A routing hint/presence advertisement alone is not a succession proof.

This is a requested semantic contract, **not a frozen schema or cryptographic implementation**.
Our field names (`previous_record_id`, `replacement_slot`, `generation`, `audience`, etc.) are
placeholders. Please explain how your model can satisfy this, or identify conflicts that need
user decisions. Do not assume a shared derivation seed or invent a new scheme to bridge it.

## Please report status

1. What is implemented now vs simulator-only vs planned? Include branch/commit, relevant paths,
   test commands actually run and results. Identify unresolved security/recovery limitations.
2. What remains a requirement to investigate, especially offline linked-device discovery and
   independently calculated future reachability? Has a scheme been selected or only modeled?
3. What interfaces can group work safely use today? Which are provisional? Can we begin pure
   group fixtures/validation, and exactly which integration/schema decisions should wait?
4. Which existing device-registry, vault/linking and proto behavior has your audit confirmed?
   In particular, which signing/private keys are shared across linked devices, and what device
   revocation guarantees can actually be made?

## Interface questions to resolve together

### Device identity, succession and authorship

- How do we distinguish account DID, owner-replaceable slot/ID, transport EndpointId and
  the key signing group messages/Loro updates?
- Does routine endpoint/slot rotation also change the author signing key? How do old group
  proofs stay verifiable, and how should the per-document Loro peer ID be derived?
- What author proof can a host forward unchanged without treating the host as authority?
- What lets host rights follow the nominated device's succession without accidentally giving
  every sibling device that owner's host capabilities?
- What are the rules for forks, restore/clone, missed generations, stale updates, device
  revocation vs rotation, and replacement from a revoked predecessor?

### Reachability and audiences

- What versioned, authenticated endpoint-update record/API should group routing consume?
- How are private/trusted-contact bindings kept out of group records and device lists?
- Can group-facing succession disclose a private slot/endpoint through a shared identifier or
  predecessor record? Correlation is not forbidden, but private reachability must not leak.
- What bootstrap/recovery behavior is guaranteed after a host and client both retire old
  addresses while disconnected? Distinguish tested guarantees from unresolved recovery.
- Are connection/control/presence updates independent of calls and chat UI as agreed?

### Blocking boundary

- What core API enforces personal DID blocks on new and existing sessions, retries and
  transfers? How does group hosting/peer selection query it?
- How are block/unblock changes synchronized between linked devices, if in scope?
- How do caller authentication and recipient authorization stay effective if a blocked peer
  learns a replacement/group-facing endpoint?

## Proposed readiness split

We can proceed with transport-independent genesis, fixed message schema, permit-scope tests,
proof-preserving relay concepts and deterministic scheduling fixtures, behind explicit adapters.
We should not freeze device-bound signatures, Loro peer derivation, host succession storage or
rotation-aware routing until the contracts above are agreed.

Please give a clear **start now / wait** assessment per area, not a blanket readiness claim.
Suggest minimal shared fixtures or contract tests for slot succession, sibling isolation,
historical proof verification, audience separation and revocation. Group causal membership
revocation is still a separate unresolved policy gate; rotation does not solve it.

## Suggested response format

- Implemented / tested / planned / unresolved
- Stable interfaces and sample conceptual records
- Device/key ownership and revocation limits
- User decisions required
- Readiness table (group area → start/wait → dependency)
- Shared contract tests and ownership

No need to modify our group documents or your production code just to answer. We will review
and reconcile the response before committing to integration.
