# Frequency-aware audio motion — simulation status

Worktree/owner: `bench/group-audio-capacity`, coordinating audio simulation session.
Owned additions: `tools/audio-motion-sim/`, benchmark `spectral.rs`, `feature_trace.rs`,
`playout_level.rs`, opt-in member recorder/CLI hooks, `test_gcall_sampling.py`, this document.
Existing capacity work is preserved. No shared audio/core implementation, production
wire protocol, renderer dependencies, desktop/Android app, staging or merge changes.

## Implemented

The level byte alone is intensity, not a spectrum. The opt-in existing Opus/QUIC
simulation now also samples PCM: local before transmission, remote **after Receiver
pull**, per source (not packet arrival or mixed-source attribution). It writes only
numerical descriptors, never raw PCM. Native scenarios use synthetic voices, not
real conversations or fully authenticated production Node group calls.

Dependency-free experimental 1024-sample Hann FFT at 48kHz, 1920-sample hop (25Hz):
- low (<500Hz), mid (500–2500Hz), high (>=2500Hz) energy fractions, excluding DC;
- power-weighted spectral centroid, **not pitch**;
- bounded positive spectral flux (onset/novelty), not emotion/identity inference.

Window is ~21ms; first feature needs 40ms of eligible PCM. Recorder resets histories
on silence/unknown recovery metadata. At most 64 streams for one selected simulation
receiver; allocation happens on first observed stream, outside any production API.
Recorder FFT/file I/O is diagnostic overhead, not a production callback design.

Cached source-level metadata is only a **candidate**: reject idle, inaudible,
invalid bytes and changed loss/concealment counters. This does NOT prove exact
PCM/level-byte lineage through compression/fade tails. A production UI API still
needs that audit and bounded off-callback feature delivery. Native fixtures have no
user mute/roster API; deterministic simulation separately exercises mute, staleness,
roster removal, end/restart. No future audio endpoints or identity policy is selected.

## Shared model and previews

Rust maths has separate fast/slow level envelopes, slower spectral shape response,
continuous phase, joined cubic contours and aspect-aware sampled arc-length travel.
Frequency balance changes curvature at **equal intensity**, not simply size. Flux
slightly influences shape/flow; it never spawns particles or resets phase.

Pool is fixed at 32, trails at 12 samples, opacity <=0.25, bounded displacement.
Reduced motion freezes existing geometry/trails/phase (including toggle without
snap), retaining opacity response. Previews are exploratory soft contours versus
short persistent soft particle marks; neither treatment is selected.

Offline previews use SVG + installed librsvg/FFmpeg tools, **not another app renderer**.
Native adapters remain Iced Canvas/Compose later. Static long-name text and small
layout snapshots were inspected. Clips were generated, not validated as native UI.

Artifacts relative to this worktree:
- `artifacts/audio-motion/dark.mp4`, `light.mp4`: 20s matched A/B, 30fps.
- `small-reduced.mp4`: same input, static geometry with opacity response.
- `native-overlap.mp4`: descriptors from actual simulated group Opus playout.
- `native/{direct,turns-retry,overlap}.features`: preserved numerical traces.
- `frames/*/performance.txt`: update CPU only, not renderer frame time.

Synthetic timeline: 0–2s silence; 2–4 low tone phrases; 4–6 same-level high tone;
6–7 pause; 7–8 loud onset; 8–12 overlap; 12–14 mute; 14–16 stale; 16–18 roster
change; 18–20 end. This is not natural speech/aesthetic validation.

## Verified commands/results

All Rust compilations/builds and simulation/native runs used absolute
`/home/abe/p2p_chat/scripts/buildlock.py`; run slots are not exclusive.

- `rustc --edition 2024 -O --test tools/audio-motion-sim/tests.rs -o artifacts/audio-motion/model-tests`, then binary: **12 passed** (includes 10-minute maximum-spectrum overlap bound test).
- Same command for `spectral_tests.rs`: **6 passed** (silence, equal-amplitude low/high tones, mixtures, chunk independence, onset/decay, reset/bounds). Initial compile failed as intended before extractor existed.
- Same for `crates/gcall-bench/src/playout_level.rs`: **2 passed**.
- `python3 -m unittest discover -s scripts -p 'test_gcall_*.py'`: **29 passed**.
- `CARGO_TARGET_DIR=/home/abe/build-cache/p2p_chat/group-audio-capacity/target CARGO_BUILD_JOBS=2 cargo build --release -p gcall-bench --bins`: passed in 2m12s.
- Harness `direct --secs 10`; desktop `--n 4 --k 3 --secs 12 --talk-pattern turn-taking` and `overlap`, each with `--feature-trace PATH --out results.jsonl`, explicit isolated release host/member binary paths: completed. Trace checks: 248/261/752 rows, all 12 columns, correct observer, finite/bounded descriptors, normalized eligible bands, end records and direct/group/local scopes.
- Initial capture command used unsupported `--jsonl`: rejected; corrected to `--out`.
- Initial short group run hit zero CPU sampling duration: two red regression tests preceded fix. Retry preserves audio summary but reports missing CPU/bandwidth rates, NOT zero. These short runs establish no capacity or steady-state memory claims.
- `rustc --edition 2024 -O tools/audio-motion-sim/main.rs -o artifacts/audio-motion/motion-sim`: passed; standalone subset emits unused API warnings.
- `motion-sim --out frames/dark --dark` (and light/small-reduced variants): 600 frames each. Dark update p95 **0.176ms**, max 0.250ms, **zero steady update allocations**; includes four synthetic extractors. Model 8,872 bytes; extractor 36,904 bytes. Recorded overlap replay p95 0.015ms excludes extraction. SVG I/O/raster/GPU/layout excluded; these are NOT native frame times or Android battery evidence.
- `ffmpeg -v error -y -framerate 30 -i frame-%04d.svg -c:v libx264 -threads 2 -crf 20 -pix_fmt yuv420p OUTPUT.mp4`: four clips produced.
- `git diff --check`: passed.

## Live native viewer

Requested after MP4 clarity was insufficient. Owned opt-in addition:
`crates/desktop/examples/audio_motion_demo.rs`, plus shared `fixture.rs`,
`fixture_tests.rs` and `tools/audio-motion-sim/run-live.sh`.

Run from any terminal in the graphical desktop session:

```sh
/home/abe/p2p_chat/.claude/worktreess/group-audio-capacity/tools/audio-motion-sim/run-live.sh
```

Script uses absolute buildlock for the release example build, two Cargo jobs and
isolated capacity target; then opens a separate Iced Canvas window. No installation,
account/profile access, speakers or network. Microphone is off by default and opt-in. Fonts/theme reuse desktop
facilities. No extra renderer or dependencies. Both panels share the same model and
actual PCM fixture as offline previews. Pause/replay, light/dark, reduced motion,
and an explicitly experimental contrast slider are included. Equal-level 200Hz,
4kHz and mixed-tone controls permit sustained inspection (silent, no audio playback).
Scenario ends/pauses at 20s; Replay explicitly resets. Tone changes preserve phase.

Fixed 30Hz deterministic simulation clock may slow under contention; not a real-time
call UI. On-screen CPU timings cover update and Canvas construction only, excluding
GPU/layout/full frame latency. Canvas allocates native geometry; no claim of zero
renderer allocations. This initial viewer does not replay recorded group traces or
adapt stacked layout at phone widths. Android remains untouched.

Fixture integration tests: **2 passed**, actual PCM extraction distinguishes the
three equal-level tone controls; lifecycle reaches end with no eligible source.
Native example build **passed**, 13m42s first build (existing native/desktop dependencies),
using isolated target, two jobs, absolute buildlock and
`cargo build --release -p desktop --example audio_motion_demo`.
Binary: `/home/abe/build-cache/p2p_chat/group-audio-capacity/target/release/examples/audio_motion_demo`
(23MiB). Can be launched directly without another build. Desktop runtime/window has
not been automatically launched or smoke-tested; user can run it in their graphical
session. No installation or changes to their running app/profile.

### Opt-in microphone addition

User requested microphone input. Same native binary now has **Start microphone /
Stop microphone**. Close/reopen an older running viewer to get the new buttons.
Default OS input is used at 48kHz (supported configuration explicitly selected);
unsupported rates/configuration or permission/device errors are reported, not
silently analysed with the wrong frequency scale. No resampler added.

Existing CPAL/rtrb dependencies reused. Callback averages interleaved channels,
converts PCM and queues fixed 960-sample blocks. No FFT/rendering, file I/O,
logging or dynamic sample buffers in the callback. Ring bounded to eight blocks
(160ms); overflow counted, >100ms-old blocks discarded and spectral history reset.
Timestamps indicate callback observation, not calibrated hardware acquisition.
UI polling is bounded to eight blocks per tick; existing `audio::level/rms` and
spectral extractor run outside the capture callback. Quiet gate RMS>200 is only
an experimental viewer choice, not production mute/VAD policy.

Mic uses elapsed wall time with bounded catch-up; synthetic scenarios retain their
fixed simulation clock. Input staleness clears eligibility. Stop, Pause, Replay,
synthetic-input selection and window close release capture; resume after Pause
requires explicit Start microphone. Stream error stops capture. Captured PCM is
RAM-only, no audio files/playback/network/feature export, no account profile opened.
Tone/roster simulations and production call audio APIs remain separate.

Test-first missing-implementation run failed as expected. Absolute buildlock,
isolated target and two jobs:
- `cargo test --release -p desktop --example audio_motion_demo mic::tests:: -- --test-threads=1`: **5 passed**, 13.89s compile (exact partial block, bounded overflow, stale rejection, existing level/silence, stereo/nonfinite conversion). No physical microphone opened during tests.
- `cargo build --release -p desktop --example audio_motion_demo`: **passed**, 1m21s.
- Physical device capture/window interaction not automatically smoke-tested; user
  starts capture explicitly. No Android suitability/latency/battery claim.

## Next

Review live native motion and refine the short particle marks/contour treatment.
Then add the Iced adapter/toggle using the same trace, measure real native drawing
frame time/allocations, and choose a treatment before Android. Production feature
extraction must have bounded delivery outside audio callbacks; no deployment/install
or renderer/performance suitability claim has been made here.
