# Audit 09: media crates, scripts/, tools/ (read-only)

## A. Priority: logic duplicated in Kotlin / desktop instead of shared Rust (CROSS-FOLDER)

| # | Kotlin side | Desktop side | Shared Rust today | Proposed API |
|---|---|---|---|---|
| 1 | VoiceMessage.kt:431 `barsFor(wave,n)` (max of slice, /255) + :476 `sqrt(bars[i])` bar height | desktop/src/voice.rs:408-420 same slice/max/`/255`/`bar = 4+(h-4)*peak.sqrt()`, n = `((w+gap)/(bw+gap)).clamp(8,64)` | core/voice.rs:326 `waveform(peaks)` produces the 64 bytes only | `tin_audio::wave::bars(wave:&[u8], n:usize)->Vec<f32>` (sqrt applied) + `bar_count(width, bar_w, gap)`; expose via JNI/uniffi. Lines saved ~25, tiny risk. |
| 2 | Mark.kt (337 l): spring model `MarkDef` (TAU, F, END_ZETA, CONNECT_HZ...), `sn()` snapping, sag/arc length | desktop/src/mark.rs (971 l) "A port of tinmark.js", same constants | tin-icons/src/draw.rs uses the mark maths (MARK-MATH.md) for static icons only | Move tier geometry + spring step into `tin-icons::mark` (`MarkModel::new(tier,px)`, `step(dt)->MarkPose{cans, ctrl pts, stroke}`; states Slack/Ringing/Taut/Pluck). Both UIs only paint the returned Beziers. Largest win (~500 duplicated lines, both are ports of the JS, so drift risk is real). Medium effort. |
| 3 | Geometry.kt (182 l) `SmoothGeom` (Figma corner smoothing, "keep identical to shape.ts", has GeometryTest vs JS) | desktop/src/geom.rs:159 `outline(w,h,radii,smoothing)`, :68 `bubble_radii`, :35 `Pos::of` run-position logic | tin-icons/src/shape.rs:13 `SMOOTH` (a third copy of smoothing maths for icons) | One `smooth_outline(w,h,Radii,smoothing)->Vec<Seg>` in tin-icons (or new tin-geom); also share bubble run-position/radii rules (geom.rs:35-68; Kotlin has its own in Conversation.kt/ChatParts.kt, not audited). ~150 lines; golden test vs JS once. |
| 4 | PgIcons.kt, Glyphs.kt, ChatGlyphs.kt, CallGlyphs.kt hand-written path strings ("playground icons") | desktop/src/voice.rs:442,479 inline SVG strings PLAY/PAUSE/SEND; mark.rs/ui glyph svgs | tin-icons generates TinIcons.kt + SVG (generated/) | Finish migration: drop PgIcons/Glyphs/ChatGlyphs/CallGlyphs path strings in favour of generated TinIcons (+ Rust `Icon` enum for iced). 30 Kotlin call sites reference the old sets. Saves ~250 lines Kotlin. |
| 5 | CallParts/CallScreens (not found to compute level) | desktop/src/call_field.rs:19 `smooth`, :23 `lerp`, :142 `note_level` (+ own `Rng(u32)` :177) | none (WGSL on desktop only) | Easing/lerp helpers belong in one small `tin-motion` module if Android call field ever shares; currently Android has no equivalent (call_field is desktop-only) -> not a duplication yet, but note `smooth` also reimplemented in Mark.kt/Compose. Low priority. |
| 6 | Kotlin AudioEngine.kt (408 l): AudioRecord/AudioTrack loops, pull/push PCM; CallController.kt:364 logs `rx rms/freq` from core stats | desktop/src/audio.rs (607 l): cpal mic+speaker loops, 200 ms ring/30 ms fill (`audio.rs:28`), echo canceller | audio::Receiver jitter buffer, core Node `push_mic/pull_speaker` | Platform I/O must stay native, but the *policy* (ring size/prefill, mic-open retry/recreate-on-error counts AudioEngine.kt:328 vs desktop Slot) could be a Rust `DeviceLoop` state machine. Medium; defer. Level metering: only `audio::level/rms` (audio/src/lib.rs:63,73) exist; no live mic-level meter duplicated in UI (no Kotlin rms found). |
| 7 | VideoIo.kt (YUV planes passed raw) | desktop/src/camera.rs | video::convert already shared via core/video_jni.rs `pushYuv/pullRgba` | Already good: conversion is Rust. Keep. |
| 8 | Voice resampling | desktop/src/voice.rs:36-120 `Down`/`Up` resamplers (16 kHz <-> device rate) | core/voice.rs has Opus only; Kotlin presumably uses MediaCodec/Android resampler | Move `Down`/`Up` to tin-audio (`audio::resample::{Down,Up}`) so Android recorder (if it ever feeds core PCM) and desktop share; desktop voice.rs would lose ~90 lines. |

## B. Media crates (video/audio/tin-icons/gcall-bench)

Duplicates found:
1. **Seeded RNG, 8 copies**: video/src/sim.rs:19 `Rng(u64)`, gcall-bench/src/lib.rs:50, audio/src/tests.rs:3, core/src/search/bench.rs:13, core/src/bin/year-bench.rs:33, core/src/gcall/rank.rs:336 (test), core/src/gcall/tests.rs:628 + core/src/voice.rs:799,959 (inline LCG `6364136223846793005`), group-sim/src/host_only.rs:78, desktop/src/call_field.rs:177. Propose tiny `testkit`/`simrng` crate (`Rng::new(seed)`, `next_u64`, `f64`, `range`, `chance`). ~80 lines, very low risk, dev-dep only.
2. **Percentile helpers**: video/src/sim.rs:324 `Report::pct`, video/src/bench.rs:10 `pct`, gcall-bench/src/lib.rs:148 `pct`, core/src/search/bench.rs:119, core/tests/group_shards.rs:496. Fold into the same testkit (`pct(&mut [T], p)`). ~40 lines.
3. **Test-signal generation**: audio `Tone` (lib.rs:91, public in prod crate) and `estimate_frequency` (:119) vs video `Pattern` (lib.rs:264); desktop camera.rs:80 `pattern()` and core/src/synth.rs. Check core synth.rs vs `audio::Tone` for overlap (not verified). Move `Tone`/`estimate_frequency` under a `testsignal` module, feature-gated.
4. **audio crate tests split in 3 files** (tests.rs 298, group_call_tests.rs 529, mix_sum_tests.rs 51) all reconstruct Sender/Receiver pairs + Rng; shared `fixtures` helper (`pair()`, `feed(sender,receiver,loss_pattern)`) would save ~60 lines. `audio/src/mixing.rs` (29 l) vs `mix` in lib.rs:81 and tools/group-audio-refinement/mix_oracle.rs (8 l): confirm oracle is a 3rd implementation of mixing (it is an oracle, keep, but reference it from a test).
5. **video crate**: media.rs is 1300 lines (Ladder, Sender, Receiver, ~370 test lines at :932). net.rs 763 + net/{impair,scenarios,tests}. sim.rs 860 re-implements link/impairment model while net/impair.rs implements impairment over real sockets: two loss/jitter/bandwidth models (`sim::Link` sim.rs:39 vs net impair). Candidate to share one `Impairment` trait/struct. Medium risk, ~150 lines.
6. video examples bench.rs (107) + bench.rs in src (79) + convert_bench.rs (64): src/bench.rs `run()` already the library form; examples/bench.rs should be a 10-line wrapper. Check duplicate arg parsing. ~50 lines.
7. core/src/video_call.rs (1126 l) `to_rgba` :739 and `yuv_fits` :300 vs video::convert (`to_rgba` convert.rs:485, `Yuv420` validation convert.rs:133). Likely thin wrapper; verify and delete the core copy. core `max_rung` :551 operates on `LadderConfig` -> belongs in `video::media::Ladder` (CROSS-FOLDER).
8. gcall-bench lib (174 l) + bins host/members: uses `audio` only; Rng/pct duplicates above; Python gcall_bench.py (208) is its device driver (adb helper duplicates, see C).
9. tin-icons is clean (single source -> Kotlin + SVG + iced). Gap: only icons; mark/smooth geometry still duplicated (A2, A3).

## C. scripts/ duplication (Python)

Repeated helpers (all ~identical):
- **adb path + PKG constants**: e2e_android.py:11-12, e2e_android_video.py:32-33, e2e_phone_phone.py:17-18, e2e_android_audio.py:28-29, e2e_android_link.py:21,23 (uses bare `adb`), regress.py:19, gcall_bench.py:25, measure_idle.py:20, record_play_demo.py. Three different ADB representations (list, str, `["adb"]`).
- **`dbg(cmd, **extras)` broadcast to DebugReceiver**: e2e_android.py:19, e2e_android_video.py:40, e2e_phone_phone.py (Phone.dbg ~:30), e2e_android_audio.py:~45 (nested), e2e_android_link.py:31 (also e2e_android_presence/link_desktop_phone via `ui.`). 5 copies x ~8 lines.
- **`logs()` / `log()` (`logcat -d -s P2PTEST`)**: e2e_android.py:27, e2e_android_video.py:48, e2e_android_link.py:38, e2e_phone_phone Phone.log, e2e_android_audio nested.
- **`wait_for(pat, timeout)` poll logcat regex**: e2e_android.py:31, e2e_android_video.py:52, e2e_phone_phone Phone.wait, e2e_android_audio nested, e2e_android_link.py `wait_log`:42.
- **`until(fn, timeout, step)`**: e2e_link.py:93 (returns None), e2e_chat.py:134 (returns fn() at end); imported by 3 android scripts from e2e_link.
- **`class Checks` (PASS/FAIL counter)**: e2e_callwait.py:32, e2e_chat.py:36, e2e_link.py:29, e2e_desktop.py:36 (identical `ok(label,cond,detail)`).
- **`peer(data,*args,env,wait,out)` launcher of p2p-peer**: e2e_desktop.py:23, e2e_callwait.py:25 (identical bar `stdout=out or DEVNULL`); e2e_chat `run`:51, e2e_link `Peer`.
- **`PEER = ROOT/"target/release/p2p-peer"` + `build_peer()` via buildlock**: e2e_chat.py:32,269; e2e_callwait.py:21,89; e2e_link.py:26,103; e2e_desktop.py:20.
- **`null_sink()` pactl p2p_test sink**: desktop_e2e.py:30 and desktop_video.py:39 (+`drop_sink`), plus `run`/`ok`/`sh_ok` subprocess wrappers (desktop_e2e.py:19,23; desktop_video.py:28,32).
- **`freq()` parse of tone result**: e2e_android.py:41, desktop_e2e.py:76, e2e_desktop.py:31 `heard`.
- **Phone fresh-install (pm clear, grant RECORD_AUDIO/POST_NOTIFICATIONS, create account)**: e2e_phone_phone.py `Phone.fresh` vs e2e_android_link.py:161 `fresh`.
- **Desktop/ROOT/TARGET/PROFILE env handling**: desktop_e2e.py:12-14, desktop_video.py:22-24 (different defaults: release vs `PROFILE=debug`).
- Duplicate WORK dirs hard-coded `/tmp/claude-1000/...` (desktop_e2e.py:14, e2e_link_desktop_phone.py:22) -> use `tempfile`.

Proposed shared module `scripts/lib/e2e.py` (+ `scripts/lib/__init__.py`, scripts do `sys.path.insert`):
```python
ROOT: Path; PKG="com.osvauld.tinline"; ADB: str   # resolve once (env ADB, which adb, ~/Android/Sdk/...)
def run(*a, check=False, timeout=None, **kw) -> CompletedProcess   # capture_output text
def until(fn, timeout, step=0.3, final=False)                       # poll helper
class Checks: ok(label, cond, detail=""); failed; exit()
class Phone:                       # serial-bound
    def __init__(self, serial=None, name=None)
    def sh(*a, check=True); def dbg(cmd, **extras); def log(tag="P2PTEST")
    def wait(pat, timeout=40, since=0)       # returns last regex match, raises SystemExit
    def fresh(name, grants=(...)); def start_app(); def home()
    # ui helpers from e2e_android_link: dump/find/tap/has/shot/type_text
class Peer / def peer(data,*args,env,wait=True,out=None)  # p2p-peer launcher
def build_peer(who)               # buildlock + cargo build -p peer
def null_sink()/drop_sink()       # pactl
def tone_freq(text)               # RESULT freq= parse
```
Estimated saving: ~350-450 of ~2.9k lines of e2e/desktop scripts; also fixes inconsistent timeouts/poll steps. Low risk (scripts unrun in CI). Do e2e_android_link UI helpers (~100 lines at :27-160) as `lib/adb_ui.py` first since three scripts already import it as `ui`.

Python tests: scripts/test_rotation_*_model.py (667+299+99 = 1065 lines) are pure symbolic models. They are documented in docs/design/* as requirement models, explicitly "NOT production code"; network_model.py says "real signature checks live in proto's rotation tests". They overlap conceptually with crates/proto/tests/rotation_*.rs (8 files) and core/tests/rotation_restore_characterization.rs, and a time-contract doc points at network_model.py:105 as a divergence. CROSS-FOLDER: once Rust rotation is shipped, these are candidates for archive/move to docs/design/models/ (not scripts/) or porting the matrix to group-sim; do not delete before the open-decision docs (rotation-open-decisions.md) are resolved. linked_discovery_options.py has a `--matrix` generator used by docs, keep. Python `Dev`/`Directory` dataclasses duplicated between discovery_model and network_model (~60 lines).

## D. Dead / weakly-referenced scripts
- No reference anywhere (docs, CI, scripts): `scripts/measure_idle.py` (248), `scripts/record_play_demo.py` (91; Play FGS demo, one-off). Candidates to move to docs/ops or delete.
- Referenced only by docs: e2e_callwait.py (docs/protocol.md), desktop_video.py + e2e_android_video.py (docs/video.md), test_video_quality_candidate.py (a design doc; "candidate" name, JVM test of capture helper), e2e_android_audio/e2e_android_presence/e2e_link_desktop_phone/e2e_android_link (only import each other; not in regress.py or README). e2e_chat.py, e2e_link.py: referenced by README? (e2e_chat count 2 - README/regress); check README table.
- Not in regress.py: e2e_callwait, desktop_video, e2e_android_video/audio/link/presence, e2e_link_desktop_phone, gcall_bench.
- CI-used: build_libaom.py, test_r2_upload.py, package_linux_tarball.sh, package_deb.sh, build_apt_repo.sh, upload_repo_r2.sh. `desktop_e2e.py` vs `e2e_desktop.py` are confusingly named twin scripts (desktop app e2e vs p2p-peer desktop e2e); rename e.g. `e2e_desktop_app.py`/`e2e_peer_calls.py`.
- stale `scripts/__pycache__` exists in the working tree (untracked? verify in .gitignore).

## E. Packaging shell
- package_deb.sh (74) and package_linux_tarball.sh (43) share lines 1-30 verbatim: ROOT/cd, version from Cargo.toml (python tomllib heredoc), ARCH->DEB_ARCH case, libaom build via buildlock, `AOM_DIR_${HOST_TRIPLE//-/_}` export, cargo build desktop with distribution target-dir, static-libaom readelf check. Extract `scripts/lib/package_common.sh` (`pkg_version`, `pkg_arch`, `build_desktop_static`). Saves ~25 lines and ensures the static check applies identically (tarball version lacks only `--who` name). build_apt_repo.sh/upload_repo_r2.sh are small and distinct. build_android.py vs build_libaom.py both resolve NDK/targets; check for shared NDK lookup (not verified).

## F. tools/ (tracked)
- tools/call-field/golden.mjs (50): node script generating golden values for desktop call_field (JS reference vs call_field.rs). Reference-only artifact.
- tools/group-audio-refinement/mix_oracle.rs (8): tiny oracle for audio mixing; see B4.

## G. File map
crates/video: lib.rs I420/VideoEncoder/Decoder(dav1d)/Pattern/psnr; aom.rs libaom RT encoder FFI; rav1e_enc.rs rav1e fallback encoder; convert.rs YUV/RGB/scale/rotate/crop; media.rs rate ladder, Sender (GOP/keyframe policy), Receiver; net.rs QUIC-stream send/recv per GOP with priority + stats; sim.rs deterministic network+codec simulator (Link, Device, Scenario, Report); cpu.rs CPU pinning/threads; y4m.rs Y4M reader; bench.rs encode benchmark; tests.rs codec round-trip tests; build.rs/csrc libaom/dav1d shims; examples: bench, convert_bench, delay, net_loopback.
crates/audio: lib.rs Opus Sender/Receiver (jitter buffer, DRED/FEC, group slots, fades, level/rms, Tone, estimate_frequency, mix); mixing.rs group mix; tests.rs/group_call_tests.rs/mix_sum_tests.rs; examples/level.rs.
crates/tin-icons: lib.rs; draw.rs mark-geometry drawing; path.rs path ops; shape.rs smoothing; icons.rs icon definitions; emit.rs SVG/Kotlin emitters; tests.rs; generated/ (TinIcons.kt, svg); bin tin-icons-gen.
crates/gcall-bench: lib.rs Rng/pct/sim member + host forwarding; bin/host.rs, bin/members.rs.
scripts: e2e_*.py/desktop_*.py device & peer e2e; regress.py runs everything; buildlock.py shared build queue; build_android.py/build_libaom.py builds; gcall_bench.py adb driver; measure_idle.py idle traffic; audio_compare.py cross-correlation scorer; test_rotation_*_model.py symbolic requirement models; test_r2_upload.py publish-order tests; package_*.sh/build_apt_repo.sh/upload_repo_r2.sh release; record_play_demo.py Play review video; test_video_quality_candidate.py JVM test runner.

## H. Ranking (payoff/risk)
1. scripts/lib/e2e.py consolidation: ~400 lines, low risk.
2. Mark model + smooth geometry into tin-icons Rust (Mark.kt/mark.rs/Geometry.kt/geom.rs/shape.rs): ~600 lines and removes drift, medium.
3. Finish icon migration to generated TinIcons (Kotlin hand glyph files): ~250 lines, low.
4. Shared testkit Rng/pct (10 copies): ~120 lines, very low.
5. Waveform bars + resamplers to tin-audio: ~110 lines, low.
6. package script common lib: ~25 lines, trivial.
7. Impairment model sharing sim.rs/net impair; core video_call.rs vs video::convert: medium.
8. Archive rotation python models after decisions land; remove measure_idle/record_play_demo if unused.
