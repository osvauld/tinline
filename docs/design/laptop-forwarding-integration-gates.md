# Laptop forwarding: production integration gates

Status: source-backed task breakdown, not a frozen full hub API/wire spec. Models,
local transport tests, the durability fix and a negotiated recent-group-contents
application slice exist in this worktree. See `own-group-content-slice.md` for exact
scope/tests; battery-saver routing and pooling are not enabled. User has authorized
continued work in this worktree without per-checkpoint approval. No authorization
to commit, install, wipe data or resolve unrelated protocol decisions is implied.

## Existing machinery to reuse

- DM contents: `chat/own.rs::chat_own_apply` verifies original DID signatures and
  writer ownership. Sibling-authored outgoing records are tracked with `put_mo_op`
  and pending markers so another device can deliver them. Do not replace this
  validator with a generic trusted-laptop snapshot import.
- Group contents: `group/doc.rs::GroupDoc` validates exact signed record inserts,
  keys, original writer IDs and document placement. `GroupCore::apply` drives meta
  derivation, day-shard import and storage. Reuse these checks for own-device imports;
  never re-sign a phone's ops as the laptop's or invent a new author/writer ID.
- Own-device group list already exists in self/3. At the base it was metadata-only;
  this worktree negotiates meta/recent-day contents on own-device sessions while
  legacy clients remain metadata-only. This is an explicit own-device exception,
  not arbitrary member sync or permission to become a nominated host.
- Group keys/device IDs already differ from private transport keys (`group-core.md`
  Devices/Writer IDs). New endpoint creation is not the first implementation step.

## P0: acknowledgement must follow actual persistence

Base defect, now reproduced and fixed in this worktree:

- `group/engine.rs::apply_meta` mutates accepted memory and logs an error from
  `store.append_meta`, then returns `Ok(Some(Changed))`.
- `apply_day` similarly logs an error from `store.append_shard` and returns success.
- `group_read` sends `Ack` for successful `core.apply`.

Therefore a persistence error can be followed by an acknowledgement of a version
vector only held in memory. A forwarding receipt must not copy that behavior.

Covered by owned red-then-green tests: meta/day storage failures, no false Ack or
fan-out, no falsely advanced durable VV, and successful retry after failure clears;
local write rollback and actual group/self readers are also covered. Still required:
test process restart after receiver persistence but before ack.
Returning `Err` alone is insufficient if an advanced in-memory doc later treats
retry as a duplicate: rollback/reload accepted memory from persisted state is also
required. Persist records/metadata/receipt state atomically where applicable.

This is a prerequisite for trustworthy forwarding, not an invitation to redesign
membership/removal/rotation. Production fix ownership: `group/engine.rs`, group store
failure-injection tests and the minimal store hooks needed; coordinate before edits.

## P1: bounded queues and scheduling

Replace eager unbounded session-message buildup with bounded control queues and
coalesced document-dirty signals, backed by durable accepted inventories. Bound
frames, doc lists, dependencies, receipt lists, dials and untrusted decoded work.
A dropped notification must not drop a durable write. A quota failure must surface
backpressure, not delivery success. Give groups fair byte/record budgets; pause old
history before interactive updates. Fairness under topology churn and partial
frames needs additional tests beyond stable-target round robin.

Model evidence: `docs/archive/models/test_forwarding_queue_model.py` checks dropped-hint repair,
quota/local admission, persistence rejection, independent peer progress and a hot
group not starving another group, while preserving exact synthetic proof tokens.
This does not prove actual validators/storage/transport handle those cases.

## P2: own-device group-content adapter

Implemented slice: capability negotiation, bounded shared inventories/cache, actual
self-loop import/export, original proofs/writers, host-hint-free empty bootstrap,
atomic phone-origin day/metadata obligations and conservative host-ack clearing.
Metadata obligations survive clean sealed-store reopen and duplicate retry; failed
clear writes retain pending state. A [negotiated history wire slice](own-history-wire-slice.md)
now pages retained days and requests bounded native ranges, tested with two actual
localhost own-device sessions across page/batch boundaries. Loss/reconnect,
large multi-scope workloads, receipt forwarding and comprehensive multi-node
integration remain incomplete. Direct host sync is unchanged.

Remaining/full-stage acceptance requirements:

- Extend the implemented bounded inventory/frontier requests for authenticated,
  nonremoved own devices with actual loss/reconnect and multi-scope load coverage;
  retain meta-first and then day-document semantics.
- Gate unknown groups on verified group state/bootstrap; no caller-selected group
  string alone grants history or membership. Retain original signed records and
  exact author/writer identity throughout export/import and pending retry paths.
- Keep group admissions, personal block/call grants and linked-device policy separate.
  Known removed memberships remain read-only; no silent rejoin or host nomination.
- Receiving a sibling-authored group update must create the appropriate upstream
  owed state for its original writer(s), not just the laptop's own `core.wid`.
  Current `group_owes` checks its own writer and owed-day state; audit both paths.
- History/blob forwarding needs independent bounds and current-reference checks.
  Groups do not currently have file/voice support (`group-core.md` Left); do not
  claim or add it silently while implementing text sync.

Compatibility must be explicit. Consider a versioned self-sync capability for group
contents; existing metadata-only clients are not desktop-ready for this route.
Do not choose a frozen new ALPN/schema until target integration tests exist.

## P3: route manager

Battery saver requires a reachable authenticated own desktop that supports required
forwarding capabilities; merely an online flag or open unrelated session is not
readiness. Direct mode never suppresses upstream sync. Desktop unavailable means
phone direct fallback; return only after durable partial catch-up. Writes stay local
until the appropriate receipt arrives, regardless of transport changes.

Use finite failure detection/recovery windows and backoff. Select concrete deadlines
from fault tests, not invented guarantees. Currently the model queues when the
laptop is reachable but upstream is stalled: per-scope bypass remains a documented
open policy choice. Expose queued/host-stored/recipient-stored states accurately.

Existing group Ack frames are connection-authenticated VVs, not standalone signed
host receipts. Forwarding those reports requires an explicit own-device-vouched
trust policy or a new authenticated receipt scheme. A laptop cache acknowledgement
is never evidence that an external host/contact received the write. A host receipt
never proves every member received or read it.

## P4: shared host transports

Local iroh tests establish distinct repeated connects and working independent
streams on one connection. Production needs a pool keyed by local endpoint/audience,
remote endpoint and ALPN; DID equality alone is insufficient.

Current group/2 accepts one stream and closes `conn` at the end of that group
session (`group_run`). Pooling requires a multiple-stream accept loop, transport
ownership independent of stream lifetime, per-stream group admission/state and
per-stream teardown. Rejecting/leaving one group must not tear down unrelated
streams. Endpoint rotation correctly closes all affected transports without
changing membership/hosting permissions or disclosing private endpoints.

Compatibility option: explicit negotiated multiplex capability/new version with
legacy one-group-per-connection fallback. Never optimistically open a second stream
on an old host and assume it will be served. No pooling between call/group/self/blob
ALPNs or between differently scoped local endpoint identities.

## Evidence before UI/mobile rollout

Real-node owned acceptance tests must cover two groups on one host and on different
hosts, two multi-device contacts, route switches, receiver quota/storage failure,
lost acks, restarts, stream isolation, rotation and full proof-path address disclosure.
Connection counters must distinguish streams/sessions/dials/actual QUIC handles.
Practical budgets need sustained resource/idle/reconnect measurements and authorized
physical Android tests. The small 1/4/16 localhost smoke is not a recommended limit.

Repository docs have older provisional rotation/removal statements; this task must
not carry those into new behavior or resolve them silently. Follow current repository
constraints (stable DID, audience separation, configurable periodic endpoint rotation)
and coordinate the separate rotation/membership work before touching those policies.
