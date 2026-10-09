# Group chat — current status and handoff

## Location and delivery state

- (Updated 2026-10-10) Group work was imported and committed into branch
  `endpoint-rotation-blocking` (`a3b8829`), worktree `.claude/worktree/endpoint-rotation-blocking`,
  which now hosts both rotation and group work under one coordinator. The old `group-chat`
  worktree is a stale duplicate. Task packets: `docs/design/rotation-agent-packets.md`.
- Coverage audit of these tests: `docs/group-sim-coverage-audit.md` (several claims below are
  overstated; see its §5 hardening table). No production group feature or integration landed.
- Other worktrees/agents own blocking and rotation. Do not copy their uncommitted implementation
  or assume their test-only models are frozen APIs.

## Agreed design

Member-hosted group relay/cache, inspired by kunki/courier but with fixed Rust schemas.
Multiple hosts; hosts are not authorities; all peers verify original author proofs and group
permits. Desktop, headless and Android hosting share an eventual core engine. Direct member
sync is fallback; gossip is optional later. Chat hosting does not imply call-host capability.

User-replaceable device IDs/slots follow owner-DID-signed succession. Hosting follows the
nominated device path, not all sibling devices with the same DID. Succession is NOT group
delegation and must not consume the delegation chain budget.

## Artifacts

| Path | Contents/status |
|---|---|
| `docs/group-chat.md` | design, security gates, staged implementation plan |
| `docs/group-chat-tests.md` | 63 conceptual scenarios including unresolved policy gates; not 63 passing tests |
| `docs/group-rotation-integration-contract.md` | ownership, adapter boundaries, shared pending acceptance cases |
| `docs/endpoint-rotation-coordination.md` | rotation agent's earlier coordination note; later constraints/user corrections take precedence |
| `crates/group-sim/README.md` | exact simulation coverage, assumptions and exclusions |
| `crates/group-sim/src/authority.rs` | test-only signed genesis and scoped delegation; posting authorization adapter |
| `crates/group-sim/src/tests.rs` | real original author proofs, insert-only Loro messages and causal dependency simulation |
| `crates/group-sim/src/reachability.rs` | group consumer of MOCK verified succession, retirement/audience checks |
| `crates/group-sim/src/scheduler.rs` | five-node network scheduler and explicit recovery/failure cases |

Workspace manifests/lockfile include the test-only `group-sim` crate. `docs/chat.md` and
`TASKS.md` point to the new group design; existing DM implementation is not changed.

## Last verified acceptance

From this worktree:

```
python3 scripts/buildlock.py --who group-sim -- cargo test -p group-sim
git diff --check
```

Result: **24 passed, 0 failed, 0 ignored**; diff whitespace check passed.
No full core, Android, desktop or real-transport group acceptance is claimed.

Executable coverage:

- DID attestations, original Ed25519 signatures, signed genesis/hash verification.
- Creator-rooted DID delegation fixtures: scope, holder, capability narrowing, delegation
  rights and maximum depth. This is a test-only format, not courier wire compatibility.
- Insert-only Loro state validation; forged identity, tampered proof and hidden mutation refusal.
- Per-author and cross-author dependency closures; all six orders of small traces; missing
  dependency negative controls; 20 seeded loss/reordering/retry schedules.
- Original-proof forwarding, two-host reconciliation, bounded pending work.
- Retired historical bindings excluded from routes; supplied complete proof paths checked
  for private/wrong-audience records; nomination isolated from siblings/other owners.
- Five nodes (three members/two hosts), ten seeded 100-event schedules: cross-author messages,
  sleep/wake, partitions, bounded queue, duplicate/drop/reorder, independent host ID replacement,
  explicit control updates, retries and convergence. Mandatory fault coverage and failure traces.
- Recovery when a replacement host can push its address to a still-reachable member; explicit
  failure after mutual retirement. No magic rediscovery.

## What is not done

G1 is **not complete**. No production group transport, storage, API, UI or host program.
No edit/delete/reaction rules, general causal meta authorization, group removal/ban policy,
expiry semantics, host-assignment revocation, durable acknowledgements, crash recovery,
files/history fetching, physical-device lifecycle or group calls.

Succession verification is mocked. Network links/control authenticity and missing-envelope
selection are idealized. Sleep retains memory; it is not a process crash. The timed schedules
recover within address overlap; separate negative cases show failure outside that assumption.
Host ID rotation does not migrate author keys or Loro writer IDs. DID-level `Host` capability
fixtures do not prove actual hosting nomination.

## Rotation coordination and unresolved gates

The rotation worktree contains our request and their response:

- `docs/group-chat-coordination-request.md`
- `docs/group-chat-coordination-response.md`

Rotation agent owns succession verification/models. Shared fixtures/oracles are still needed
for device replacement, sibling isolation, historical evidence vs current permission,
revoked predecessors/assignments, capability preservation and audience-safe complete histories.
No production succession or unified blocking API is frozen.

Current transport keys also sign chat/sync and influence Loro peer IDs: do not freeze those
semantics before joint agreement. Linked devices share the mnemonic, hence DID authority;
unlinking a known key is not cryptographic exclusion of a hostile retained mnemonic.

Group removal/concurrency, stale-frontier/backdating, expiry and delegated revocation need
explicit deterministic policy oracles. Address proofs/permits do not solve offline discovery.

## Next work (proposed, not started)

**Persistence/crash-recovery harness**, before real transport or UI wiring:

1. Specify the durability boundary and what acknowledgements mean.
2. Crash before commit: no storage ack, no partial accepted state.
3. Commit before fan-out/ack: restart retains original proofs; retries are idempotent.
4. Recover proof/dependency indexes without silently accepting corrupt data.
5. Lost push repaired by reconciliation from persisted originals.
6. Distinguish simulated atomic snapshots from real filesystem/vault durability tests.

In parallel, settle group policy gates and integrate the rotation agent's verified model through
adapters. Keep conceptual scenarios, model tests and actual integration acceptance distinct.
