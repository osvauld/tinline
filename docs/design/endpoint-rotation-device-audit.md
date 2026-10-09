# Rotation: linked-device audit and initial discovery model

Status: initial audit and executable knowledge model only; no production changes.

## Existing behavior verified in source

- `crates/core/src/sync/link.rs`: LinkGrant exports `me.profile.mnemonic` and
  registry entries. `create_linked_account` recovers the DID from that mnemonic,
  while retaining a fresh local `device_secret`.
- `crates/core/src/store.rs`: Profile stores mnemonic and local device secret;
  OwnDevice is identified by `device: [u8; 32]`, its transport public key. There
  is no separate stable installation slot in that registry structure yet.
- `crates/core/src/sync/engine.rs`: own-device handshake uses
  `accept_self_hello`, our DID, the transport key, and registry tombstones.
  Registry dialing uses non-removed device keys and retained relay hints.
- `crates/core/src/sync/accdoc.rs`: account registry is indexed by device text;
  removal tombstones merge monotonically. Contact removal/blocking is already
  coupled here, so separating those product actions is not a free assumption.
- `docs/design/device-linking.md` explicitly states mnemonic transfer is chosen
  and unlinking is not cryptographic revocation. A hostile mnemonic holder can
  create fresh DID-signed device identities. Endpoint rotation cannot fix that.
- Chat `verify_auth` already checks attested endpoint possession, grants, contact
  membership, and DID blocking. `note_device` is not a complete rotation protocol.

## Consequences

1. Add a stable device slot independently of EndpointId before treating rotation
   as a normal device update. Audit every registry/tombstone/signing-key consumer.
2. Never rotate the mnemonic as an incidental transport operation.
3. Shared deterministic private-key derivation could let siblings impersonate
   transport identities. Deriving public reachability is a distinct requirement;
   no safe concrete construction has been selected or implemented.
4. Any shared prediction secret retained by a removed device continues revealing
   future addresses until replaced. Public-key predictability alone also retains
   reachability knowledge even if it does not grant signing capability.
5. A fresh secret excluding the removed device also excludes an offline allowed
   device until that device receives fresh knowledge via recovery.
6. Strong compromised-device exclusion requires revisiting shared DID authority,
   not merely adding an endpoint schedule or replacing one discovery secret.

## Initial executable evidence

Run from rotation worktree:

```
python3 scripts/test_rotation_discovery_model.py
```

Result: 6 tests passed. No core build was run; inherited conflict markers remained (resolved 2026-10-10, see rotation-baseline-report.md)
in `crates/core/src/node.rs` and baseline repair is still pending.

The deterministic model checks discovery/update requirements: independent device
schedules, shared prediction across offline intervals, retained knowledge after
unlink, epoch refresh excluding both removed and disconnected allowed devices,
blocked update exclusion without timer changes, stale ordering and sibling
isolation. Opaque tuples represent addresses; idealized knowledge represents a
hypothetical secret. This is NOT a key-derivation implementation, signature test,
revocation proof, or real transport simulation. Perfect clock knowledge is assumed.

## Real-signature characterization tests

Added `crates/proto/tests/rotation_identity_boundary.rs` and ran:

```
python3 scripts/buildlock.py --who endpoint-rotation -- cargo test -p proto --test rotation_identity_boundary
```

Result: 5 passed, 0 failed. These use real DID signatures and the existing pure
`accept_self_hello` verifier; transport-authenticated keys are supplied as inputs,
not established via live QUIC. They confirm identity continuity, endpoint binding,
wrong-DID rejection, and two current limitations: old attestations remain valid
without a separate ordering policy, and tombstoning an endpoint does not prevent
a mnemonic holder from attesting another one. No full core baseline is claimed.

Proposed slot rule: retain a stable random installation identifier across routine
endpoint rotation. A new/replacement installation is a separate authorization
operation, not automatically equivalent to rotating the same device's address.
This is pending coordination with the group task's device-slot succession model.

## Next gates

- Agree on stable device slots with group-chat owner; keep slot replacement and
  ordinary endpoint rotation distinct.
- Decide whether the current shared-mnemonic trust model is retained. Do not
  promise cryptographic exclusion of a hostile linked device under that model.
- Decide whether offline deterministic discovery is worth its retained-knowledge
  trade-off; compare it with signed updates plus an explicit recovery channel.
- Then add real signed succession tests and state-machine crash/replay tests.
  Cryptographic construction and wire format remain unfrozen.
