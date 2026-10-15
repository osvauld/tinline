# Group hosts and personal sync devices: offline topology model

Status: **candidate simulation only**, before further production routing changes.
Source: `scripts/test_host_topology_model.py`; reuses the immutable-record,
quota-limited replica and issuer-aware receipt primitives in
`test_forwarding_queue_model.py`. No new production routing is enabled.

## Roles and assumptions

- A group host is explicitly nominated for a scope. A personal relay is an own
  device assigned to a phone; assigning it does not nominate a group host.
- Multiple hosts exchange original records. Members/relays connect to eligible
  hosts; hosts fan out through those sessions. No host does not invent a mesh.
  An admitted member device may be explicitly nominated as a host.
- Authorship remains the original device, with a synthetic proof including its
  owner ID. Account ownership, group admission, original proof validity and
  authenticated receipt origin are **global test oracles**, not wire protocols.
- `online` is a fixture control, not global presence exposed to a host. A host
  can observe its sessions, not infer whether a phone behind a relay is online.
- Membership changes reach every model node instantly. Tests deny subsequent
  records/ranges after scope removal, retain earlier accepted records, and make
  no hostile-shared-mnemonic or retroactive cryptographic revocation claim.
- Each abstract tick schedules connected links; ticks are not seconds. Failure
  detection, discovery, host nomination and membership metadata sync are not modeled.

## Routing candidates compared, not selected

Own-device reconciliation happens around upstream transfer. Before closing direct
routing for a scope, the phone and relay must have equal accepted message inventories
for that scope and both retain scope admission. A returning reachable relay with
missing data does not immediately become exclusive. This is a simplified inventory
oracle, **not** a production readiness protocol or proof of current upstream freshness.

A caught-up reachable relay with an upstream partition has two explicit candidates:

1. **Queue:** keep phone-to-relay routing, retain records until upstream returns.
2. **Scope bypass:** retain own sync but use direct routing for scopes whose relay
   cannot reach an eligible host/contact device.

Both eventually converge after partitions end in tested finite schedules. Queue
uses fewer phone message connections but can delay messages indefinitely during an
upstream outage. Bypass can deliver through a reachable direct path but adds phone
connections and dial work. Neither is silently made the product default. An initial
handover can keep a direct route under either policy until relay catch-up completes.
Explicit Direct mode keeps upstream routing even when a relay is available.

## Records, receipts and attachment bytes

Accepted records are immutable. Quotas reject writes explicitly. Best-effort hints
are bounded and lossy; reconciliation scans accepted inventory. Local, personal
relay, nominated host and named member storage receipts remain distinct. A host
receipt cannot establish all members' delivery or any user's read state. `read`
changes only through an explicit user action (no tests synthesize reads from Acks).

Attachment references are accepted alongside a record through a signed-reference
oracle. Blob identifiers stand for encrypted content hashes; sizes must agree.
Bytes transfer separately as **verified contiguous prefixes**, with a per-directed-
link byte budget for an entire round, receiver quota, prefix persistence on restart,
deduplication and fair rotation across scopes/files. No real ciphertext, hash checks,
AEAD, Bao ranges, filesystem or iroh-blobs implementation is exercised.

Message catch-up does not imply attachment completion. A phone may stay on its own
sync route while a large attachment continues fetching. Group attachments remain a
proposed product feature, not enabled production support.

The fairness test initially failed after enforcing the round byte budget: a second
own-link visit advanced the file cursor with zero remaining budget, repeatedly
skipping the small file's turn. The model now returns before advancing cursors when
no budget remains. This is a discovered **model scheduler defect**, not a claim of
finding the same bug in the app.

## Symbolic connection results

Messages pool only across equal local/remote endpoint identities and protocol.
Separate ALPNs remain separate. Unpooled mode counts the current per-group shape;
pooled mode is a proposed implementation, not currently shipped.

| Fixture, after catch-up | Phone message connections | Host connections |
| --- | ---: | ---: |
| 6 users, 2 groups, one host, proposed pooling | 1 each | 6 |
| 3 users, 100 groups, one host, proposed pooling | 1 each | 3 |
| Same fixture, per-group upstream shape | 1 each | 300 |

For the 100-group fixture, each personal relay has 2 message connections with
pooling (own + host), versus 101 without it. A relay handling two groups and three
contact devices has five message connections; its phone still has one after
catch-up. Each contact device is counted separately, not collapsed by owner ID.

An active file transfer over a separate blob ALPN adds a phone connection: the test
observes **2**, not 1. A separate candidate own-sync blob multiplexing mode observes
1, but that feature is not implemented. External group blob links use the symbolic
group identity, own blob links use the own/main identity; the model is not an endpoint
privacy/disclosure audit. Calls, relay service connections and discovery are excluded.

There is **no whole-app global connection/dial/byte budget** in this standalone topology model.
A new [fault/budget overlay](host-fault-budget-model.md) couples bounded per-device
message/Ack work and connections, network-wide new dials, queued frames and delayed
failure observations. It does not yet combine blob traffic with those global budgets.
Each link is bounded, but host aggregate work grows with connected members. The
existing `test_connection_budget_model.py` separately models bounded fair leases;
it is not yet coupled to this topology's transfers. No capacity/energy inference
is justified by these counts.

## Validation

Owned offline commands (no builds or device sessions):

```sh
python3 -B scripts/test_host_topology_model.py
python3 -B scripts/test_sync_routing_model.py
python3 -B scripts/test_connection_budget_model.py
python3 -B scripts/test_forwarding_queue_model.py
```

Latest run: **23 + 10 + 12 + 12 = 57 passed; 0 failed, 0 skipped**.
New suite approximately 8 seconds; fixture values are not production limits.

New cases cover host fan-out, sleeping phones with reachable relays, returning-relay
catch-up, both upstream-partition policies, multiple hosts/failover, complete host
outage, explicit member hosting without invented mesh, independent personal roles,
instantaneous scope removal, host persistence failure, lost Acks/restart, full/slow
members, blob resume/dedup/quota, separate-vs-multiplexed blob connections, 100 groups,
multiple contact devices, per-scope fallback, file/scope fairness and byte bounds.
Eight seeded schedules each have 40 writes, device availability changes and restarts,
followed by fault-free bounded reconciliation and exact-original convergence.

## Remaining simulations before routing implementation

1. The [fault/budget overlay](host-fault-budget-model.md) now couples message/Ack
   connection/dial/byte/work budgets and packet queues to the topology. Next couple
   blob traffic and measure backlog/service rounds under sustained workloads.
2. Add delayed failure detection, stale peer knowledge, route generations and
   hysteresis; inventory equality alone is not a readiness certificate.
3. Model frame loss/reordering, delayed membership changes, unsolicited requests,
   byte/dependency/verification work bounds and scope-isolated teardown.
4. Model paged inventories while history changes, retained operation batching,
   document-size rejection, receipt retention, quota release and restart cursors.
5. Compare lazy/explicit attachment fetching versus eager fetching, replica cache
   retention, interrupted/corrupt ranges and routing attachment WANTs over own sync.
6. Model any candidate direct-member fallback separately: do not infer a complete
   mesh, discovery, host succession or new membership rights from this fixture.

Then map model invariants to authenticated multi-node Rust/iroh acceptance tests.
Physical Android energy/capacity measurements still require authorization. See
[forwarding queue model](forwarding-queue-model.md),
[connection audit](sync-connection-audit.md),
[integration gates](laptop-forwarding-integration-gates.md), and
[current content slice](own-group-content-slice.md).
