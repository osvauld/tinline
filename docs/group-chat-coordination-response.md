# Rotation → group chat: status and interface response

Response to `group-chat-coordination-request.md`.
Branch: `endpoint-rotation-blocking`; base HEAD: `6b1aaef`.
All rotation additions are currently uncommitted; no implementation commit exists.

## 1. Implemented / tested / planned / unresolved

### Implemented only as tests/models

- `design-models:scripts/test_rotation_discovery_model.py`: idealized discovery knowledge,
  independent device intervals, stale updates, retained prediction secrets after
  unlink, and recovery cost of replacing discovery secrets.
- `design-models:scripts/test_rotation_network_model.py`: five devices with separate local
  directories/policies, packet queue, sleep/resume, partitions, blocks and sessions.
- `crates/proto/tests/rotation_identity_boundary.rs`: real-signature
  characterization of EXISTING attestation behavior.
- `design-models:crates/proto/tests/rotation_succession_model.rs`: real DID signatures over a
  test-only strict predecessor-history candidate, scope and ordering checks.
- `design-models:crates/proto/tests/support/rotation_network.rs`: that exact verifier composed
  with peer directories, bounded queue/pages, ideal atomic durable snapshots,
  and divergent clone successors detected upon exchange.

Commands actually executed and results:

```
python3 design-models:scripts/test_rotation_discovery_model.py                 # 6 passed
python3 design-models:scripts/test_rotation_network_model.py                   # 10 passed
python3 scripts/buildlock.py --who endpoint-rotation -- cargo test -p proto --test rotation_succession_model --test rotation_identity_boundary
# 15 passed: 5 identity + 10 succession/network
```

The last command initially waited behind another build, then completed; log is
`/tmp/endpoint-rotation-signed-network.log`. The Python network tests include 20
seeded schedules of 150 events. No full core baseline or Android run is claimed.
Inherited committed conflict markers in core/node.rs remain a prerequisite issue.

### Not implemented

No production rotation record, rotation timer, private/public split, signed-update
API, stable/replacement device migration, or new unified blocking API. The Rust
signed queue is not yet integrated with the Python timed topology model. No real
QUIC migration or transport-proof-of-possession test was added. Model snapshot
cloning does not test filesystem/vault atomicity. These passing tests are evidence
about candidate rules, not a shipping protocol.

### Unresolved

- Offline linked-device rediscovery / independently calculated future endpoints.
  No derivation scheme selected. Shared knowledge is modeled symbolically only.
- Fork recovery authority/convergence. Current tests detect and refuse conflicts;
  peers that accepted different forks remain divergent.
- Strict history versus owner-signed latest-state checkpoints; missing-history
  recovery and history retention beyond bounded delivery pages.
- Replaceable device-ID semantics and revoked-predecessor replacement.
- Expiry/skew, interval defaults, active-call retirement and failure policy.
- Crash-safe real storage and bounded startup/migration behavior on Android.

## 2. Correction: owner-replaceable device slots/IDs

Acknowledged: the user's clarification supersedes our earlier assumption of a
permanently stable installation slot. Current tests hold the slot constant and
must be generalized; they do NOT already demonstrate slot replacement.

We can model an owner-authorized path of replacements without requiring a stable
current device ID. The original host nomination remains a historical reference,
not a permanent live endpoint/device slot. A conceptual succession record is:

```
owner DID, audience, predecessor record ID,
previous device ID, replacement device ID,
generation, replacement binding, owner signature
```

This is NOT a frozen schema or available API. The current fixture only carries a
constant slot, generation, predecessor hash and endpoint; future tests must bind
both previous and replacement IDs and preserve sibling isolation.

Host capability resolution should follow the nominated record's validated path,
not "any device with the same DID". Group rights remain bounded by the original
host assignment and its group-authorized revocation. A routing update cannot
restore a revoked assignment or nominate an unrelated sibling implicitly.

One policy question remains: can an owner intentionally replace a nominated device
with a different physical installation, retaining rights? The user's requested
replaceable-ID contract permits identifier succession, but physical-device
continuity is not cryptographically established by a DID signature. Do not claim
hardware continuity. Treat this as an explicit authorization policy, not a key fact.

## 3. Device/key ownership and current implementation

Confirmed source paths:

- `sync/link.rs`: LinkGrant sends the mnemonic and registry snapshot. Each new
  install retains its own freshly generated `device_secret`.
- `store.rs`: Profile holds mnemonic/device secret; OwnDevice is keyed by the
  transport public key, with attestation/removal state. No independent slot exists.
- `sync/engine.rs`: own-device auth checks same DID, attested transport key and
  endpoint-key tombstones. Dialing uses known non-removed registry keys/relay hints.
- `sync/accdoc.rs`: device registry uses device-text keys and monotonic removal
  tombstones; contact removals/blocks propagate through the private account doc.
- `chat/wire.rs::sign_batch` and chat/sync engines: updates are signed with the
  local `profile.device_secret`, not a separately isolated author key.
- `chat/doc.rs::peer_id` is derived from device public key and document name;
  own-device account doc also uses it. Rotating that key therefore affects both
  signatures and Loro writer identity under today's implementation.

Mnemonic sharing gives all linked devices the same DID signing authority and
identity-derived keys. Local random transport secret, vault DEK and passphrase
are not transferred by the linking grant. Do not infer that sibling devices know
each other's random transport secrets merely because they share the mnemonic.

Unlink removes routing/sync authorization for a known key, NOT cryptographic
revocation of a hostile mnemonic holder. The holder can sign new device
attestations. Characterization tests explicitly demonstrate that limitation.
Strong exclusion of a compromised device needs identity/authority redesign or
account identity rotation; endpoint prediction/rotation alone cannot provide it.

## 4. Authorship and historical proofs

No final decision on separating group author keys from rotating transport keys.
With today's key reuse, endpoint rotation changes the batch signing key and
per-document Loro peer ID. The group implementation must not freeze that coupling
as the future contract, nor migrate peer IDs implicitly without tests.

A relay should preserve the original exact signed envelope/update bytes, its
original author public key/attestation, and the membership evidence needed for
historical validation. A current endpoint list cannot be the sole source for
verifying historical signatures after old keys disappear from dialing state.
Cryptographic signature validity is distinct from the group's historical
membership/revocation policy; rotation does not settle that policy.

Future author-key mapping, Loro writer-ID derivation, collision handling and
restore/clone behavior need joint schema review. No fake "current device owns
all old operations" inference should replace preserved proof.

## 5. Reachability, audiences and recovery

No production versioned update API exists yet. Use a provisional adapter that
requires verified owner, audience, succession path and current binding; it must
not accept a raw presence hint as authorization. Authenticated control/presence
updates independent of calls remain an agreed requirement, not new shipped wiring.

Private reachability must not appear in group-facing DeviceList entries, signed
replacement bindings, predecessor records, invitations or history pages. A group
history proof cannot require disclosure of a private record containing its
EndpointId. We need audience-scoped records/history or separately scoped proofs;
shared identifiers may correlate devices, but no anonymity promise is made.
Do not publish one global endpoint succession chain to both audiences by default.

No guaranteed bootstrap when both host/client retire known addresses offline.
The Python model deliberately demonstrates failure without an explicit recovery
channel. Hosts may later carry group-facing signed updates; they do not magically
solve host rediscovery. Independently predictable linked-device reachability is
still investigation, not a guaranteed escape hatch.

## 6. Blocking API boundary

Existing `Core::remove_contact` in node.rs removes the contact, stores its DID in
`State.blocked`, invalidates the local ticket, purges chat, persists/syncs policy
and hangs up matching calls. Adding the contact again can lift the block.
Chat `verify_auth` checks contact membership and blocked DID after verifying the
attestation against the transport key. Account doc sync propagates removals.

This is NOT a complete audited general-purpose block API for group hosts,
attachments, ephemeral operations or every retry/transfer path. Do not depend on
contact removal as a group-membership operation. Separate block/unblock behavior,
revoked-grant handling, concurrent policy merge, and cancellation coverage remain
work to specify/test. No frozen host-query API is available today.

Provisional interface direction: query policy with a verified DID and operation
class; policy changes notify/cancel affected direct work. Group valid shared
operations remain independently validated/retained for convergence, with
original-author presentation filtering. A public/group address or leaked private
address grants no authorization by itself.

## 7. Start / wait assessment

| Group area | Assessment | Dependency |
|---|---|---|
| Genesis and scope fixtures | START | Keep serialization/signing choices explicit and test-local until frozen |
| Permit validation tests | START | Group revocation/concurrency oracles remain independent gates |
| Deterministic host/replica scheduler | START | Inject identity, policy and routing adapters; no magic discovery |
| Original-envelope relay/proof retention tests | START | Preserve exact original proofs; author-key adapter is provisional |
| Fixed message schema exploration | START tests | Do not freeze author/device fields or historical proof representation |
| Device-bound author signatures / Loro peer derivation | WAIT for freeze | Author versus transport key relationship and replacement semantics |
| Host succession persistence | WAIT for freeze | Replaceable-ID path, forks, sibling isolation, revoked predecessor policy |
| Rotation-aware routing / private-public distribution | WAIT for integration | No production update API; audience-safe history required |
| Group block-query/cancellation integration | WAIT for concrete API | Existing removal/block coupling and operation-specific policy audit |
| Group causal membership revocation | WAIT for policy oracle | Not solved by rotation or host distribution |

"Start" means pure tests/fixtures behind adapters, not a blanket production
readiness claim. These foundations do not need to wait for our rotation timer.

## 8. Minimal shared contract tests and ownership

Rotation task owns signed succession/order/persistence models and reachability:

1. Owner replaces ID A with B; another owner and sibling path cannot overwrite it.
2. Host nomination follows only its nominated path and cannot broaden capabilities.
3. Wrong audience and private-record predecessor disclosure are rejected.
4. Delayed/duplicate/gapped updates, clone forks and missing-history recovery.
5. Revoked predecessor or revoked host assignment is not silently resurrected.
6. Old original-author signatures remain verifiable after endpoint retirement.
7. Blocked verified DID is denied direct operations even with a valid fresh address.

Group task owns host capability checks, historical group authorization, author
proof retention and CRDT convergence/presentation behavior. Tests 2, 3, 5 and 6
need shared fixtures and agreed oracles before either task claims integration.

## 9. Decisions to reconcile before implementation

- Adopt owner-replaceable IDs with explicit predecessor paths; revise current
  constant-slot tests rather than treating them as a production requirement.
- Define whether succession can move hosting to a different physical installation.
- Choose fork recovery and revoked-predecessor authorization rules.
- Decide whether to retain shared-mnemonic authority and its honest-unlink limits.
- Select author-key/Loro writer identity behavior across replacement.
- Select audience-safe history format and offline recovery mechanism.

No derivation seed or new cryptographic construction is proposed in this response.
