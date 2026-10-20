# Connection audit and laptop forwarding: long-horizon task

Worktree: `.claude/worktrees/sync-connection-audit`
Branch: `feature/sync-connection-audit`
Base: `214a6644ac8b2231f42e8c01a175ee1749338fcd`

## Ownership and scope

This worktree owns its new routing models, audit notes and targeted tests. Do not
edit integration `dev`, the release-channel worktree or other agents' files.
Production changes require an exact file-ownership check before editing shared
subsystems. No commits, device installs or data wipes without authorization.

Goal: determine actual connection use and practical budgets, then implement
proof-preserving desktop forwarding with mobile Battery saver/Direct modes.
The original model is copied here, not production behavior.

## Compute policy (user requirement)

- Check the shared queue with `python3 /home/abe/p2p_chat/scripts/buildlock.py --status`
  before every heavy build/test batch; check again between batches.
- Use unique queue identity `sync-connection-audit` for all heavy work.
- Long low-compute inspection/waiting and tiny offline models may run directly,
  as requested by the user. Classify by resource use, not wall-clock duration.
- Run only tests owned by this task or narrowly selected regressions needed for
  an owned production change. No full-workspace suites or unrelated benchmarks.
- Start with offline model/source audit. Do not compile merely to inspect code.
- Plain queued `cargo test` automatically splits compilation and execution:
  compile lock is released before obtaining a run slot. Other agents may compile
  during our test execution. Do not hide cargo test inside a shell wrapper that
  defeats this split.
- Already-built benchmarks and model stress batches use `--run`, never hold the
  compile lock during an entire measurement campaign.
- Use bounded individual benchmark cases, conservative concurrency and explicit
  case time budgets. Queue each next case separately, check contention, and defer
  additional batches when others are waiting or memory is scarce.
- The current lock system provides admission, not preemption or strict FIFO
  fairness. It cannot make a single long compile yield mid-command, and CPU-heavy
  run jobs can still compete with compilation. Keep our jobs small; do not claim
  the queue alone prevents compute monopolization.
- No duplicate jobs after tool timeout; inspect our existing job before retrying.
  Never terminate another agent's processes or delete their outputs.
- Device validation is a later gate and must use the device lock. No phone install
  authorization has been given.
- Keep logs/results/build outputs out of Git; retain only intentional test source
  and concise reproducible validation summaries.

## Milestones

1. Source audit: endpoint creation, protocol/ALPN connections, streams, per-device
   session maps, dial concurrency, retries, bounds and teardown. Read current
   chat/group/rotation contracts before changing behavior.
2. Expand owned deterministic models: many groups on shared/distinct hosts,
   multi-device contacts, per-scope authorization, bounded queueing, route
   handover, partitions and original author proof preservation.
3. Targeted instrumentation and independent real-endpoint tests. Distinguish
   logical subscription count, sessions, QUIC connections, streams and dials.
4. Small incremental capacity benchmarks: idle memory/CPU/traffic, active delivery,
   reconnect bursts and slow consumers. Select desktop/mobile budgets from data,
   not guessed universal iroh limits. Stop increasing scale on resource pressure.
5. Production forwarding: reuse own-device synchronization, retain unacknowledged
   writes, preserve scope validation and expose laptop-storage versus recipient
   delivery distinctly. Separate calling endpoints and audit all disclosure paths.
6. Physical Android energy/doze validation only with explicit device authorization.

At each milestone record exact commands, outcomes, skipped cases and limitations.
Offline models cannot prove cryptographic, transport, persistence or battery claims.

## Initial evidence (source inspection only)

- `crates/core/src/chat/engine.rs`: session map keyed by remote device; outbound
  contact dialing tries listed devices and returns after a successful handshake.
  This is not a verified whole-app count or capacity ceiling.
- `crates/core/src/sync/engine.rs`: own-device dial round considers all nonremoved
  linked devices with per-device dialing guards/backoff, not a laptop-only hub.
- `crates/core/src/chat/own.rs`: existing own-device DM document export/import
  groundwork. Explicit document/batch caps do not establish connection limits.
- `scripts/buildlock.py`: compile/run split for plain cargo test; four run slots,
  admission gates on CPU pressure and available memory. No strict FIFO scheduler.

## Progress checkpoint

- First source-audit report: `sync-connection-audit.md`.
- Added `docs/archive/models/test_connection_budget_model.py`: shared/distinct host demand,
  multi-device contacts, separate ALPN/local identities and bounded lease/dial
  scheduling. Twelve tests passed.
- Existing routing model: ten tests passed, including 2,000 seeded transitions.
- Important correction: current groups do not share host transports, and current
  own-device group sync shares metadata, not group contents. Both require real
  integration work; proposed counts must not be described as current behavior.
- Bounded forwarding/receipt model now has twelve passing tests, including storage
  failure and fair shared-host group scheduling. Latest owned offline suite total:
  34 passed (routing 10, budget 12, forwarding 12).
- Isolated localhost iroh package: four transport tests and one 1/4/16-connection
  inventory test passed. Compilation used two jobs via buildlock; latest rerun
  waited behind another agent rather than bypassing admission. See the tool README.
- Integration gates: `laptop-forwarding-integration-gates.md`. Audit found group
  imports log persistence errors but can still reach session acknowledgement;
  production fault-injection/rollback work is required before forwarding receipts.
- Next: owned production fault-injection tests for meta/day import persistence,
  then the minimal durable-import fix before content forwarding. No application
  behavior changed; no device operations or maximum-capacity claim.
