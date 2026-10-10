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
  nothing needs to be installed. The spike links the system libaom via pkg-config.

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

PSNR ~40 dB for both. The pattern is easier than a real camera; re-measure with real footage
and on an ARM phone before fixing resolution/bitrate defaults.

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

1. Real camera footage in the benchmark; ARM numbers (emulator is x86; a phone run needs the
   user's OK).
2. Static libaom build for Android (NDK, cmake) and desktop.
3. iroh transport against `media::Sender`/`Receiver` (uni stream per group, priority by
   group, control stream for reports/keyframe requests), checked against the same scenarios.
4. Call protocol: video offer/accept in `CallHello`/`Accept`, camera on/off mid-call.
5. Capture/render: Android CameraX -> I420, desktop camera; render on both.
