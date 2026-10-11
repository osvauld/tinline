# Video calls (spike: `crates/video`)

Status: design + simulation; wired into 1:1 calls in the core (no UI yet).

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
- **Shipping**: every shipped build links libaom statically; users never install it. See
  [Shipping libaom](#shipping-libaom).
- **rav1d from its git tag** v1.1.0: the crates.io package lacks `src/arm/asm-offsets.h`, so
  the ARM assembly doesn't build from it.

## Shipping libaom

- **Source pin: release tarball + SHA-256**, not git-at-commit. `scripts/build_libaom.py` fetches
  `libaom-3.14.1.tar.gz` from `storage.googleapis.com/aom-releases` (the official release, also
  GPG-signed upstream) and checks its SHA-256 on every run before using the tree; a mismatch
  deletes the download and fails. Why: SHA-256 rather than git's SHA-1, no git needed, 6 MB
  instead of a clone, and a stamp ties the extracted tree to the hash. The tarball was checked
  byte-identical to tag v3.14.1 = commit `03087864cf4b`. Vendoring the tree is ruled out (size).
  Bumping: change `VERSION`/`SHA256`/`COMMIT` in the script and `aomTag` in
  `android/app/build.gradle.kts`.
- **Build**: `python3 scripts/build_libaom.py --target host|<android triple>` (cmake + Ninja +
  nasm; NDK for Android) builds a static realtime-only encoder under
  `~/build-cache/p2p_chat/libaom/install-v3.14.1-<target>` and prints that prefix. Idempotent;
  a rerun re-verifies and is a no-op build. A build dir configured from another source tree
  is wiped.
- **Linking** (`crates/video/build.rs`): `AOM_DIR_<triple>` (dashes as underscores) or
  `AOM_DIR` links `<prefix>/lib/libaom.a`. Without either, dev host builds fall back to the
  system libaom via pkg-config (dynamic); `AOM_STATIC_REQUIRED=1` turns that fallback into a
  build error, and Android always requires the static build.
- **Where it is wired**: `scripts/build_android.py` (per ABI, every build) and gradle's
  `AOM_DIR_<triple>`; `scripts/package_linux_tarball.sh` and `scripts/package_deb.sh` (host
  build, `AOM_STATIC_REQUIRED=1`, then fail if `readelf -d` shows a `libaom` NEEDED entry);
  `.github/workflows/release.yml` (linux job via the package scripts, plus an `ldd` check; android
  job builds both ABIs before gradle); `.github/workflows/ci.yml` (cached, sets `AOM_DIR`).
- **Windows**: there is no Windows build yet. When there is: MSVC target, same script with a
  `--target x86_64-pc-windows-msvc` branch running cmake with the Visual Studio/Ninja generator
  from a VS dev prompt (nasm on PATH), producing `aom.lib`; build.rs's static link line works
  unchanged (`static=aom`). Cross-building from Linux (mingw) is possible with libaom's
  `build/cmake/toolchains/x86_64-mingw-gcc.cmake` but would make Rust use the gnu target.
- **Licences**: libaom is BSD-2-Clause plus the Alliance for Open Media Patent License 1.0;
  rav1d is BSD-2-Clause. Texts in `packaging/licenses/` (installed by the deb to
  `/usr/share/doc/tinline/third-party/`, in the tarball's `licenses/`, AUR
  `/usr/share/licenses/tinline/`) and in Android's `assets/licenses/` (shown in Settings ->
  About -> Open-source licences). Desktop About names both.

## Wire format (per group stream)

`[group: u64 BE]`, then frames `[len: u32 BE][capture_us: u64 BE][AV1 temporal unit]`.
Frame 0 of a group is a keyframe. Receiver caps frame size (`max_frame`, default 1 MiB).
No resolution field: the size only changes at a keyframe, whose AV1 sequence header carries
it, and rav1d follows it with the same decoder instance.

## Resolution ladder (`media::Ladder`, owned by `Sender`)

| rung | floor (`min_bps`) | up when bitrate >= (130% of floor) | ceiling (`max_bps`) |
|---|---|---|---|
| 320x180 | - | - | 500 kbps |
| 640x360 | 250 kbps | 325 kbps | 1.2 Mbps |
| 960x540 | 600 kbps | 780 kbps | 2 Mbps |
| 1280x720 | 1 Mbps | 1.3 Mbps | 3 Mbps |

- Start: the highest rung whose floor the start bitrate covers (500 kbps -> 640x360).
- **Down at once**, skipping rungs, as soon as the target bitrate falls below the floor.
- **Up one rung at a time**, after the bitrate has stayed above the up threshold for 2.5 s
  without a break, and not within 4 s of a step down.
- **Encode time**: the app reports each frame's encode time (`Sender::encode_time`). If the
  p90 of the last 30 frames (>= 10 samples) exceeds 60% of the frame interval (20 ms at
  30 fps), step down and block the rung above for 30 s. Stepping up also needs the p90
  scaled by the pixel ratio to fit the budget, so a device that can't encode the next rung
  never tries it (phone pinned: 540p p90 ~13 ms -> 720p predicted ~24 ms, so it stays at
  540p; unpinned it stays at 360p).
- **Ceiling**: the encoder bitrate is clamped to the rung's `max_bps`; without it a step
  down for CPU sends a keyframe sized for the old bitrate, which flooded the link in the
  simulation and dropped a further rung.
- `LadderConfig::max_rung` / `Sender::set_max_rung`: device or peer capability.
- **Switching**: `Sender::resolution()` is the size for the next captured frame; on a change
  the sender wants a keyframe, so the new size always opens a new group. The codec layer
  re-creates the encoder at the new size (its first frame is a keyframe anyway); the
  receiver needs nothing.

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

## Call protocol and core API (`crates/core/src/video_call.rs`)

**Negotiation** (`proto::Msg`, JSON on the call's control stream):
- `CallHello.video` / `Accept.video`: `Option<VideoOffer { max_pixels }>`. Presence = "this
  device does AV1 video in this call"; `max_pixels` = largest picture it wants to receive
  (today 1280x720). The callee answers with an offer only if the hello had one and video is
  enabled on it (`set_video_enabled`, default on); the caller ignores an offer it didn't ask
  for. Both present = video negotiated, else the call is audio only (nothing else changes).
- The sender caps its ladder (`LadderConfig::max_rung`) at the highest rung within the peer's
  `max_pixels`.
- Mid-call `Msg::Video { camera: bool }`, only once video is negotiated: sent when the call
  goes active (current state) and on every change. Keyframe requests and pausing need no call
  message: the video control stream has keyframe requests, and "camera off" is just no frames.

**Streams**: when the call goes active with video negotiated, each side runs one `VideoSend`
and one `VideoRecv` on the call connection for the whole call (the call's control bi stream
is already accepted, so the next `accept_bi` is the peer's video control stream). With the
camera off nothing is encoded; turning it on forces a keyframe (new group). The receiver's
stall detector asks for keyframes every 600 ms meanwhile; harmless (1 byte, ignored until a
frame is encoded). The endpoint's transport config sets `max_concurrent_uni_streams = 16`
(nothing else in the core uses uni streams).

**Threads**: encoder thread (`tl-video-enc`, pinned with `video::cpu::pin_to_fast_cores`,
libaom 2 threads, speed 10, keyint 300): takes the newest pushed frame (as pushed: I420,
raw `YUV_420_888` planes, RGBA or YUYV), and turns it upright and into the size it picks with
`video::convert::prepare` in one pass (range conversion for full-range Android frames too).
The size: the upright picture fit inside `Sender::resolution()` (rung turned to the picture's
orientation), keeping aspect, never upscaling, even sides (`video_call::fit`). Fit, not
crop-to-rung: a 4:3 camera stays 4:3 (640x480 -> 480x360 on the 640x360 rung) so nothing of
the picture is lost and no pixels are spent on bars; `prepare`'s center crop then only trims
what even rounding loses. RGBA/YUYV take one extra pass (`rgb_to_i420`/`yuyv_to_i420`) first.
It (re)creates the encoder on a size change, sets the bitrate, encodes, reports the encode
time (`VideoSend::encode_time`, stamped on the sender's clock), sends. Decoder thread
(`tl-video-dec`, pinned, rav1d 2 threads, `frame_size_limit` 720p) fed through a 2-slot
channel by the task awaiting `VideoRecv::recv` (a slow decoder backs up into the link, where
the sender sees it); after a decode error it skips to the next keyframe and requests one.

**API** (UniFFI, on `Node`; all about the current call):

| | |
|---|---|
| `set_video_events(VideoEvents)` | `on_video_state(VideoInfo { call_id, negotiated, local_camera, remote_camera })`: when the call goes active (if we offered video) and on any camera change |
| `set_video_enabled(bool)` / `video_enabled()` | offer/take video in calls (default on) |
| `set_camera(on) -> Result` | also before the call is answered; `NotFound` without a call, `Protocol` once active without video |
| `push_video_frame(width, height, rotation, i420) -> bool` | packed I420 straight from the camera, any size up to 4096 a side, `rotation` = degrees clockwise to upright (CameraX `rotationDegrees`); latest wins; false = dropped (no video, camera off, bad size) |
| `push_video_frame_yuv(width, height, rotation, y, y_row_stride, u, v, uv_row_stride, uv_pixel_stride, full_range) -> bool` | Android: CameraX `YUV_420_888` planes copied as they are (one copy across FFI); pixel stride 1 or 2, planes may end after the last pixel; `full_range` = JFIF samples (normal for Android cameras). Conversion runs on the encoder thread. Else like `push_video_frame` |
| `push_video_frame_rgba(width, height, rotation, rgba)`, `push_video_frame_yuyv(width, height, rotation, stride, yuyv)` | desktop cameras: packed RGBA / YUY2 (even width), converted on the encoder thread |
| `pull_video_frame() -> Option<VideoFrame { width, height, data, seq }>` | newest decoded upright I420 frame not pulled yet; poll on the display clock |
| `pull_video_frame_rgba() -> Option<VideoFrame>` | the same frame as packed RGBA (`convert::i420_to_rgba`, on the caller's thread): iced `image::Handle::from_rgba`, Android `Bitmap` fallback |
| `video_info()`, `video_stats()` | state; frames/keyframes sent, send size, bitrate, encode ms, frames decoded, decode errors, received size |

Pull, not a callback, for frames: decoded video is latest-wins like a display, so a pull on
vsync drops what the renderer can't show for free; no platform code runs on the pinned codec
threads (a callback would run Kotlin/JNA on them); it matches `pull_speaker`. Low-rate state
changes are callbacks, in their own `VideoEvents` trait so existing `NodeEvents` implementors
are untouched.

**Tests** (`crates/core/tests/video_calls.rs`, two nodes over iroh like `calls.rs`):
`cargo test -p p2pcore --release --test video_calls`: video both ways (pattern 640x480 pushed
at 30 fps, >= 45 decoded frames in 3 s each way, 4:3 kept, 0 decode errors); camera off/on
mid-call (remote state events, no frames while off, frames resume on a new group); video
refused by the callee -> audio-only call continues.

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

The device model is the phone's measured encode time (`sim::Device::phone`: 2.1 ms +
18 ns/pixel, +-20%, one frame at a time; `phone_unpinned` for the A53 cores). The fake codec
gives deltas the target bitrate up to 0.3 bits/pixel and keyframes a size fitted to the
real-footage table above (key/delta ~7 at 180p / 200 kbps down to ~3.3 at 720p / 1.5 Mbps).

Before the ladder: fixed size, fixed 5 ms encode. After: ladder + phone device model.
Latency p50 / p95 over the whole run.

| Scenario | before | after | sizes (after) |
|---|---|---|---|
| clean 2 Mbps, 20 ms | 64 / 93 ms, 0 freezes | 65 / 99 ms, 0 freezes | 360p -> 540p at 3.5 s |
| 3% random loss | 51 / 115 ms, 0 | 53 / 110 ms, 0 | 360 -> 540 -> 180 (19.4 s) -> 360 -> 540 |
| 2 Mbps -> 300 kbps at 10 s | 64 / 828 ms, 0.3 s | 60 / 844 ms, 0 | 540 -> 360 (10.2 s) -> 180 (10.8 s) |
| 500 ms outage | 62 / 97 ms, 0.9 s | 64 / 96 ms, 0.7 s | 360 -> 540 |
| relay: 75 ms +-20 ms, 1.5 Mbps | 126 / 155 ms, 0 | 131 / 177 ms, 0 | 360 -> 540 |
| real AV1, 2% loss + 400 ms outage | 49 / 106 ms, 0.5 s | 40 / 84 ms, 0.6 s | 360 -> 180 -> 360 |
| 2 M -> 200 k (10 s) -> 2 M (20 s), 40 s | 60 / 2824 ms, 4.7 s (9) | 58 / 1829 ms, 0.9 s (3) | 540 -> 180 at 10.3 s -> 360 (24.2) -> 540 (27.2); 4 switches |
| slow device (unpinned), clean | - | 56 / 62 ms, 0 | 360p throughout, 0 switches |
| phone throttles 2x at 15 s | - | 58 / 78 ms, 0 | 540 -> 360 at 15.1 s, stays |
| real AV1, 2 M -> 200 k (6 s) -> 2 M (12 s), 20 s | 56 / 2232 ms, 0.6 s (12 s run, no recovery) | 52 / 2493 ms, 3.0 s, 0 decode errors | decoded 360, 540, 180, 360 |

The p95 in the bandwidth-drop rows is the bottleneck buffer draining (64 KB is 1.7 s at
300 kbps, 2.6 s at 200 kbps): the rate controller backs off to 85% of delivered, so it only
drains slowly. That is pre-existing; the ladder makes it shorter, not longer. Open issues:
- A step down's keyframe waits behind that queue, and the old group's unsent frames are reset,
  so with real AV1 the switch costs a freeze of up to ~the queue time (3.0 s total above).
  Options: drain faster (cut below delivered while queueing is high), or AV1 reference
  scaling (libaom `resize_mode`), which changes size without a keyframe.
- The ladder only steps down for CPU after 10 frames at the new speed; a sudden 2x slowdown
  at 720p costs ~1/3 s of late frames.

The first version used the congestion window for the bitrate; the simulation showed
bufferbloat (p50 256 ms on a clean link, 1.5 s after a bandwidth drop), which is why rate
control is delay-based.

## Capture/render conversion (`video::convert`)

Pure Rust, safe, no dependencies; `cargo test -p video --release convert`.

- **In:** `Yuv420` borrows any strided 4:2:0 picture: Android `YUV_420_888` (row stride,
  pixel stride 1 = planar or 2 = the NV12/NV21 views where U and V overlap; planes may end
  right after the last pixel, as Android buffers do), NV12/NV21 (`Yuv420::nv12`), or an
  `I420`. Desktop extras: `yuyv_to_i420` (YUY2), `rgb_to_i420` (RGB/BGR/RGBA/BGRA). MJPEG
  cameras are out of scope: decode them first (or pick the camera's YUYV/NV12 mode).
- **`prepare`** = the capture pipeline in one pass over the camera buffer: optional crop ->
  center-crop to the output aspect -> scale (copy, exact 2:1 box, or area average) ->
  full->limited range -> rotate (CameraX `rotationDegrees`) / mirror. Scaling happens before
  rotation, so rotation only touches output-sized data. Output sizes round down to even and
  crop origins to even (chroma alignment). Send unmirrored; mirror only the self-view.
- **Colour:** BT.601 limited range everywhere. `csrc/aom_rt.c` signals no colour description
  (decoders then assume 601-ish limited). Android camera YUV is normally full range (JFIF), so
  Android uses `Range::Full` (verify on the device: Y of a black/white scene should span
  ~0..255). Alternative later: signal full range in the bitstream (`AV1E_SET_COLOR_RANGE`).
- **Out:** `i420_to_rgba` into a caller buffer (iced `image::Handle::from_rgba` on desktop).
- **Android render (recommended):** keep per-frame work in Rust. Kotlin hands a `Surface`
  (SurfaceView) to Rust once; Rust does `ANativeWindow_fromSurface`,
  `ANativeWindow_setBuffersGeometry(w, h, RGBA_8888)` at each resolution change, then per
  frame `lock` -> `i420_to_rgba` into the locked buffer (row stride from the buffer) ->
  `unlockAndPost`. The compositor scales to the view for free, no JNI per frame, no Bitmap.
  Simpler fallback: `i420_to_rgba` into a direct `ByteBuffer` + `Bitmap.copyPixelsFromBuffer`
  (`ARGB_8888` is RGBA in memory). If RGBA conversion shows up on A53-class phones at 720p,
  move to a GL shader that samples the three planes (`GL_R8` textures), no CPU conversion.

Cost per frame, `cargo run -p video --release --example convert_bench` (i7-1165G7, loadavg
~8 from other builds, median ms):

| path | ms |
|---|---|
| `prepare`: YUV_420_888 pixel stride 2, 640x480, R90 -> 360x640 (portrait 640x360) | 0.53 |
| `prepare`: 640x480, R90 -> 640x360 landscape (crop 270x480, upscale) | 1.21 |
| `prepare`: 640x480, R0 -> 640x360 (crop only) | 0.15 |
| `prepare`: 1280x720, R90 -> 360x640 (2:1 box) | 0.93 |
| `prepare`: 1280x720, R0 -> 960x540 (general area average) | 2.28 |
| `i420_to_rgba` 640x360 / 1280x720 | 0.33 / 1.31 |
| `yuyv_to_i420` 640x480 | 0.16 |
| `rgb_to_i420` BGRA 1280x720 | 1.28 |

Expect ~4-8x these on the phone's A53 cores (to be measured; keep conversion on the pinned
fast cores too). Cheapest capture: ask CameraX for a size whose aspect matches the encoder
(crop-only or exact 2:1), since the general (non-integer) scaler is the slow path.

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

1. ~~Real footage, ARM numbers~~ done (above). ~~Static libaom in every shipped build~~ done:
   pinned tarball + SHA-256, Android, deb/tarball packaging and CI (see Shipping libaom).
2. ~~Resolution ladder in `media::Sender` (switch at a keyframe), added to the simulation.~~
   done (above).
3. iroh transport against `media::Sender`/`Receiver`: **built** (`video::net`) and checked
   against the simulation's scenarios over an emulated link (see Real transport vs
   simulation). ~~`max_concurrent_uni_streams` in the call endpoint's transport config~~ done
   (16). Still to do: rate control under random loss; freeze at ladder step-up on the real
   transport; sim model fixes listed there.
4. ~~Call protocol: video offer/accept in `CallHello`/`Accept`, camera on/off mid-call.~~ done
   (see Call protocol and core API).
5. Capture/render: Android CameraX -> I420, desktop camera; render on both. Pure-Rust
   conversion done (`video::convert`, above) and used by the core's encoder thread
   (`push_video_frame_yuv`, `pull_video_frame_rgba`); still to do: CameraX/desktop capture
   glue and the renderers, phone numbers for `convert_bench`.
