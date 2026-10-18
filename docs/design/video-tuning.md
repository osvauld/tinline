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
continue through the original adaptive path. Build/test results are recorded after
running the focused checks through `tasks` and the LAN worker; device validation
has not been performed.

This worktree predates `dev`'s sharper-video suggestions. Integration with those
changes, including `VideoOffer.hint_flags`, must be checked separately before a
merge or release. No merge or release is authorized by this implementation.
