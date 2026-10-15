# Bounded host work and delayed/lossy transport simulation

Status: **offline candidate model only**, before further routing implementation.
Source: `scripts/test_host_fault_budget_model.py`, an overlay on
[the host topology model](host-topology-model.md). Production routing is unchanged.

## What changed relative to the first topology model

1. Collect desired own/group/contact links before admitting any session. A rotating
   lease scheduler limits **each device's total message connections across all
   scopes/protocols**, plus total new dials across the modeled network per round.
   Leases can rotate; a one-connection device need not hold own and upstream links
   concurrently. This is not proof of a production pool's lifecycle safety.
2. TX and RX message/Ack bytes share one aggregate per-device round budget. Sending
   a packet and receiving/verifying a packet each consume a bounded work unit.
   Fairness rotates transfer priority as well as connection admission.
3. Record and receipt frames enter a bounded in-flight packet queue. Packet loss,
   duplication, delayed delivery, reverse ordering at equal due times, partitions
   and bounded receive deferral are explicit. Sender originals survive queue rejection.
4. Planning uses cached link-health observations that expire after a configurable
   abstract grace period. Freshly caught-up own-device inventories must remain ready
   for consecutive abstract rounds before returning to exclusive relay routing.
   Stale routing during the detection gap cannot create actual sessions or receipts.
5. Delivery rechecks current source/receiver scope admission, immutable original
   identity and receipt origin. A packet queued before removal cannot regrant a scope.
6. Oversized messages are retained/deferred, cannot consume the entire in-flight
   queue, and cannot prevent another scope's small message from progressing.
   Operation fragmentation/range batching is still required before such an oversized
   document could converge; safe deferral is **not** successful full-history sync.

## Explicit test oracles and excluded work

The overlay shares the prior model's global accepted-inventory, author-proof,
account ownership, membership and authenticated receipt-origin oracles. Source
candidate selection knows the destination's actual inventory. This idealizes
reconciliation and does not prove stale peer-VV correctness, dependency availability,
Loro range import, receipt cryptography, or secure own-device pairing.

Periodic probes update cached physical link observations. Probe traffic/cost is
excluded, observations are idealized, and membership updates reach all replicas
instantly. Timers are rounds, **not milliseconds or a proposed failure deadline**.
The return streak is a simplified readiness gate, not upstream freshness proof or a
route-generation protocol. Partial migrations, stale admission metadata, abrupt
restart of a route manager and hostile shared-mnemonic behavior need separate models.

Packet bytes are an explicit fixture estimate based on UTF-8 payload/proof/scope
length plus framing constants, **not production wire sizes**. The frame-count limit
also bounds queue bytes by `queue_limit * byte_limit`; outgoing oversize frames are
never queued. Budget units do not measure CPU, bandwidth, RAM, radio wakeups or power.

This overlay transfers message records and receipts only. It does not transfer blob
ranges or charge blob/voice/call traffic against these global budgets. The previous
model's blob tests still cover per-link resume, quotas, dedup and file/scope fairness;
**combined message/blob global scheduling remains a gate**. Authenticated frame
parsing and verification-work bounds in the app cannot be inferred from Python objects.

Scope removal here denies further packets, including old queued ones, while retaining
stored data. This is not a replacement for the protocol's pre-removal history/owed
upload rules. Host nominations do not arise from personal relay assignment. There is
no discovery or automatic mesh when all nominated hosts are unavailable.

## Validation and findings

Owned batch, admitted through the shared queue:

```sh
python3 /home/abe/p2p_chat/scripts/buildlock.py --who sync-fault-budget-model -- \
  python3 -B scripts/test_host_fault_budget_model.py
```

Latest run: **14 passed, 0 failed, 0 skipped**, approximately 9 seconds.
A fresh older-topology regression attempt through buildlock waited 120 seconds
behind another agent's Android unit-test/build job and timed out **before test
execution**. Process inspection confirmed no surviving owned waiter/test job; no
duplicate was launched or admission bypassed. The preceding 57-test baseline is
unchanged but was not freshly rerun in that attempt. `git diff --check` passed.

- Eight users and twelve groups converge while each device has at most two message
  connections, the network has at most two new dials per round, and host work/bytes
  are bounded across peers rather than receiving a fresh budget per group.
- A two-packet queue produces backpressure, preserves accepted originals and repairs
  records/receipts after service becomes available.
- 35% loss, duplication, delayed frames and equal-time reverse delivery preserve
  original authorship; finite history converges after the injected faults stop.
- A host stored record with dropped Acks stays unconfirmed on the phone; after host
  restart and receipt-path recovery it is confirmed without duplicate authorship or
  fake member-read state.
- Relay and upstream failure observations lag actual outages. Eventually direct
  routing becomes eligible; returning relay routing requires stable catch-up first.
- A scope removed with a packet in flight is not restored by its delayed delivery.
  Substituted records and fabricated host receipts are rejected before storage receipt.
- Host outage retains author data until recovery. A hot finite 24-record scope does
  not prevent a second scope's record from progressing with small shared work budgets.
- Oversized records stay pending without claiming destination storage, while small
  independent-scope records can succeed.
- One connection and one new dial per round can rotate own/upstream leases and
  reconcile the tested finite history, rather than inventing unbounded concurrent links.
- Four seeded schedules include 20 writes each, online/offline relay churn, lossy
  delayed duplicate packets, then fault-free convergence under bounded admission.

The first delayed-failure test failed because planning consulted the relay's actual
`online` flag, leaking global truth into a phone decision despite the intended stale
observation. It now uses the cached observation/grace period. Actual transfer still
checks real reachability. This was a **model abstraction bug**, not an app bug claim.
The forged-receipt test also isolates injected events from concurrent legitimate
uploads, so a genuine host receipt cannot accidentally satisfy its negative fixture.

Convergence tests stop faults and writes, restore all admitted nodes, and run finite
fixture-specific drain periods. They do not promise progress through permanent
partitions, under unlimited input, or for every possible topology/admission schedule.
No chosen fixture budget is a proposed production default.

## Next simulation gates

A new [paged sync model](paged-sync-model.md) covers both hops with received
frontier WANTs, stale pages/session generations and shared message/blob byte/queue
budgets. It is symbolic, does not run Loro, and does not yet combine that exchange
with this overlay's multi-user connection/dial/work scheduler.

- Couple message and attachment byte/work scheduling, include aggregate host cache
  quota, lazy attachment WANTs and refill/deadline/backpressure behavior.
- Replace global inventory knowledge with paged, stale and lossy peer-frontier
  knowledge; model growing archive passes, operation dependencies and range batches.
- Add route/session generations and restart of readiness state; queued callbacks from
  obsolete routes must not suppress a new direct path or erase new obligations.
- Delay membership/host metadata independently of data and receipts, respecting the
  current provisional protocol rather than selecting new security policy silently.
- Compare sustained workloads and bounded backlog growth/service latency under
  different budgets, not only finite post-fault convergence.

Then translate invariants into real authenticated multi-node tests. Energy and
Android capacity remain empirical, device-dependent questions requiring authorization.
