# Video calls (spike: `crates/video`)

> The `cargo --example` tools named below (net_loopback, bench, convert_bench, delay, cam_probe) were removed on 2026-10-15; recover them with `git show archive/examples-2026-10-15:<path>`. The measured numbers stay valid as a record.

Status: design + simulation; wired into 1:1 calls in the core; Android UI (camera, render,
call screen); desktop UI (camera + render).

## Opt-in HD capture (Android and Linux desktop)

Settings **Prefer HD** now requests 1280x720 capture with bounded SD fallback;
Auto remains the default. The AV1 software encoder, rate/CPU ladder and peer
limits are unchanged, so HD capture is not a guarantee of HD transmission. Both
apps display actual **Sending** dimensions and label HD only at 1280x720 or its
portrait equivalent. Android preference changes apply at the next camera bind.

Linux desktop supports YUYV, NV12 and MJPEG (often needed for USB 720p30).
**Find cameras → Refresh → Camera** selects a capture node for the next call;
a missing selected camera never silently switches to another. Prefer HD applies
at the next camera start. See [HD capture integration and validation](design/video-hd-hardware.md)
for exact checks, local-call evidence and remaining limits. No GPU/hardware
encoding has been added. Older spike measurements below describe their original
capture settings, not the new opt-in preference.

## Decisions

- **Codec: AV1.** Encoder = libaom in realtime mode (C, through `csrc/aom_rt.c`); decoder =
  rav1d (Rust port of dav1d). The decoder is the side that parses peer-controlled bytes, so it
  is the memory-safe one; the encoder only ever sees our own camera frames.
- **Hardware VP8 for 1:1 calls** (decided 2026-10-16, [design](design/video-hw-vp8.md)): an
  Android phone with a hardware VP8 encoder sends VP8 when the peer decodes it (the offer's
  codec lists, `proto::send_codec`); phones decode VP8 with MediaCodec, the Linux desktop with
  libvpx (static, VP8 decoder and realtime encoder, `scripts/build_libvpx.py`, video feature
  `vp8`; peer keyframes above `max_pixels` are refused before libvpx parses them). The desktop
  also encodes VP8 (libvpx, software, `vpx::Vp8Encoder`) but only sends it to a peer that
  decodes VP8 in hardware (a Pixel 3a otherwise decodes the laptop's AV1 in software at 15 fps);
  two desktops stay on AV1. AV1 stays the default and the fallback. Follow-up before release:
  sandbox the desktop libvpx decoder (C parsing peer bytes, unlike rav1d).
- **rav1e rejected for calls**: built for offline encoding. Even at speed 10 with low latency
  it holds 4 frames (~130 ms) before the first packet (lookahead is structural) and is ~20-40x
  slower than libaom RT. Kept in the crate only for comparison (`rav1e_holds_frames_back`).
- **Transport: one QUIC stream per group of pictures** (Media over QUIC style, as in n0's
  iroh-live). A group = a keyframe and the frames that depend on it; QUIC retransmits within
  it, groups are independent, the newest group is sent first, and a stale group is reset.
  Audio stays on datagrams so it never waits behind video. Group calls later: a host relays
  groups to members (MoQ is relay-shaped), same wire format.
- **Rate control: delay-based at the app layer** (GCC idea). The receiver reports delivered
  rate (sliding 100 ms) + queueing delay (least delayed frame of the last 100 ms, arrival
  minus the sender's stamp, taken when the frame is handed to the transport, above its 10 s
  minimum; the clock offset cancels) every 50 ms. The
  sender backs off to 85% of delivered when queueing > 40 ms or the link is *starved*
  (delivered < 50% of the delta frames we sent, two reports in a row), takes a second look
  150 ms later (median delivered; the ladder waits for it), then **drains** the standing
  queue: below delivered until queueing < 15 ms, then ramps +8% per 100 ms. Backlog > 300 ms
  (and not draining): reset the group, force a keyframe, halve the rate. Details: [Rate control](#rate-control-drain-on-a-bandwidth-drop).
- **Shipping**: every shipped build links libaom statically; users never install it. See
  [Shipping libaom](#shipping-libaom).
- **rav1d from its git tag** v1.1.0: the crates.io package lacks `src/arm/asm-offsets.h`, so
  the ARM assembly doesn't build from it.

## Shipping libaom

- **Source pin: release tarball + SHA-256**, not git-at-commit. `scripts/build_libaom.py` fetches
  `libaom-3.15.2.tar.gz` from `storage.googleapis.com/aom-releases` (the official release, also
  GPG-signed upstream) and checks its SHA-256 on every run before using the tree; a mismatch
  deletes the download and fails. Why: SHA-256 rather than git's SHA-1, no git needed, 6 MB
  instead of a clone, and a stamp ties the extracted tree to the hash. The tarball was checked
  byte-identical to tag v3.15.2 = commit `af3dc9aadc79`. Vendoring the tree is ruled out (size).
  Bumping: change `VERSION`/`SHA256`/`COMMIT` in the script (gradle reads `VERSION` from it).
  3.15.2 (from 3.14.1) takes the 3.15.0 encoder security fixes (CVE-2026-56208..56211,
  CVE-2026-13906, multithreaded-encode double free) and the 3.15.2 oss-fuzz fixes.
- **Build**: `python3 scripts/build_libaom.py --target host|<android triple>` (cmake + Ninja +
  nasm; NDK for Android) builds a static realtime-only encoder under
  `~/build-cache/p2p_chat/libaom/install-v3.15.2-<target>` and prints that prefix. Idempotent;
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

`[group: u64 BE]`, then frames `[len: u32 BE][stamp_us: u64 BE][AV1 temporal unit]` (`stamp_us`:
sender clock, when the frame was handed to the transport; until 2026-10-17 the capture time).
Frame 0 of a group is a keyframe. Receiver caps frame size (`max_frame`, default 1 MiB).
No resolution field: the size only changes at a keyframe, whose AV1 sequence header carries
it, and rav1d follows it with the same decoder instance.

Logs (`p2pcore::video_call`, info): once a second `video send WxH@fps kbps cap=.. key=..
overuse=.. starved=.. queue=..ms | recv WxH@fps dec=..ms`; on every ladder step
`video step 720p30 -> 720p15 reason=rate est=820kbps`; on every hint we send
`video hint max=..px fps=.. reason=..`. The receiver also steers its frame-rate tier from the
p75 decode time over 2 s (above 60% of the interval: a tier down; below 30% for 10 s: a tier up).
The apps report the per-frame pull cost with `report_video_render_ms`; its p75 over 2 s is added to the decode p75 (needs >= 10 samples).
That rule is for software decoders, where wall time of `decode` is CPU work. A hardware MediaCodec decoder's wall time is pipeline latency (a Pixel 3a: 12 ms at 360p, 25 ms at 720p, with the decoder thread at ~3% CPU), so with `recv_hardware` the policy ignores it and judges throughput instead: the decoder is "behind" when, over the last 2 s, it delivered under 80% of the frames it received (>= 20 received); that counts as 100% of the interval (a tier down), otherwise only the render cost counts, with the same 60% / 30% thresholds and 10 s hold before a tier up.

## Resolution ladder (`media::Ladder`, owned by `Sender`)

**Since 2026-10-16 (`feature/video-adaptation`) the ladder is two-dimensional**: steps
(rung, frame rate 15 / 24 / 30), frame rate first on the way down, a rung's floor scaled by
`0.4 + 0.6 x fps / 30`, 5 s holds and 10 s after a step down, a flap block, receiver hints
(`media::Hint`), frame skipping (`Sender::take_frame`) with the encoder bitrate scaled to the
tier (`Sender::encoder_bitrate`), and one keyframe per switch (`Sender::poll_keyframe`). See
`docs/design/video-adaptation.md` (sections 2, 3, 6, 7 results and 10 deviations); the rules
below are the 30 fps-only ladder they extend.

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
- **Encode time**: the app reports each frame's encode time (`Sender::encode_time`). The
  first 15 frames after a new encoder (start, size change) are ignored (libaom's first
  frames are slower, and so is a machine that is just setting up the call). If the
  **median** of a full window of 30 frames (1 s) exceeds 60% of the frame interval (20 ms
  at 30 fps), step down and block the rung above for 5 s, doubling with each further CPU
  step down (5, 10, 20, 30 s max), back to 5 s after a minute without one. The median, not
  the p90: on a busy desktop one frame in ~15 is preempted for 15-45 ms, and a smaller
  picture doesn't help with that. Stepping up needs the **p75** (>= 10 samples) scaled by
  the pixel ratio to fit the budget, so a device that can't encode the next rung never
  tries it (phone pinned: 540p ~13 ms -> 720p predicted ~24 ms, so it stays at 540p;
  unpinned it stays at 360p).
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

- **`VideoSend`** (`accept(conn, cfg)` waits for the receiver's control stream): `send(stamp_us,
  key, payload)` (stamp taken right before the call) never blocks; the encoder loop reads `bitrate()` / `wants_keyframe()`. Each
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
  version byte `2`, then messages, big-endian: `1 [delivered_bps u32][queuing_us u64]` rate
  report (every 50 ms; delivered over the last 100 ms), `2` keyframe request (explicit or receiver stall), `3 [group u64]
  [offset u64]` ack, `4 [max_pixels u32][max_fps u8][reason u8]` hint (what the receiver can
  use: the drawn size rounded up to a rung, `0` = not shown; the frame rate it decodes and
  draws, `0` = no limit; reason 0 none, 1 display, 2 decode, 3 thermal, 4 battery, 5 user; sent
  when set and on every change; an unknown reason or `max_fps` > 120 ends the session).
  Unknown type ends the session; there is no v1 compatibility (pre-release).
  See `docs/design/video-adaptation.md`.
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
- `CallHello.video` / `Accept.video`: `Option<VideoOffer { max_pixels, max_fps, decode, encode, hw_decode, hw_encode }>`.
  Presence = "this device does video in this call"; `max_pixels` = largest picture it wants to
  receive (today 1280x720); `max_fps` = highest frame rate it decodes and shows (30; the
  peer's ladder stays at or below it); `decode`/`encode` = its codecs, best first (both always have
  AV1); `hw_decode`/`hw_encode` = the subsets that run in hardware (MediaCodec hardware codec;
  libaom, rav1d and libvpx are software). Each direction uses `proto::send_codec(sender,
  receiver)`, computed by both ends: of the sender's `encode` codecs the receiver decodes, the
  one with the most hardware wins (one point if the sender encodes it in hardware, one if the
  receiver decodes it in hardware), ties go to the sender's `encode` order, none = AV1.
  `video_call/codecs.rs` builds the offer and picks the encoder/decoder. `VideoStats`
  (uniffi record, read by both apps) carries `send_codec`/`recv_codec` ("av1"/"vp8") and
  `send_hardware`/`recv_hardware` (bool, false until video is decided). The callee answers with an offer only if the hello had one and video is
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
| `push_video_frame_yuv(width, height, rotation, y, y_row_stride, u, v, uv_row_stride, uv_pixel_stride, full_range) -> bool` | Android: CameraX `YUV_420_888` planes copied as they are; pixel stride 1 or 2, planes may end after the last pixel; with pixel stride 2 pass the interleaved chroma plane once (NV21: `v` = the V buffer, `u` empty; NV12 the reverse) to copy a third less; `full_range` = JFIF samples (normal for Android cameras). Conversion runs on the encoder thread. Else like `push_video_frame` |
| `push_video_frame_rgba(width, height, rotation, rgba)`, `push_video_frame_yuyv(width, height, rotation, stride, yuyv)` | desktop cameras: packed RGBA / YUY2 (even width), converted on the encoder thread |
| `pull_video_frame() -> Option<VideoFrame { width, height, data, seq }>` | newest decoded upright I420 frame not pulled yet; poll on the display clock |
| `pull_video_frame_rgba() -> Option<VideoFrame>` | the same frame as packed RGBA (`convert::i420_to_rgba`, on the caller's thread): iced `image::Handle::from_rgba`, Android `Bitmap` fallback (4x the bytes of I420 across the FFI) |
| `video_info()`, `video_stats()` | state; frames/keyframes sent, send size, bitrate, encode ms, frames decoded, decode errors, received size; `send_fps`, `recv_fps` (last second), `send_cap {pixels, fps, reason}` (what the peer's hint, our budget and its offer allow), `ladder_steps` and `last_step_reason` (`rate`, `cpu`, `hint`, `budget`) |
| `set_video_view(width_px, height_px, visible)` | the remote picture's drawn size in device pixels and whether it is on screen; call on layout changes. The core asks the peer for the next rung at or above it (min 320x180; hidden = 180p at 15 fps); a shrink holds 1 s, anything else goes at once. No-op without a call |
| `report_video_render_ms(ms)` | the app's cost to pull and prepare one remote frame; call once per drawn frame |
| `video_send_label(stats)` (free function) | the picture label from the stats, one text for every app: `HD · 24 fps`, `540p · slow network`, `540p · their screen`; empty before a frame was sent |
| `set_video_budget(VideoBudget { thermal, power_save })` | Android thermal status (0-6) and battery saver. MODERATE (2): 15 fps up to 540p; SEVERE (3+): 15 fps up to 360p; power save: 24 fps up to 540p. Lowers the hint we send and caps our own ladder. No-op without a call |
| `video_frame_handle() -> u64` | this node's handle for the direct (non-UniFFI) frame calls, see Frames across the FFI |

Rust-only (not exported): `push_video_planes(.., y: &[u8], .., u: &[u8], v: &[u8], ..)` =
`push_video_frame_yuv` from borrowed planes, and `pull_video_frame_rgba_into(&mut [u8]) ->
Pulled { None | TooSmall { width, height } | Frame { width, height, seq } }` (no allocation for
the pixels; `TooSmall` leaves the frame for the next call). Android's JNI entry points call
these.

Byte arrays in this API are `video_call::Bytes` (see Frames across the FFI); Rust callers keep
`push_video_frame(.., Vec<u8>)` and read `VideoFrame.data` like a `Vec<u8>` (`Deref`).

Pull, not a callback, for frames: decoded video is latest-wins like a display, so a pull on
vsync drops what the renderer can't show for free; no platform code runs on the pinned codec
threads (a callback would run Kotlin/JNA on them); it matches `pull_speaker`. Low-rate state
changes are callbacks, in their own `VideoEvents` trait so existing `NodeEvents` implementors
are untouched.

**Frames across the FFI.** UniFFI 0.29 passes every byte array through a `RustBuffer`: Kotlin
asks Rust for a buffer (a JNA call; Rust zero-fills it), `put`s the `ByteArray` into it, Rust
lifts it into a `Vec<u8>`; returns go the other way. UniFFI's own `Vec<u8>` lift/lower is a
generic per-element loop (bounds check + push per byte), the main Rust-side cost. `Bytes` is a
newtype with the same wire form and metadata (Kotlin still sees `ByteArray`, no Kotlin change)
whose lift is one `memmove` of the buffer it already owns and whose lower is one
`extend_from_slice`; `VideoFrame.data` and the exported `push_video_frame`/`_yuv` use it.
Cost of one 640x480 I420 frame, Rust side only (`cargo test -p p2pcore --release --lib
bytes_cross_like_vec -- --nocapture`, i7-1165G7, includes a clone of the frame):
lift `Vec<u8>` 0.51 ms -> `Bytes` 0.016 ms, lower (pull) 0.28 -> 0.021 ms. That was only
~0.5 ms of the 4.6-5.7 ms the emulator (x86) measured per push: the rest was Kotlin/JNA
(`ByteBuffer.get` into a `ByteArray`, the JNA call that allocates and zero-fills the
RustBuffer, `put` into it, the call). No UniFFI 0.29 type maps to a direct `ByteBuffer` or a
borrowed slice, so Android's frames now skip UniFFI:

**JNI entry points** (`crates/core/src/video_jni.rs`, Android only, `jni` 0.21; Kotlin
`object VideoJni` in `VideoIo.kt`, `external fun`s in the same `libp2pcore.so`):
- `VideoJni.pushYuv(handle, w, h, rotation, y, yRowStride, u, v, uvRowStride, uvPixelStride,
  fullRange)` takes the `ImageProxy` planes' direct buffers as they are. Rust reads each with
  `GetDirectBufferAddress`/`GetDirectBufferCapacity` (null or non-direct = refused); with pixel
  stride 2 and U, V one byte apart (`video_call::chroma_spans`) the interleaved plane is the
  union of the two buffers, read once. `push_video_planes` checks the planes against the
  size/strides like `push_video_frame_yuv` and copies only the bytes the frame spans into the
  encoder's slot (one memcpy per plane), then returns: the `ImageProxy` closes right after.
- `VideoJni.pullRgba(handle, dst, info)`: Rust converts the newest frame to RGBA straight into
  the Kotlin-owned direct `dst` (`pull_video_frame_rgba_into`); `info` = width, height, seq.
  -1 = `dst` too small (Kotlin allocates `w * h * 4` and calls again; the frame waits), 0 = no
  frame, -2 = bad arguments. Kotlin then `Bitmap.copyPixelsFromBuffer` (`ARGB_8888` is RGBA in
  memory).
- **No pointers from Kotlin**: the node is found by `Node::video_frame_handle()`, a number in a
  global registry of `Weak<Inner>` (dead nodes pruned, a new node after an account switch gets
  a new handle; `VideoIo` re-asks when its `Node` object changes). A stale or bogus handle finds
  nothing. Buffers are only touched within the address/capacity the JVM reports, during the
  call; panics are caught at the boundary. A Kotlin bug gives `false`/-2, not memory corruption.
- `VideoIo.jni = false` (DebugReceiver `video_path jni=0`) keeps the UniFFI path for comparison:
  planes copied into reused `ByteArray`s (the NV21 plane once, plus the U view's last byte)
  -> `push_video_frame_yuv`; I420 `pull_video_frame` + Kotlin conversion + `setPixels`.

Per frame on the x86_64 emulator (camera 640x480 NV21, desk's video 240x180; host load 4-15
from other builds, so wall times are mostly the thread waiting for a vCPU: the thread-CPU rows,
`Debug.threadCpuTimeNanos`, are the cost; median of the per-second averages over a call,
`scripts/e2e_android_video.py --path jni|uniffi`, ranges over runs):

| | V9 (I420 pack + `push_video_frame`) | UniFFI `push_video_frame_yuv` | JNI `pushYuv` |
|---|---|---|---|
| push, wall ms (Kotlin copy + call) | 0.11-0.15 + 4.6-5.7 | 0.63 + 11.7 (load 12-15) | 0 + 0.83-2.2 (load 4-12) |
| push, thread CPU ms | - | 1.2-2.2 | 0.10-0.12 |
| push without a call, hot buffers (`video_io_bench`), median ms | - | 0.72-0.90 | 0.012 |

| render 240x180 | (a) UniFFI I420 pull + Kotlin I420->ARGB + `setPixels` | (b) JNI RGBA pull + `copyPixelsFromBuffer` |
|---|---|---|
| pull, wall ms | 14.8 (V9: 1.3-2) | 0.31-0.34 |
| to bitmap, wall ms | 8.9 (V9: 1.7-2.6) | 0.14 |
| thread CPU ms, both | 2.0-2.8 | 0.10-0.15 |

At 480x360 (b) measured 1.06 + 0.24 ms wall, 0.35 ms CPU. So the render path is (b); the
`ANativeWindow` path (Capture/render conversion) would drop the Bitmap copy too, a later step.

**Tests** (`crates/core/tests/video_calls.rs`, two nodes over iroh like `calls.rs`):
`cargo test -p p2pcore --release --test video_calls`: video both ways (pattern 640x480 pushed
at 30 fps, one side as padded NV21 planes through `push_video_planes` and the other side's
renderer on `pull_video_frame_rgba_into`, >= 45 decoded frames in 3 s each way, 4:3 kept, 0
decode errors; frame handles one per node, stable); camera off/on
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

## Rate control: drain on a bandwidth drop

`media::Sender::on_report`, in order:

- **Overuse** = queueing > 40 ms, or **starved**: delivered < `starve_pct` (50%) of what the
  sender handed to the transport over the last two reports, short by >= `starve_min_bps`
  (300 kbps), in `starve_reports` (2) reports in a row. Not in the first second of the call
  nor 300 ms after a drain (our own rate jumps ahead of what the receiver has seen yet).
  Starvation shows a report or two before the queueing delay: right after a drop the big
  frames crawl through the slower link and the *latest complete* frame's delay still looks
  fine while the queue grows by ~19 KB per 100 ms (1.5 Mbps into 200 kbps).
- On overuse the **estimate** (`bitrate`, drives the ladder) goes to 85% of delivered (at most
  every 200 ms), and the encoder gets the **drain** rate: delivered minus the queue
  (queueing x delivered) spread over `drain_us` (500 ms), at least `drain_pct` (25%) of
  delivered (and never below 25% of `min_bps`: with a slow decoder what gets through follows
  what we send, and the drain spiralled to 1 kbps); when starved straight to 25%, since the
  queue is bigger than the delay shows. `Sender::bitrate()` = min(estimate, drain);
  `Sender::draining()`.
- **Second look** (V11): 150 ms after a decrease (three more reports) the estimate is set again
  from the *median* delivered of those reports, min(estimate before the cut, 85% of it), and
  the ladder waits for it. The first cut's 100 ms window can still hold bytes from before a
  drop (real iroh went 540p -> 360p -> 180p, a keyframe each), a single 100 ms window at
  300 kbps holds 3-4 packets (+-30%), and a stream stalled ~100 ms on a tail loss reads as two
  starved reports (the sim with PTO went straight to 180p under 3% loss). The encoder rate
  (drain) drops at once; only the estimate and the ladder wait. If the queue is gone by the
  second look the cut was a blip and the estimate comes back up to that median.
- **Starvation counts delta frames only.** A keyframe takes several report intervals to cross
  the link; counting it, our own step up to 720p (16 KB keyframe) looked like a starved link
  and stepped straight back down (the busy-desktop flapping below).
- **Queueing delay** (receiver): the least delayed frame of the last 100 ms against the 10 s
  minimum (was the latest frame). A queue delays every frame; one late frame (a preempted
  encoder, a retransmission) is not a queue. Costs about a frame of reaction to a slow
  build-up (worst frame after the 300 kbps drop 152 ms, was < 150).
- **Learned baseline** (sender): a queueing delay that hasn't been under `underuse_us` for 5 s
  while the estimate is below half of `max_bps` is not a queue we cause (every cut drains
  the path within a second or two): its minimum over those 5 s joins a baseline that is
  subtracted from reports (at most 100 ms; any lower report lowers it). Without it a
  receiver clock running 0.5% fast (a VM, the emulator) reads ~50 ms of queueing forever and
  the call sat at 180p / the floor. Not above half of `max_bps`: there a delay between the
  thresholds is our own standing queue at the link's capacity, and learning it let it grow
  (2 Mbps -> 300 kbps drop: max 814 ms).
- **Stale queue reset**: draining for more than `drain_us` while still sending at least half
  of what gets through means the queue is not in the network (a receiver that decodes slower
  than we send: `VideoRecv` only reads when its decoder takes frames). A lower rate can't
  drain it; once frames wait > `max_backlog_us` the sender resets the group and sends a
  keyframe the receiver can jump to (no rate halving). Bounds latency at ~0.5 s instead of
  growing 7 ms per frame.
- The drain ends when queueing < 15 ms; the ramp (+8% per 100 ms, at most 1.5 x delivered)
  starts 500 ms after the last decrease.
- **Wi-Fi spikes** (2026-10-16, `Sender::stall_was_spike`): at an overuse episode's first
  second look the cut is taken back to 85% of the rate before the episode when two reports
  since carried >= 75% of that rate and either a report within 500 ms delivered < 50% of the
  estimate (the stall a spike starts with) or one since delivered > 110% of the old rate and
  300 kbps more (the burst after). For 1.5 s after (`SPIKE_HOLD_US`) overuse whose delay doesn't
  grow past 150% of its lowest + 20 ms is held: no cut, drain no lower than 70% of the old rate,
  no starvation verdicts. Growing delay, starvation or the end of the hold go back to the rules
  above. While a report since the cut delivered < 10% of the old rate (the burst still to come:
  a 130 ms spike, a tail loss waiting for its PTO), the first second look is put off once by
  150 ms; the ladder waits with it, the drain doesn't. See Wi-Fi delay spikes below.
- **Delay bursts / stalls** (2026-10-17, `fix/rate-recovery`): nothing delivered for 1.5 RTT is a
  stall, not capacity: cuts keep 50% of the old rate while it is fresh, a fast climb back to the
  good rate after, one backlog reset per stall without halving, no further cuts under a flat
  delay. See "Delay bursts vs capacity drops" below.
- **Backlog reset** needs nothing delivered for 500 ms (`DELIVERING_US`), not one empty report:
  a Wi-Fi stall delivers nothing for 50-150 ms, then everything, and the reset (a keyframe into
  the queue, rate halved) made the real call's 2 Mbps -> 500 kbps in one step.
- **No backlog reset while draining** (and the last report showed bytes arriving, and not
  stale, above): with `video::net`'s ack-based backlog the queue in the path counts as backlog, and resetting
  the group can't pull bytes back out of the network; it only threw away frames and sent
  another keyframe into the queue. A stalled receiver (nothing delivered) still resets.
- The ladder follows the estimate (at the second look). Measured and rejected: holding the
  old size until the drain ends (so the smaller keyframe goes into an empty queue). With real
  AV1, libaom at 540p can't get near a 50 kbps target; the queue grew and the AV1 switch
  scenario froze 5.4 s with 9.7% loss. A small picture is what drains.
- Receiver reports every 50 ms (was 100 ms) over a sliding 100 ms window: same wire message,
  ~260 B/s of control traffic. 100 ms reports with the same rules: real-AV1 switch p95
  1757 ms / 1.7 s frozen vs 1220 ms / 1.4 s. A single starved report (no confirmation)
  gave the best drop numbers but false positives under 3% loss (a lost packet stalls the
  stream about an RTT) and at call start.

What it cannot fix: what was sent between the drop and the first report that shows it
(~150 ms, ~1 s of queue at 200 kbps) still has to drain at the new link rate. So the bigger
bottleneck buffer doesn't matter any more (256 KB scenario: the queue peaks below 64 KB).

## Simulation

`cargo test -p video --release sim -- --nocapture`. Deterministic: virtual clock, seeded RNG,
a bottleneck link (rate, delay, jitter, loss, buffer, outages, rate changes) and a QUIC model
(per-stream retransmission, newest-first scheduling, resets, loss-based cwnd, pacing).

**Backlog model** (`Scenario::backlog`): `Unacked` (default) reports what `video::net`
reports, bytes written more than srtt + 40 ms ago and not acked by the receiver app (so it
includes the bottleneck queue); `Unsent` is QUIC's unsent bytes, what the sim used before.
Stream resets now travel behind the bottleneck queue, as RESET_STREAM does; before, a reset
overtook the queued bytes of its group and the receiver dropped them. `Report::queue` /
`Report::reports` trace the bottleneck queue and the rate reports; `SIM_TRACE=1` prints them
around the drop, `SIM_BACKLOG=unsent`, `SIM_DRAIN=<pct>`, `SIM_STARVE=<pct>` for experiments.

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

Drain (V6). "ladder" = the table above (old sim); "+sim fixes" = the same controller on the
`Unacked` backlog model with resets behind the queue; "drain" = this rate control. Latency
p50 / p95 ms over the whole run, frozen (> 250 ms gaps), wire kbps where it moved.

| Scenario | ladder | ladder + sim fixes | drain | sizes (drain) |
|---|---|---|---|---|
| clean 2 Mbps | 65 / 99, 0, 1679 kbps | 65 / 99, 0 | 64 / 95, 0, 1645 kbps | 360 -> 540 at 3.6 s |
| 3% loss | 53 / 110, 0, 1010 kbps | 53 / 110, 0 | 54 / 110, 0, 1085 kbps | 360 -> 540 -> 360 (24.5) -> 540 -> 360 |
| 2 M -> 300 k at 10 s | 60 / 844 (max 1133), 0 | 60 / 842, 0 | **57 / 86 (max 587), 0** | 540 -> 180 at 10.2 s |
| 2 M -> 300 k, 256 KB buffer | - | 60 / 842, 0 (queue < 64 KB) | **57 / 86, 0**, queue peak < 64 KB | as above |
| 500 ms outage | 64 / 96, 0.7 s | 64 / 100, 0.5 s | 64 / 96, 0.6 s | 360 -> 540 |
| relay 75 +-20 ms, 1.5 Mbps | 131 / 177, 0, 1272 kbps | 131 / 177, 0 | 128 / 160, 0, 1199 kbps | 360 -> 540 |
| real AV1, 2% loss + outage | 40 / 84, 0.6 s | 48 / 110, 0.6 s | 46 / 99, 0.7 s | 360 -> 540 -> 360 |
| 2 M -> 200 k -> 2 M, 40 s | 58 / 1829 (max 2223), 0.9 s | 56 / 1362, 1.2 s | **55 / 89 (max 929), 0.3 s** | 540 -> 180 (10.2) -> 360 (23.4) -> 540; 4 switches |
| slow device | 56 / 62, 0 | 56 / 62, 0 | 56 / 63, 0 | 360p, 0 switches |
| throttles 2x at 15 s | 58 / 78, 0 | 58 / 78, 0 | 57 / 74, 0 | 540 -> 360 at 15.1 s |
| real AV1, 2 M -> 200 k -> 2 M, 20 s | 52 / 2493, 3.0 s | 46 / 2083, 2.5 s | **48 / 1220, 1.4 s** | decoded 360, 540, 180, 360, 540 |

Same numbers with `SIM_BACKLOG=unsent` except the outage (56 / 80 ms, 0.6 s) and the AV1 loss
run (43 / 95 ms, 0.6 s). Open issues:
- Real AV1 drop: 1.4 s frozen is the 540p frames queued before the drop is noticed (libaom
  frames at 540p / 1.4 Mbps are bursty; the 64 KB buffer fills in ~0.2 s). Shorter needs an
  earlier signal (per-packet arrival timing at the receiver, i.e. GCC's transport-wide
  feedback; QUIC ack timing in `net`) or AV1 reference scaling (libaom `resize_mode`).
- ~~Real transport steps down in two steps on the 300 kbps drop~~: fixed by the second look
  (V11, below).
- The ladder only steps down for CPU after half a second of frames at the new speed (V11:
  median of a full 1 s window, was p90 of 10 frames); a sudden 2x slowdown costs ~0.6 s of
  late frames (throttle scenario: 540p -> 360p at 15.7 s, was 15.1 s).

**V11 sim model changes** (all scenarios below use them):
- The receiver app is pull-based like `VideoRecv` + the core's 2-slot decoder channel: QUIC
  data is read only while the decoder queue has room, acks (every 10 ms) and reports only go
  out while it is being awaited, a reset drops the unread data of its stream. Before, the
  sim read everything on arrival, so a slow decoder couldn't be modelled.
- Loss detection like QUIC: a dropped packet is found lost when a later packet is acked and
  9/8 RTT have passed, or at the PTO (srtt + 4 rttvar + 25 ms max_ack_delay) for a tail loss.
  Before: always ~1.1 RTT. Tail losses now stall a stream ~100 ms, as on real iroh.
- The congestion window only grows while at least half used (QUIC doesn't grow it when
  app-limited). Before, it grew on every ack and never limited.
- `Device::spikes` (preempted frames), `Device::busy_desktop()`, `Scenario::decode_changes`
  (slow receiver), `Scenario::receiver_clock_ppm`, `KeyCauses` in the report (why each
  keyframe: start, rate, cpu, request, backlog, other), `SIM_TRACE=reports`.

With these the existing scenarios move little (same controller, loss 3%: 1085 -> 1017 kbps,
p95 110 -> 110); the sim's loss case is still more optimistic than real iroh, see below.

**V11 before / after** (`cargo test -p video --release --features net sim -- --nocapture`;
"before" = V6 controller on the V11 sim model; latency p50 / p95 / max ms over the run,
frozen > 250 ms, wire kbps, keyframes by cause):

| Scenario | before | after |
|---|---|---|
| slow start (4x slower for 1 s), new | 180p from 0.3 s for the whole call, 530 kbps, keys 2 (cpu 1) | 360 -> 540p at 4.2 s, 1766 kbps, keys 2 (rate 1) |
| busy desktop steady call (50 s), new | 360 -> 180p at 1.6 s (cpu), 720p never; keys 4 (cpu 2) | 540p 3.6 s -> 720p 7.8 s, stays; keys 3 (rate 2), 1624 kbps |
| busy desktop, CPU fix only (intermediate) | | flaps 540 <-> 360 every 4-7 s: keys 9 (rate 8) |
| slow receiver (25 fps decode 5-15 s), new | 54 / 1235 / 1486, backlog 3, recovers 540p at 22 s | 54 / 395 / 475, backlog 3, recovers 540p at 22.9 s |
| receiver clock 0.5% fast, new | stuck at 180p from 8.9 s, 331 kbps (V11 without the learned baseline) | 180p 9.3 s -> 360p 14.8 -> 540p 18.2, 958 kbps |
| clean 2 Mbps | 65 / 98 / 146, 1655 kbps, keys 2 | 68 / 105 / 146, 1719 kbps, keys 2 |
| 3% loss | 53 / 110 / 196, 1017 kbps, keys 5, 540 -> 360 -> 540 -> 360 | 61 / 125 / 245, **1281 kbps, keys 2**, 540p from 5 s |
| 2 M -> 300 k | 56 / 92 / 588, 180p at 10.2 s | 61 / 110 / 610, 180p at 10.3 s (one step) |
| 2 M -> 200 k -> 2 M, 40 s | 55 / 90 / 930, 0.3 s | 57 / 99 / 963, 0.3 s, same sizes |
| 500 ms outage | 64 / 95 / 192, 0.6 s | 66 / 103 / 170, 0.6 s |
| relay 75 +-20 ms, 1.5 Mbps | 126 / 148 / 239, 1120 kbps | 133 / 183 / 238, 1313 kbps |
| throttles 2x at 15 s | 540 -> 360 at 15.1 s | 540 -> 360 at 15.7 s |
| real AV1, 2% loss + outage | 38 / 85, 0.6 s, 537 kbps | 41 / 91, 0.7 s, 685 kbps |
| real AV1, 2 M -> 200 k -> 2 M | 49 / 1220 / 1736, 1.4 s | 49 / 1459 / 2017, 1.7 s |

Costs: clean/relay run at a higher rate with ~10-35 ms more p95 (the window-minimum queueing
delay sees a building queue a frame later); the AV1 drop freezes 0.3 s longer (its 180p
keyframe waits for the second look behind 540p frames).

The first version used the congestion window for the bitrate; the simulation showed
bufferbloat (p50 256 ms on a clean link, 1.5 s after a bandwidth drop), which is why rate
control is delay-based.

### Wi-Fi delay spikes (2026-10-16, `fix/wifi-spike-estimator`)

**The trace** (real call, Pixel 3a <-> Tiger Lake laptop on one 2.4 GHz home router, direct
path; per-second app stats, no per-report log). Neither CPU busy (laptop's busiest thread ~32%,
Pixel <= 23%), 1 packet lost in 2 min. RTT 7-15 ms at 500 kbps; seconds at 40-65 ms every 10-30 s,
peaks 82 and 127 ms, sometimes 2-3 s in a row; single 100 ms windows delivering 0-184 kbps,
then a burst (up to 2.5 Mbps while sending 2 Mbps); one report of 522 ms queueing. While the Pixel
sent 2 Mbps its RTT sat at 20-65 ms (7-11 ms at 500 kbps), so its uplink may at times be only a
little above 2 Mbps. Each spike took the sender to 180p and 10-20 s to climb back:

| when | sender | estimate | rule |
|---|---|---|---|
| 21:12:30 | Pixel | 2000 -> 500 kbps | two backlog resets (drops 0 -> 2), each halving: an empty report ended the "no reset while draining" exemption |
| 21:12:31 / 21:12:48 / 21:13:38 | Pixel | -> 354 / 109 / 154 kbps | overuse (q 165 ms; or 3-6 overuse reports): drain and estimate cut to the delivered rate, which follows our own rate |
| 21:14:07-10 | Pixel | 500 -> 100 kbps | drops 4 -> 8 with nothing delivered, RTT 127 ms |
| 15:43:00-01 UTC | laptop | 2000 -> 663 -> 350 | starvation at 720p15 (1-2 frames per 100 ms window) |
| 15:44:04-08 UTC | laptop | -> 432 -> 155 | 12 overuse reports, then a backlog reset; also one `reason=cpu` step at ~32% CPU (not looked into) |

**Sim model**: `Link::spikes` `(from, to, extra_us)`: packets leaving in the window take 60-100%
of `extra_us` longer, both directions, in order, nothing lost (a stall, a second of delay, a
burst). Scenario `wifi`: 20 Mbps, 4 ms +- 4 ms one way, 512 KB buffer, 1 s spikes of 60, 45, 130
and 85 ms at 8, 30, 52 and 75 s, 90 s, phone model; the same at 2.4 Mbps and 1.6 Mbps (128 KB
buffer) and with real AV1 (30 s, 85 ms at 8 s, 130 ms at 20 s). From 5 s: mean target rate,
share of frames shown at >= 540p, lowest size; latency p50 / p95 / max over the run.

| Scenario | before | after |
|---|---|---|
| wifi, 20 Mbps | 22/35/172; 1540 kbps, 53% >= 540p; 360p at 8.7 s (est 357 kbps) until 43.7 s, 720p24 -> 540p at 52.8 s | 32/38/216; **1955 kbps, 98%**, 720p24 from 12 s, no step down |
| wifi, 2.4 Mbps | 34/79/199; 1209 kbps, 48%; 360p at 9.1 s, 180p at 52.8 and 75.8 s (est 100-132 kbps) | 64/92/193; **1827 kbps, 98%**, 720p24 from 14 s; between spikes p95 86 ms, max 163 (was 69 / 155) |
| wifi, 1.6 Mbps (new) | not measured (same rules apply) | 50/108/197; 1176 kbps, 66%, 180p at 52.9 s (the old rules: see below); between spikes p95 101 ms |
| real AV1, wifi | 14/95/184; 578 kbps, 11%, 180p at 8.8 and 21.3 s | 28/104/184; **1879 kbps, 95%**, 720p24 from 12 s, 0 decode errors |

Every other scenario (latency p50 / p95 / max, frozen, wire kbps, keyframes, sizes; before = this
branch's base):

| Scenario | before | after |
|---|---|---|
| clean 2 Mbps; 2 M -> 300 k; deep buffer; 500 ms outage; relay; 2 M -> 200 k -> 2 M; drift; slow receiver; slow start; size hint; hw keyframes; 2 M -> 700 k; real AV1 2 M -> 200 k -> 2 M | (each its own numbers) | unchanged, same numbers |
| 2 M -> 1 M (1b) | 64/120/285, 1265 kbps; 540p at 24.3 s | 64/115/285, 1278 kbps; 540p at 21.5 s |
| 3% loss, 2 Mbps | 62/131/234, 1249 kbps | 58/122/221, 1191 kbps |
| 1.5 M, 3% loss, 60 s | 57/120/206, 1083 kbps, 0 switches after 10 s | 59/122/204, 1133 kbps, 0 |
| busy desktop | 70/108/208, 1662 kbps, 720p at 12.0 s | 70/109/221, 1655 kbps, 720p at 13.3 s |
| 2 M / 900 k flapping | 68/122/261, 1184 kbps | 68/122/261, 1187 kbps |
| throttled 2x | 69/95/158 | 69/96/158 |
| slow device | 77/112/155 | 78/107/152 |
| fps hint | 66/90/138, 1921 kbps | 64/89/138, 1905 kbps |
| real AV1, 2% loss + outage | 50/130/224, 0.6 s, 360p | 48/124/224, 0.6 s, 540p from 11.7 s |

Trade-offs: on a fast link the spike's own delay is now sent through (p50 22 -> 32 ms at 20 Mbps:
2 Mbps at 720p instead of 360p); near the link's capacity (2.4 Mbps) a spike adds ~20 ms to p95
between spikes (a queue of ours drains at 70% instead of 25%). Below the top rate (1.6 Mbps) the
link is full when the spike comes, so no burst above the old rate shows and the stall reads as a
slower link: the old rules, down to 180p for 10 s. Telling those apart needs more than the delivered
rate (per-packet arrival timing). Loss and keyframe bursts sometimes match the spike signature
too (3% loss: 10-20 a minute); their numbers above stay within noise.

Run: `cargo test -p video --release --lib sim:: -- --nocapture` (`SIM_TRACE=reports SIM_FROM=<s>
SIM_TO=<s>` prints the reports in a window).

### Delay bursts vs capacity drops (2026-10-17, `fix/rate-recovery`)

**The call**: Pixel 3a <-> laptop, same home router, direct path. Both directions sat at
180p / 100-190 kbps for the whole call (the day before, same pair: 1-2 Mbps at 540p). Router
ping during the call: 5% lost, 42 of 285 replies over 100 ms, max 1023 ms (2.5-5.6 ms and no
loss without a call); audio on the same connection: RTT 8-48 ms. A working LAN with delay
bursts, not a 150 kbps link. Laptop log per second: 500 kbps -> 100 at 1 s (`overuse=1
drops=1`), then 100, 103, 28, 25, ... and later +10 kbps/s climbs knocked back to 100 with each
backlog reset (`drops` and `key` going up together, 01:39:27-31: four in four seconds with no
overuse report). Pixel: 500 -> 100 -> 25 kbps in 2 s, `draining=true`, `delivered_kbps=0` and
`q_ms` 469-756 for seconds.

**Why** (all present before 068fd7e; that commit's spike rule only fires at rates where a
burst shows above `starve_min_bps`):
- A burst delivers nothing for a while, so nothing is acked and the backlog passes 300 ms at
  our rate: a **backlog reset halved the rate** (and sent a keyframe into the same stall).
- **Overuse cut to 85% of delivered**, which was 0 in the stall, or followed our own drain;
  **every 200 ms again** while the delay stayed flat, so 1.4 Mbps fell to 200 kbps in a second.
- The **drain** went to its floor, 25 kbps, with nothing getting through.
- The **ramp back** needed < 15 ms queueing (the call's RTT sat at 15-40 ms between bursts) and
  was capped at 1.5x the last 100 ms delivered window, which at 100-200 kbps is one or two
  packets: ~10 kbps a second. A burst every few seconds kept it at the floor; and with
  `up_after_down_us` (10 s) + `up_hold_us` (5 s) the ladder never got back above 180p.

**Principle**: a link that got slower still delivers, at its rate, while the delay grows. A
transient burst delivers *nothing* for longer than loss recovery explains, then everything
late. Nothing getting through is no evidence about capacity; a queue that builds while the link
delivers steadily is.

**Rules** (`media::Sender`):
- **Stall**: reports with < 1% of the estimate delivered for 1.5 round trips (at least 140 ms),
  or a report that late. The RTT comes from the transport (`Sender::set_rtt`: `video::net`
  passes the path RTT, the sim its srtt), the lowest of the last 5-10 s (a burst inflates the
  smoothed RTT just when this needs the path's own). On a 250 ms relay a lost packet's
  retransmission is not a stall.
- An overuse episode with a stall in it (or within 500 ms before) cuts the estimate and the
  drain to no less than **50% of the rate before**, and doesn't lower the remembered good rate.
  Only while the stall is fresh: 1 s (or two round trips) after its last empty report, a queue
  still standing is ours and the episode goes back to the normal rules (a 5 Mbps -> 600 kbps
  drop that came with a stall had stayed at ~2 Mbps with ~1 s of queue).
- When the episode ends, the ramp climbs back to the **good rate** (the rate the path last
  carried with no queue) at **+25% per smoothed round trip** (at least 100 ms), starting a round
  trip after the last cut. An episode without a stall sets the good rate to its median delivered
  rate (capacity) and the ramp is the normal one.
- **Backlog reset while the path is quiet** (an empty or late report) after a healthy one: one
  per stall, no rate halving.
- **Later cuts in an episode** only while the delay grows (by 20 ms since the last cut), the
  link is starved, or it delivers > 110% of what we send (our queue draining at its rate). A
  flat delay keeps the drain instead of chasing our own lowered delivered rate.
- Below 400 kbps the normal ramp's cap uses the most delivered in the last 0.5 s.
- Ladder: leaving a rung entered within the flap window through a frame-rate step inside it
  now blocks the whole rung (the cut now comes in steps; `adapt_flapping_link`).

**Sim**: `Link::spike_loss` (loss during spikes of >= 50 ms), `HwCodec::key_min` (hardware
keyframes don't shrink with the target; Pixel VP8 at 25 kbps made 1.5-4 KB ones),
`network_matrix` (`SIM_MATRIX=<name>`, `SIM_MATRIX_SEED=<n>`, `SIM_MATRIX_NOCHECK=1`). Each
matrix scenario runs 90 s from call start (so the start is included), three seeds averaged;
fake codec, phone encode model:

1. **lan**: 10 Mbps, 3 ms +-6 ms one way, a 100-600 ms burst every 2-7 s (one in the first
   second) with 5% loss, 10-25 ms stretches in between. The best case.
2. **cafe**: 2-6 Mbps changing every 3-8 s, 8 ms +-10, 2% loss, a 100-1000 ms burst every
   3-10 s (5% loss in it).
3. **lte**: 30 ms +-10 one way, 1-8 Mbps changing every 2-5 s, 384 KB buffer (0.4 s at 8 Mbps,
   3 s at 1 Mbps), 0.5% loss, a 1-1.7 s handover stall every 15-30 s.
4. **relay**: 120 ms +-20 one way (RTT ~250 ms), 3 Mbps, 0.5% loss, a 100-600 ms burst every
   3-8 s (3% loss in it).
5. **weak**: a steady 650 kbps, 20 ms +-15, 0.5% loss, 64 KB buffer.
6. **step**: 5 Mbps, then 600 kbps from 15 s on (row 2: 20-90 s). **step+stall**: the same drop
   arriving with a 300 ms window of 800 ms delay and 5% loss.

Mean target kbps, % of frames >= 540p / >= 360p, latency p50 / p95 ms, freezes (> 250 ms) per
run and seconds frozen; before = b83e596:

| Scenario | before | after |
|---|---|---|
| lan | 411 kbps, 0% / 24%, 26 / 315 ms, 22.0 (10.8 s) | **1513 kbps, 76% / 100%**, 40 / 236 ms, 25.0 (14.5 s) |
| cafe | 478 kbps, 0% / 24%, 27 / 134 ms, 18.3 (12.1 s) | **1494 kbps, 76% / 100%**, 50 / 146 ms, 20.0 (15.4 s) |
| lte | 877 kbps, 9% / 62%, 56 / 94 ms, 5.0 (5.8 s) | **1403 kbps, 86% / 100%**, 77 / 144 ms, 7.3 (7.0 s) |
| relay | 374 kbps, 0% / 36%, 153 / 506 ms, 37.7 (16.8 s) | 425 kbps, 0% / 38%, 156 / **597** ms, 44.7 (18.5 s) |
| weak | 533 kbps, 0% / 100%, 72 / 129 ms, 0 | 552 kbps, 0% / 100%, 74 / 126 ms, 0 |
| step | 711 kbps, 11% / 100%, 62 / 115 ms, 0 | 711 kbps, 11% / 100%, 63 / 113 ms, 0 |
| step, 20-90 s | 539 kbps, 66 / 116 ms, 0 | 539 kbps, 67 / 114 ms, 0 |
| step+stall | 721 kbps, 11% / 100%, 63 / 113 ms, 1.0 (0.9 s) | 722 kbps, 36% / 100%, 70 / 117 ms, 1.7 (1.4 s) |
| step+stall, 20-90 s | 539 kbps, 66 / 116 ms, 0 | 530 kbps, 76 / 117 ms, 0 |

`lan_delay_bursts_seeds` (120 s, six seeds, software / hardware encoder): 400 / 399 kbps ->
1406 / 1106 kbps; >= 540p 2% / 2% -> 69% / 42%; time under 150 kbps 18.9% / 18.2% -> 1.7% /
2.1%; median time back to 80% of the pre-burst rate 2.2 / 2.0 s -> 0.9 / 0.8 s; frozen 12.8 /
12.0 s -> 17.8 / 16.1 s.

Existing scenarios (p50 / p95 / max ms, wire kbps; unchanged ones left out): 3% loss 58/122/221
-> 60/129/221, 1191 -> 1253 kbps; 1.5 M 3% loss 59/122/204 -> 59/125/219; 500 ms outage
68/109/181 -> 71/114/287; real AV1 2% loss + outage 48/124/224 -> 51/164/331 (0.6 -> 0.8 s
frozen); wifi 1.6 M 50/108/197 -> 65/112/246, 1228 -> 1323 kbps, keys 8 -> 3; wifi 2.4 M
64/92/193 -> 66/93/199, 1864 -> 1923 kbps; 2 M / 900 k flapping 68/122/261 -> 65/121/309,
1187 -> 1264 kbps; throttled 69/96/158 -> 71/108/158; ladder dip 55/127/1006 -> 55/114/1006;
drift 46/72/138 -> 46/56/138. Clean, 2 M -> 300 k, deep buffer, relay 1.5 M, real AV1 switch and
Wi-Fi spikes, slow device/receiver/start, hints, hw keyframes: within noise.

**Trade-offs** (no one rule serves all):
- More rate means more data in flight when a burst comes: on the burst scenarios freezes go up
  (~3-5 s per 90-120 s) and p50 by 10-20 ms, for 3-4x the rate and 540p instead of 180p.
- **Relay (RTT 250 ms)**: little gain (+14% rate) for +90 ms p95 and +7 freezes. The bottleneck
  there is QUIC's congestion window (about 1 Mbps at that RTT with loss), not the link, and the
  bursts' losses take an inflated round trip to repair. Each rule costs a little there
  (ablation, each rule off alone: p95 620 -> 539-620 ms, the stall floor the most); pacing the
  climb by the smoothed RTT got back a little (620 -> ~590 ms). Telling a burst loss from capacity at high RTT
  needs per-packet timing (QUIC ack times in `net`), not the 100 ms delivered rate.
- Outages that really lose packets (the 500 ms outage, real AV1 loss + outage) are stalls too:
  draining at 50% instead of 25% costs ~40-100 ms on the worst frames after one.
- LTE: p95 94 -> 144 ms at 1.6x the rate (a bigger buffer filled a little at each step up).
- Not device-validated: the next real call should show `overuse`/`drops` no longer climbing
  together with the rate at the floor.

Run: `cargo test -p video --release --lib sim:: -- --nocapture`; `SIM_MATRIX=1` per-seed lines.

### A busy sender is not a queue (2026-10-17, `fix/laptop-to-phone`)

**The call** (after the rules above): Pixel 3a <-> Tiger Lake laptop, home 2.4 GHz Wi-Fi, VP8
both ways. Pixel -> laptop (hardware encoder): 540p, 1.9 Mbps, overuse count flat. Laptop ->
Pixel (libvpx): 180p for 75 of 80 s, ~250 kbps, an overuse report every ~5 s (69 -> 88 in 30
s), queue readings 40-60 ms, no drops; each cycle climbed to the 180p ceiling (500 kbps) and was
cut to 115-280.

**Cause** (laptop log, per second): the seconds with overuse are the seconds the laptop *sent*
fewer frames (18-20 fps instead of 26-30; corr(fps, overuse) -0.37, corr(fps, queue) -0.47) and
its own decode+render p75 rose to 9-16 ms (5-7 ms otherwise; corr with fps -0.61): the laptop was
busy, and its camera -> encode -> send path ran tens of ms late. The receiver measured queueing
from the **capture** time, so that lateness read as a queue in the path. The phone's hardware
encoder takes a steady ~3 ms, so the other direction never saw it. The phone's receive side
can't be ruled out from these logs (no per-frame timing there; see below), but the sender-side
correlation is direct. Capture stamps also read each step up's longer encode as a queue.

**Changes**:
- Frames are stamped when handed to the transport (`encode_loop` -> `VideoSend::send`), as GCC's
  abs-send-time does. Encode time is the ladder's CPU rule's business, not the rate's.
- Ladder: dips below the next step's threshold adding up to <= 25% of the 5 s hold
  (`up_dip_pct`) no longer restart it (a burst's 50% cut every few seconds kept 180p forever).
- Debug log `video timing` once a second (`RUST_LOG=p2pcore=debug`): encode p50/p90/max,
  capture -> send p50/p90/max, frames, camera frames replaced before encoding, receive waits on
  the decoder > 5 ms (count, max; arrivals are stamped late while it waits).
- Sim: `Device::busy` (every frame up to N ms slower for a while), `Scenario::stamp_capture` /
  `SIM_STAMP_CAPTURE=1` (the old stamps), `SIM_UP_DIP=<pct>`.

`busy_sender_is_not_a_queue` (10 Mbps LAN, `busy_desktop` encoder, busy 0.5-2 s every 2-7 s with
frames 20-60 ms late; 90 s, three seeds):

| | kbps | >= 540p | >= 360p | p50 / p95 ms | overuse reports / run |
|---|---|---|---|---|---|
| capture stamps (before) | 414 | 1% | 20% | 12 / 70 | 162-284 |
| send stamps, no dip rule | 1124 | 30% | 55% | 18 / 85 | 0 |
| send stamps + dip rule | **1814** | **78%** | **100%** | 26 / 99 | 0 |

(Without the dip rule two seeds stayed at 180-360p: busy periods also trip the CPU rule.)

Network matrix and existing scenarios (before = 1d949e1, same worker; kbps, >= 540p / >= 360p,
p50 / p95 ms, freezes (s)):

| Scenario | before | after |
|---|---|---|
| lan | 1513, 76% / 100%, 40 / 236, 25.0 (14.5) | 1477, 68% / 100%, 40 / 243, 25.7 (14.7) |
| cafe | 1494, 76% / 100%, 50 / 146, 20.0 (15.4) | 1530, 89% / 100%, 55 / 132, 18.3 (16.6) |
| lte | 1403, 86% / 100%, 77 / 144, 7.3 (7.0) | 1550, 86% / 100%, 79 / 147, 6.3 (7.4) |
| relay | 425, 0% / 38%, 156 / 597, 44.7 (18.5) | 406, 0% / 35%, 156 / 568, 40.0 (18.3) |
| weak | 552, 0% / 100%, 74 / 126, 0 | 557, 0% / 96%, 74 / 131, 0 |
| step / 20-90 s | 711, 63 / 113; 539, 67 / 114 | 712, 63 / 114; 540, 67 / 116 |
| step+stall / 20-90 s | 722, 36% 540p, 70 / 117, 1.7 (1.4); 530 | 733, 10% 540p, 62 / 120, 1.7 (1.5); 540 |
| lan seeds sw / hw (120 s, 6 seeds) | 1406 / 1106 kbps, 69% / 42% >= 540p, frozen 17.8 / 16.1 s | 1489 / 1516, 78% / 78%, 19.4 / 17.8 s |
| clean 2 Mbps | 1559 kbps wire, 70 / 108 | 1681, 78 / 122 |
| wifi 1.6 M (calm p95) | 1205 kbps, 102 ms | 1322, 120 ms |
| 2 M -> 300 k (worst after 11.5 s) | 59 / 112, max 638 | 64 / 134, max 1220 (drained by 11.8 s) |
| 2 M -> 200 k -> 2 M | 55 / 114, max 1006, 0.3 s frozen | 56 / 568, max 1912, 1.6 s frozen |

**Trade-off**: the capture stamps' encode time (+6 ms at 540p, +13 at 720p after a step up)
kept the rate ~20% under the sims' 2 Mbps links, which equal our `max_bps`. Without it the rate
runs at ~1.9 Mbps, and a sudden drop to 200-300 kbps finds ~2x as much in flight (detected
~150 ms after the drop either way): 1-2 s of stale video instead of 0.6-1 s. On links with
headroom above 2 Mbps (most LANs) nothing changes. If that matters more than the busy-sender
case, the fix is a deliberate margin (e.g. a lower ramp near the last overuse rate), not encode
time in the delay signal; the relaxed assertions carry the numbers.

**Other findings**:
- 500 kbps at 180p is the rung's `max_bps`: the estimate is clamped to it. Stepping to 360p30
  needs 325 kbps (130% of its 250 kbps floor) held 5 s, and 10 s after the last step down.
- The `cpu` step at 360p (360p24 -> 360p15, the second of the call): at 30 fps the CPU rule
  needs 30 encode samples in a second, and the laptop sent 10-22 fps then, so it could only
  fire once the rate rule had moved to 24 fps; it then saw a median over 25 ms (60% of 24 fps's
  frame time). libvpx speed 6 (`cpu-used -6` is fixed speed 6, not automatic) at 360p should
  take a few ms on that CPU; together with its 540p decode at 5-15 ms (~2 ms expected) the
  laptop was starved of CPU then. Wall-clock encode time counts preemption, which is right
  for "can this machine keep up", so nothing changed there; the `video timing` line shows it.
- libvpx at 180p delivered ~50% of a 100-180 kbps target (Pixel received 40-60 kbps), and the
  normal ramp is capped at 1.5x delivered, so the estimate sat at 100 kbps for up to 6 s
  (GCC does the same: no evidence the link carries more). Left as is.
- The VP8 decoder on Android (`c2.qti.vp8.decoder`) waits up to 20 ms for each picture; while
  the 2-frame decoder channel is full `run_recv` doesn't read the network, so arrivals are
  stamped late. Not visible in these logs; `recv_wait` in the timing line measures it.

**Next device call**, capture with `RUST_LOG=info,p2pcore=debug` on the laptop and logcat for
`p2pcore` debug on the phone: `video timing` lines both sides (a laptop `capture->send p90`
under ~15 ms and phone `recv_wait>5ms` near 0 confirm both sides are clean), `video send`
overuse counts both ways, the CPU governor/power profile and `top -H` of the laptop during the
call (what else is busy: camera MJPEG decode on the camera thread, UI), and router ping.

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

## Android app (capture, render, UI)

**Adaptation (step 3):** `RemoteVideo` reports its `onSizeChanged` size with
`setVideoView(w, h, started)` (visible = lifecycle STARTED; false again on dispose). The view
crops to fill, so the reported size is the view, not the picture. `CallVideo.watchBudget`
(registered while the call has video) feeds `PowerManager` thermal status and
`ACTION_POWER_SAVE_MODE_CHANGED` / `isPowerSaveMode` to `setVideoBudget`; `VideoIo.pull` reports
each frame's pull + bitmap time with `reportVideoRenderMs`. The line under the header uses
`videoSendLabel(stats)`. No policy in Kotlin. Needs a phone to check the thermal and power-save paths.

Files: `VideoIo.kt` (the only place frames cross the FFI), `CallVideo.kt` (state, CameraX,
`RemoteRenderer`), `CallScreens.kt` (`VideoCallContent`), Settings "Video in calls".

- **State**: `CallVideo` implements `VideoEvents`; the call screen shows the video layout once
  `negotiated` and either camera is on, otherwise the voice layout with a Camera button (only
  when negotiated; no button for an audio-only call). Camera off by default in every call.
- **Capture**: CameraX `ImageAnalysis`, `YUV_420_888`, `STRATEGY_KEEP_ONLY_LATEST`, 640x480 (4:3,
  closest lower), front camera by default (flip button rebinds), plus a `Preview` into a
  `PreviewView` (TextureView mode) for the self-view, which CameraX mirrors for the front camera;
  the sent picture is not mirrored. Analyzer thread `tl-camera`: `VideoIo.push` hands the
  planes' direct buffers to `VideoJni.pushYuv` (full range, see Frames across the FFI): one
  call, Rust copies each plane once; the core converts, rotates and scales on the encoder
  thread.
- **Render**: `RemoteRenderer` runs a `Choreographer` on its own `HandlerThread`
  (`tl-video-render`): every vsync `VideoJni.pullRgba` into a direct buffer (Rust converts to
  RGBA); on a new frame `copyPixelsFromBuffer` into one of two alternating `Bitmap`s, published
  to a Compose `Canvas` that draws it cropped to fill the screen. Nothing on the main thread
  but the draw. Later: the `ANativeWindow` path above (no Bitmap, no per-frame JNI).
- **Permission**: CAMERA is asked for when the user first taps the camera button (it is also
  used for QR scanning); refused -> "Camera permission needed - they can still hear you" and
  the call goes on audio only.
- **Lifecycle**: capture only while the call screen is visible. CameraX is bound to the
  `CallActivity` lifecycle and unbound when the self-view leaves composition (camera off, call
  ended). `onStop` (home, minimise, screen off) turns the camera off in the core (`set_camera
  (false)`, so the peer sees the avatar rather than a frozen picture) and `onStart` turns it
  back on. So `CoreService` needs no `camera` foreground-service type (Android 14 requires it
  only for capture from the background, which we don't do).
- **Audio route**: turning the camera on moves audio to the speaker (a video call is held
  away from the ear; that also releases the proximity wake lock).
- **Debug hooks** (`DebugReceiver`): `video_stats`, `camera on=1|0`, `flip`, `video_enabled
  on=1|0`, `callscreen`, `video_path jni=1|0` (frames through `VideoJni` or UniFFI),
  `video_io_bench n=200` (push cost per path without a call); during a video call
  `video_stats` is also logged every second (per-frame wall and thread-CPU times).

**E2E** (`scripts/e2e_android_video.py`, emulator + `p2p-peer`): phone calls desk, desk answers
with `--video-pattern 640x480`, the phone turns on its (virtual scene) camera; checks frames
sent/decoded/rendered on both sides, no decode errors, both see the other's camera on; saves a
screenshot of the call screen and the desk's last decoded frame; prints the phone's per-frame
costs (`--path uniffi` to measure the UniFFI path). Then desk calls phone with `--no-video`: the
phone sees `negotiated=false` and the call carries audio.

Results on the x86_64 emulator (`osvauld-api35`, virtual-scene camera, host load 2.5-15 from
other builds; `python3 scripts/e2e_android_video.py`, both `--path`s): passes. Screenshot
`docs/video-screens/android-video-call.png` (desk's pattern full screen, the emulator's camera in
the corner), and `android-video-call-desk.png` (what the desk decoded from the phone's camera).
HOME during a video call -> `local=false` at once, CameraX released (no active camera client),
the desk sees `remote=false`; back to the call screen -> camera on again, new keyframe.

Per frame (V9, before the JNI path; now see Frames across the FFI): `push_video_frame` of
packed I420 cost 4.6-5.7 ms (15-46 under load) on the emulator and capped the phone's capture
at 10-30 pushed frames/s when the host was busy; the I420 pull 1.3-2 ms and the Kotlin
conversion 1.7-2.6 ms at 240x180. With `VideoJni`: push 0.11 ms, pull + bitmap 0.15 ms thread
CPU.

- **Colour**: V9 passed the camera's full-range Y as limited range: washed out, whites
  clipped (in its `android-video-call-desk.png`, the desk's decoded frame, 41% of the pixels,
  the sky, were exactly 255). Now `full_range = true` (`Range::Full`, compressed to limited
  range on the encoder thread): the sky is 249 with its lavender tint, nothing clipped (luma
  7-254).
- **Rate control on the emulator**: the phone sent 180x240 at its 500 kbps ceiling (the ladder
  doesn't step up: the emulator's encode times, 1-55 ms, predict too slow for 360p). The desk's
  video to the phone ramped to 480x360 at 1.2 Mbps, then at ~4 s dropped to 240x180 at the
  100 kbps floor and stayed there for the remaining 20 s (0 decode errors, ~20 fps decoded).
  So the phone's receiver keeps reporting queueing delay: the emulator's NAT (slirp) path and a
  busy VM. Not reproduced on real devices yet; worth a check on the phone before blaming
  `media` (the simulated controller recovers from drops in a few seconds).
  **V11**: two ways `media` could get stuck like this are now simulated and handled (see Rate
  control): a receiver clock running fast (the VM's; 0.5% reads as ~50 ms of queueing
  forever and the sim sat at 180p) -> the sender's learned baseline; and a receiver that
  decodes slower than we send (~20 fps decoded here; `VideoRecv` only reads when the decoder
  takes frames, so it looks like a slow link whose queue no bitrate drains) -> the drain no
  longer spirals below 25 kbps, and a stale queue is reset to a keyframe (latency bounded
  ~0.5 s). What the sender can't fix: a receiver that keeps decoding fewer frames than we
  send gets a keyframe every ~2 s; the real fix is a lower frame rate (the core would ask
  the sender whether to encode a frame; not built).

## Desktop (iced, `crates/desktop`)

**Adaptation (step 3):** `video_surface` records the remote picture's contained size times the
window scale factor each time it is drawn; the 33 ms video tick reports it with
`set_video_view(w, h, visible)` on the first draw and on every change. `visible` is false when
the picture was not drawn for a second (stage hidden, call minimised to the strip, window
minimised). The pull cost of each remote frame goes to `report_video_render_ms`. The picture
label (top-left of the stage, fixed 220 px box) is `video_send_label` (`HD · 24 fps`,
`540p · slow network`); the HD chip itself still reads "HD". No budget on desktop. Needs a
two-instance run to see the hint arrive (resize the window, minimise it).

**Camera: V4L2 directly through `linuxvideo`** (0BSD, pure Rust: `libc`, `bitflags`, `log`,
`uoctl`; no bindgen, no libv4l, no C). Candidates:
- `linuxvideo` 0.3: chosen. Small, safe API (`Device` -> `video_capture(PixFormat)` ->
  `into_stream` -> `dequeue`, mmap buffers); last release 2025-07, less used (~45k downloads).
- `v4l` 0.14 (MIT): the widely used one (~8M downloads), equivalent API, but its default
  `v4l2-sys-mit` runs bindgen at build time (libclang on every build host); last release
  2023-05. Fine fallback if `linuxvideo` goes stale.
- `nokhwa` 0.10 (Apache-2.0, release 2026-05): cross-platform wrapper (V4L via `v4l`, MSMF,
  AVFoundation, optional OpenCV); much heavier, own frame/format abstractions and MJPEG
  decoding we don't need on Linux. The candidate for Windows/macOS later.
- PipeWire camera portal (`ashpd` + `pipewire` bindings): the only option inside Flatpak or a
  sandbox, and the way to share the camera with other apps. C library (libpipewire) plus
  bindgen; deferred until we ship a Flatpak. The capture code is one small module
  (`camera.rs`), so it can grow a portal backend next to V4L2.

Capture (`camera.rs`): first `/dev/video*` node with VIDEO_CAPTURE + STREAMING and a YUYV or
NV12 format (YUYV preferred: every UVC webcam has it), asks for 640x480 at 30 fps (the driver
picks the nearest), and on its own thread (`tl-camera`) converts each buffer to I420
(`frames::to_i420` = `video::convert::yuyv_to_i420` / NV12 view, stride from the driver),
writes a mirrored 320x240 RGBA self-view into a slot and calls `push_video_frame` (rotation
0). Stopping = dropping the handle: the thread sees the flag after its current frame and
closes the device (never joined, so a stalled camera can't block the UI). No camera found =
the call screen's camera button reads "No camera" and is disabled. MJPEG is not read: this
laptop's camera (and most UVC webcams) offers YUYV 640x480 at 30 fps; 720p30 is MJPEG-only
here (YUYV 720p is 10 fps). For 720p later: `zune-jpeg` (MIT/Apache/Zlib, pure Rust) decodes
to YCbCr, then 4:2:2/4:4:4 -> I420. Windows/macOS: compiles, no camera yet (only the test
pattern). Range: this camera's YUYV is limited range (luma 27..230 on a normal scene), so no
range conversion; `to_i420` can squeeze full range if a camera needs it.

Render (`vcall.rs`, `view.rs` `video_stage`): while the call is active, video negotiated and
either camera on, a 33 ms `iced::time::every` tick pulls the newest decoded frame
(`frames::pull` = `pull_video_frame` + `i420_to_rgba`) and wraps it in
`image::Handle::from_rgba`; the self-view slot is taken the same way. The stage replaces the
avatar: peer video large (`ContentFit::Contain`), our mirrored picture 220 px wide in the
bottom-right corner (`stack!`), the peer's avatar with "Camera off" while their camera is off;
the device pickers hide while video shows. All frame in/out is in `frames.rs`, so moving to
the core's RGBA pull (V8) is a one-function change. Buttons: Camera (toggle; only once video
is negotiated, starts off for every call), Settings > Calls > "Video in calls"
(`set_video_enabled`, default on).

Cost per frame, `cargo run -p desktop --release --example cam_probe [pattern]` (i7-1165G7,
median; second number with loadavg ~11 from other builds):

| step | thread | ms |
|---|---|---|
| YUYV -> I420 640x480 | camera | 0.15 / 0.36 |
| self-view: 2:1 scale + mirror + RGBA 320x240 | camera | 0.24 / 0.60 |
| I420 -> RGBA 1280x720 | UI | 1.52 / 3.10 |
| `Handle::from_rgba` 720p (incl. a 3.7 MB copy) | UI | 0.16 / 0.45 |

So at 720p the UI thread spends ~1.7 ms per remote frame (~5% of one core at 30 fps) plus
the GPU upload; 640x360 is ~4x cheaper. Each frame is a new image id, so iced uploads a new
texture and drops the old one; if that shows up, a custom `shader` widget with a persistent
texture (or YUV planes sampled in the shader) is the next step.

Checks: `cargo test -p desktop frames` (YUYV tight/padded, NV12 padded, full->limited, short
buffers, self-view size and mirroring). Real camera: `cam_probe` (this laptop: YUYV 640x480,
opened in 28 ms, 30.1 fps). Two instances: `scripts/desktop_video.py` (release build with
`--features test-hooks` and `-p peer`; `PROFILE=release`) runs two apps in a private headless
sway with audio on a null sink: alice calls bob (`P2P_AUTO_CALL`), bob answers, both turn the
camera on (`P2P_AUTO_CAMERA`) with the synthetic pattern (`TINLINE_FAKE_CAMERA=pattern`;
test-hooks builds only), then checks VSTATS (frames sent/decoded, no decode errors) on both
sides and saves a `grim` screenshot of both windows.

Results (release, direct over loopback): 50 s run, load ~6: 1470 frames sent and 1470
decoded each way, 0 decode errors, both sides at the camera's 640x480 (540p rung, capped at
the input size) and 2 Mbps after ~4 s, ~29 fps rendered, UI cost 0.5 ms per 640x480 frame
(pull + RGBA + handle). Screenshot: `docs/video-screens/desktop-call.png` (alice left, bob
right; peer large, own mirrored picture bottom right). A 20 s run at load ~8-10 stepped down
for encode time in the first seconds (480x360 at p90 > 20 ms with two encoders, two decoders
and two UIs on a busy machine) and stayed at 240x180 / 500 kbps for the 30 s block: the
ladder's CPU rule doing its job, but on a shared desktop a single slow start costs 30 s of
low resolution. The 50 s run also sent a keyframe every 7-10 s (1 -> 8).

**V11** (simulated as `slow_start` and `busy_desktop_steady_call`, see Simulation): the first
15 encode times after a new encoder are ignored, a step down for CPU needs the median of a
full second over budget (preempted frames don't count), and the block is 5 s, doubling on
repeats. The periodic keyframes were ladder flapping, not the receiver's stall detector
(KeyCauses: all `rate`, no `request`): an encoder preempted for 15-45 ms makes one frame
late and a 100 ms delivered window short, which read as queueing (-> 85% of a short
delivered rate, below the 720p/540p floor -> step down, keyframe; 2.5 s + 4 s later step up,
keyframe); and the 720p step-up keyframe itself counted as "sent", so the link looked
starved and the ladder stepped straight back down. Fixed by the window-minimum queueing
delay, delta-only starvation and the second look; the sim's busy desktop now stays at 720p
for 42 s with 3 keyframes. Not yet re-measured in the two-instance desktop run (it is an
e2e script; the lead's to run). Note for the core: with a 640x480 camera the 540p and 720p
rungs encode the same 640x480 picture, yet a switch between them still forces a keyframe;
`set_max_rung` to the first rung that covers the camera would avoid those.

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

With the ladder merged, before V6, the lead saw a ~235 ms freeze, 1 group reset and 3
keyframes at the 360p -> 540p step-up on the clean and relay links (`net_real_clean` failed
its keyframe count). Cause: the harness read `resolution()`, then reported the encode time
(which can step the ladder up and ask for a keyframe), then read `wants_keyframe()`: a
keyframe at the old size, then another at the new size a frame later (35 KB + 24 KB at
1.5 Mbps = ~0.3 s). Fixed in the harness (take size, rate and keyframe in one
`with_sender`, report encode time after the encode, as the sim and `core::video_call` do).
Not a transport or backlog problem.

After V6 (drain + harness fix), one run each, load 3-8:

| Scenario | real p50 / p95 | real frozen (> 200 ms) | real mean bitrate | resets / keyframes |
|---|---|---|---|---|
| clean 2 Mbps, 20 ms | 63 / 98 | 0 | 1646 kbps | 0 / 2 |
| 3% loss | 45 / 119 | 0.2 s | 721 kbps | 4 / 7 |
| 2 Mbps -> 300 kbps at 10 s | last 15 s 65 / 97 | 0.43-0.47 s | last 10 s 252 kbps | 2-3 / 4-5 |
| 500 ms outage (both ways) | 62 / 93 | 0.66 s | 1593 kbps | 2 / 4 |
| relay 75 (+0-20) ms, 1.5 Mbps | 127 / 174 | 0 | 1202 kbps | 0 / 2 |

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
  ~1 RTT, or use a robust minimum over the report window). **V11: done** (window-minimum
  queueing delay, second look with a median), see below.
- **V11 on real iroh** (two runs, machine load 20-68 from other builds, so RTTs ran 50-90 ms
  over the 40 ms path; "V6" = the table above):

  | Scenario | V6 | V11 p50 / p95 | V11 frozen | V11 mean bitrate | V11 resets / keyframes (by cause) |
  |---|---|---|---|---|---|
  | clean 2 Mbps | 63 / 98, 1646 kbps, 0 / 2 | 66-67 / 105-108 | 0 | 1675 kbps | 0 / 2 (start, rate) |
  | 3% loss | 45 / 119, 721 kbps, 4 / 7 | 48-50 / 127-138 | 0-0.24 s | 770-927 kbps | 2-4 / 4-7 (all rate) |
  | 2 Mbps -> 300 kbps | last 15 s 65 / 97, 2-3 / 4-5 | last 15 s 65 / 113-126 | 0-0.43 s | last 10 s 247-249 kbps | 1-2 / 3-4 |
  | 500 ms outage | 62 / 93, 1593 kbps, 2 / 4 | 65 / 106-108 | 0.67-0.70 s | 1636 kbps | 2 / 4 |
  | relay 75 (+0-20) ms | 127 / 174, 1202 kbps, 0 / 2 | 130-131 / 191-204 | 0 | 1242-1248 kbps | 0 / 2 |

  Loss: +7-29% bitrate over V6, but still 0.6-0.7x the sim's (1281 kbps) and still a
  540 <-> 360 flap or two per 30 s (a cwnd-limited QUIC sender queues for real, which the
  delay signal rightly sees). Drop: the first cut no longer picks the rung (traced with
  `SIM_TRACE=1 SIM_FROM=10 SIM_TO=11`, which prints every report the sender got). 2 of 4 runs
  went straight to 180p; the other 2 went 360p at the second look, 180p 0.4-0.8 s later. That
  is the 360p floor, not stale bytes: 250 kbps = 85% of 294 kbps, and the 300 kbps link's
  reports read 236-373 kbps (median 300-340), so a 300 kbps link sits on the boundary.
  Keyframes in a steady call (issue: one every 7-10 s on the desktop): clean and relay send
  exactly 2 (start + the 360p -> 540p step).
- **Sim model, to adjust:** (1) congestion window under random loss (cwnd floor ~1 BDP and
  sender-side queueing when cwnd-limited; today the sim's window rarely limits); (2) loss
  detection by PTO for tail packets plus ACK delay (25 ms max_ack_delay); (3) outages in both
  directions; (4) return-path jitter. Nothing to change in `net` for these scenarios.
  **V11:** (2) PTO / time-threshold loss detection and (1) no window growth while
  app-limited are in; still optimistic under loss (sim 1281 vs real ~930 kbps): Cubic's
  window after a loss is still Reno-like and nothing models ACK delay; (3) and (4) open.

## Next

1. ~~Real footage, ARM numbers~~ done (above). ~~Static libaom in every shipped build~~ done:
   pinned tarball + SHA-256, Android, deb/tarball packaging and CI (see Shipping libaom).
2. ~~Resolution ladder in `media::Sender` (switch at a keyframe), added to the simulation.~~
   done (above).
3. iroh transport against `media::Sender`/`Receiver`: **built** (`video::net`) and checked
   against the simulation's scenarios over an emulated link (see Real transport vs
   simulation). ~~`max_concurrent_uni_streams` in the call endpoint's transport config~~ done
   (16). ~~Freeze at ladder step-up on the real transport~~ (harness ordering, fixed). Still
   to do: rate control under random loss; sim model fixes listed there. ~~Fast drain after a
   bandwidth drop~~ done (Rate control).
4. ~~Call protocol: video offer/accept in `CallHello`/`Accept`, camera on/off mid-call.~~ done
   (see Call protocol and core API).
5. Capture/render: Pure-Rust conversion done (`video::convert`, above) and used by the core's
   encoder thread (`push_video_frame_yuv`, `pull_video_frame_rgba`). Android capture/render/UI
   done (see Android app); desktop camera + render done (see Desktop). ~~Android on
   `pushVideoFrameYuv` (full range), zero-copy FFI for frames~~ done (JNI, see Frames across
   the FFI). Still to do: `ANativeWindow` render, phone numbers for `convert_bench`, a run on
   real phones.
