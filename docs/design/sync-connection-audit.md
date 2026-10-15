# Connection audit — first source pass

Status: source inspection and offline models only. No production changes.
Base: `214a6644ac8b2231f42e8c01a175ee1749338fcd`.
Paths below are relative to this worktree. Counts are modeled demand, not measured
live iroh connections or a production capacity limit.

## Findings

| Subsystem | Evidence | Implication |
|---|---|---|
| Main endpoint | `crates/core/src/node.rs:1947–1968` | Main endpoint accepts calls/contact control, chat, blobs, self-sync and linking ALPNs. Separate protocol connects must be counted separately; one endpoint is not one connection. |
| Group endpoint | `crates/core/src/group/engine.rs:896–913` | Separate persisted endpoint secret and group/group-call ALPNs already exist. Do not implement a redundant second group endpoint without auditing existing identity separation. |
| Group dialing | `group/engine.rs:1319–1446` | Iterates group-host relations, uses `(gid, host)` dialing/backoff keys and calls `ep.connect` for each. Hello selects one group; the runtime does not multiplex several groups over one session. |
| Group host selection | `group/engine.rs:1365–1416` | Considers eligible active hosts per group, not just one preferred host across the account. Several hosts per group increase demand further. |
| Group dial admission | `group/engine.rs:1344–1360` | Per-relation in-flight deduplication, but no shared dial semaphore is visible in this loop. A large ready target set can spawn many dials. |
| Group incoming admission | `group/engine.rs:1177–1209` | Spawns a task per incoming connection with timeouts. No app-level pending-handshake semaphore visible here. Transport-internal limits have not been audited. |
| Contact dialing | `crates/core/src/chat/engine.rs:374–414,456–493` | Outbound kick skips a DID with an existing session, serially tries contact devices and stops after one success. Do not assume outbound connects to every device of every contact. |
| Contact session inventory | `chat/engine.rs:62–68` | Session map is per remote device; multiple devices belonging to a person can nevertheless have active sessions. |
| Contact dial budget | `chat/engine.rs:229,396–399` | Four-permit outbound chat dial semaphore; this caps concurrent dial attempts, not retained sessions or other subsystem dials. |
| Own-device dial inventory | `crates/core/src/sync/engine.rs:781–811` | Considers all nonremoved sibling endpoints, with per-device deduplication and backoff. No laptop-hub preference or shared global dial semaphore visible here. |
| Group self-sync | `crates/core/src/group/own.rs:1–8,18–103` | Shares group tokens/host hints; explicitly does NOT forward group contents. Receiving a token starts ordinary host dialing. Laptop-only group sync requires a new contents/proof path, not just a mode toggle. |
| DM self-sync | `crates/core/src/chat/own.rs:1–24,87–110` | Existing own-device DM documents/export machinery can be reused subject to its real validators and delivery semantics. |
| Queueing | `chat/engine.rs:560`, `group/engine.rs:1532` | Session command channels are unbounded. This is a resource-risk finding, not evidence of a measured leak. Production forwarding needs bounded queues and reconciliation after dropped/coalesced notifications. |
| Keepalives | `node.rs:1947–1954`, `group/engine.rs:896–899` | Both configure five-second keepalive and twenty-second idle timeout. Actual idle traffic/radio wake cost remains unmeasured. |
| Streams vs connections | `node.rs:1954` | `max_concurrent_uni_streams(16)` is a per-connection unidirectional stream setting, NOT a sixteen-connection limit. |
| Main incoming admission | `node.rs:2025–2090` | Pending/reserved handshake semaphores exist, with permits released early for own-sync/link ALPNs. These are not whole-app live-connection caps. |

No defensible whole-app maximum connection count has been established. This pass
has not inspected all iroh transport defaults, group calls, blob transfer pooling,
relay internals, all teardown paths or production runtime metrics.

## Current versus proposed demand

Fixture assumptions: one active nominated host per group; unique remote host
endpoints; one active chat device per person unless specified; no calls, blobs,
invitation fetches, reconnect overlap or own-device sessions included.

| Fixture | Current per-group/device model | Proposed pooled direct model |
|---|---:|---:|
| 2 groups, 1 host, 2 people | 4 | 3 |
| 100 groups, 1 host | 100 | 1 |
| 100 groups, 10 hosts, 50 people | 150 | 60 |
| 2 groups, 1 host, 2 people with 3 active devices each | 8 | 7 |

The proposed pooling key is `(local endpoint identity, remote endpoint, ALPN)`.
Same DID is insufficient; different local endpoint audiences/ALPNs cannot be
merged indiscriminately. Within a transport, each group still needs independent
admission, membership-change handling and document authorization.

Battery saver proposes one phone-to-desktop message-sync session. The desktop
still bears upstream demand. Calls/blobs can add phone connections, and different
laptop protocol paths may require additional connections. Neither count model
promises one total phone socket.

## Offline models and validation

Commands executed directly as tiny low-compute tests, with `-B` to avoid cache files:

- `python3 -B scripts/test_sync_routing_model.py`: **10 passed**, 0.038 s.
- `python3 -B scripts/test_connection_budget_model.py`: **12 passed**, 0.003 s.
- `git diff --check`: passed for tracked changes; new files were manually reviewed.

No tests ignored. No Rust builds, device operations, real QUIC load tests or battery
measurements were run. The routing model includes twenty seeded 100-step schedules.
The budget model additionally checks shared-host counts, multiple contact devices,
ALPN/audience separation, independent authorization and bounded/fair lease selection.

Model budget inputs (eight connections, two concurrent dials in one test) are
arbitrary test fixtures, NOT recommended mobile limits. Round-robin leases model
finite eventual service only with a stable reachable target set. They do not prove
fairness under topology churn or justify frequent production reconnects.

## Next work

1. Audit real group validation/storage/self-sync contracts and transport lifecycle;
   freeze proof-preserving group-content forwarding before any production adapter.
2. Extend the model with bounded notification/outbox queues, slow-consumer recovery,
   partial reconciliation and scoped receipts. Keep call/block/group policies separate.
3. Add targeted real-endpoint metrics/tests for our changes after file-ownership
   coordination: count current connections by endpoint/ALPN, active streams,
   pending handshakes/dials, retries and closed-but-retained handles.
4. Measure small queued capacity cases, releasing admission between cases. Choose
   practical caps only after CPU/memory/traffic and mobile testing.

## Follow-up checkpoint

The “no Rust builds” statement above describes the initial source pass, not the
current cumulative task. Subsequent owned validation:

- Offline routing 10 + budget 12 + forwarding 12 = **34 tests passed**.
- `tools/iroh-connection-audit`: four localhost transport tests plus one inventory
  test passed, no ignored tests, using two compiler jobs and the shared queue.
- Repeated same-endpoint connects really are distinct QUIC connections; two streams
  can share one connection, and a stream reset does not close unrelated streams.
- Inventory cases 1, 4, 16 kept all transports usable and observed remote close.
  These are not maximum-capacity, memory-per-connection or mobile-energy results.
- New source concern: `apply_meta`/`apply_day` log append failures but return success;
  `group_read` can then acknowledge. See `laptop-forwarding-integration-gates.md` for
  required production failure/rollback tests before forwarding receipts.

Detailed commands, single-run samples and limitations: the isolated tool README
and `forwarding-queue-model.md`. Application code remains unchanged.
