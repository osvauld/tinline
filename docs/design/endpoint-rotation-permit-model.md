# Owner-replaceable device IDs: succession-permit candidate

Status: test-only candidate, not a frozen protocol. No production changes.
Reference reviewed: osvauld2 courier token/policy/proof modules at
`90d22ecf00215fcff2ce52b2b5893ac7bd58aef2`. No courier code was copied.

## Pattern and distinction

`design-models:crates/proto/tests/rotation_permit_model.rs` uses domain-separated signed claims
and payload-hash IDs, following courier's permit pattern. Claims explicitly bind
owner DID, previous/replacement device IDs, replacement endpoint, predecessor,
generation, audience, version and validity. Device IDs change; the nomination
anchor is a historical reference, not a permanently fixed current device ID.

Succession history is NOT a nested delegation chain. A test follows twelve
rotations using pages of four, without courier's eight-link delegation limit.
Group delegation remains a separate authority check.

Candidate expiry rule: expired permits may advance verified historical knowledge;
only an unexpired current binding can authorize live use. Expiry never rolls back
the accepted generation. Current access additionally requires endpoint possession
and applicable policy. The test uses real endpoint signatures over a fixture
challenge; it is not a replay-safe live handshake implementation. No trustworthy
historical creation-time claim is inferred from a signed timestamp.

## Executed tests

```
python3 scripts/buildlock.py --who endpoint-rotation -- cargo test -p proto --test rotation_permit_model --test rotation_succession_model --test rotation_identity_boundary
```

Latest result: 33 passed: 18 permit-model/network/lifecycle tests, 10 existing
succession/network tests, 5 existing identity-boundary tests. Also re-ran Python
network/discovery models: 10 + 6 passed.

New tests cover owner replacement/endpoint possession, sibling-path isolation,
wrong signer, validity expiry versus history, more than eight successors, audience
substitution, tampering, forks, revoked immediate predecessor, unchanged state on
failure, page atomicity/idempotence, and host rights tied to the nominated path.
Host rights are a trusted nomination fixture, NOT verified group permits; a
revoked assignment remains denied even after successful device succession.

## Limitations and next tests

- Now composed with a dedicated direct-session queue in
  `design-models:crates/proto/tests/support/rotation_permit_network.rs`. Five added tests cover
  five-device address distribution, replacement-ID sibling collisions, wrong
  signatures, gaps/catch-up, duplicate/session replay, blocked announcement
  exclusion, expired routes, mutual retirement failure, and separate audience
  histories. The Python timer scheduler is still separate.
- Each peer has independent verified histories and learned routes. Delivery uses
  the sender's retained destination address, not a global current-address oracle.
  Expired bindings cannot be dialed through learned routes; expired history also
  invalidates bootstrap fallback instead of reviving an old route.
- Added seven timed/crash scenarios in `support/rotation_permit_lifecycle.rs`:
  independent intervals with no blocks, five sender crash boundaries, overdue
  resume retaining blocks without timer changes, receiver crash before/after
  accepting an update, offline catch-up through expired history, and per-receiver
  bounded challenge expiration/replay. The candidate overdue policy creates one
  actual successor on resume and schedules the next deadline one interval later;
  it does not fabricate a key for each missed timer tick.
- Group-history tests verify two group-facing successors with no private records
  and inspect exported claims for fixture private endpoints/record IDs. A signed
  group update spliced to a private predecessor is refused. This is fixture-level
  evidence, not a general information-flow proof or a production export API.
- Same current-generation forks are detected, not resolved. Older fork evidence
  after advancing, clone recovery and history pruning remain unresolved.
- Revocation set checks cover the anchor, current head and incoming permit.
  Revoking an older intermediate record after later successors are accepted needs
  an explicit cascade/recovery policy and retained ancestry evidence; not tested.
- No bounds on serialized bytes or full history storage, only page record count.
- Audience separation assumes correctly scoped initial anchors. The queue models
  one active endpoint per simulated device; simultaneous public/private endpoints,
  all inventories/export surfaces and malicious initial anchor selection are not
  covered. Challenges are now per-receiver (eight outstanding, five fake-clock
  units TTL), with a persisted allocation high-water mark and volatile outstanding
  set cleared on restart. No globally accumulated replay set remains. Durable
  allocation before challenge issuance is an ASSUMPTION, not a production design;
  an actual session-bound random challenge may avoid that per-request disk cost.
- No permit renewal-without-rotation policy, missed-history checkpoint scheme,
  cryptographic linked-device revocation, or offline endpoint prediction selected.
- Timer advancement is explicit monotonic fake time. No wall-clock rollback,
  interval reconfiguration, storage corruption/power-loss, author-key/Loro
  migration, OS scheduling or live iroh. Snapshot clones assume ideal atomic
  persistence. Tests exercise chosen policies; they do not freeze them.
- Unknown replacement ID collisions with sibling/current/historical IDs need a
  registry-level policy; path checking alone is not a complete registry.

Next: share candidate oracles with group-sim and settle renewal, fork recovery,
older ancestor revocation, interval reconfiguration and real time/persistence
contracts. The timed fixture retains full history and only sends histories fitting
one page; long-offline paging with live possession of the newest key still needs
an integrated test/API (historical page endpoint keys are not necessarily live).
Active-route authorization and stored historical evidence must stay separate in
any production implementation.
Do not freeze the schema or interpret candidate revocation semantics as consensus.
