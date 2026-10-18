# Developer video tuning (phase 1)

Implementation on `explore/codec-negotiation`. This is measurement tooling, **not**
a new automatic codec rule or evidence that one codec gives a better picture.

## Controls

- Android debug builds: Settings → Diagnostics → Video tuning.
- Desktop debug builds (`debug_assertions`): Settings → Video → Video tuning.
- Release UI has no controls or debug readout. The shared Rust API is present with
  inert defaults; Android's native Rust release profile does not determine whether
  the Android UI is a debug build.
- Session-only, not saved to disk or synced. Recreating the Node resets settings.
- Send codec: Auto / AV1 / VP8, with only locally supported encoders listed. Rust
  rejects unsupported codecs and unknown parameter values. Receiving capabilities
  remain unchanged, so forcing one direction does not force the reverse direction.
- Bitrate cap: Auto / 0.5 / 1 / 2 / 3 Mbps. This is **not a fixed or guaranteed rate**:
  the network controller still backs off and drains queues. With an explicit cap,
  it replaces the normal per-rung bitrate ceiling too, so both codecs can be tested
  with the same ceiling at a small resolution.
- Fixed rung: Auto / 360 / 540 / 720p. This intentionally bypasses bitrate- and
  encode-time-driven ladder steps. It uses the highest frame-rate tier the peer and
  device allow. Peer offer/hint limits, hidden-picture hints, thermal and power-save
  caps still win; lifting a cap restores the requested test rung immediately.
  Normal adaptive hysteresis is deliberately not used in fixed test mode.
- Capture still keeps its aspect ratio and never upscales. Turn on HD capture and
  check the **actual** dimensions: selecting 720p does not create a 720p camera.
- Controls are snapshotted into each call when our offer is constructed (caller:
  before dialling; callee: on answer). Later settings changes cannot mutate that
  offer or its sender. They apply when a future offer is constructed, not mid-call.

## Negotiation and unsupported peers

The send offer's `encode` and `hw_encode` lists are filtered to the chosen codec.
Auto uses exactly the existing hardware score and sender-order tie break. The
wire shape does not change, but a forced-VP8 offer may omit AV1 from `encode`.

Both updated peers use `proto::video_codecs` to validate both directional choices
against the actual offered lists before starting video. No common codec means
**audio only**, with an explicit developer diagnostic, never silent AV1 sending
in place of a forced VP8 test. Unsupported local codec requests fail immediately
without changing the prior settings.

Use matching updated builds for tuning. Older peers assume AV1 is always offered
and can fall back to it for an unsupported forced VP8 offer; this experiment does
not introduce a compatibility protocol to repair those peers' assumptions.
Normal Auto offers keep AV1 and retain the existing behavior.

## Readout

Android debug calls show an overlay; desktop debug calls show it in **About this
call**. Android Diagnostics also shows it while a call exists. Both use the Rust
`video_tuning_stats_label` formatter, refreshed roughly once per second:

- Send/receive codec and hardware/software, actual dimensions, admitted/decoded fps.
- Measured admitted and received **payload** kbps, distinct from the sender's target.
  Admitted bytes are not QUIC transmitted or acknowledged bytes; received bytes are
  complete payloads before decode. Neither includes transport overhead.
- Last encode wall time and two-second decode p75 wall time. For hardware codecs,
  wall time includes pipeline latency and must not be described as CPU cost.
- Current ladder rung, step count/reason and this call's test size/bitrate cap.

Rates before a complete initial measurement interval are zero, not a validated
throughput measurement. Decoded fps is not rendered/displayed fps; no live
sharpness score, freeze detector or end-to-end delay measurement is added here.

## A/B procedure and limits

Use the same scene, camera, lighting, motion, capture setting and both devices.
Start with 360p and the same bitrate cap, run AV1 then VP8 in separate calls.
Record actual dimensions, admitted/received payload rate, fps and timing in both
directions. Reject a matched comparison if size, sustained fps or actual bitrate
is materially different, or the peer/budget caps prevent the requested size.
Fixed mode may deliberately expose a device's inability to keep up; it is not a
production quality policy. Phone use/install requires separate user permission.

Next phases remain separate: matched-clip quality measurement, device capability
benchmarks, a quality-first start rule, and a negotiated live codec-switch protocol.
No SSIM self-decode, automatic trials or live codec switching is implemented.

## Validation

Deterministic tests cover filtered directional offers, unsupported peers/local
encoders, validation, offer/call setting snapshots, readout rate semantics, and
fixed-rung behavior under CPU/rate pressure, peer hints and device caps. Defaults
continue through the original adaptive path. Device validation has not been performed.

Results after merging `dev` (2026-10-18, all through `tasks`, `-j 2`, named targets):

| Check | Job | Result |
|---|---|---|
| `cargo test -p p2pcore --lib video_call` (incl. `tuning`, `hd`, `adapt_policy`) | 585 | 44 passed |
| same with `--features vp8` (`VPX_DIR` = build-cache libvpx 1.17.0), after the `hint_flags` assertion | 613 | 46 passed |
| `cargo test -p proto --lib tests::wire` | 590 | 13 passed |
| `cargo test -p proto --test golden_wire` | 593 | 5 passed, 1 ignored (`dump`, generator) |
| `cargo test -p video --release --features net --lib` (media, net, sim) | 599 | 106 passed, 6 ignored (5 `net_real_*` 30 s real-time, `debug_latency_by_second`) |
| `cargo test -p p2pcore --release --test video_calls` | 603 | 4 passed |
| `cargo check -p desktop -p peer` (debug) | 596 | ok, no warnings |
| `cargo check --release -p desktop` (tuning UI compiled out by `debug_assertions`) | 614 | ok, no warnings |
| Android `:app:assembleDebug -Pabis=arm64-v8a` | 624 (local, after the LAN worker's 586/184 hit Gradle `OutOfMemoryError: Java heap space` and hung; cancelled) | BUILD SUCCESSFUL |

## Measurement tooling already in the tree (for the codec table)

Phase 1 of the handoff plan wants quality and cost per codec x bitrate x resolution. What
exists, and what it can't do yet:

- `video::sim` (`--features sim`, on via `p2pcore/dev-tools`): a virtual-clock link simulator
  for **rate control**: `FakeCodec` (sizes only), `HwCodec` (lagging hardware-style output) and
  `Av1Codec` (real libaom + rav1d on the synthetic `Pattern`). It reports bitrate, rungs, latency,
  freezes and keyframes, but has **no VP8 codec and no picture-quality metric**, so it can't fill
  the table as it is.
- `video::bench::run` (`p2pcore::video_bench` with `dev-tools`; Android debug `DebugReceiver video_bench`): libaom RT encode + rav1d decode times and
  average packet size at 180/360/540/720p, on the synthetic pattern or a local 4:2:0 y4m
  (`video::y4m`). AV1 only; no PSNR.
- `video::psnr_y` (luma PSNR) and `video::vpx::{Vp8Encoder, Vp8Decoder}` (libvpx, feature `vp8`)
  exist. A cheap offline next step: a VP8 arm in the bench plus `psnr_y` per row, then the same
  y4m clip through both codecs at 0.5/1/2/3 Mbps x 360/540/720p. Pixel hardware VP8 still needs
  the device (`hwvp8-lab`).
- The decode-cost fix (`89355034`, hardware decoders judged by throughput) has deterministic
  tests only: `video_call::tests::adapt_policy::hardware_decoder_*`, both passing above. Its old
  job 48 was interrupted while compiling; no numbers survived.

Wi-Fi/LAN sims (`068fd7e0`, a Wi-Fi stall is not a slower link; old job 55 was cancelled in
admission, no numbers survived), rerun once: job 611,
`cargo test -p video --release --features net --lib -- --nocapture --test-threads=1
sim::tests::wifi sim::tests::real_av1_wifi sim::tests::lan_delay`, 7 passed. From 5 s on:

| Scenario | Mean | >= 540p | Lowest | p95 latency | Freezes |
|---|---|---|---|---|---|
| `wifi_delay_spikes` (20 Mbps, 1 s spikes) | 2935 kbps | 100% | 720p | 40 ms | 0 |
| `wifi_delay_spikes_near_capacity` 2.4 Mbps | 2063 kbps | 100% | 540p | 122 ms | 0 |
| same at 1.6 Mbps | 1352 kbps | 100% | 540p | 111 ms | - |
| `real_av1_wifi_delay_spikes` (libaom) | 2896 kbps | 100% | 720p | 128 ms | 0 |
| `lan_delay_bursts` (software-style) | 1686 kbps | 80% | 180p | 226 ms | 30 (19.8 s) |
| `lan_delay_bursts_hw` | 2281 kbps | 94% | 360p | 447 ms | 36 (20.1 s) |
| `lan_delay_bursts_slow_link` 600 kbps | 476 kbps | 0% | 360p | 424 ms | 17 (8.9 s) |

The Wi-Fi rows are above the commit message's (1988 / 1792 kbps), most likely because the send
cap went from 2 to 3 Mbps afterwards (`e17d29c0`); not re-run at the old cap to confirm. These are simulations, not call measurements.

## Sharper-video suggestions and idle links

Merged with `dev`'s sharper-video suggestions (`hd::send_ok` / `hd::recv_ok`, `Hint::room`
on control message 5, `VideoOffer.hint_flags`) and idle peer links. Tuning never touches either:

- A tuned offer still says `hint_flags: true`; only `encode`/`hw_encode` are filtered.
- The suggestion rule is unchanged and reads the same sender state as before. A tuned call
  feeds it tuned inputs, so treat any suggestion there as a readout, not as evidence about
  the link: a 0.5 or 1 Mbps cap keeps the sending rule false (the rate estimate stays under
  its threshold), and a fixed 540p/720p rung above an HD-off 640x480 capture can make it hold
  (the capture really is the limit). With Auto/Auto/Auto nothing differs from `dev`.
- Session lifetime (idle close, consent-based presence, `docs/chat.md`) is a chat-link policy;
  a live 1:1 call already counts as activity there. Tuning adds no activity and keeps no link.

Video stays off by default: tuning chooses what an offer allows, never turns the camera on.
