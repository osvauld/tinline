# Laptop audio capacity investigation

Worktree `.claude/worktreess/group-audio-capacity`, branch `bench/group-audio-capacity`,
base dev `da83722`. User authorized increasing-count experiments, not a production
cap change. Owned files: this document, `scripts/gcall_capacity.py`,
`scripts/test_gcall_capacity.py`, `scripts/gcall_metrics.py`,
`scripts/test_gcall_metrics.py`, `scripts/gcall_onsets.py`,
`scripts/test_gcall_onsets.py`, `scripts/gcall_bench.py`,
`scripts/gcall_memory.py`, `scripts/test_gcall_memory.py`,
`crates/gcall-bench/src/bin/{members,host}.rs`,
`crates/gcall-bench/src/{lib,talk_pattern,scheduled_voice_tests}.rs`,
local diagnostic/capacity artifacts. No other
worktree is modified. No staging, commits, merge or device changes.

## Reference coverage

Read-only references:
- dev `docs/design/group-call-bench.md`: measured phone N=3/5/8 and desktop N=8;
  harness has actual Opus and QUIC but no production membership/authorization.
- search worktree `docs/scale-year.md`, `docs/group-shards.md`, tests
  `chat_shards.rs` and `group_shards.rs`: day-shard scale, loopback liveness,
  bursts, restart/backfill. Search changes are not in this dev-based checkout.
- group-video-sim `docs/design/group-video-{lab,budget,adaptation,stability}-report.md`:
  mock-only budgets, shared-resource accounting, original-demand scoring, burst
  admission and retained overload failures. Not physical capacity measurements.
- dev `crates/audio/src/{tests,group_call_tests}.rs`: codec loss/jitter, idle
  recovery, speaker switching and mixing coverage.

## First results / exact commands

From this worktree:

```
python3 scripts/test_gcall_capacity.py
python3 -m py_compile scripts/gcall_capacity.py scripts/test_gcall_capacity.py
git diff --check
python3 scripts/gcall_capacity.py --counts 12 --secs 30 --wait-secs 0 --allow-busy --out artifacts/gcall-capacity-smoke
```

Test-first missing-module run failed before implementation. Then 4 guard tests
passed; syntax and whitespace checks passed. N=12 smoke completed, exit 0.
Host CPU 4.8% of one core, 0.6% of machine, measured over 20 s; wire uplink
1326.1 kbps/downlink 674.6 kbps; zero host sends failed and zero receiver loss.
Speech onset p50 96.9 ms, p95 471.6 ms, max 699.5 ms. This p95 is not accepted
service quality; investigate speaker selection/switch-in as well as contention.
Raw output and queue snapshots retained. Smoke is explicitly allowed on a busy
machine, not an uncontended capacity qualification.

Follow-up commands, sequential background runner `/tmp/gcall-capacity-runner.log`:

```
python3 scripts/gcall_capacity.py --counts 16,24,32 --secs 30 --wait-secs 0 --allow-busy --out artifacts/gcall-capacity-smoke-more
python3 scripts/gcall_capacity.py --counts 12,16,24,32 --secs 300 --wait-secs 1800 --out artifacts/gcall-capacity-sweep
```

The latter waits up to 30 minutes per count for idle compile/run/waiter slots,
CPU pressure <=2%, and load <=2. Measurements use an absolute dev buildlock
with `--run`; queue snapshots every 5 s expose overlapping jobs. This does NOT
reserve exclusive machine use: another agent can start during a run. Such runs
are flagged, retained, and must not be described as uncontended. Queue evidence
cannot detect every unqueued process. Binary paths/hashes are in each manifest.

## Metric/source audit (2026-10-12)

Source: `crates/gcall-bench/src/bin/members.rs` `pull`, `summary`, `run_member`;
`host.rs` default duty; `lib.rs` `pct` and `Voice`.

- The mouth time is artificially one frame (20 ms) before generation tick, not
  real microphone capture. Scheduling delays can enter the measurement.
- Audible threshold is RMS >200 per receiver slot, before final mix/hardware.
- Match is latest source onset <= pull time, once per receiver per onset; it has
  no decoded-frame lineage. Delayed old audio can match a newer spurt, and ongoing
  audible output can mask a new onset. Six deterministic interpretation tests
  include a counterexample; these are reference/guard tests, NOT production tests.
- Samples >1000 ms are counted in `late_onsets`, excluded from latency quantiles.
  Never-heard onsets have no count. Empty quantiles print zero, not no-data.
- Host slot 0 is excluded; host receiver itself is not summarized by members.
- `spurts` counts source events, `m2e_n` counts receiver observations: dividing
  them is not a delivery/fairness score. Different receivers share source events.
- Host duty defaults to 0.2, each member to 1/N. Nominal expected speakers is
  0.2+(N-1)/N, not exactly one. Short-run realized duty differs from the nominal
  because of startup/minimum gaps. Larger N reduces per-member samples.
- Receiver packet-loss counters cannot detect speech never admitted by host.

`gcall_metrics.py` preserves raw results, emits conditional p95/null when no
samples, exposes late count/fraction, and refuses to establish service capacity.
It cannot repair missing attribution/denominators from old summaries. Runner
attaches these warnings to future manifests. Existing smoke raw files unchanged;
separate `metric-audit.jsonl` files added.

N=16/24/32 contended 30 s smoke runs completed, each exit 0. N=32 had 90 late
observations plus 1380 included observations (~6.1% of matched observations late);
never-heard remains unknown. No performance verdict from these smoke results.
Long sweep waited for idle and exited after the 30-minute timeout without a run;
no benchmark runner remains active. No heavy tests were rerun during this audit.

Exact lightweight commands: `python3 scripts/test_gcall_metrics.py` (6 pass),
`python3 scripts/test_gcall_capacity.py` (4 pass),
`python3 -m py_compile scripts/gcall_metrics.py scripts/gcall_capacity.py scripts/test_gcall_metrics.py`,
`python3 scripts/gcall_metrics.py artifacts/gcall-capacity-smoke/results.jsonl`,
`python3 scripts/gcall_metrics.py artifacts/gcall-capacity-smoke-more/results.jsonl`,
`git diff --check` (pass). Metrics test-first missing-module invocation failed
before implementation. Result audit files are generated from these CLI commands.

Before capacity qualification, add an explicit per-source spurt/per-receiver
opportunity ledger (including never-heard, censored tails, >1s delays), separately
measure forwarding selection and audible latency, add trace/frame-origin evidence
rather than treating RMS crossing as lineage, and control turn-taking/overlap.
Missing evidence must stay unknown, never silently count as success.

## Opt-in trace instrumentation (native validation pending)

Added `--onset-trace FILE` in the benchmark member/host binaries. Member events
use one shared member-process microsecond clock: ready/finish, spurt start/end,
successfully enqueued send sequence, packet arrival sequence, RMS audible
transition. Host events use its own microsecond clock: packet receipt and
forwarding admission. Host admission is NOT a successful fanout/delivery claim.
Trace I/O is explicitly diagnostic overhead; it is absent unless enabled.
No product code/ALPN/cap is changed. Original aggregate SUMMARY remains intact.

`gcall_bench.py desktop --trace-prefix PREFIX` wires separate member/host traces;
phone tracing is explicitly refused rather than accidentally writing remote
paths. `gcall_onsets.py MEMBER_TRACE --host-trace HOST_TRACE` constructs an offline
per-source/per-receiver spurt ledger using send sequence numbers and actual
ready/finish intervals. It retains deadline misses, late arrivals, and censored
end-of-run opportunities separately. Experiment deadline defaults to 1000 ms,
not a product service guarantee. Host slot 0 remains outside this ledger.

Metrics:
- onset-to-first matching packet arrival: source gating + host selection +
  transport + scheduling, not host selection alone;
- first observed host packet to forwarding admission: same HOST clock only,
  includes frame spacing while waiting; no cross-process timestamp subtraction;
- arrival-to-RMS audible transition: proxy, never verified decoded-frame lineage.
  A subsequent spurt ends the proxy search; no transition remains unknown.
  Packet attribution does not establish attribution of decoded PCM.

10 deterministic ledger tests passed (including clock offset, host admission
without delivery, stale-spurt packet, late/no arrival, censoring, incomplete
trace), plus existing 6 metrics/4 sweep guards: **20 lightweight tests passed**.
Test-first: module missing, then host-clock tests failed with unsupported argument
before implementation. Python syntax, owned-Rust rustfmt and `git diff --check`
pass. Rust build and actual trace smoke are NOT yet claimed passed.

Exact validation: `python3 scripts/test_gcall_onsets.py`,
`python3 scripts/test_gcall_metrics.py`, `python3 scripts/test_gcall_capacity.py`,
`python3 -m py_compile scripts/gcall_onsets.py scripts/gcall_bench.py`,
`rustfmt --edition 2024 crates/gcall-bench/src/bin/members.rs crates/gcall-bench/src/bin/host.rs`,
`git diff --check`.

Queued native validation (isolated target, log `/tmp/gcall-onset-build-v2.log`):

```
CARGO_TARGET_DIR=/home/abe/build-cache/p2p_chat/group-audio-capacity/target python3 /home/abe/p2p_chat/scripts/buildlock.py --who capacity-trace-build -- cargo build --release -p gcall-bench
python3 /home/abe/p2p_chat/scripts/buildlock.py --who capacity-trace-diagnostic --run -- python3 scripts/gcall_bench.py desktop --n 3 --k 2 --secs 30 --host-bin /home/abe/build-cache/p2p_chat/group-audio-capacity/target/release/host --members-bin /home/abe/build-cache/p2p_chat/group-audio-capacity/target/release/members --label trace-wiring --trace-prefix artifacts/gcall-onset-diagnostic/group3 --out artifacts/gcall-onset-diagnostic/results.jsonl
python3 scripts/gcall_onsets.py artifacts/gcall-onset-diagnostic/group3.members.trace --host-trace artifacts/gcall-onset-diagnostic/group3.host.trace > artifacts/gcall-onset-diagnostic/group3-ledger.json
```

The diagnostic may overlap other agents and measures instrumentation wiring,
not clean performance. Earlier queued command was cancelled while still waiting
(before editing host instrumentation), without touching anyone else's job.
Next: check native build/trace output, controlled turn-taking/overlap schedules,
verified playback attribution/never-audible evidence, trace drops/truncation,
RSS/queue growth and quiet repeated capacity measurements. Offline ledger stores
the complete trace and is not a bounded production telemetry design.

## Native trace validation and controlled schedules

The isolated release build completed successfully in 4m27s. Three-person,
K=2, 30-second diagnostic completed successfully (shared busy machine): the
ledger contains 11 member-pair opportunities, all with packet arrival within the
1000ms experiment deadline. Host-observed wait ~20ms normally, one ~60ms;
onset-to-first packet ~40ms normally, one ~81ms; audible proxy adds ~20–78ms.
These validate tracing, not hardware mouth-to-ear latency or host capacity.
Raw files: `artifacts/gcall-onset-diagnostic/group3.{host,members}.trace`,
`group3-ledger.json`, `results.jsonl`. Build/result log:
`/tmp/gcall-onset-build-v2.log`. The native baseline count is not a production test.

Added optional `--talk-pattern turn-taking|overlap` to desktop harness. Each
scheduled utterance is 50 frames (1s), followed by 10 silent frames (200ms).
Turn-taking rotates slots 1..=members, one active member at a time. Overlap
activates every member together. Both use a shared member-process clock to
avoid independent schedule epochs. Host stays silent via `--silent-host` but
still runs its encode/decode/mix participant; host is not removed to lower costs.
Direct/phone controlled-pattern paths are explicitly rejected. These are
experiment fixtures, NOT product defaults or realistic conversation predictions.
Original random pattern remains the default; its signal path is preserved by
factoring the same PCM renderer, rather than substituting a different codec.

21 lightweight Python tests pass (10 ledger + 7 metrics + 4 runner guards),
including distinguishing scheduled duty from random duty. Native schedule
invariants and PCM edge/reproducibility tests passed (4/4, 0.06s; release test
build 15.25s). Total targeted tests now **25 pass**, no skips. Binary rebuild
is still in progress; controlled diagnostic comparisons follow it sequentially in
`/tmp/gcall-pattern-validation.log` using the isolated target and absolute dev
buildlock. No automatic quiet capacity sweep was restarted.

Exact queued commands, with
`CARGO_TARGET_DIR=/home/abe/build-cache/p2p_chat/group-audio-capacity/target`:

```
python3 /home/abe/p2p_chat/scripts/buildlock.py --who capacity-pattern-tests -- cargo test --release -p gcall-bench --lib
python3 /home/abe/p2p_chat/scripts/buildlock.py --who capacity-pattern-build -- cargo build --release -p gcall-bench
```

Then each pattern is run with `gcall_bench.py desktop --n 4 --k 1 --secs 30
--talk-pattern PATTERN --host-bin $CARGO_TARGET_DIR/release/host --members-bin
$CARGO_TARGET_DIR/release/members --trace-prefix artifacts/gcall-pattern-diagnostic/PREFIX
--out artifacts/gcall-pattern-diagnostic/results.jsonl`, under its own buildlock
`--run` slot. Each trace is analysed by `gcall_onsets.py MEMBER_TRACE --host-trace
HOST_TRACE` into a separate ledger. K=1 (+one provisional slot) against three
simultaneous sources is an intentional overload/speaker-selection control, not
an attempt to score selected streams only. Late/missing opportunities remain in
the ledger; no assertion that every simultaneous speaker should be forwarded.

Next: verify these queued outcomes, inspect per-pair misses/proxy absence,
retain failing overload controls, add memory/queue-growth sampling and trace
integrity checks, then quiet repeated 12/16/24/32 qualification. Full app/control
plane, network loss/jitter, WAN/relay and sharded chat/backfill contention remain
separate future tests; no adaptive production admission implemented.

## Controlled K=1 outcomes and K=3/memory follow-up

The controlled binary rebuild passed (2m49s). Both 4-participant K=1 30s
diagnostics completed under heavy shared load, with zero reported packet loss.
Turn-taking: 48/48 eligible member-pair opportunities arrived by the 1000ms
experiment deadline; no missing audible proxies. Overlap: 124 arrived by the
deadline, 20 missed, 2 tail-censored, out of 146 eligible opportunities; 22 lacked
an audible proxy. Do not equate receiver packet-loss zero with universal speech
service. K=1 plus one temporary provisional slot deliberately limits selection
among 3 simultaneous sources. This is policy/scheduling pressure, not proof of
host compute exhaustion. Historical conditional audible p95 was 103.5ms versus
407.2ms; contention was uncontrolled, so these are not isolated causal timings.
Artifacts: `artifacts/gcall-pattern-diagnostic/`; log
`/tmp/gcall-pattern-validation.log`. No failure control was discarded.

Added Linux RSS/VmHWM sampling to desktop harness for host and the entire members
process, plus client-process CPU. Sampled RSS peak, observed lifetime HWM,
first/last/delta, endpoint slope, valid sample count and raw observations are
retained; missing/exited process memory stays unknown. Sampling begins ~5s
into the run and repeats every ~10s, so growth after the final sample can be
missed. RSS includes shared pages; neither it nor a positive slope proves a
leak/unique-memory consumption. The client figure is aggregate simulator memory,
NOT per-phone RAM. Short runs cannot prove long-term steady state.

Memory tests: missing fields, units/HWM versus VmSize, growth without leak claim,
partial samples, real local/exited sampler, harness host-versus-client labels:
**6/6 passed**. Re-ran ledger 10/10, metrics 7/7, guards 4/4: **27 Python tests
pass**. The unchanged native schedule tests previously passed 4/4; total targeted
coverage 31. `py_compile` for new sampler/harness/runner and `git diff --check`
pass. No Rust source changed for the memory sampler.

Current sequential background job `/tmp/gcall-k3-memory-validation.log`:

1. `gcall_bench.py desktop --n 4 --k 3 --secs 30 --talk-pattern turn-taking`
   with both binary paths under isolated release target, diagnostic trace prefix
   `artifacts/gcall-k3-memory-diagnostic/turn`; analyse trace into its ledger.
2. Same for `--talk-pattern overlap`, prefix `overlap`. Both run under absolute
   buildlock `--run`, not uncontended performance claims. Normal K=3 now has
   enough steady selected slots for the three member sources.
3. `gcall_capacity.py --counts 12,16,24,32 --k 3 --talk-pattern turn-taking
   --secs 300 --wait-secs 1800 --bin-dir /home/abe/build-cache/p2p_chat/group-audio-capacity/target/release
   --out artifacts/gcall-capacity-turn-memory`.
4. Same sweep for overlap, output `artifacts/gcall-capacity-overlap-memory`.

Sweep runner now records/passes explicit K and talk pattern. Each count waits
up to 30min for idle queue/load; timeout stops the chain rather than forcing a
busy capacity run. It is not an exclusive reservation: later interference is
recorded and must be flagged. These longer sweeps disable diagnostic tracing;
legacy missing-never-heard/conditional-latency limits therefore still apply.
They qualify resource behavior only, NOT complete speech service. Trace-based
service tests and real clients remain necessary before setting admission policy.

Current queued runs are pending at the time of this entry; no results yet
claimed for K=3 memory comparisons or longer sweeps. Next inspect outcomes,
compare before/after memory with trace overhead stated, add conservative trace
integrity and decoded-audio evidence, and coordinate quiet replay with the other
agents. No production or other checkout modifications, no commits/merge.

## Completed K=3 memory diagnostics / first quiet-window run

Both K=3 4-participant 30s diagnostic runs completed, exit 0. Turn-taking:
48/48 eligible member-pair opportunities arrived by the 1000ms deadline, all
had audible proxies. Overlap: 146 arrived by deadline, 6 tail-censored, no
observed deadline misses; the 6 tails had no proxy. Tracing overhead and heavy
shared load prohibit performance/causal claims. Host sampled RSS peaks ~21MiB;
aggregate three-client process ~27–28MiB. These are not per-device estimates.

First longer turn-taking count N=12 completed (300s, 290s sampled, exit 0).
Manifest `other_jobs_observed=false`, no queued job overlap seen, but the queue
is not a complete system-process monitor. Host CPU 2.89% of one core; aggregate
11-member simulator CPU 8.69% of one core. Host wire up/down 586.0/217.5kbps;
reported receiver loss zero; legacy conditional onset p95 97.9ms (never-heard
remains unknown because long resource runs do not trace).

Host RSS first/last 23,220/25,844KiB; aggregate clients 44,028/79,168KiB. Client
RSS was ~60,508–60,612KiB between sample offsets 100–250s, then rose to 79,168KiB
by offset 290s. Do NOT call this steady-state or a leak: warmup, codec/transport
allocation, offline measurement vectors and other factors need attribution.
Both host and simulator retain benchmark observations; diagnostic ledger is
also offline/unbounded, so it cannot establish bounded production memory.
Raw timeline retained in `artifacts/gcall-capacity-turn-memory/results.jsonl`.

On user instruction to continue, queue was quiet (pressure 0%, load 1.7/8),
N=16 turn-taking was already running. No duplicate job started. Existing serial
runner continues N=24/32 then overlap counts, with idle gating and queue snapshots.
No completed result is claimed yet for those remaining counts at this checkpoint.

## Completed 12/16/24/32 resource sweeps

All eight 300-second runs completed, exit 0; runner is finished. No benchmark
continues at this checkpoint. Source: each sweep's results.jsonl/manifest.json.
Numbers below are single-run observations, not repeated capacity qualification.
CPU is percent of ONE core; client CPU is aggregate simulator-process CPU.
RSS is final sampled process RSS in MiB, not per-device RAM.

| Pattern | N | Host CPU | Client CPU | Legacy conditional p95 ms | Excluded >1s onset observations | Host uplink Mbps | Host/client RSS MiB | Other queued jobs observed |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| turns | 12 | 2.89 | 8.69 | 97.9 | 0 | 0.59 | 25.2 / 77.3 | no |
| turns | 16 | 3.57 | 11.73 | 99.1 | 0 | 0.82 | 27.8 / 93.3 | yes |
| turns | 24 | 3.89 | 17.29 | 100.1 | 0 | 1.27 | 29.2 / 183.6 | no |
| turns | 32 | 2.77 | 16.66 | 102.0 | 0 | 0.66 | 30.8 / 205.8 | yes |
| overlap | 12 | 9.34 | 46.47 | 886.1 | 788 | 2.25 | 24.7 / 67.1 | yes |
| overlap | 16 | 10.93 | 64.17 | 894.2 | 1371 | 3.25 | 26.2 / 99.1 | yes |
| overlap | 24 | 14.24 | 104.43 | 896.3 | 1936 | 5.10 | 29.8 / 192.5 | no |
| overlap | 32 | 15.96 | 138.21 | 925.6 | 1922 | 6.98 | 32.9 / 329.9 | no |

Reported receiver loss and host send failures were zero in these observations.
That does not count unforwarded/unheard speech. Overlap p95 omits all >1s
observations and never-heard opportunities; do not portray ~900ms as an upper
bound or a satisfactory service result. K=3 + temporary provisional slot is
intentionally much smaller than the simultaneously speaking member population.
Low host CPU at N=32 does not resolve this speaker-selection/overlap policy.

Only no-other-queued-job rows are candidate quiet runs; unqueued processes can
still interfere. Contaminated rows require quiet repeats. N=32 turns uplink and
CPU unexpectedly drop versus N=24: investigate workload/selection/scheduling
and trace before extrapolating. No memory steady-state/leak claim; source
benchmarks retain unbounded observation vectors and instantiate many receivers.

Next: repeat contaminated counts, trace selected larger cases with explicit
opportunity denominators, separate top-K selection intent from useful speech
service, attribute memory growth and investigate N=32 turn-taking anomaly.
No increase to production caps or adaptive admission defaults is justified yet.

## Limits / next tests

The prebuilt harness supports 64 slots, so larger experiments need no production
changes. Production still caps desktop calls at 12, control frames at 4 KiB;
raising one numeric constant alone is unsafe. No adaptive admission policy is
implemented. Synthetic members share this laptop and loopback; these results
exclude WAN/relay, microphones/speakers, authorization/control-plane cost,
actual app memory/thermal behavior and live GUI. CPU is measured for host,
not total participants. Existing harness does not measure RSS growth.

Next: inspect each feed's switch-in quality (aggregate latency can hide starvation),
add tested RSS/peak/steady memory sampling and bounded queue accounting, exercise
production membership/control size bounds without changing production defaults,
then independent client/network cases and realistic contention with chat/backfill.
Use a separate future base for search-sharded combined-load tests; never claim
this da83722 experiment includes search changes. Any expanded thresholds,
headroom and overload behavior remain explicit experimental proposals.
