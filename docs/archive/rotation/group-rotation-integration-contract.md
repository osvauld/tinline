# Group/rotation integration constraints

Status: agreed boundaries, **not a frozen production API**. Rotation agent owns succession
verification/rotation models; group agent owns group authorization and proof-preserving sync.
See `endpoint-rotation-coordination.md`, the rotation worktree's
`docs/group-chat-coordination-response.md`, and `group-chat-tests.md`.

## Separate contracts

1. **Delegation** grants group authority from creator/admin to a holder. Scope narrows and
   delegation depth is capped. **Device succession** replaces the owner's device ID/binding;
   it is not another delegation link. Thousands of rotations must not consume courier's
   eight-link delegation budget. Succession still needs bounded verification/storage/work,
   with a separate validated checkpoint/history policy, not unbounded recursive nesting.
2. **Historical proof** preserves original keys, attestations and scoped succession evidence.
   Expired routing/succession records may be retained as evidence without becoming current
   routes or permissions. Historical validity and current access checks must be separate.
   Retaining old proofs must never make an expired access grant usable. Expiry/revocation
   oracles remain unresolved; current simulator deliberately has no expiry feature.
3. **Device nomination** is not a DID-level Host capability. Hosting requires the nominated
   device's verified succession path AND a still-authorized group assignment. Siblings do not
   inherit it merely by sharing the DID. Assignment revocation must not be undone by rotation.
   Physical hardware continuity is not proven by an owner DID signature.
4. **Author identity** is provisional. Existing transport keys sign chat/sync and determine
   Loro peer IDs. Do not freeze their coupling or an implicit migration. Preserve exact original
   envelopes, old verification keys and historical group authority; new endpoint discovery is
   not evidence of ownership of old operations.
5. **Mnemonic sharing** gives linked devices account signing authority. Unlink excludes a
   known key operationally but cannot cryptographically revoke a hostile retained mnemonic.
   Neither group adapters nor endpoint rotation may promise otherwise.
6. **Audience separation** applies to entire proof paths: group-facing bindings, predecessors,
   inventories and history must not reveal private endpoints. One global succession chain
   reused across audiences is not the default. Retained group history must verify without
   fetching/disclosing private predecessor records.
7. **Discovery** is separate from authorization. Permits and proofs cannot locate an unreachable
   host. Offline mutual rotation and future-address prediction remain unresolved; the simulator
   must not silently assume a magic current-address oracle.

## Current simulator boundary

`PostAuthorization` is a test-only injected adapter. Signed group fixtures check group permit
chains; legacy scheduler fixtures use explicit allowlists. Neither validates device succession,
current access expiry or hosting nomination. `Capability::Host` is only a DID-level scope fixture,
not an authorization check for a nominated device. Replica objects named hosts model relay/cache
behavior, not permission to become a host.

Routing, author-key mapping and hosting nomination will use separate adapters once the rotation
contracts are settled. Do not add rotation history to `Grant.parent` or accept DID equality as
proof of nominated-device continuity. No production schema/wire/storage commitment follows
from the simulator's structs.

## Shared acceptance cases (pending integration)

| Case | Expected invariant | Owners |
|---|---|---|
| More than eight rotations | unchanged delegation depth/capabilities; succession processing bounded independently | rotation + group |
| Sibling asks to host | denied without explicit nomination or authorized succession from nominated path | group, rotation fixture |
| Old proof after retirement/expiry | historical signature still verifiable; no current route/access granted by retention | rotation + group |
| Revoked predecessor/assignment | no silent resurrection; exact replacement/recovery policy must be frozen first | rotation + group |
| Succession keeps hosting | original capability bounds preserved; no host-call or admin widening | group, rotation fixture |
| Audience-safe history | full group proof verifies with no private endpoint/predecessor disclosure | rotation + group |
| Transport/author transition | unchanged original proofs remain valid; writer-ID/clone rules explicitly tested | rotation + group |
| Offline mutual retirement | unresolved discovery reported as unreachable, not proof failure or guaranteed recovery | rotation |

These are specifications, not passing tests. Require shared fixtures and frozen policy oracles
before claiming integration. Core blocking/cancellation remains the rotation/blocking agent's
API boundary; group removal/revocation remains a separate group policy gate.
