# Video adaptation: variable frame rate, steadier resolution, receiver hints

Status: step 1 (`crates/video`: 2-D ladder, hints in `media`, keyframe fixes, sim) implemented
2026-10-16 on `feature/video-adaptation`, with the deviations in section 10; steps 2-4 open.
Builds on `docs/video.md` (ladder, rate control, transport) and `docs/design/video-hw-vp8.md`
(codec negotiation).

## Problem

- The sender adapts **resolution only** (`media::Ladder`, 180p-720p); the frame rate is fixed
  at 30 (`SenderConfig.fps`, `pipes.rs` `FPS`, MediaCodec `frame-rate`). The only answer to a
  tight link or a slow encoder is a smaller, blurrier picture, and every rung change costs a
  keyframe.
- On a good LAN (Pixel 3a -> laptop, VP8 at ~2 Mbps) the ladder **bounces**: 720p -> 360p ->
  540p -> 720p within a minute, a keyframe each time. Phone <-> phone shows the same ladder.
- The receiver has no say beyond `VideoOffer.max_pixels` (fixed 1280x720 at call start): the
  sender can't know that the picture is drawn in a small window, that the decoder or the UI
  thread can't keep up, or that the phone is throttling.

## Goals

1. Degrade **frame rate before resolution** when bandwidth or CPU is short (user's choice;
   see D1), and come back the same way.
2. **No bouncing**: at most one resolution switch per minute on a stable link.
3. The **receiver coordinates**: display size, decode/display capacity, thermal/battery state,
   sent on a channel the sender already has, so the sender never sends what can't be shown.
4. All policy in Rust (`crates/video`, simulated), the apps only report sizes and states.
5. The chip tells the truth: "HD · 15 fps", "540p · slow network".

Non-goals: simulcast/SVC, audio changes, changing the AV1/VP8 choice mid-call.

## 1. Channel: receiver hints on the video control stream (recommended, D3)

The receiver already opens one bi control stream per direction (`video::net`, version byte
`1`; messages `1` rate report every 50 ms, `2` keyframe request, `3` ack). Add:

- Version byte `2`. Unknown types still end the session (pre-release: both ends update
  together, no compatibility shim; see memory "no back-compat").
- `4 [max_pixels u32][max_fps u8][reason u8]` **hint**, receiver -> sender. Sent once when the
  stream opens and again on every change (reliable stream: no repeats). Fields:
  - `max_pixels`: largest picture worth sending now, from the **drawn size** of the remote
    picture in device pixels (the sender picks the highest rung at or below it); `0` = not
    visible (D4).
  - `max_fps`: what the receiver can decode and draw (30, 24, 15; 0 = no limit), from
    measured decode + render cost and thermal/battery state.
  - `reason`: `0` none, `1` display size, `2` decode/render too slow, `3` thermal, `4` battery
    saver, `5` user (a "data saver" toggle later). For the sender's chip and logs only.
- `VideoOffer` gains `max_fps: u8` (device ceiling, 30 today) so the first frames already
  respect a low-end peer. The ladder cap from `max_pixels` stays as is.

Why not `Msg::Video` on the call's JSON control stream: hints are about the media rate, and
`media::Sender`/`Receiver` must take them directly so the **simulation** (`video::sim`) can
test the whole policy without iroh. One rate loop, one stream, one place to read.

## 2. Sender policy: a 2-D ladder (resolution x frame rate)

Frame-rate tiers: **30, 24, 15** fps (D2: 15 is the floor; below that lip motion breaks and
the queueing-delay window, 100 ms, often sees no frame).

Each resolution rung keeps its 30 fps floor (`min_bps`); at tier `f` the floor is
`min_bps x (0.4 + 0.6 x f/30)` (15 fps -> 70%, 24 fps -> 88%: fewer frames need fewer bits,
but not linearly, since each frame is sharper). Ceilings stay per rung.

Order of steps, **fps first** (D1), as the estimate falls:

```
720p30 -> 720p24 -> 720p15 -> 540p30 -> 540p24 -> 540p15 -> 360p30 -> ... -> 180p15
```

Rules (extend `Ladder`, which becomes `Ladder` over `(rung, tier)`):

- **Down**: as today, at once, to the first step whose floor the estimate covers. A frame-rate
  step needs **no keyframe** (section 3), so most drops are now invisible to the receiver.
- **Up**: one step at a time; the hold time (`up_hold_us`) goes 2.5 s -> **5 s**,
  `up_after_down_us` 4 s -> **10 s**. Stepping up in fps needs only the bitrate; stepping up
  in resolution additionally needs the encode-time prediction (today's p75 rule), computed
  against the **new tier's** frame interval.
- **Flap block**: a step that is undone within 20 s of being taken blocks that step for
  30 s, doubling to 2 min (the CPU block generalized; `cpu_block_*` become `block_*`). That
  alone turns 720->360->540->720 into one visit to 540p and a longer stay.
- **Encode time** (CPU): the budget is 60% of the **current tier's** frame interval, so a
  slow encoder first gets 41 ms (24 fps) then 66 ms (15 fps) per frame before the picture
  shrinks (`ladder_slow_device_stays_low` becomes "stays at 540p15"). The 30-sample window
  becomes time-based (1 s) so 15 fps doesn't take 2 s to react.
- **Caps**: `max_rung = min(offer.max_pixels, hint.max_pixels)`, `max_tier = min(offer.max_fps,
  hint.max_fps)`, plus the sender's own device budget (section 5). A cap change that lowers
  the current step applies at once; raising it goes through the normal up rules.
- `Sender::frame_interval_us()` / `Sender::take_frame(capture_us) -> bool`: the encoder loop
  (`pipes.rs`) skips a captured frame that arrives sooner than the interval since the last
  encoded one. Capture stays at 30 on both platforms (CameraX, v4l2); skipping a latest-wins
  slot is free.

## 3. Encoders at a lower frame rate without a keyframe

Both encoders budget bits per frame as `bitrate / configured fps`. Reconfiguring the fps
re-creates the encoder (a keyframe, what we're trying to avoid), and MediaCodec can't change
`frame-rate` at run time (only the bitrate, `PARAMETER_KEY_VIDEO_BITRATE`). So keep the
encoder at 30 fps and hand it `bitrate x 30 / tier_fps`: the per-frame budget is right, and
the wire rate comes out at the target because only `tier_fps` frames per second are encoded.
`VideoStats.bitrate_bps` keeps reporting the target, not the scaled value.

Verify on the Pixel with the existing self-test (`cmd=mediacodec_vp8`, add `fps=15`): the
measured kbps must track the target within 15% and PSNR must rise versus 30 fps at the same
rate. If a hardware encoder ignores the trick (CBR by wall clock), fall back to reconfiguring
on tier change for that codec (one keyframe, still better than a resolution switch).

## 4. Receiver measurements (what drives the hint)

| source | desktop (iced) | Android | hint |
|---|---|---|---|
| drawn size | bounds of the remote picture widget, physical px | Compose `onSizeChanged` of the remote view | `max_pixels` = next rung at or above the drawn size (never below 320x180); `0` when the picture isn't on screen |
| decode + show cost | `VRENDER cost_ms` (pull + RGBA + handle) + decoder thread time, p75 over 2 s | MediaCodec/rav1d decode time + JNI pull, p75 | > 60% of the current frame interval -> `max_fps` one tier down; < 30% for 10 s -> one tier up |
| thermal | none (laptop fans) | `PowerManager.thermal status` >= MODERATE -> 15 fps + 540p; SEVERE -> 15 fps + 360p | `reason` 3 |
| battery saver | none | `isPowerSaveMode` -> 24 fps + 540p | `reason` 4 |

The apps call one core API, `set_video_view(width_px, height_px, visible)` and
`set_video_budget(VideoBudget { thermal: u8, power_save: bool })`; the core turns them into
hints (hysteresis, tier rounding) and sends them. Rust-first: no policy in Kotlin or iced.

The same `VideoBudget` also caps the **sender's own** ladder (a hot phone sends less too),
with no message needed.

## 5. Visible state

`VideoStats` gains `send_fps`, `recv_fps` (measured over 1 s), `send_cap: {pixels, fps,
reason}` (what the peer's hint and our budget allow), and `ladder_steps` with the last
reason (`rate`, `cpu`, `hint`, `budget`). The chip and its label read from this (fits the
pending "honest HD chip" item): "HD · 15 fps" while sending 720p15; "540p · slow network";
"540p · their screen" when the peer's hint caps us. Debug log line on every step:
`video step 720p30 -> 720p15 reason=rate est=820kbps`.

## 6. The bounce: diagnose before tuning (step 0)

The real-call bounce has two candidate causes; the log tells which, so check first:

- (a) **Starvation false positive**: a Wi-Fi power-save stall of ~100 ms on the phone's radio
  reads as two starved reports -> 25% drain, estimate = 85% of a tiny delivered -> 360p.
  Visible as `rate_starved` ticking up at each step down in the desktop log.
- (b) **Keyframe perturbation**: the step-up keyframe at 720p (hardware VP8 overshoots:
  1.64 Mbps measured for a 1 Mbps target) fills the queue -> `queuing > 40 ms` -> overuse cut
  -> step down -> repeat. Visible as `rate_overuse` ticking with `last_queuing_us` > 40 000
  right after a `keyframes_sent` increase.

Either way the flap block (section 2) stops the loop; (a) additionally wants the starvation
check to need a third report when the previous report delivered normally; (b) wants the hw
encoder's keyframe budget capped (MediaCodec has no direct knob: request the keyframe at a
lower bitrate parameter and restore it a frame later).

**Step 0 result: (b).** On the Pixel `rate_starved` stays ~0, while every step up moves
`rate_overuse` 11 -> 24 within a second, `keyframes_sent` by 2-5 (8-12 KB each against 2.5 KB
deltas at 720p) and the step down follows 0.8 s later. Fixed in `media::Sender` (step 1):

- **One keyframe per switch.** `wants_keyframe()` stays true until a keyframe comes out of the
  encoder, and MediaCodec's output lags its input by a few frames, so the encoder loop asked
  (`request-sync`) again with each of them. `Sender::poll_keyframe(now)` asks once and not again
  for 500 ms unless the keyframe never came; a receiver/transport request within 250 ms of a
  sent keyframe is for that keyframe and is ignored (`KEY_DEDUPE_US`; this layer has no RTT,
  500 ms swallowed a real request in `net_keyframe_request_starts_group`).
- **Overuse grace.** For a second after a keyframe the queueing its own burst explains (its
  size at our rate, at most 200 ms, falling linearly to 0 over the second) is not overuse.
  A flat allowance hid a standing queue for the whole second (p95 277 ms after a 720p step).
- **Keyframes sized for 30 fps.** `encoder_bitrate()` (section 3) is the unscaled target while
  a keyframe is due: encoders size it from the per-frame budget, and at 24 fps the 720p
  keyframe came out ~55 KB (220 ms on 2 Mbps) instead of ~26 KB.
- **Flap block** as designed, plus: a step down within 1 s of a starved report is a link that
  got slower, not a flap (section 10).

Sim (`adapt_hw_keyframes`: `sim::HwCodec`, output 2 frames late, every requested input a
keyframe 4x the mean delta; fast encoder, 1.8 Mbps, 5 ms +-5 ms): before 30 keyframes (20
"other": asked again while the first was in the encoder), 720p at 7.5 s -> 180p 0.6 s later,
6 resolution switches after 10 s, p95 193 ms; after 3 keyframes (start + 2 steps), 360p ->
540p -> 720p at 12 s and stays, 0 switches down, p95 176 ms (p50 ~120 ms is the codec's 2-frame
lag).

## 7. Acceptance tests

Deterministic, in `crates/video` (sim, `--release`), unless noted. Baselines first: run each
scenario on today's code and record shown/latency/switches, so the change is measured.

1. **fps before resolution**: 2 Mbps -> 700 kbps at 720p30. Expect 720p24 or 720p15 within
   the second look, resolution >= 540p for the whole run, latency p95 and freezes no worse
   than today's baseline, 0 extra keyframes for fps steps.
2. **no bouncing**: 1.5 Mbps with 3% random loss for 60 s. Expect <= 1 resolution switch
   after the first 10 s (today's baseline: measure; the real call showed 3 in a minute).
3. **drawn-size hint**: hint `max_pixels = 640x360` at t=5 s, lifted at t=25 s. Expect no
   frame above 360p from the next keyframe on; after the lift, one rung up at a time with the
   5 s hold, 720p reached by t=45 s on a 3 Mbps link.
4. **fps hint**: hint `max_fps = 15` on a 3 Mbps link. Expect <= 16 frames/s sent within
   200 ms, wire rate <= target, 0 keyframes caused by the hint.
5. **slow encoder, fps first**: the `ladder_slow_device_stays_low` scenario. Expect it to hold
   the higher resolution at a lower tier instead of a lower resolution at 30 fps.
6. **flap block**: a link that oscillates 2 Mbps / 900 kbps every 8 s. Expect <= 2 resolution
   switches per minute after the first block.
7. **round trip** (`crates/core/tests/video_calls.rs`, two iroh nodes): receiver sets
   `set_video_view(320, 180, true)` then `set_video_budget(thermal MODERATE)`; the sender's
   `VideoStats.send_cap` shows it within 1 s and `send_fps` <= 16 within 2 s.
8. **Pixel self-test** (device, manual): `mediacodec_vp8 fps=15 bps=1000000`: measured kbps
   within 15% of target, PSNR-Y higher than at 30 fps.
9. **Real call** (device, manual, after merge): Pixel -> laptop HD for 2 min on the LAN:
   <= 2 resolution switches after the first 10 s in the log; the chip shows the fps.

### Results (step 1, `cargo test -p video --release sim -- --nocapture`)

Baseline = `feature/hw-vp8` controller on the same sim (codec trait change only; its numbers
match the V11 table in `docs/video.md`). Latency p50 / p95 / max ms over the run, frozen
(> 250 ms gaps), keyframes (rate / cpu / other causes), resolution sizes over time. New
scenarios use a fast encoder (720p in ~9 ms) unless noted; old ones the phone model.

| Scenario | baseline | step 1 |
|---|---|---|
| 1. 2 M -> 700 k at 20 s | 62/116/420, 0 s, keys 4; 360 -> 540 (3.6) -> 720 (6.9) -> **360p** (20.3) | 67/108/348, 0 s, keys 4; 360 -> 540 (6.1) -> 720 (11.9) -> **540p24** (20.3) |
| 1b. 2 M -> 1 M at 20 s (new) | - | 64/120/285, 0 s; 720p30 -> **720p15** at 20.4 s (no keyframe), 540p30 at 24.2 s (open issue 2) |
| 2. 1.5 M, 3% loss, 60 s | 56/119/200, keys 4; 540 (5.4) -> 720 (39.8) -> 540 (40.1): **2 after 10 s** | 57/120/206, keys 2; 540p at 6.3 s, stays: **0 after 10 s** |
| 3. size hint 640x360 5-25 s, 3 M | - (no hints) | 44/60/138; 360p while hinted, 540p 30.0 s, 720p 35.9 s; 0 hint keyframes |
| 4. fps hint 15 at 15 s, 3 M | - (no hints) | 66/90/138; <= 16 frames/s from 200 ms after, wire <= target, 0 keyframes, 720p kept |
| 5. slow device (unpinned) | 56/63/72, **360p30** throughout | 77/112/155; **540p15** at 6.1 s, **540p24** from 12.7 s, 1 switch |
| 6. 2 M / 900 k every 8 s, 80 s | 64/134/294, keys 12; **7 switches in 20-80 s** | 68/122/261, keys 4; 720 (21.6) -> 540 (25.4), then 540p at 15-24 fps: **1 in 20-80 s** |
| hw keyframes (section 6), 1.8 M | 110/193/554, keys 30 (other 20); 6 after 10 s | 121/176/210, keys 3 (other 0); 720p from 12 s, 0 down |
| clean 2 M | 68/105/146, keys 2; 540p | 70/108/158, keys 3; 540p30 -> **720p24** (12.0) |
| 3% loss, 2 M | 61/125/245; 540p | 62/131/234; 540p, 24 fps from 25 s |
| 2 M -> 300 k | 61/110/610; 540 -> 180 (10.3) | 59/111/638; 540 -> 360p24 (10.3) -> 360p15 -> 180p (13.9) |
| 500 ms outage | 66/103/170, 0.6 s | 68/109/181, 0.6 s; 720p24 from 16.4 s |
| relay 75 +-20 ms, 1.5 M | 133/183/238 | 134/197/256 |
| 2 M -> 200 k -> 2 M, 40 s | 57/99/963, 0.3 s; 540 (26.6 s) | 55/127/1006, 0.3 s; 360 (25.9) -> 540 (31.8) -> 720p24 (37.6, ~190 ms for 1 s) |
| busy desktop, 50 s | 69/102/146, keys 3; 720p30 (7.8) | 70/108/208, keys 3; 720p30 (12.0) |
| throttles 2x at 15 s | 58/81/182; 540 -> **360p** (15.7) | 69/95/158; 720p24 -> **720p15** (15.5, no keyframe) -> 540p (16.6) |
| slow start | 70/106/143; 540p (4.2) | 71/112/164; 540p (6.8) -> 720p24 (12.7) |
| receiver clock 0.5% fast | 48/87/140, 958 kbps (30 s) | 46/72/138, 940 kbps (40 s run: the climb back from 180p is slower) |
| slow receiver (25 fps decode 5-15 s) | 54/395/475, backlog 3 | 78/213/333, backlog 0; 540p15 6.8 s -> 540p24 16.8 s |
| real AV1, 2% loss + outage | 41/91/178, 0.7 s; 360 -> 540 -> 180 -> 360 | 50/130/224, 0.6 s; 360p throughout |
| real AV1, 2 M -> 200 k -> 2 M | 49/1459/2017, 1.7 s | 44/1317/1758, 1.1 s |

Fewer frames are shown where the tier is lower (that is the point); "freezes" count gaps over
250 ms, which 15 fps (67 ms) never makes on its own. Costs: the phone model now takes 720p24
where it stayed at 540p30 (its 720p encode time fits 24 fps's budget, not 30's): p95 +3-6 ms on
a clean link and a ~190 ms bump for a second when it steps up near a link's capacity; the
climb back after a drop is slower (10 s after a step down, 5 s holds).

## 8. Decisions

**Decided 2026-10-16 (user took the recommendations): D1 fps first, D2 floor 15 fps, D3 hint
message on the video control stream, D4 keep sending 180p15 while hidden, D5 bitrate-scaling
trick, confirmed by the Pixel self-test.** The options are kept below for the record.

- **D1 order of degradation**: fps first (recommended, asked for) vs balanced (alternate fps
  and resolution steps). Config knob `Degrade::{FpsFirst, Balanced}` either way, default
  per the decision.
- **D2 fps floor**: 15 (recommended) vs 10. 10 saves ~15% more bits at the bottom rung at
  the cost of visibly stuttering motion.
- **D3 hint channel**: video control stream message `4` (recommended) vs `Msg::Video` JSON.
- **D4 not visible** (`max_pixels = 0`, the remote picture hidden or app in background):
  keep sending 180p15 so the picture is instant when it returns (recommended, ~150 kbps), or
  stop the camera for the peer (0 kbps, a keyframe and ~300 ms when it returns).
- **D5 hardware encoder at low fps**: bitrate-scaling trick (recommended, no keyframe) vs
  reconfigure per tier; the Pixel self-test (test 8) settles it.

## 9. Plan (Sonnet helpers, one per step, buildlock `--who vfr-<step>`; lead merges)

| step | what | owner | est. |
|---|---|---|---|
| 0 | diagnose the bounce from the existing log counters (section 6); 1-page note | Sonnet | 30 min |
| 1 | `media`: tiers, 2-D ladder, flap block, time-based encode window, hints in `Receiver`/`Sender`, sim scenarios 1-6 | Sonnet | 3 h |
| 2 | `net` v2 hint message; `pipes.rs` frame skipping + bitrate scaling; `VideoOffer.max_fps`; `VideoStats` fields; `set_video_view`/`set_video_budget`; test 7 | Sonnet | 2 h |
| 3 | desktop: widget size -> `set_video_view`, chip text; Android: view size, thermal, power save -> core; chip text | Sonnet | 2 h |
| 4 | Pixel self-test at 15 fps (test 8), then the real call (test 9) | lead + user | 30 min |

Branch: `feature/video-adaptation` off `feature/hw-vp8` (it needs the VP8 encoder path).
Merge order: hw-vp8 to dev first (pending the user's OK), then this.

## 10. Step 1: where the implementation differs from sections 2-3, and why

1. **A call starts at 30 fps** (the highest rung whose 30 fps floor the start rate covers:
   360p30 at 500 kbps, as before), not at the first step the start rate covers (540p15). The
   start isn't a step down, and starting every call at 15 fps cost a second of motion.
2. **A new size needs its bitrate at the current frame rate.** Going down is fps first;
   going up, 540p30 -> 720p needs 720p30's 1.3 Mbps, not 720p15's 910 kbps. Measured: landing
   at 720p15 from 540p30 at ~1 Mbps fell back to 540p within 3 s under 3% loss (every frame, and
   the keyframe, is twice the size at 15 fps and the delay signal is noisier), 6 switches after
   10 s instead of 0. The two directions now have a hysteresis band instead of a bounce.
3. **A resolution step up lands at the highest tier the bitrate and the encoder allow** (one
   rung at a time), not always at the new rung's lowest tier: 360p30 -> 540p30, not 540p15.
   Only a slow encoder lands lower (the slow device: 360p30 -> 540p15; D1: size before fps).
4. **Every step up checks the encode-time prediction**, fps steps too (against the new tier's
   budget, pixel ratio 1). Without it 540p24 -> 540p30 on the slow device would step straight
   back down for CPU and flap on the CPU block.
5. **CPU and flap blocks are one mechanism (`Backoff`) with two settings**: CPU keeps 5 s
   doubling to 30 s, forgotten after 60 s (its tests and the busy-desktop results depend on
   it); flap is 30 s doubling to 2 min, forgotten after 5 min. A step down for CPU blocks the
   step left and all above; it doesn't count as a flap. A step down for a cap (hint) is
   neither, and doesn't start the 10 s wait.
6. **Flap block scope**: the lowest step up taken in the last 20 s that a step down undoes;
   if that step changed the size and the step down leaves the rung, the whole rung is blocked
   (landing at 720p24 inside a 720p30 block is the same keyframe again). **A step down within
   1 s of a starved report is not a flap** (`Ladder::real_drop`): the link really got slower
   (2 M -> 200 k 4 s after a step up blocked 540p for 30 s, recovery at ~45 s instead of ~32 s).
   Step 0 found the bounce is overuse after keyframes, not starvation, so this keeps the block
   for the bounce.
7. **The CPU step down goes to the highest lower step whose predicted encode time fits its
   budget** (one or more steps, fps first), not always one rung.
8. **Keyframe bitrate unscaled, overuse allowance decays, dedupe 250 ms**: section 6.
9. **Tried and dropped**: scaling `overuse_us` / `underuse_us` by 30 / tier (to absorb the
   bigger frames' delay spread at 15 fps). It kept 1 Mbps at 720p15 but raised p95 by 20-50 ms
   in most scenarios (clean 141, throttled 151, flapping link 178).

**Acceptance 1 as written can't hold at 700 kbps**: the estimate is 85% of what's delivered
(~600 kbps), below 720p15's 700 kbps floor, so the step is 720p30 -> 540p24 (one keyframe);
"resolution >= 540p" and "p95 no worse" hold. 1b (1 Mbps) shows the 720p15 step.

Open issues:

1. The phone model goes to 720p24 on a clean 2 Mbps link (point 3 + D1). If the user prefers
   540p30 there, a knob that keeps a step up at the current tier for CPU as well (as for the
   bitrate) gives the old behaviour; the slow device would then stay at 360p30.
2. At 15 fps on a link near the rate (720p15 on 1 Mbps) the estimate drifts down (delay signal
   noise from ~50 ms frames); 540p30 4 s later. Wants a receiver-side fix (queueing over the
   last N frames, or per-packet arrival timing), not thresholds.
3. 180p has no floor, so the bottom rung never drops frame rate for the bitrate (only for caps
   / hidden).
4. `poll_keyframe` and `encoder_bitrate` are used by `pipes.rs` and the sim; `mediacodec.rs`'s
   self-test has its own loop (step 4, `fps=15`). The `net` v2 message, `VideoOffer.max_fps`,
   `VideoStats` and the app APIs are steps 2-3; `Ladder::take_steps` is there for the
   `video step` log line.
