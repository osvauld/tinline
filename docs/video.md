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

## Transport (`video::net`, feature `net`)

Runs `media::Sender`/`Receiver` over the call's iroh `Connection` (audio stays on datagrams).

- **`VideoSend`** (`accept(conn, cfg)` waits for the receiver's control stream): `send(capture_us,
  key, payload)` never blocks; the encoder loop reads `bitrate()` / `wants_keyframe()`. Each
  `Out::Open` is a task: `open_uni`, `set_priority(group)` (newest first), writes, `finish`,
  `reset` on `Out::Reset` (also after finish, while QUIC may still be sending it).
- **`VideoRecv`** (`new(conn, cfg)` opens the control stream): `recv()` yields frames in decode
  order and is cancel-safe; nothing is read unless it is awaited, so a stalled decoder looks
  like a stalled link to the sender. `request_keyframe()` after a decode error.
- **Backlog = written more than RTT + 40 ms ago and not acked by the receiver.** QUIC's send
  buffer (`send_window`, 10 MB by default) accepts writes long before the bytes leave, so
  "not accepted by write" only grows after seconds of queue. The receiver acks the bytes it
  has read per group every 10 ms; this also counts the bottleneck queue and a receiver that
  stopped reading. `Sender::feedback` runs every 10 ms on that.
- **Control stream** (one bi stream, opened by the receiver; the sender's half is unused):
  version byte `1`, then messages, big-endian: `1 [delivered_bps u32][queuing_us u64]` rate
  report (every 100 ms), `2` keyframe request (explicit or receiver stall), `3 [group u64]
  [offset u64]` ack. Unknown type ends the session.
- **Peer-controlled bytes, bounded**: frame > `max_frame` -> that stream is stopped and a
  keyframe requested; group id >= 2^48 or not newer than the newest seen (duplicate, late
  stale group) -> stream stopped before anything is buffered; streams of groups older than
  the one playing are stopped; more than `max_groups` (16) groups at or ahead of the playing
  one, or more than `max_buffered` (32 MiB) across them -> `NetError::Protocol`. Reader tasks
  feed a 16-slot channel, so at most `max_streams` QUIC stream windows sit in between.
  `VideoRecv` lowers `max_concurrent_uni_streams` to 8, but QUIC can't take back credit the
  peer already has (100 by default), so the endpoint's transport config must set it (below).
- **Endpoint transport config** (`crates/core/src/node.rs`, `Node::start`; the endpoint is
  shared by all ALPNs, and nothing in core or iroh-blobs uses uni streams, so 8 is safe):

  ```rust
  let transport = QuicTransportConfig::builder()
      .keep_alive_interval(Duration::from_secs(5))
      .max_idle_timeout(Some(Duration::from_secs(20).try_into().expect("small timeout")))
      // Video group streams (video::net): the peer's credit from the handshake on.
      .max_concurrent_uni_streams(VarInt::from_u32(8))
      .build();
  ```

  (`VarInt` is `iroh::endpoint::VarInt`.) Leave the rest at iroh's defaults: per-stream receive
  window 1.25 MB (~5 s of 2 Mbps video) x 8 streams bounds QUIC's buffering per connection
  (`VideoRecv::max_buffered` bounds the app side); the connection window is unlimited by
  default, which is fine with the stream count capped; `send_window` (10 MB) doesn't matter
  because backlog comes from app-level acks; keep the default congestion controller (Cubic,
  see "Real transport vs simulation" for BBR/NewReno).
- **Tests** (`cargo test -p video --release --features net net`, two endpoints on loopback):
  300 frames in order and intact with keyframes at group starts; receiver paused 1 s -> sender
  resets, drops the rate, forces a keyframe, receiver resumes on the new group; keyframe
  request -> new group; oversized frame / garbage headers / duplicate and old groups / too many
  groups / too much buffered; queueing reports lower the bitrate.
- `cargo run -p video --release --features net --example net_loopback -- [frames] [w] [h]`:
  libaom -> iroh loopback -> rav1d, capture-to-decoded latency. Laptop, 300 frames at 30 fps,
  2 encoder threads, rate ramped to 2 Mbps: 640x360 p50 4.8 / p95 7.5 ms, 1280x720 p50 5.4 /
  p95 15.7 ms; 0 decode errors, 0 resets.

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

**In the app** (debug APK, Rust built in release; `DebugReceiver` `video_bench`, Tinline in the
foreground, 147 frames, encode p50 / decode p50 ms). The app is in the `top-app` cpuset (all 8
cores allowed), yet the scheduler keeps the codec's worker threads on the A53s; raising thread
priority (-8) changes nothing. Pinning the creating thread to the fastest cores
(`video::cpu::pin_to_fast_cores`, from `cpuinfo_max_freq`; workers inherit it) fixes it:

| | 320x180 | 640x360 | 960x540 | 1280x720 |
|---|---|---|---|---|
| in app, 4 threads, unpinned | 6.7 / 2.3 | 14.8 / 4.9 | 22.6 / 7.9 | 33.3 / 12.5 |
| in app, 4 threads, pinned to A73s | 3.1 / 1.0 | 6.3 / 2.1 | 11.4 / 3.5 | 18.5 / 5.4 |
| in app, 2 threads, pinned | 5.0 / 2.1 | 7.2 / 3.1 | 13.2 / 5.1 | 21.8 / 7.8 |

So calls pin the encode and decode threads to the fast cores (2 encoder threads are enough up
to 540p). Linking rav1d into `libp2pcore.so` needs `-Wl,-Bsymbolic` (`crates/core/build.rs`):
its assembly addresses Rust statics PC-relatively, which a shared library can't do for
preemptible symbols.

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

## Real transport vs simulation

`video::net` over real iroh connections through an in-process UDP link emulator
(`crates/video/src/net/impair.rs`, test-only; netem without root): bottleneck rate with a
drop-tail byte queue, delay + jitter (in order unless `reorder`), random loss, outages, rate
changes, seeded. A is bound to `[::1]` only, B to `127.0.0.1` only, and the proxy has one
socket per family, so iroh's hole punching can't find a direct path; every test asserts that
all paths of both connections point at the proxy and that the proxy's byte counters match
both connections' UDP counters. Same scenarios, seeds and `FakeCodec` as the simulation, 30
fps for 30 s real time; latency = capture -> `VideoRecv::recv` + the sim's 7 ms codec time.
Return path: same delay/jitter, no rate limit; outages hit both directions (the sim only
drops the forward direction).

`cargo test -p video --release --features net net_real -- --ignored --nocapture
--test-threads=1` (2.5 min; `net_impair_no_bypass` and the emulator's unit tests run by
default). `VIDEO_CC=bbr|reno` swaps the QUIC congestion controller. Laptop, load 1-3.

| Scenario | sim p50 / p95 | real p50 / p95 | sim / real frozen (> 200 ms) | sim / real mean bitrate | real resets |
|---|---|---|---|---|---|
| clean 2 Mbps, 20 ms | 64 / 94 | 65 / 101 | 0 / 0 | 1743 / 1702 kbps | 0 |
| 3% loss | 52 / 115 | 49 / 156 | 0 / 0-0.4 s | 1044 / 590-820 kbps | 2-3 |
| 2 Mbps -> 300 kbps at 10 s | last 15 s 68 / 118 | last 15 s 63 / 103 | 0.7 / 0.7-1.0 s | last 10 s 256 / 248 kbps | 2 |
| 500 ms outage at 10 s (both ways) | 63 / 98 | 63 / 89 | 0.9 / 0.7-0.8 s | 1680 / 1626 kbps | 2-3 |
| relay 75 (+0-20) ms, 1.5 Mbps | 127 / 154 | 129 / 171 | 0 / 0 | 1263 / 1233 kbps | 0 |

p50/p95 from 2 s on unless noted; ranges are over repeated runs. No decode-order violations
in any run; no queue drops at the emulated bottleneck (the delay-based controller keeps the
queue short; 300 kbps after the drop with no latency creep).

Findings:

- **Clean, rate drop, outage, relay: real matches the sim** within run-to-run noise. App-level
  ack backlog works as modelled (1-3 resets where the sim has 0-1 drops). After the outage
  video is back ~0.3 s after the link. The relay p95 is ~20 ms higher: the emulator also
  jitters the return path (acks, reports), the sim doesn't.
- **Random loss is where the sim is optimistic: real bitrate 55-80% of the sim's, p95 +40
  ms.** QUIC's loss-based congestion window drops to about one BDP (Cubic cwnd ~10 KB at 50
  ms RTT) and video waits in QUIC's send buffer; tail losses (last packet of a frame) are
  only recovered by PTO, not after ~1.1 RTT as in the model. Both show up as queueing delay,
  so the delay-based controller backs off. Congestion controller, 3% loss: NewReno cwnd 6 KB,
  417 kbps, 0.5 s frozen; Cubic 590-820 kbps; BBRv3 580-850 kbps, p95 = sim. BBRv3 costs on
  clean/relay links (mean 1452 / 851 kbps: its RTT probing reads as queueing delay), so it is
  no overall win: **keep Cubic**. The fix belongs in rate control (`media`): tell
  retransmission delay from queue build-up (e.g. ignore one-off delay spikes no longer than
  ~1 RTT, or use a robust minimum over the report window).
- **Sim model, to adjust:** (1) congestion window under random loss (cwnd floor ~1 BDP and
  sender-side queueing when cwnd-limited; today the sim's window rarely limits); (2) loss
  detection by PTO for tail packets plus ACK delay (25 ms max_ack_delay); (3) outages in both
  directions; (4) return-path jitter. Nothing to change in `net` for these scenarios.

## Next

1. ~~Real footage, ARM numbers~~ done (above). Static libaom: `scripts/build_libaom.py` works
   for the benchmark; still to wire into `scripts/build_android.py` / release packaging
   (vendored source, not fetched at build time).
2. Resolution ladder in `media::Sender` (switch at a keyframe), added to the simulation.
3. iroh transport against `media::Sender`/`Receiver`: **built** (`video::net`) and checked
   against the simulation's scenarios over an emulated link (see Real transport vs
   simulation). Still to do: set `max_concurrent_uni_streams` in core's transport config
   (snippet under Transport); rate control under random loss; sim model fixes listed there.
4. Call protocol: video offer/accept in `CallHello`/`Accept`, camera on/off mid-call.
5. Capture/render: Android CameraX -> I420, desktop camera; render on both.
