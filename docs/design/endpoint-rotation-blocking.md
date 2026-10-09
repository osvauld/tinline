# Endpoint rotation as blocking

Status: agreed direction; implementation not started.

Execution plan: [endpoint-rotation-implementation-plan.md](endpoint-rotation-implementation-plan.md).
That plan supersedes the initial implementation ordering below: simulate and test
signed endpoint succession first; group-chat integration is deferred.

## Product intent

Endpoints rotate automatically on a configurable interval, whether or not anyone
is blocked. Blocking excludes a contact from subsequent address updates; it does
not trigger rotation. At the next scheduled rotation, retire the old endpoint so
its address no longer lets the blocked contact call us. Keep DID-level rejection
as defense in depth: learning the new address must not bypass a block.

The account DID stays stable. Transport keys are replaceable. We retain allowed
contacts' addresses and initiate connections to them from our new endpoint,
proving the same account identity with a DID-signed device attestation and
transport proof of possession. Receivers authorize against their existing
relationship/grants, not merely knowledge of their address. Returning from a new
endpoint reuses that established identity relationship; no new contact invitation
or introduction is required.

No peer delegation chains, forwarding certificates, or Double Ratchet are
required for this direct address-update flow.

## Reachability scopes

- Public/group-facing endpoint: announce to authorized group nodes/members.
- Private endpoint: announce only to explicitly trusted contacts.
- Never disclose the private endpoint through a public DeviceList or handshake.
- Endpoint separation is not IP anonymity. Authorized recipients can leak an
  address, so identity-level blocks remain mandatory.

## Initial implementation plan

1. Audit endpoint lifecycle, persisted transport keys, attestations, contact
   removal/blocking, and DeviceList exchange.
2. Define signed, replay-resistant address updates, scoped by device and audience.
   Persist ordering state; account for multiple devices rather than replacing
   all devices when one endpoint rotates.
3. Add authenticated control-plane update delivery independent of placing a call
   or sending text. Connections also carry presence and ephemeral data: exchange
   signed address updates on those authenticated sessions. Verify the DID,
   endpoint-key binding, audience, authorization, and update ordering before
   replacing the stored endpoint; an unsigned presence hint is not sufficient.
4. Add a configurable periodic rotation schedule, persist its state, and retire
   the previous endpoint at rotation. Blocking persists the denied DID and excludes
   it from updates without triggering rotation. Define overdue rotation on resume,
   crash recovery, and handling of active calls before implementation.
5. Retry delivery to allowed offline contacts using their retained addresses.
6. Add public/private endpoint separation and audience-filtered announcements.
7. Test configured rotation intervals, rotation without blocks, exclusion of
   blocked contacts at the next rotation, identity continuity, spoofed/stale
   updates, block bypass, restart recovery, private-address leakage, and retries.

## Constraints to resolve

- Group hosts are rendezvous points for group-facing rotation: authenticate to
  the host from the new endpoint, submit an owner-signed versioned update, and
  have the host retain/distribute it to authorized members. Recipients verify
  the owner's signature, not merely the host's assertion. Private endpoint
  updates must not enter this group distribution path.
- Both private contacts rotating while unreachable still needs recovery;
  direct dialing alone cannot solve it. Group-host reachability after the
  host itself rotates also needs an explicit bootstrap/recovery rule.
- Decide retirement timing: retaining the old endpoint retains old reachability.
- Existing per-install device identity is also the EndpointId; audit device
  registry/linking assumptions before rotating that key.
- Specify how group authorization and private trust interact after a block.
