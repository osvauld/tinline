# Video rate control: simulation-first tuning (2026-10-19)

Branch `feature/video-sim-tuning`. Goal: video quality closer to WhatsApp with royalty-free codecs
only (VP8, VP9, AV1). Rate control first, because the 2026-10-19 calls showed that the codec was
not what held the picture at 180p.

## Method

1. Fit a sim scenario to each real call, from its logcat `video send` lines (one a second: size,
   fps, encoder rate, overuse count, the last report's queueing). A scenario counts as fitted when
   the **current** controller does in the sim what it did in the call.
2. Change the controller behind a switch (`SenderConfig::detector`), run every scenario under both
   switches (the scoreboard), keep a change only if it fixes the real-call scenarios without
   making the existing ones worse.
3. Check on devices (Pixel 8 / Pixel 3a / moto g52), and calibrate the sim's device models from
   on-device codec timings.

Run: `cargo test -p video --release --lib real_calls -- --nocapture --test-threads=1`
(`SIM_SECONDS=1` per-second lines in the log's shape, `SIM_STEPS=1` ladder steps,
`SIM_DETECTOR=trend` the other detector, `SIM_MOBILE=walk|spikes`, `SIM_SEED=<n>`).
Scenarios: `crates/video/src/sim/tests/real_calls.rs`.

## Real calls fitted

### moto g52 -> phone on mobile data, direct (2026-10-19 21:52-21:57)

Real: 180x320 at 25-30 fps, ~100 kbps for minutes after the receiver's hidden-picture cap went.
The last report's queueing once a second: p25 13, p50 27, p75 58, p90 154 ms, max 987;
3-5 reports a second over the 40 ms overuse threshold.

Sim (`real_moto_to_mobile`, 8 Mbps path, picture hidden until 30 s):

| Downlink delay model | After the cap goes | Queue p25/50/75/90 ms | Overuse reports/s |
|---|---|---|---|
| `spikes` (stall then burst, in order) | back to 360-720p in 6-15 s, 1.1-1.6 Mbps | 7/12/33-86/92-191 | 6-7 |
| `walk` (`Link::delay_walk`: smooth rise and fall, no stall) | **stays at 180p, 86-131 kbps** | 13-21/29-39/52-63/80-96 | 7-10 |

Only the smooth walk reproduces the call. The existing spike rule (`stall_was_spike`) already
tells a stall-then-burst from a queue; a delay that rises without a stall reads as our own queue
every time it crosses 40 ms, and the ramp-up needs the queue under 15 ms, which this path seldom
gives. So the fixed thresholds, not the codec or the path's capacity, held the call at 180p.

### moto g52 -> Pixel 8 over a relay (2026-10-19 22:04)

Real: 360x480 at 22-25 fps, 1.2 Mbps (the 360p ceiling), queue p90 10 ms; `starved` dips about
twice a minute; at 24 s `360p30 -> 540p15 est=1200`, 1.5 Mbps, one report at 63 ms, then
`540p15 -> 360p24 est=221kbps` and 300-480 kbps after.

Sim (`real_moto_relay`, 10 Mbps relay, 60 ms one way, 0.5-0.8 Mbps dips of 200-400 ms, 50-90 ms
hiccups every 5-15 s): 360p30 at 1.2 Mbps like the call; dips and hiccups cut the rate to
25-500 kbps for 1-2 s (the call's deep cut, which is what matters). It does not step up to 540p
as the call did: the sim's moto encode model and the 360p ceiling keep it at 360p30. Not fitted
further; the target (`never under 500 kbps or 360p after 10 s`) tests the deep cut.

Found on the way: on a thin link at 15 fps a frame's own serialisation (its bytes at the link
rate, ~19 ms for 3.7 KB at 1.6 Mbps) reads as queueing over `underuse_us` (15 ms), so the rate
never ramps. A delay-trend detector does not see a constant delay.

## Trend detector (`crate::detect`)

`Detector::Trend` is GCC's delay-based detector (libwebrtc `TrendlineEstimator`): a least-squares
slope of the smoothed queueing delay over the last 20 reports (1 s), scaled by
min(samples, 60) x 4, against a threshold that adapts (up 0.0087/ms, down 0.039/ms, 6-600,
ignoring jumps 15 over it). Overuse once over it for 10 ms on two reports with the trend not
falling. Hard limits around it: over 250 ms (plus the keyframe allowance) is overuse whatever the
trend; ramp-up needs no overuse and under 80 ms.

What it took on top of the bare detector (each found with `SIM_TRACE=reports` and a
temporary per-decision log; all are in `media.rs`, `TREND_*`):

1. **Ramp-up only after the path was empty recently, and a queue standing 3 s is overuse.** The
   trend doesn't see a flat queue: a link we fill at its rate held 100-300 ms.
2. **Slower climb with a queue** (+3% per 100 ms over 15 ms, +1% over 40 ms), and near the rate the
   last overuse began at (±15%, for 10 s, at least a packet per response time: GCC's
   near-convergence increase). The trend shows a queue ~400 ms after it starts; at +8% per
   100 ms the rate went 547 -> 826 kbps on a 650 kbps link in that time (a 350-870 kbps sawtooth).
   Only after an overuse where the path delivered 10% short of what we sent: the mobile walk's
   every bump held the climb down otherwise.
3. **Under 15 ms is never overuse and always an empty path.** The regression holds a rise for up
   to a second after its queue is gone: after a relay dip, a second at 110 kbps with nothing
   queued, and the ladder down to 180p.
4. **Episodes and drains end only when the queue is gone (<15 ms)**, not on the laxer ramp-up
   rule: a relay dip's queue built on to 170 ms before the next cut.
5. **The second look keeps the cut while a queue stands (>40 ms)**: the trend says no overuse at
   the top of a queue that stopped growing.

### `smooth_cut` (`SenderConfig::smooth_cut`, either detector)

The first cut, and the drain, size from the last 500 ms's mean delivered rate as well as the last
report; the second look's capacity median uses only reports where the link set the delivered rate
(10% under or over what we sent). One 100 ms report with nothing in it (a 50-90 ms relay hiccup)
took 680 kbps to 100, and a drain to 25 kbps.

### Capacity probing (`SenderConfig::burst_room`, `ReceiverConfig::report_burst`)

Delay alone can't tell the mobile walk from a queue of ours: a downlink scheduler holding
packets looks, report by report, like a bottleneck we fill (delivered falls short while the delay
rises, then overshoots as it falls). The path's capacity tells them apart: a queue of ours at
120 kbps needs a 120 kbps link, and the walk's path carries 8 Mbps.

- The receiver times every frame of three packets or more, first read to last
  (`RateReport::burst_bps`, the median over 3 s). It is a lower bound on the capacity: the
  transport paces a frame out at 1.25 x cwnd / RTT, and an app-limited cwnd grows only while
  half of it is used, so on a roomy path a burst comes in at ~2.5x our rate, and on a link we
  fill at the link's rate (~1x).
- At 150 kbps no frame spans three packets: then, every 2 s, the sender appends a 4.8 KB padding
  record (`PAD_STAMP`) after a frame, which the receiver times and drops. It counts as sent and
  delivered, like video (left out of delivered, a probe filling a 600 kbps link for 64 ms read
  as the link falling short).
- While bursts come in at 2x our rate or more, delay is the path's: overuse only over
  `TREND_HARD_US` (250 ms), ramp-up under `TREND_LOW_US` (80 ms).
- Wire: a report with a burst rate is message 6 (17 bytes), else 1 as before; a probe is a record
  in the group's stream. Both off by default (`report_burst`, `burst_room`): a receiver from
  before this would decode a probe, a sender from before it refuses message 6. The sender probes
  only once the receiver has reported a burst rate (its first keyframe gives one).

### Scoreboard (8 seeds, means; `SIM_SEEDS=8`)

kbps / % of frames >= 360p / p95 latency ms. `c+b`: `smooth_cut` and `burst_room`. Every row and
column, with 540p, freezes and keyframes: `docs/design/video-scoreboard-baseline.txt` (compare a
run with `SIM_SEEDS=8 SIM_SCOREBOARD_BASE=<that file>`). pixel and busy_sender: the same under
all.

| Scenario | fixed | fixed+cut | trend+cut | fixed+c+b | trend+c+b |
|---|---|---|---|---|---|
| lan | 2025 / 95% / 317 | 2136 / 97% / 324 | 2080 / 100% / 335 | 2498 / 100% / 352 | 2358 / 100% / 356 |
| cafe | 1847 / 100% / 208 | 1890 / 100% / 178 | 2234 / 100% / 211 | 1851 / 100% / 303 | 2284 / 100% / 296 |
| lte | 1670 / 99% / 166 | 1783 / 100% / 183 | 2061 / 100% / 258 | 1831 / 100% / 190 | 2011 / 99% / 235 |
| relay | 440 / 34% / 574 | 474 / 77% / 657 | 467 / 57% / 649 | 501 / 74% / 635 | 467 / 51% / 680 |
| weak | 584 / 98% / 125 | 604 / 98% / 132 | 612 / 95% / 156 | 598 / 95% / 145 | 610 / 97% / 182 |
| step | 709 / 100% / 127 | 714 / 100% / 126 | 716 / 100% / 173 | 713 / 100% / 136 | 718 / 100% / 172 |
| step+stall | 718 / 100% / 119 | 721 / 100% / 118 | 726 / 100% / 151 | 720 / 100% / 133 | 721 / 100% / 172 |
| pixel busy | 1549 / 100% / 201 | 1724 / 100% / 205 | 2055 / 100% / 209 | 1805 / 100% / 206 | 2055 / 100% / 209 |
| mobile spikes | 1402 / 99% / 316 | 1612 / 99% / 339 | 2013 / 100% / 429 | 1958 / 100% / 420 | 2085 / 100% / 430 |
| mobile walk | 112 / 0% / 214 | 124 / 0% / 213 | 348 / 33% / 221 | 855 / 80% / 247 | 1008 / 83% / 261 |
| moto relay | 1166 / 100% / 174 | 1131 / 93% / 173 | 1185 / 100% / 175 | 1159 / 93% / 186 | 1007 / 79% / 184 |

Reading it:

- **`smooth_cut` is a safe win**: the thin relay path 34% -> 77% at 360p, mobile spikes
  1402 -> 1612 kbps (540p 71% -> 94%), at today's delay. Moto relay 100% -> 93% (one seed in
  eight drops to 180p in a dip).
- **Probing fixes the call that failed**: the mobile walk 112 kbps / 0% -> 855-1008 kbps /
  80-83% at 360p, mobile spikes to ~2 Mbps; p95 +10-30 ms on thin links with `fixed`.
- **The trend detector** adds 30-60 ms of p95 on thin and jittery links (lte, weak, step) for
  more rate and 540p; with probing it adds little over `fixed+c+b`.
- **Probing is fragile in the sim**: the burst rate when the pacer is the limit (~2.5x) is close
  to the 2x rule, so small changes move results (between the last two runs, a change to when
  probing may start took trend+c+b on moto relay from 100% to 79% at 360p). It must be checked
  on phones, where quinn's pacer and Wi-Fi/LTE aggregation set the burst rate, not this model.
  Wi-Fi aggregation can make bursts read fast on a slow-sustained link: the hard 250 ms limit
  is what bounds the harm.

Real-call targets (`--ignored`, they fail under today's controller):
`cargo test -p video --release --lib real_calls -- --ignored --nocapture --test-threads=1`,
with `SIM_CUT=1 SIM_PROBE=1` (and `SIM_DETECTOR=trend`): **the relay call passes** under both
detectors (360p throughout, the lowest 3 s mean 509-1039 kbps, ~1.15 Mbps mean); **the walk does
not yet**: 43-92% at 360p per seed with `fixed`, 66-74% with `trend` (target: 80% on each; the
spikes model is at 100%).

## Devices

### Pixel 3a (sargo, Android 12, SP2A.220505.008), 2026-10-20

From `/vendor/etc/media_codecs*.xml` and `/apex/com.android.media.swcodec` (read only).
Frame rates are the vendor's measured ones (`media_codecs_performance.xml`, throughput).

| Codec | Encode | Decode |
|---|---|---|
| VP8 | **hardware** `c2.qti.vp8.encoder`: 328 fps 180p, 188 fps 360p, 51 fps 720p, 45 fps 1080p; software `c2.android.vp8.encoder`: 275 / 120 / 35 / 15 | **hardware** `c2.qti.vp8.decoder`: 340 fps up to 720p, 199 fps 1080p; software: 562 / 256 / - / 30 |
| VP9 | software only (`c2.android.vp9.encoder`, at most 1280x720; no measured rate) | **hardware** `c2.qti.vp9.decoder`: 336 fps 720p, 199 fps 1080p; software: 483 / 143 / 78 / 47 |
| AV1 | none | software only (`c2.android.av1.decoder`, at most 2048x2048; no measured rate) |

As a `sim::Device`, hardware VP8 encode is ~1 ms + 20 ns/px (5.3 ms at 360p, 19.6 ms at
720p: 720p30 uses ~60% of the frame budget). The sim's Pixel uses `fast()` (no encode cost);
these are throughput, not our pipeline's per-frame latency (the app's `encode` stat in the
call overlay has that; it is not in the per-second log line yet).

So on this phone the royalty-free choice is VP8 in hardware both ways; VP9 only as a receive
codec (hardware decode, software encode capped at 720p); AV1 receive only, in software.

## Phones: Cut+probe is the default (2026-10-20)

A/B on the relay path (moto g52 <-> Pixel 3a): on "Cut" the moto's camera video sat at 360p6, 25-150 kbps (delay with no queue behind it
read as overuse); with Cut+probe on both phones it ran 540p, 0.6-2 Mbps, 15-24 fps, and the
Pixel stayed at its top step (3 Mbps). So the apps' "auto" (and an empty tuning) is now
Cut+probe (`VideoTuning::configure_rate`); the tuning sheet keeps "Plain (old)", "Cut" and
"Trend+probe" for A/B calls. The probe still needs the peer's offer to say `burst_probe`. The
sim's `SenderConfig::default()` is unchanged (its tests and the scoreboard's `fixed` row are the
old controller; Cut+probe is the `c+b` variant).

### Software VP8 input layouts (2026-10-20)

Google's `c2.android.vp8.encoder` hands out ByteBuffer input with a flexible `color-format`, no
`image-data` (before or after start, nor per buffer), and a kind in `android._color-format`
that is not what it reads. Writing NV12 there gave a grey picture with colour lines (moto
sending VP8). Found with the self-test's chroma PSNR (`mediacodec_vp8_selftest`, DebugReceiver
`mediacodec_vp8 --ez sw true`), fixed in `mediacodec.rs` (`Layout::of`, `or_nv21`):

| Phone | Names | Reads | Encode p50 480x640 / 540x960 / 720x1280 |
|---|---|---|---|
| Pixel 3a | 19 (I420), stride 512 | NV21 (I420 does not fit its 491489-byte buffer) | 4.0 / 4.8 / 5.4 ms |
| moto g52 | YV12 | plain I420 (U first, chroma rows half the stride) | 4.1 / 4.3 / 7.7 ms |

Chroma PSNR after the fix: 45-49 dB at every size on both (22 dB with U/V swapped). The moto's
software VP8 is ~7x cheaper than its software AV1 (27-30 ms at 480x640): VP8 could be its
default send codec (not decided).

## Next

- Phones: the probe is idle on the moto side (burst=0 there: the Pixel reads the moto's frames
  in one go): time bursts at packet arrival.
- Scoreboard (`sim/tests/scoreboard.rs`, `--ignored`): every scenario x variant in one table;
  the 3-seed `fixed` baseline is in `docs/design/video-scoreboard-baseline.txt`.
- Codecs and paths in the scoreboard: hardware VP8, software VP8 (libvpx), software AV1; direct
  vs relay.
- Device calibration (Pixel plugged in): MediaCodec inventory (VP8/VP9/AV1, encode/decode,
  hardware or software) and per-size encode/decode timings, into `sim::Device` presets.
