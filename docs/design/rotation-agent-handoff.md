# Endpoint rotation — long-running session handoff

## 0. Start here

Worktree: `/home/abe/p2p_chat/.claude/worktree/endpoint-rotation-blocking`
Branch: `endpoint-rotation-blocking`
Base when this handoff was written: `6b1aaef`.
No rotation implementation commits have been made. Rotation additions are currently
untracked/uncommitted. Verify state rather than assuming these facts remain current.

This is an execution guide, not approval to resolve every open protocol question.
Use `rotation-agent-tasks.md` for assignments and `rotation-agent-status.md` for
claims/results. The coordinating session owns cross-agent integration.

First commands (from this worktree):

```
pwd
git branch --show-current
git rev-parse --short HEAD
git status --short
python3 scripts/buildlock.py --status
```

Read, in order:

1. `AGENTS.md` and this handoff.
2. `docs/design/endpoint-rotation-blocking.md` (intent; historical initial ordering).
3. `docs/design/endpoint-rotation-implementation-plan.md` (full plan and progress).
4. `docs/design/endpoint-rotation-device-audit.md` (existing identity limitations).
5. `docs/design/endpoint-rotation-permit-model.md` (latest candidate and limits).
6. `docs/group-chat-coordination-request.md` and
   `docs/group-chat-coordination-response.md`.
7. Read-only group contract in sibling checkout:
   `/home/abe/p2p_chat/.claude/worktree/group-chat/docs/group-rotation-integration-contract.md`.
8. `docs/protocol.md`, `docs/design/device-linking.md`, `docs/design/multi-device.md`
   and relevant vault/chat docs before changing those subsystems.

Some earlier docs describe stable slots. The later user clarification wins:
**device IDs/slots may be replaced through owner-signed succession**. The original
nomination can remain a historical reference; it is not a permanent current ID.
Keep historical notes labeled; don't silently treat every earlier proposal as settled.

## 1. User-agreed product direction

- Account DID remains stable; an account has multiple linked devices.
- Transport endpoints rotate periodically on a configurable interval with or
  without blocks. Blocking does not trigger/reset the rotation timer.
- Blocked contacts do not receive subsequent private reachability updates. Old
  endpoints retire; core verified-DID blocking also prevents access through leaks
  or alternate attested endpoints. Do not promise eliminating raw connection attempts.
- We retain others' addresses and connect from our replacement endpoint. Existing
  identity proofs/relationships avoid requiring a fresh contact invitation.
- Connections carry authenticated control/presence/ephemeral data, not only calls,
  text or audio. Address updates must work without a call or visible chat message.
- Device replacement is authorized with DID-signed, scoped succession permits.
  Reuse courier's signed-permit pattern, not an improvised cryptographic primitive.
- Group delegation permits allow authorized members to invite others. That authority
  is separate from owner device succession, and routine rotation does not consume
  the delegation-chain depth budget or grant broader hosting/admin permissions.
- Public/group-facing reachability and private/trusted-contact reachability are
  separate audiences. No private endpoint leaks through group history/proofs/lists.
- Group hosts can eventually distribute owner-signed group-facing updates; group
  chat is NOT required for the first rotation implementation.
- No anonymity guarantee, no Double Ratchet requirement, no peer-introduction chain
  required for direct identity continuity.

## 2. Current implementation truth

### Existing production code (not newly implemented here)

- Linking transfers the mnemonic; every linked device has the account's DID signing
  authority. The random local `device_secret` is not transferred as a sibling secret.
- Device registry entries are currently keyed by transport public keys.
- Own-device authentication checks same DID, attested transport key and tombstones.
- Chat authentication reuses grant/contact/block checks with endpoint attestation.
- Contact removal currently also blocks the DID, purges chat and ends matching calls.
- Transport secret also signs chat/self-sync batches; Loro peer IDs depend on device
  public key and document name. Rotation is therefore not just swapping a socket key.
- Core base contains committed merge markers in `crates/core/src/node.rs` (verify
  their current state). No full-core passing baseline has been claimed here.

### Added here: test-only artifacts

| Path | Purpose |
|---|---|
| `scripts/test_rotation_discovery_model.py` | Idealized knowledge/prediction trade-offs; 6 tests |
| `scripts/test_rotation_network_model.py` | Separate peer directories, scheduling/partitions/blocks; 10 tests |
| `crates/proto/tests/rotation_identity_boundary.rs` | Real signatures characterizing existing auth; 5 tests |
| `crates/proto/tests/rotation_succession_model.rs` | Earlier fixed-slot history candidate; tests + included queue |
| `crates/proto/tests/support/rotation_network.rs` | Earlier signed queue/crash/fork model; together 10 tests |
| `crates/proto/tests/rotation_permit_model.rs` | Owner-replaceable IDs, expiry/history, possession candidate |
| `crates/proto/tests/support/rotation_permit_network.rs` | Per-peer routes, scoped histories, signed direct update queue |
| `crates/proto/tests/support/rotation_permit_lifecycle.rs` | Independent timers, crash boundaries, bounded challenges |

The three permit files together have 18 tests. Latest targeted Rust run: **33 passed**.
Python models: **16 passed**. These counts are historical results, not guarantees
for your checkout; rerun after changes. No production verifier/timer/store/network
API has been added, no Android measurement or full workspace baseline established.

### Current model assumptions — do not promote accidentally

- Real Ed25519 signatures, but fake endpoints/transport delivery and ideal atomic
  snapshot clones. Crash tests are NOT disk/vault/power-loss tests.
- Strict contiguous predecessor history; fork detection is not fork convergence.
- Expired records may advance historical knowledge, not become active routes.
- One actual successor on overdue resume, next deadline = resume time + interval.
  No fabricated generations for missed ticks. This is a candidate policy.
- Fixture replacement IDs are deterministic small byte patterns, NOT production IDs.
- Receiver challenges: eight outstanding, five fake-time units TTL; nonce allocation
  high-water is assumed durable. These are test parameters, NOT product defaults.
- One endpoint per simulated device. Simultaneous public/private endpoints, author-key
  migration, real OS time and transport behavior are not modeled.
- Private and group anchors are initialized separately in tests. Initial anchor
  authentication and a complete safe export/inventory API still need specification.
- Host assignment is a trusted policy fixture, NOT verified group permit integration.

## 3. Required decisions / production gates

Record a decision, rationale, owner, and tests BEFORE freezing behavior:

| Gate | What needs resolving |
|---|---|
| G-DISCOVERY | Linked devices calculating future reachability vs independent random keys + recovery; no scheme selected |
| G-AUTHORITY | Preserve shared-mnemonic trust model? Strong hostile-device revocation is impossible under current authority sharing |
| G-FORK | Clone/fork detection, quarantine and authorized recovery; no last-arrival-wins shortcut |
| G-REVOCATION | Older-ancestor revocation after later successors; replacing revoked predecessor; group assignment remains separate |
| G-HISTORY | Bounded catch-up/checkpoints, history pruning, offline periods, live latest-key possession while verifying expired predecessors |
| G-TIME | Default/minimum interval, jitter, configuration changes, wall-clock skew/rollback, validity and overdue resume |
| G-RENEWAL | Permit expiry when no replacement is available; renewal without rollback or resurrection |
| G-KEYS | Transport vs author keys, historical verification, Loro writer identity, clone/restore and device registry migration |
| G-RETIREMENT | Active sessions/calls/transfers at retirement; fail-closed behavior if replacement cannot start |
| G-BLOCK | Remove vs block UI/API, concurrent block/unblock sync, grants and queued work after unblock |
| G-STORAGE | Atomic commit boundaries, journal/recovery, sealed secrets, endpoint startup/retirement ordering |

A useful next concrete failure case: the latest network model binds possession to
its page's last endpoint. For more than one page of old history, that endpoint may
already be retired. Separate live session authentication of the newest binding
from bounded historical-page verification; test it before choosing the interface.

## 4. Execution order

### Wave 1 — can proceed now, with disjoint files

A. Baseline/source audit and isolated merge-marker repair proposal.
B. Permit-model gaps: multi-page history with current possession, forks/revocation.
C. Lifecycle model gaps: time changes/configuration, startup failures, restart state.
D. Group/key compatibility and discovery/security research (docs/read-only source).

Do not start multiple agents editing the same support module. See task packet file
ownership. Coordinator integrates child modules and shared type changes serially.

### Wave 2 — only after policy decisions and model review

- Extract agreed pure record/verifier into proto; tests target the real API before
  implementation. Version/domain/schema/byte bounds must be explicit.
- Keep independent model/reference tests rather than rewriting oracles to match code.
- Get group-agent agreement on succession output, author mapping and audience exports.

### Wave 3 — serial persistence → control → scheduling

- Persist device succession/key/timer and receiver ordering state safely under vault.
- Add authenticated control updates and central DID policy/cancellation.
- Rotate actual iroh endpoint lifecycle with bounded overlap and documented retirement.
- Add platform interval configuration and Android resume/boot/network-change behavior.

Do not skip directly from passing fixture tests to UI/Android code. Do not freeze a
shared group API until the appropriate gates above are decided.

## 5. Test commands and reporting

From THIS checkout:

```
python3 scripts/test_rotation_discovery_model.py
python3 scripts/test_rotation_network_model.py
python3 scripts/buildlock.py --who rotation-AGENT -- cargo test -p proto --test rotation_identity_boundary --test rotation_succession_model --test rotation_permit_model
```

For a narrower iteration, run `cargo test -p proto --test rotation_permit_model FILTER`
through buildlock. For proto regressions, run `cargo test -p proto` through buildlock.
Full core/workspace tests follow baseline repair; don't claim targeted tests prove them.
Use unique logs such as `/tmp/rotation-AGENT-tests.log`. If a tool timeout occurs,
report pending/failure accurately; avoid accidentally starting duplicate build jobs.

Do not run workspace-wide formatters over another agent's changes or broken base
files. Format only your owned files. Tests must assert unchanged accepted state on
rejection, not merely an error. Include negative controls and deterministic traces.
A test passing against a local model is not a failing production-facing test: name
which kind it is and which assumptions it embeds.

## 6. Shared-worktree safety / handoff discipline

- Before claiming: inspect status board and working diff; reserve exact files.
- Shared files (`proto/src/lib.rs`, Cargo manifests/lockfile, shared test model types,
  core/node.rs, status/planning docs) have one integrator at a time.
- Prefer new independently named test modules during parallel work; ask coordinator
  to add module declarations and shared helpers after review.
- Do not touch another agent's checkout. Read sibling group docs/code as needed and
  write requests/responses in this worktree. Ask before copying reference code.
- No automatic staging, commits, rebases, cherry-picks or cleanups. Coordinator may
  create narrowly scoped commits only if the user asks/authorizes it.
- On completion provide: files changed, decisions vs proposals, exact test output,
  limits, next step, and any newly observed pre-existing failure.
- Update status board without overwriting another agent's entry; coordinate writes.
- Preserve original author proofs and user data. Never use real profiles/keys for tests.
- Keep the user informed when a policy choice blocks work instead of silently choosing.

## 7. Completion definition

The task is not done when simulations pass. Completion requires tested production
protocol + crash-safe storage + real direct/relay control delivery + retirement +
core blocking, migration of existing linked-device identities without lost data,
and platform lifecycle coverage. Private/public separation and group integration
need separately tracked acceptance if deferred. Document offline-discovery and
shared-mnemonic limits honestly in the product; no claims beyond tested guarantees.
