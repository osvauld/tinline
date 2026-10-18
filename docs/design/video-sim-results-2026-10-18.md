# Video simulation results, 2026-10-18

Branch `integrate/test-2026-10-18` (HEAD 75b0dd69) vs baseline `dev` 0562ea6e.

## Verdict

No change in video adaptation behaviour. `git diff dev..HEAD -- crates/video` is one file
(`media.rs`, +73): an opt-in `LadderConfig::fixed_rung: Option<usize>` (default `None`) plus
3 unit tests. The deterministic simulation output is **line-for-line identical** on branch and
baseline (diffed after stripping build/log noise). The ignored real-network tests are
wall-clock sensitive and fail intermittently on both sides (see below); no regression
attributable to the branch.

## Commands (all via LAN worker 192.168.1.7, `-j 6`, `--release`)

Wrapper: `tasks run --who video-sims --offload -- tasks-offload --worker abe@192.168.1.7 --who video-sims --ssh-key <key> -- <cmd>`

| # | Command | Tree | Job (local / worker) | Result |
|---|---|---|---|---|
| 1 | `cargo test --offline --locked -j 6 -p video --release --features net --lib sim:: -- --nocapture --test-threads=1` | branch | 636 / 199 | 32 passed, 0 failed, 1 ignored (`debug_latency_by_second`) |
| 3 | same | dev | 645 / 206 | 32 passed, 0 failed, 1 ignored |
| 2 | `... --lib net_real -- --ignored --nocapture --test-threads=1` | branch | 640 / 202 | 2 passed, 3 failed (clean, loss_3pct, outage_500ms) |
| 7 | same, rerun | branch | 661 / 217 | 3 passed, 2 failed (clean, outage_500ms) |
| 4 | same | dev | 649 / 209 | 3 passed, 2 failed (bandwidth_drop, clean) |
| 5 | `env RUST_LOG=off cargo test ... -p p2pcore --release --test video_calls` | branch | 652 / 212 | 6 passed |
| 6 | `env RUST_LOG=off cargo test ... -p p2pcore --release --lib video_call` | branch | 659 / 215 | 44 passed |

Logs: scratchpad `video-sims-*.log` (not in git).

## Deterministic sims: branch == baseline

Every scenario printed identical numbers on both trees. Key rows (from the printed lines):

| Scenario | Wire kbps | Final/peak size | p95 ms | Freezes |
|---|---|---|---|---|
| clean_link | 1760 | 720p | 130 | 0 |
| relay_path | 1335 | 540p | 192 | 0 |
| random_loss_3pct | 1153 | 540p/360p | 116 | 0 |
| bandwidth_drop 2M->300k | 694 | 360p | 167 | 1 (0.3 s) |
| outage_500ms | 1775 | 720p | 132 | 1 (0.7 s) |
| wifi_delay_spikes | 2980 | 720p@24-30 | 40 | 0 |
| wifi_delay_spikes_near_capacity 2.4M / 1.6M | 2110 / 1397 | 720p / 540p | 121 / 111 | 0 / 0 |
| adapt_* (6 scenarios) | 1109-2465 | up to 720p | 100-184 | 0 (flapping_link: 2) |
| lan_delay_bursts (sw / hw) | 1542 / 2085 | 540-720p | 220 / 427 | 30 / 36 |
| lan_delay_bursts_seeds mean sw / hw | 2051 / 2007 | | 311 / 381 | frozen ~19-20 s |
| network_matrix lan / cafe / lte / relay / weak | 1907 / 1828 / 1675 / 406 / 557 | | 333 / 176 / 157 / 568 / 131 | 24.7 / 18.0 / 6.7 / 40.0 / 0 |
| clean_start worst | 540p 2.6 s, 720p 4.4 s (clean); 4.1 / 5.9 s with bursts | | | |
| real_av1_* (3) | 593-2666 | | 1317 (switch) / 160 / 112 | 2 / 1 / 0 |
| slow_receiver, receiver_clock_drift, busy_sender | pass | | 418 / 57 / 72-108 | 0 |

(The lan_delay_bursts and network_matrix freeze counts are long-standing, as designed for those hostile scenarios.)

## Ignored real-network tests (real time, shared worker; noisy)

Real-run p95 ms / frozen ms (>200 ms) / keyframes:

| Test | branch run 1 | branch run 2 | dev |
|---|---|---|---|
| clean | 118 / 0 / 3 FAIL | 124 / 0 / 3 FAIL | 124 / 1500 / 4 FAIL |
| bandwidth_drop | 289 / 1216 / pass | 133 / 948 / pass | 305 / 2578 / FAIL (frozen 2291 ms after drop) |
| loss_3pct | 147 / 859 / FAIL (>3 freezes) | 130 / 0 / pass | 138 / 0 / pass |
| outage_500ms | 124 / 970 / FAIL (p95 125.2 > limit) | 135 / 1020 / FAIL (p95 126.0) | 117 / 1026 / pass |
| relay_path | 193 / 0 / pass | 201 / 0 / pass | 194 / 0 / pass |

Reading: `clean` fails on both trees on the strict assertions (keyframes <= 2 on branch gets 3, dev 3-4;
p95 limit 120 ms, observed 116-124). `outage_500ms` fails on the branch twice by a hair (p95 125-126 ms
vs a threshold near 125) while dev passed once at 117 ms; with n=1 on dev this is within the run-to-run
spread seen elsewhere (loss_3pct and bandwidth_drop flipped between runs on the same tree) and the
video crate diff cannot touch it (fixed_rung is None by default). **Flag, not a confirmed regression**:
if outage_500ms is wanted as a gate, rerun dev a few times to establish its pass rate. The sim twins of
every real test are identical on both trees.

## Limitations

- Real-network tests ran on a worker shared with another agent; load (loadavg ~3) affects timing.
- Baseline net_real run once; branch twice.
- Worker run times in the log are not claims about the product.
