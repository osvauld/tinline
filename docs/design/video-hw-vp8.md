# Hardware VP8 for 1:1 calls

Status: DECIDED 2026-10-16 (user): option A — Android sends VP8 from the hardware encoder when
the peer can decode VP8; receivers decode VP8 with Android MediaCodec (phone) and libvpx
(desktop). AV1 (libaom + rav1d) stays the default and the fallback. Follow-up before release:
run the desktop libvpx decoder in a sandboxed process (it parses peer bytes and is C, unlike
rav1d).

## Why

Software AV1 on budget phones tops out at 360p30 (Pixel 3a: libaom p75 11 ms at 360p, 540p
predicted 25 ms > the 20 ms budget). The Pixel 3a's `c2.qti.vp8.encoder` (hardware) measured
1280x720 at 29.4-29.7 fps, follows bitrate changes within a second (2500 -> 1000 -> 2500 kbps:
980 / 2490 delivered), keyframe on request in 20-32 ms, thermal status 0 over 30 s, sharp at
1 Mbps (lab branch `hwvp8-lab`, `HwVp8BenchActivity`). VP8 is royalty-free (no H.264 patent
question). End-to-end latency was not measured (the lab's clock comparison was wrong).

Update 2026-10-16 (user): the desktop also encodes VP8 (libvpx, software) and negotiation is
hardware-aware, because the Pixel 3a decodes the laptop's AV1 in software at a 15 fps cap but has
a hardware VP8 decoder. See "Hardware-aware offer" below.

## Negotiation (proto)

`VideoOffer` gains what the device can decode and encode, best first (the original shape; the
hardware lists follow below):

```rust
pub enum VideoCodec { Av1, Vp8 }            // serde: "av1", "vp8"
pub struct VideoOffer {
    pub max_pixels: u32,
    pub decode: Vec<VideoCodec>,           // always contains Av1
    pub encode: Vec<VideoCodec>,           // Vp8 only with a hardware VP8 encoder
}
/// What `sender` sends to `receiver`: the first of the sender's `encode` the receiver decodes.
pub fn send_codec(sender: &VideoOffer, receiver: &VideoOffer) -> VideoCodec
```

Both ends know both offers, so each computes the codec of each direction; no extra message.

### Hardware-aware offer

```rust
pub struct VideoOffer {
    pub max_pixels: u32, pub max_fps: u8,
    pub decode: Vec<VideoCodec>, pub encode: Vec<VideoCodec>,   // both always contain Av1
    pub hw_decode: Vec<VideoCodec>,   // subset of decode that runs in hardware
    pub hw_encode: Vec<VideoCodec>,   // subset of encode that runs in hardware
}
```

`send_codec(sender, receiver)`: over the codecs in `sender.encode` that `receiver.decode` has,
score = (sender hw-encodes it) + (receiver hw-decodes it); the highest score wins, ties go to the
sender's `encode` order, none = Av1. Why: either end's hardware makes VP8 worth it. A phone with
a hardware encoder spares its CPU, a phone with a hardware decoder spares the decode that
software AV1 cannot sustain above 360p15; a desktop sending to another desktop gains nothing from
VP8 and AV1 is better quality per bit, so score 0/0 keeps the first of `encode`, which the
desktop lists as `[Av1, Vp8]`. Phones with a hardware encoder list `[Vp8, Av1]`. Table:

| sender -> receiver | codec |
| --- | --- |
| desktop -> desktop | Av1 both ways |
| Pixel (hw VP8 enc+dec) -> desktop, desktop -> Pixel | Vp8 |
| Pixel <-> Pixel | Vp8 |
| desktop -> phone with only a software VP8 decoder | Av1 |
| desktop -> phone with a hardware VP8 decoder (no hw encoder) | Vp8 |

`hw_decode` on Android is whether the decoder `MediaCodecVp8Decoder` gets by default (the
framework's pick for the type) has a hardware name (`is_hardware_name`; cached probe
`mediacodec::hardware_vp8_decoder`). `hw_encode` is the existing `hardware_vp8_encoder` probe.
Android decoder selection is unchanged.

### Desktop encoder

`vpx::Vp8Encoder` (video feature `vp8`, `csrc/vpx_enc.c`, same wrapper style as `aom_rt.c`):
VPX_DL_REALTIME, cpu-used -6, CBR, error resilient, no lookahead, one packet per frame,
keyframe on request or at `keyint`, token partitions from the thread count, live `set_bitrate`
through a config update. `scripts/build_libvpx.py` now builds the VP8 encoder too (VP9 stays
off). A resolution change builds a new encoder, as with libaom.

### Stats

`VideoStats.send_hardware` / `recv_hardware` (bool next to `send_codec`/`recv_codec`): whether the
encoder we send with and the decoder we receive with are hardware. Set when video is decided from
our own offer (`hw_encode.contains(send codec)`, `hw_decode.contains(recv codec)`). Apps read
them through the existing uniffi `VideoStats` record; the "VP8 - hardware" indicator is UI work.
Pre-release: no compatibility with the old offer (docs/design no-back-compat). Golden wire
tests are regenerated.

## Codec seams (video crate)

- Encoders already share `VideoEncoder` (I420 in, `Packet { data, key }` out, `set_bitrate`).
- New `VideoDecoder` trait (packet bytes in, `Vec<I420>` out), implemented by the rav1d
  `Decoder` (renamed `Av1Decoder` or kept), `vpx::Vp8Decoder` (libvpx, desktop/linux; static
  build like `scripts/build_libaom.py` → `scripts/build_libvpx.py`, decoder-only, VP8-only,
  pinned SHA-256, under ~/build-cache/p2p_chat/libvpx) and the Android MediaCodec decoder.
- The core picks encoder/decoder from `send_codec` per direction (`pipes.rs`).
- The ladder is unchanged. With a hardware encoder the measured encode time is small, so the
  CPU gate lets it climb to 720p by bitrate alone; `max_rung` still comes from the peer's
  `max_pixels`.

## Android (core, Rust via NDK)

Logic stays in Rust (rust-first): `crates/core` (android only) wraps NDK `AMediaCodec`
(`ndk` crate or `ndk-sys`):

- `MediaCodecVp8Encoder: VideoEncoder` — by name, preferring a hardware VP8 encoder
  (`c2.qti.*`, `OMX.qcom.*`, ... not `c2.android.*`/`OMX.google.*`); CBR, realtime priority,
  long I-frame interval, keyframe via `request-sync`, bitrate via `video-bitrate` parameter;
  ByteBuffer YUV input from the existing `prepare` I420 (convert to the codec's colour format,
  usually NV12/semi-planar). Synchronous dequeue with short timeouts on the encoder thread; no
  unbounded queues. `encode` returns the packets available now (MediaCodec may lag by a frame;
  the packet carries its own key flag).
- `MediaCodecVp8Decoder: VideoDecoder` — output to ByteBuffer, converted to I420.
- `hardware_vp8_encoder() -> bool` probe for the offer (cached once per process).

Kotlin changes are limited to stats/log lines; no codec logic in Kotlin.

## Acceptance tests

Deterministic (CI-able):
1. proto: `send_codec` table (hardware scoring, see above: desktop↔desktop Av1; Pixel↔desktop
   Vp8 both ways; software-only phone Av1; Pixel↔Pixel Vp8; neither → Av1); offer serde golden.
2. video: libvpx `Vp8Decoder` decodes a checked-in tiny VP8 sample (generated by libvpx's
   encoder in the test or a fixture) to the expected size; garbage / truncated / oversize
   (`max_pixels`) packets give errors, never panics; fuzz-style random bytes for N iterations.
3. core: the desktop VP8 send path (`new_encoder(Vp8)` = libvpx) feeds the VP8 receive loop and
   decodes (`desktop_vp8_encoder_feeds_the_vp8_receive_path`); a full in-process call between
   two hosts stays AV1 (`video_both_ways` asserts codecs and hardware flags). A host cannot
   offer a hardware VP8 decoder, so a VP8 call over the real transport needs a device.

Device (manual, with permission):
4. Pixel 3a → desktop, HD on: `send=1280x720` (or 720x1280), ~30 fps received, desktop shows
   it; desktop → Pixel stays AV1.
5. Pixel ↔ g52: both directions VP8 when both have hardware VP8 encoders.
6. Poor link (relay-only): the ladder steps down and back as with AV1; keyframe requests work.
7. A device without hardware VP8 offers `encode: [Av1]` and the call behaves exactly as today.
