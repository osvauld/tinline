# Endpoint rotation — coordination with group chat

Status: design/test planning, not implemented. This note is from the endpoint
rotation task; it does not replace the group-chat design or change its code.

Rotation worktree: `.claude/worktree/endpoint-rotation-blocking`
Branch: `endpoint-rotation-blocking`
Detailed plan there: `docs/design/endpoint-rotation-implementation-plan.md`

## Agreed direction

- Stable account DID, several linked devices, independently replaceable iroh
  transport EndpointIds. Existing linking establishes the account relationship.
- Endpoints rotate automatically on a configurable interval, with or without
  blocks. Blocking does not trigger rotation.
- Personal blocks apply to verified DIDs in core immediately. Blocked contacts
  are excluded from subsequent private endpoint updates. Scheduled retirement
  makes their previously known endpoint unusable; leaked replacement addresses
  must still not bypass DID authorization.
- Retain allowed peers' addresses and connect from the replacement endpoint.
  DID-signed attestations and transport possession prove identity continuity;
  no new invitation or contact introduction is needed.
- Connections carry control/presence/ephemeral data as well as text/audio.
  Signed, versioned endpoint updates belong on authenticated control sessions;
  no call or text message is required. Unsigned presence is not authorization.
- No anonymity claim. No new peer-delegation chain or Double Ratchet is required
  for address rotation. Group membership delegation remains a separate concern.

## Multi-device requirement — newly raised, not yet designed

Linked devices must continue finding and communicating with each other while
rotating independently, including after disconnected periods. The user has
requested investigation of whether linked devices can independently calculate
each other's future endpoint identities.

This is a REQUIREMENT TO INVESTIGATE, not an implemented key-derivation scheme.
Distinguish deriving public reachability from sharing/deriving transport private
keys. Do not assume a common seed is safe: a compromised or revoked device might
predict or impersonate sibling endpoints unless secrets and epochs are rotated
appropriately. Existing account-key sharing also constrains revocation guarantees.
Audit actual linking/vault/device-registry behavior before promising isolation.

The original rotation plan proposes independent random transport keys with
signed sequence records. That choice must be revisited against offline linked-
device discovery before freezing the protocol. Simulate both discovery and
revocation consequences; do not invent a cryptographic derivation ad hoc.

Required model: stable account DID -> stable device slot -> rotating endpoint
binding. Sequence/generation is per device and audience, not one account-wide
counter. Updating one device must not overwrite siblings. Device revocation is
not a personal DID block or group ban.

## Group integration boundary

First rotation milestone is a deterministic simulator, protocol tests, and direct
private control flow. It must not depend on group-chat implementation.

Later, group hosts can retain and distribute owner-signed, versioned group-facing
endpoint updates. Recipients verify the owner, scope, and ordering themselves;
a host is a courier/rendezvous point, not the identity authority. Host rotation
itself still needs a bootstrap/recovery rule.

Public/group-facing and private/trusted-contact reachability must stay separate.
Never put private endpoint bindings in group records or an unrestricted DeviceList.
A personally blocked shared-group member may still learn a group-facing address;
that does not grant direct access or change group membership.

## Interface issue found in group-chat.md

The current `HostAssignment` names `device_key`. If that is the rotating iroh
EndpointId, routine rotation could invalidate host assignments or require a new
admin grant every interval. Agree a stable device identifier plus authenticated
current endpoint binding before implementing this interface. Rotation must not
silently nominate a different device or confer hosting rights.

Invitations/bootstrap hints must also tolerate retired endpoints; signatures
prove identity but cannot locate an unreachable peer. Account for refresh and
fallback without treating stale routing hints as current authorization.

## Blocking alignment

Agree with the group task's separation of personal blocking, admin-authorized
membership changes, and device revocation. Enforce direct access policy in core,
including existing sessions/retries/transfers, not only UI filtering. Preserve
valid shared CRDT operations and causal dependencies while hiding blocked
original authors' messages/notifications. Host forwarding must not disguise the
original author. Do not implicitly change group membership on a personal block.

## Test-first coordination

Rotation tests first cover signed succession, stale/reordered updates, fork
handling, missed generations, crash recovery, configured timing, and three-peer
blocking. Add linked-device cases: independent schedules, offline rediscovery,
slot isolation, revoke-one-device, restore/clone, and no unintended sibling-key
exposure. Tests must distinguish guaranteed behavior from unresolved recovery.

Please coordinate stable device identifiers, device revocation, audience-scoped
updates, and private block synchronization before either task freezes wire or
storage schemas. No group-chat source files were modified by this note.
