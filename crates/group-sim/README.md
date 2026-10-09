# group-sim

Test-only first group simulation slice. No production API and no transport/UI wiring.

```sh
python3 scripts/buildlock.py --who group-sim -- cargo test -p group-sim
```

Uses real DID/device attestations, Ed25519 author signatures and Loro updates. Identities and
schedule seeds are deterministic test fixtures (never production secrets). Message keys and
insert-only schemas are provisional; the signing domain explicitly marks simulation data.

Twenty-four tests cover original-author relaying/offline delivery, all six permutations of small
traces, missing-dependency negative controls, tampering, wrong group/outsider/identity,
extra Loro mutation injection, two-host partition recovery, 20 seeded loss/reordering/retry
schedules, bounded pending queues, signed genesis and scoped DID delegation chains.

New authority fixtures verify the creator-signed genesis hash and original grant chain:
holder binding, scope, capability narrowing, delegation authority and chain-depth cap.
Legacy scheduler tests still use explicit allowlists. The grant model is test-only and is
not the courier token format; expiry and revocation are intentionally absent, not solved.

Cross-author dependencies are explicit signed envelope IDs with transitive closure. Validate
actual Loro prerequisites against that declared closure, not incidental receiver knowledge;
omitting a required dependency fails even when the receiver already has it. Receivers
scratch-import and compare semantic state to accepted insert-only records, rather than
trusting signatures alone to make operations safe. Original envelopes survive forwarding.
General causal meta/frontier authorization is not yet implemented.

Five group-consumer reachability tests use a **mock verified-succession adapter**, not a new
rotation verifier. They separate historical bindings from active routes at retirement, reject
private/wrong-audience records anywhere in the supplied complete proof path, isolate nominated
devices from siblings, route/retry messages after host device-ID replacement, and expose failed
discovery after offline retirement. Mock paths assume rotation verified ownership/link ordering;
cryptographic succession, omitted-path detection, fork/rollback and proof-size bounds remain the
rotation contract. Simulated host ID changes do not change author keys or prove writer migration.
The group directory retains historical evidence only after the audience guard; it never routes
using an expired predecessor. This models routing retirement, not access-grant expiry.

The five-node scheduler adds three tests: ten seeded 100-event schedules with three members
and two host replicas, plus explicit reachable-update recovery and mutual-retirement failure.
It combines cross-author causal writes, bounded queues, duplication/loss/reordering, sleep/wake,
partitions, independent host ID replacements and control-frame delivery. Mandatory faults and
coverage counters ensure each seed exercises the intended failures; traces report the seed and
recent events. Healthy recovery delivers address updates over still-known routes before message
retries. There is no automatic global directory refresh. Replica contents/proofs and fan-out
counts converge; a retired queued destination can lose a frame without losing the local write.

The scheduler uses idealized authenticated links and missing-envelope selection (as if a VV
exchange completed), not real handshake/session code. Bootstrap paths are explicit fixtures.
The random schedules recover within address overlap; separate tests expose what happens when
that assumption fails. Host rotation changes routing device IDs only, not author keys or Loro
writer identities. Sleep retains in-memory state and is not a process-crash test.

Not covered: edits/deletes, rotation, author-key succession, device forks/collisions, group
revocation/ban semantics, durable storage, endpoint discovery, file transfers, actual host
capability proofs or OS lifecycle. In-memory host receipts do not establish persistence.
A complete-set reconciliation pass repairs loss in the scheduler; it is not a bandwidth-efficient
production sync protocol. See `docs/group-chat-tests.md` for the larger conceptual plan.
