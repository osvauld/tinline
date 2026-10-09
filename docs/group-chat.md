# Group chat — member-hosted sync

Status: **design, not implemented**. This is the agreed direction for group chat v1;
1:1 chat remains unchanged. See [chat.md](chat.md) for the existing implementation.
Rotation integration constraints are tracked in
[group-rotation-integration-contract.md](group-rotation-integration-contract.md); succession
and delegation remain separate, provisional contracts.

## 1. Decisions and boundaries

- Pure P2P: no sovereign node and no service operated by us.
- A group may nominate several **member devices as hosts**: laptops/desktops, a standalone
  headless host program, or Android phones (including dedicated old phones). Hosting is a
  device capability, not a desktop-only role. It is opt-in locally and authorized by the group;
  it does not confer administration rights.
- Hosts cache, reconcile and relay. They are not authorities over group state.
- Every device, including every receiving client, validates original author proofs before
  importing. Loro handles merging valid operations; it does not authenticate or authorize them.
- Fixed Rust schemas and validation, not Lua, arbitrary manifests or uploaded code.
- Authenticated direct iroh sessions first. Direct member sync remains a fallback.
- No iroh-gossip in v1. It can later accelerate live delivery, never replace reconciliation.
- Group calls use one selected member host for media forwarding, independently of the set
  of chat hosts. Chat has no leader, primary host or host election requirement.

Offline delivery is conditional: a sender must reach a peer that stores its update, and that
peer (or another replica) must later overlap online with the recipient. Hosts improve that
availability; they cannot guarantee it. Removing a host cannot erase copies it already holds.

## 2. Kunki reuse map

Reference: `~/osvauld2/kunki` and `~/osvauld2/courier` (local source reviewed for this design).
Record a source revision before copying implementation; these paths are not dependencies.

| Reference | Reuse/adapt | Do not carry over |
|---|---|---|
| `kunki/src/bridge.rs`: `Gate`, `Envelope` | authenticated requests, replay bounds, dispatch vocabulary | UDS-only transport, shared-clock assumptions |
| `kunki/src/admin.rs`: `accept_sync` | scratch validation, persist before fan-out, read checks | host-rooted authority, flattened host-issued grants |
| `kunki/src/push.rs`: `Pusher`, `LiveRegistry` | transport-independent fan-out, bounded queues, catch-up on missed push | raw unverified snapshots as proof |
| `courier/src/sync.rs` | VV-based reconciliation and missing-dependency repair | `desktop_roll_back`, replacement with host state |
| `courier/src/token.rs`, `access.rs`, `role.rs` | signed delegation, scope narrowing, depth caps, grant cone | node-local issue/revocation records as global truth |
| Kunki generic app design | subscriptions, shard discovery, ephemeral relay | Lua rules, manifests, workspace/app routing |

Keep pure validation separate from storage/network I/O, as courier/kunki do. Do not import
all of kunki or recreate a generic app runtime. Existing courier scopes are workspace-oriented:
a group scope adapter or versioned token format is needed, not unchecked string substitution.

## 3. Group identity and documents

Genesis is DID-signed by the creator:

```
Genesis { protocol_version, creator_did, created_at_ms, random_nonce }
gid = BLAKE3(domain || canonical_genesis_bytes)
```

Verify signature and hash on admission; bind all permits, updates and invitations to `gid`.
The creator DID roots delegation; no hosting device becomes a new root.

Fixed document addresses:

- `group/{gid}/meta`: genesis reference, name/avatar, signed grants and revocations,
  membership events, nominated host devices, history policy.
- `group/{gid}/{YYYY-MM-DD}`: UTC-day message shards, reusing the DM record shape.
- `group/{gid}/user/{did}`: optional own-slot cursors/settings, not arbitrary shared writes.

Invitations carry the verified genesis, a recipient-bound membership chain and authenticated
bootstrap device hints. Endpoint/relay hints are routing hints, never identity proofs.
Meta is fetched and validated first; shard inventories then list recent days and older history.
A host's inventory is not exhaustive truth; reconcile with another host/member when possible.

## 4. Permissions and hosting

Use roles for people (`admin`, `member`; moderation is a later extension), and separate
capabilities/assignments for hosting. **Admins may create admins**, subject to delegation
rules. Default member rights are read/post and author-only edit/delete.

Host nomination names an owner and a device slot at an initial generation. The device
ID/slot is **replaceable by its owner**, not a permanently fixed endpoint key:

```
HostAssignment { gid, holder_did, initial_device_slot, grant_id, capabilities }
DeviceSuccession {
  owner_did, previous_record_id, previous_slot, replacement_slot,
  generation, audience, replacement_binding, owner_signature
}
capabilities: relay_chat, serve_history, serve_blobs, host_call
```

These are conceptual fields, not a frozen wire format. The owner signs device succession
with their stable DID identity; verify the replacement attestation and endpoint possession.
Peers resolve the current host through the authenticated succession from its nominated slot.
Routine device-ID replacement does not require an admin regrant. The owner can update their
own device IDs, not another person's, and cannot broaden the original hosting capabilities.

A generic address announcement is not sufficient succession proof. Bind predecessor, generation,
replacement and audience to the signature; reject rollback and cross-owner substitution.
Old records remain verifiable for history but do not authorize current sessions. Fork resolution,
revoked-slot replacement and lost-generation recovery must be specified before implementation.
Do not claim independent device-compromise isolation if linked devices share the DID signing key.

Local user consent and disk/network limits are also required. Chat hosting and call hosting
are independently enabled. Nomination cannot grant posting as another person, permission to
mint membership, or a bypass around validation. A normal member may still sync directly.

### Host runtimes

Use the same Rust host engine for the desktop app, an optional headless program and Android.
A dedicated headless install still needs an identity, DID-attested device and explicit group
admission/host assignment; it is not an anonymous sovereign node. Its setup/unlock and account
linking policy need to reuse the current device model rather than invent an authority shortcut.

A host does not have to be permanently online. A laptop sleeping or a phone being suspended
reduces availability; clients retry other replicas and reconcile on return. Advertise hosting
capabilities separately from transient reachability. Prefer a plugged-in, stable device with
sufficient storage, but never treat its advertisement as proof of persistence.

Android hosting requires an explicit user-enabled mode, visible foreground-service notification,
compatible service classification/permissions, power-management guidance and bounded background
work. Test screen-off, doze, network changes, process death and reboot on physical devices;
existing call-service behavior does not prove continuous group-host availability. Old phones
are candidates only if they can run a supported build with adequate storage and security updates.
Use charging/storage/network limits, and warn against continuous charging of damaged batteries.
Chat caching is much lighter than call media forwarding: accepting a chat host does not imply
it can host calls or sustain their uplink/CPU requirements.

Every verifier checks the chain root, recipient, group scope, delegation rights, grant cone,
chain-depth bound and revocation state. Revoking a parent invalidates descendants under the
agreed causal policy. Host announcements must be signed and reference a valid assignment;
load, reachability and available content are advisory claims.

## 5. Author proofs survive forwarding

A QUIC connection proves the **forwarder's device**, not the original author's identity.
A host must forward immutable original author envelopes; it cannot replace author signatures
with its own or sign an aggregate diff as if it authored all operations.

Proposed envelope (exact encoding requires a versioned wire spec before implementation):

```
GroupUpdate {
  version, gid, doc_id,
  author_did, device_attestation,
  permit_chain, meta_frontier,
  authored_at_ms, causal_dependencies,
  loro_update,
  author_device_signature
}
```

The signature binds a domain separator and all semantic fields, including group/doc,
meta frontier, dependencies, permit identity and exact update bytes. Length-prefix/encode
unambiguously; never sign ambiguous concatenations. Envelope ID is a hash of canonical signed
content. Per-message signatures are retained as a second authorship check, not a substitute
for operation authorization.

Bind the Loro peer ID to the author's attested device and document. Each author envelope
contains only that author's operations. Peers retain original envelopes and an operation-range
index so missing updates can be served without losing proofs. Filtering/re-exporting an old
signed batch invalidates its signature: resend the original envelope, tolerate overlap, and
use operation ranges/VVs to find the needed originals.

Current DM history imports use a two-party vouched-snapshot path. **Do not reuse that path
unchanged for groups**: message signatures alone do not prove membership changes, operation
ownership, deletion rules or authorization of an entire snapshot. V1 history retains enough
original proofs to replay/validate from genesis. Proof-preserving compaction is a separate
security design; a host snapshot alone is never a trusted checkpoint.

## 6. Validation and causal policy

Apply the same pure validator on sender, host and recipient:

1. Bound and decode the frame, addresses, chains, dependencies and Loro payload.
2. Verify genesis, device attestation, original signature and author-to-operation binding.
3. Resolve the validated meta frontier and causal prerequisites; if missing, **defer and
   request dependencies**, do not import as valid or permanently reject merely for arrival order.
4. Check group-scoped authority at that frontier, then fixed field/ownership rules.
5. Validate in a scratch replica before atomically persisting accepted proofs/state/indexes.
6. Acknowledge persistence, then fan out. Never forward known-invalid updates.

Unknown/invalid fields, cross-group replay, impersonation, immutable-field changes and writes
outside the granted scope fail closed. Deterministic rules must operate on causal state,
not whichever aggregate state or local wall clock happens to be available on arrival.
Concurrent legitimate edits need order-independent validation as well as CRDT merging.

A host refusal is not authoritative: do not roll back valid local history to its VV. Distinguish
`Invalid`, `NeedDependencies`, `UnauthorizedSession`, `QuotaExceeded` and retryable transport
errors. Locally invalid writes must not enter the accepted replica in the first place.

### Removal: security gate before implementation

Intended baseline: a removal in a write's causal past blocks the write; genuinely concurrent
writes remain valid. **A signer-selected old meta frontier does not prove concurrency.** A
malicious removed member can keep signing against stale history. Neither signatures, permits
nor Loro alone prove that a newly created write predates removal.

Before release, specify and test:

- how message dependencies bind to meta knowledge, and what stale-frontier writes mean;
- deterministic conflict resolution for concurrent grants, removals and admin changes;
- grant expiry semantics without arrival-time-dependent historical validation;
- partition behavior and the limits of distinguishing delayed writes from malicious backdating.

Do not claim immediate global revocation while partitioned. Host/session access and future
content distribution use the latest validated membership known locally; this is separate from
historical validity. Stronger cutoff guarantees may require signed epochs/activation rules,
with an explicit availability trade-off. No host becomes a clock or final adjudicator.

## 7. Member-host protocol and multi-host sync

Proposed separate ALPN `tinline/group/1` on the existing endpoint. Reuse DID/device attestation
verification, but not DM contact grants as group authority. A group invitation can authorize
peers who are not contacts. Authenticate both endpoints; bind requests to connection/session
and request IDs. Bound replay state; account for reconnect/restart rather than assuming shared
clocks as the local kunki bridge does.

Typed messages:

- `GroupHello`: group identity, membership proof, meta VV, available recent shards.
- `SyncRequest`: document, accepted VV, bounded missing-envelope/dependency requests.
- `Updates`: original signed envelopes (bounded batch).
- `StoredAck`: accepted persisted envelope IDs/VV, not merely enqueued bytes.
- `Subscribe` / `Unsubscribe`: authorized documents; push is best-effort.
- `HistoryList`: paginated shard inventory and verifiable history references.
- `Ephemeral`: sender-bound presence/typing, bounded TTL and per-device rate limits.

Endpoint rotation and DID blocking are handled by the separate identity/reachability work.
Use lightweight authenticated connections to distribute permitted address updates and presence;
no call is required. Host routing hints track verified endpoint generations. Host assignments
follow owner-DID-signed device-slot succession as specified above, without requiring an admin
grant for every rotation. An address announcement alone must not grant hosting rights. Private
bindings must not leak into group-facing succession records; audience-specific records need
an explicit continuity policy.

One connection can serve several authorized groups; authorization is per group/document,
not inherited from another subscription. Re-check read permissions on pull, subscription and
fan-out after membership changes. Bounded queues can drop pushes; reconciliation repairs gaps.

Phones normally maintain one preferred host session plus an optional second replica target.
Try other nominated hosts on failure, then known members. Hosts reconcile symmetrically with
other hosts using the same protocol: meta first, recent shards next, older history on demand.
Periodic anti-entropy and reconnect sync repair missed pushes. No primary, majority consensus
or host-certified state. Duplicate envelopes are idempotent; partitions converge once valid
proofs/dependencies are exchanged.

Deduplicate by envelope ID and accepted op ranges, not an untrusted sender VV. Advertising a
VV does not advance our acknowledgement of that sender. Verify IDs against persisted content.
A dishonest host can acknowledge and later discard data; cross-replication improves availability
but is not a cryptographic guarantee of future retention.

## 8. Storage, attachments and delivery indicators

Extend existing sealed chat storage with group metadata, accepted envelopes, dependency index,
shard VVs and replica receipt state. Hosts use their own local vault keys. Host policy bounds
retained days, bytes and transfer concurrency; eviction must not silently promise durability.

Attachments keep the existing random-key, chunked AEAD ciphertext format and iroh-blobs.
Keys travel inside authorized message envelopes over authenticated encrypted sessions.
Blob serving checks current requester authority, applicable history policy and an accepted
reference to the hash. Being a host or knowing a ciphertext hash is not sufficient. Group
hosts that are members can read messages and attachment keys; ciphertext storage does not make
an authorized host blind to content.

`history all` / `since_join` is validated against membership history, not a host claim. Returning
a whole snapshot must not leak restricted records or keys. Since-join export and dependencies
need explicit design/tests before enabling that policy; use `history all` for the first slice.
Previously authorized members may retain old content. No remote deletion guarantee.

Delivery states should distinguish:

- local accepted write;
- stored by another replica (host/member acknowledgement);
- delivered to particular recipients (their acknowledgements);
- read, only if separately implemented.

Do not label a host receipt as delivery to all members. Membership changes require a defined
recipient set for an “all delivered” indicator; defer that UI rather than invent semantics.

## 9. Group calls (separate implementation)

One call host forwards media; other chat hosts are candidates/backups, not simultaneous media
leaders. Signed call control binds call ID, group, participants, selected host device and a
host-generation identifier to avoid stale takeover signaling. Define who selects a replacement
and how split-brain calls recover before implementing automatic failover.

QUIC encryption is hop-by-hop through that host: without application-layer media encryption,
the host can access/alter media. Do not advertise participant-to-participant media secrecy
based only on signatures for control messages. Media encryption/key distribution and sender
binding require a separate call spec. This milestone does not implement group calls.

## 10. Threat model and limits

Protect against forged authors/permits, cross-group replay, unauthorized pulls, proof stripping,
malicious snapshots, slow consumers and unbounded resource use. Cap frame size, decoded op
counts, pending dependencies, sessions, subscriptions, chains, inventories and blob work before
expensive verification; rate-limit authenticated peers too. Sandbox/scratch imports are not a
substitute for limits on decompression/memory/CPU. Reject unsupported protocol versions.

A malicious authorized peer/host can withhold, delay, replay, lie about retention and leak
content it legitimately knows. Cryptographic validation cannot prevent these. Compare multiple
replicas for availability; do not silently accept “the host said so”. Network membership and
traffic metadata remain visible to peers and relay/discovery infrastructure.

Gossip, if later introduced, needs confidential group payloads, a defined key/epoch distribution
and rotation protocol, and limits against topic traffic abuse. Secret topics are not access
control. Gossip must not bypass the validator or read/forwarding policy.

## 11. Build stages and acceptance tests

The conceptual cases and observables are specified in [group-chat-tests.md](group-chat-tests.md).
Write executable tests before each stage; use `scripts/buildlock.py` for heavy builds. Preserve
existing DM/call behavior and run existing core regression tests. No conceptual case is a
passing executable test, and this document marks no stage done.

| Stage | Scope | Required acceptance |
|---|---|---|
| G1 | Pure group genesis/schema/permits/validator | tampered genesis/attestation/chain rejected; scope/depth/cascade tests; causal-policy gates below resolved |
| G2 | Direct sync between three members | independent concurrent posts converge; duplicate/out-of-order updates; third-party relay preserves author proofs; reconnect catch-up |
| G3 | Opt-in host engine; desktop first, headless/Android adapters follow | A sends while B offline; host persists, restarts, B catches up; quota/slow subscriber cannot stall others; host receipt distinct from recipient delivery; each runtime verified separately |
| G4 | Two hosts | H1/H2 partition with concurrent writes, later converge; either host killed; phones switch targets; stale host never rolls clients back |
| G5 | History/files and app UI | scroll older shards with proofs; interrupted ciphertext transfer resumes; unauthorized blob/history refused; nominate/disable hosts and surface delivery states |
| G6 | Group calls, separate spec | selected media host, authenticated control, defined failover and media confidentiality tests |

Security gates for G1/G2:

- Same valid input set delivered in different orders yields identical accepted operations/state.
- Unknown causal/meta dependencies remain pending within caps, then validate once available.
- Revoked author, delegated admin descendants, stale-frontier/backdated writes and concurrent
  removal have explicitly tested outcomes (no claim of a solution from a frontier alone).
- A malicious host cannot forge a third member, substitute its signature, inject raw snapshot
  state, change keys/attachments or force client replacement.
- Removing a host/member blocks future authorized serving/subscriptions at informed peers;
  offline/partition and old-copy limitations are demonstrated, not hidden.
- Invalid updates never enter accepted storage or fan-out; malformed/oversized data is bounded.

First implementation slice: **G1 only**, with a pure typed module and tests. Freeze canonical
signing format and causal validation policy before wiring transport. Reuse kunki patterns,
not its node-authority assumptions.
