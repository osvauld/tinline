# Production audio refinement — test-first packet

Worktree `.claude/worktreess/group-audio-capacity`, branch `bench/group-audio-capacity`,
base `da83722`. Dev has since advanced to `33145bd` (profile sync); this packet
has not imported it or merged/staged anything. All older benchmark work preserved.

## Ownership / scope

Reserved in this worktree only:
- `crates/proto/tests/gcall_capacity_bounds.rs` (new)
- `crates/core/tests/support/group_call_capacity.rs` (new)
- `crates/core/tests/group_call.rs` (only child-module registration)
- this document and the capacity research report.

No product implementation/cap/admission change yet. Tests use only synthetic
accounts and private temporary data. No device installations/wipes or cross-agent
source changes. Absolute dev buildlock and isolated target are used.

## Production audit

- `proto::gcall::MAX_PARTICIPANTS=12`; client verifies this bound on signed Start,
  Roster and HostRequest. Changing the host cap alone does not permit larger calls.
- `MAX_CONTROL=4096`; the roster includes real DID-signed joins and a host signature.
  12 fits; 16/24/32 need independent serialization/frame-bound work. Host `frame()`
  uses `expect` based on current 12-person invariant, so raising only a constant
  risks panics rather than graceful overload handling.
- `Cond::cap()` is class/network based (desktop 12, phone 8/4), not measured host
  capacity. Rank filters expected participants by advertised caps. No adaptive
  compute/uplink estimator, resource budget API or headroom decision exists.
- Client codec receivers are created per roster slot and retained across speaker
  changes until membership changes; current memory is bounded by participant cap,
  not just K. Benchmark aggregate client RSS is not evidence of a product leak.
- Host replay signatures `seen` and per-DID join ordering grow over a call's lifetime.
  Replay security cannot be weakened by arbitrary pruning. Long-call/churn resource
  bounds need explicit tests and a replay/ordering retention decision.
- Speaker selection is top-K + temporary K+1. Simultaneous speech intentionally
  exceeds selected capacity; no promise of all speakers audible simultaneously.
  Bounded recovery after old speakers go silent can be tested without inventing
  fairness/default UX policy.

## Added tests

### Production protocol (real signatures)

`gcall_capacity_bounds`:
1. 12-member roster is verified, framed, decoded and reverified with signed joins.
2. Existing semantic and transport bounds reject 16/24/32.
3. Oversize length prefix is rejected before a body arrives.
4. Explicit ignored candidate red gate: 16-member roster can be conveyed/verified.
   This is run deliberately with `--ignored`; current FrameTooLarge is expected.
   It does not prescribe binary/JSON/chunking/schema or a new default cap.

### Fully integrated real nodes

`group_call::capacity_tests` reuses the real group-call integration helpers, NOT
`StaticDirectory`: ordinary group invitations, signed CallHostPermit/acceptance,
announcements through chat host, Node APIs and iroh group endpoints.

1. 13 legitimate members: 12 join the current call; an extra attempted join must
   not migrate/end existing peers or modify the authoritative roster; after one
   leaves, the eligible extra member can join. This characterizes the current
   boundary and protects refusal/recovery behavior, not permanent 12-person policy.
2. Five joined nodes: three selected speakers go silent, a different member speaks;
   actual decoded PCM must carry the new tone at every other node and not return
   the speaker's own tone. Speaker changes must preserve group membership.
   Five-second drive is a test tolerance under shared load, not a product SLA.

A full larger-call real-node acceptance test waits for explicit admission-budget
and signed-roster transport interfaces. There is no honest way to test a non-existent
budget API without inventing its semantics. Existing wire red gate captures the
first concrete production obstacle before choosing those interfaces.

## Exact commands / current status

Source formatting and `git diff --check` passed. Native tests are queued, not yet
claimed run. Logs `/tmp/gcall-production-wire.log`, `/tmp/gcall-production-nodes.log`.
With `CARGO_TARGET_DIR=/home/abe/build-cache/p2p_chat/group-audio-capacity/target`:

```
python3 /home/abe/p2p_chat/scripts/buildlock.py --who capacity-wire-current -- cargo test --release -p proto --test gcall_capacity_bounds -- --nocapture
python3 /home/abe/p2p_chat/scripts/buildlock.py --who capacity-wire-red -- cargo test --release -p proto --test gcall_capacity_bounds candidate_sixteen_member_roster_can_be_sent_and_verified -- --ignored --nocapture
python3 /home/abe/p2p_chat/scripts/buildlock.py --who capacity-real-nodes -- cargo test --release -p p2pcore --test group_call capacity_tests:: -- --nocapture --test-threads=1
```

The wire runner intentionally exits nonzero at the red acceptance test, only after
current-boundary tests pass. Verify the actual failure text (not a compile/queue
failure) before calling that an established red gate. Real-node tests run serially
inside their binary; no full-core/workspace test suite is scheduled.

## Policy/protocol gates before implementation

1. Larger signed-roster transport: retain 4KiB with compact representation or bounded
   pages, versus an explicitly larger bounded/versioned control frame. Compare byte
   limits, signature verification, partial/forked roster rejection and allocation
   before choosing. Do not just raise MAX_CONTROL/cap and retain panic assumptions.
2. Admission input: owner-configured resource budget/headroom versus measured estimator
   with conservative fallback. Both need finite hard wire/resource bounds; resource
   changes should refuse new joins without ejecting healthy peers by accident.
3. Selection service: intended maximum simultaneous speakers, new-speaker recovery,
   starvation/fairness policy and overload disclosure. Spare CPU does not select UX.
4. Replay/history retention for long calls and churn; no unbounded growth or replay
   resurrection through naive eviction.

Next: read targeted outputs, fix confirmed regressions first, then propose bounded
record/admission interfaces and tests for user review. No implementation choice is
silently selected by these tests; no merge/cap-increase is justified yet.
