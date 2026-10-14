# HD preference and hardware acceleration candidate

Worktree: `.claude/worktrees/video-hd-gpu`, branch `feature/video-hd-gpu`,
based on dev `2306e2e`. Opt-in HD capture; not a hardware-encoding implementation
or a claim of guaranteed HD on all devices/networks.

## Implemented candidate

- Android device-local Settings switch **Prefer HD**, off by default (Auto).
  Requests 1280x720/16:9 on the next camera bind, including release builds;
  unsupported use-case binding retries the existing SD preference once.
- Existing explicit debug `-PvideoHdCapture=true` experiment remains a default
  only when there is no saved user choice. Explicit Auto overrides that flag.
- AV1 codec, thread count, rate/CPU ladder, peer limits, aspect preservation and
  no-upscale safety are unchanged. HD capture does not imply HD transmission.
- Call UI shows actual sending dimensions; labels HD only when both dimensions
  reach 1280x720 (orientation-independent). Lower dimensions are not attributed
  to network vs CPU without evidence.
- Debug `video_hardware` action reads MediaCodec metadata for AV1/H.264 encoders
  and decoders, hardware/software flags (unknown on API 28), and declared 720p30
  capability. Explicit action only, never automatic. Does not create codecs,
  process frames, benchmark, or change the active call codec.
- Added deterministic helper tests for saved settings and actual-resolution labels.

## Validation

No benchmarks or phone/emulator access/install. `git diff --check` passed.
Attempted command from repository root:

```sh
python3 scripts/buildlock.py --who video-hd-unit -- \
  python3 .claude/worktrees/video-hd-gpu/scripts/test_video_quality_candidate.py
```

The command timed out after 180 seconds waiting for shared build admission;
compilation/tests never started. Confirmed no remaining job for this owner.
Subsequent focused helper validation passed **13/13** using the same runner
under owner `video-hd-unit-validate`. An initial Android compile attempt with
existing generated bindings failed because those bindings lacked current core
APIs (contact requests, avatars, recovery key); they are not valid for this base.
The subsequent x86_64 build was interrupted before completion; no pass claimed.

At the user's request, the current build targets **arm64-v8a only**, using
release Rust. Gradle now reads UniFFI metadata from the first requested ABI's
library instead of requiring x86_64. The metadata reader does not run the codec.
Active command (no install/benchmarks):

```sh
cd android
python3 ../scripts/buildlock.py --who video-hd-arm64-release -- \
  ./gradlew --offline --no-daemon :app:assembleRelease :app:testReleaseUnitTest \
  --tests com.osvauld.p2p.VideoQualityTest -Pabis=arm64-v8a -PrustRelease=true
```

Log: `/tmp/video-hd-arm64-release.log`. **BUILD SUCCESSFUL in 16m 59s**:
release native compilation, regenerated bindings, release Kotlin, R8/lint and APK
packaging completed. Gradle release `VideoQualityTest`: **13/13 passed**, zero
failures/errors/skips. Existing deprecation warnings and JNA strip warning only.
APK: `android/app/build/outputs/apk/release/app-release.apk` (44,938,224 bytes).
ZIP inspection verified all five native libraries, including `libp2pcore.so`,
are under **arm64-v8a only**. `git diff --check` passed. No benchmarks,
installation, device validation or production HD-quality claim.

## Desktop USB-camera candidate

Added desktop device-local **Prefer HD** (off by default), bounded 1280x720 mode
attempts followed by SD fallback. MJPEG is preferred at HD because USB webcams
commonly cannot deliver raw 720p30. Known negotiated modes below 24fps are tried
in another format before SD fallback; unknown cadence is not claimed as 30fps.
MJPEG decoding uses the existing image/JPEG dependency on the capture thread,
with input, allocation and dimension limits, then converts RGB to limited-range
I420. No codec or transport changes, and no hardware/GPU encoder yet.

Settings **Find cameras → Refresh** enumerates capture nodes without streaming.
The **Camera** picker stores an explicit `/dev/videoN` selection for the next
call. A missing selected camera never silently selects another. Device paths may
change after replug/reboot; stable device identity remains future work.
The desktop call stage labels actual sending dimensions (HD only at 1280x720 or
portrait equivalent). Existing sender network/CPU adaptation remains in force.

Previous desktop builds were stopped by their owner before changing owned
sources; no other agent processes or build outputs were removed. Current queued
validation, with no camera access or benchmarks:

```sh
AOM_DIR=/home/abe/build-cache/p2p_chat/libaom/install-v3.14.1-host AOM_STATIC_REQUIRED=1 \
python3 scripts/buildlock.py --who video-hd-desktop-usb-validate -- bash -c \
 'cargo test --offline --locked --release -p desktop --bin p2p-desktop camera:: && \
  cargo test --offline --locked --release -p desktop --bin p2p-desktop frames:: && \
  cargo build --offline --locked --release -p desktop'
```

Log `/tmp/video-hd-desktop-usb-validate.log`: release camera tests **2/2** and
frame-conversion tests **6/6** passed (zero failed/ignored; 147 and 143 unrelated
tests filtered respectively). Release desktop executable built successfully,
with static libaom (no libaom dynamic dependency); existing dead-code warnings.
Executable: `target/release/p2p-desktop`.

User reports successful local-network Android HD calling and desktop USB-camera
operation after selecting `/dev/video2`; no independent end-to-end resolution
trace or cross-network validation is claimed. ARM APK installation and replacement
of the local desktop binary were explicitly authorized and completed. Desktop
binary backup retained; account/app data untouched. No benchmark was run.
Ready for integration as opt-in HD capture, not a general HD/GPU completion claim.

## Remaining work and acceptance gates

1. Broaden Android normal-call smoke checks: camera flip, background/return,
   persisted preference, audio continuity and poor-link adaptation.
2. Broaden desktop normal-call checks: unsupported-HD fallback, camera disconnect,
   persisted picker selection and audio continuity. MJPEG malformed/dimension
   checks are covered by the targeted deterministic tests. Stable camera IDs and
   switching camera within a current call remain future work.
3. Inspect available Linux hardware encoder backends (VA-API / NVIDIA) without
   running performance benchmarks. GPU presence is not proof of AV1 encode support.
4. Adaptive frame cadence: design/test 720p at reduced fps; the current software
   AV1 ladder assumes 30fps. Do not merely change codec FPS while pushing 30fps.
5. Hardware encoding requires an explicit design: Android MediaCodec ownership,
   camera-surface pipeline, timestamps, keyframes, bitrate updates and bounded
   buffers. Existing decoder uses memory-safe rav1d for peer bytes; do not silently
   replace it with hardware decode.
6. AV1 hardware encoding is device-dependent. H.264 fallback would require codec
   negotiation and security/product review; it is not part of this patch.
7. Real phone/desktop comparisons require explicit permission. No benchmarks
   unless separately requested. For normal test calls record actual capture and
   send/receive dimensions, cadence, audio continuity and thermal observations.
   Validate unsupported-HD fallback, poor links, camera flip/lifecycle, and older
   peer compatibility. A quality preference must never be shown as guaranteed HD.
