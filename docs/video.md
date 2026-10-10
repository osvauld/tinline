# Video calls (spike: `crates/video`)

Status: design + simulation. Not wired into calls or the apps yet.

## Decisions

- **Codec: AV1.** Encoder = libaom in realtime mode (C, through `csrc/aom_rt.c`); decoder =
  rav1d (Rust port of dav1d). The decoder is the side that parses peer-controlled bytes, so it
  is the memory-safe one; the encoder only ever sees our own camera frames.
- **rav1e rejected for calls**: built for offline encoding. Even at speed 10 with low latency
  it holds 4 frames (~130 ms) before the first packet (lookahead is structural) and is ~20-40x
  slower than libaom RT. Kept in the crate only for comparison (`rav1e_holds_frames_back`).
- **Transport: one QUIC stream per group of pictures** (Media over QUIC style, as in n0's
  iroh-live). A group = a keyframe and the frames that depend on it; QUIC retransmits within
  it, groups are independent, the newest group is sent first, and a stale group is reset.
  Audio stays on datagrams so it never waits behind video. Group calls later: a host relays
  groups to members (MoQ is relay-shaped), same wire format.
- **Rate control: delay-based at the app layer** (GCC idea). The receiver reports delivered
  rate + queueing delay (frame arrival minus capture time, above its 10 s minimum; the clock
  offset cancels) every 100 ms; the sender backs off to 85% of delivered when queueing
  > 40 ms and ramps +8%/report when < 15 ms. Backlog > 300 ms: reset the group, force a
  keyframe, halve the rate.
- **Shipping**: release builds compile libaom from source and link it statically (as opus is);
  nothing needs to be installed. `AOM_DIR=<prefix from scripts/build_libaom.py>` links that
  static build; without it the crate falls back to the system libaom via pkg-config.
- **rav1d from its git tag** v1.1.0: the crates.io package lacks `src/arm/asm-offsets.h`, so
  the ARM assembly doesn't build from it.

## Wire format (per group stream)

`[group: u64 BE]`, then frames `[len: u32 BE][capture_us: u64 BE][AV1 temporal unit]`.
Frame 0 of a group is a keyframe. Receiver caps frame size (`max_frame`, default 1 MiB).

## Codec benchmark (i7-1165G7, synthetic camera pattern, 30 fps)

`cargo run -p video --release --example bench -- aom|rav1e [frames] [threads] [speed]`

| | 320x240 | 480x360 | 640x480 | 1280x720 |
|---|---|---|---|---|
| libaom RT speed 10, 4 threads: encode p50 | 0.8 ms | 1.1 ms | 1.4 ms | 2.8 ms |
| libaom RT speed 10, 1 thread | 0.9 ms | 1.7 ms | 2.4 ms | 9.5 ms |
| rav1e speed 10, auto threads | 16.5 ms | 33 ms | 48 ms | 124 ms |
| rav1d decode p50 | 0.3 ms | 0.5 ms | 0.7 ms | 1.5 ms |
| frames held by encoder (libaom / rav1e) | 0 / 4 | 0 / 4 | 0 / 4 | 0 / 4 |

PSNR ~40 dB for both. The pattern is easier than real footage; see below.

## Real footage: laptop vs phone

Xiph `FourPeople_1280x720_60` (the standard video-conferencing clip), decimated to 30 fps,
89 frames, scaled per row. libaom RT speed 10 encode / rav1d decode, p50 (p95) ms per frame.
Run: `Y4M=<file> bench aom 89 <threads> 10`. Phone binary built with
`scripts/build_libaom.py --target aarch64-linux-android` + `AOM_DIR=... cargo ndk -t arm64-v8a`.

| | bitrate | frame size (key / avg) | laptop 4 thr | g52 big cores, 4 thr | g52, 2 thr | g52, 1 thr | g52 decode |
|---|---|---|---|---|---|---|---|
| 320x180 | 200 kbps | 5.6 KB / 0.8 KB | 0.6 | 3.0 (3.7) | 3.5 | 2.6 | 1.0 |
| 480x270 | 350 kbps | 8.8 KB / 1.4 KB | 0.8 | 4.3 (4.9) | 4.5 | 5.7 | 1.4 |
| 640x360 | 600 kbps | 12.5 KB / 2.4 KB | 1.4 | 7.9 (19.5) | 7.2 | 9.7 | 2.3 |
| 960x540 | 1 Mbps | 15.9 KB / 4.1 KB | 2.4 | 10.5 (13.8) | 13.0 | 19.9 | 3.6 |
| 1280x720 | 1.5 Mbps | 20.2 KB / 6.1 KB | 4.2 | 14.9 (20.4) | 21.7 | 33.5 | 5.1 |

Laptop = i7-1165G7 (loadavg ~3.5 from other builds). Phone = moto g52 (Snapdragon 680:
4x Cortex-A73 2.4 GHz + 4x A53), `taskset f0` = the A73 cores. **Unpinned, the shell process
ran on the A53 cores and everything was 3-5x slower** (720p encode 50 ms, decode 19 ms): in
the app the encoder/decoder threads must get big cores (foreground app, high thread priority /
Android performance hint), and we must measure that in-app.

rav1e on the phone (big cores, 4 threads): 24 ms at 320x180, 79 ms at 640x360, 267 ms at
720p, 4 frames held: confirms it is out.

Conclusion: on a budget 2022 phone AV1 realtime fits easily: 640x360 in ~8 ms on two big
cores, 720p in ~15-22 ms; rav1d decodes 720p in 5 ms. Proposed ladder, chosen by the rate
controller: 320x180 below ~250 kbps, 640x360 default, 960x540 / 720p when the link and the
device's measured encode time allow.

## Simulation

`cargo test -p video --release sim -- --nocapture`. Deterministic: virtual clock, seeded RNG,
a bottleneck link (rate, delay, jitter, loss, buffer, outages, rate changes) and a QUIC model
(per-stream retransmission, newest-first scheduling, resets, loss-based cwnd, pacing).

| Scenario | latency p50 / p95 | freezes |
|---|---|---|
| clean 2 Mbps, 20 ms | 64 / 93 ms | 0 |
| 3% random loss | 51 / 115 ms | 0 |
| 2 Mbps -> 300 kbps at 10 s | 64 ms p50; adapted below 300 kbps | 0.3 s |
| 500 ms outage | 62 / 97 ms | 0.9 s |
| relay: 75 ms +-20 ms, 1.5 Mbps | 126 / 155 ms | 0 |
| real AV1 (libaom -> rav1d), 2% loss + 400 ms outage | 49 / 106 ms, 0 decode errors | 0.5 s |

The first version used the congestion window for the bitrate; the simulation showed
bufferbloat (p50 256 ms on a clean link, 1.5 s after a bandwidth drop), which is why rate
control is delay-based.

## Next

1. ~~Real footage, ARM numbers~~ done (above). Static libaom: `scripts/build_libaom.py` works
   for the benchmark; still to wire into `scripts/build_android.py` / release packaging
   (vendored source, not fetched at build time).
2. Resolution ladder in `media::Sender` (switch at a keyframe), added to the simulation.
3. iroh transport against `media::Sender`/`Receiver` (uni stream per group, priority by
   group, control stream for reports/keyframe requests), checked against the same scenarios.
4. Call protocol: video offer/accept in `CallHello`/`Accept`, camera on/off mid-call.
5. Capture/render: Android CameraX -> I420, desktop camera; render on both.
