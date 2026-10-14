# Controlled 1:1 HD-capture candidate

Historical validation of `74b5ea8`. The new user-selectable HD worktree candidate
is described in [video-hd-hardware.md](video-hd-hardware.md); the test results below
do **not** validate that newer candidate.

This patch adds an experiment, not an accepted release-quality fix:

- Debug opt-in `-PvideoHdCapture=true`: prefer 1280x720/16:9 CameraX capture,
  bounded unsupported-use-case fallback to existing SD preference.
- Default/debug false and release retain 640x480/4:3 preference. Actual negotiated
  analysis size/rotation is logged separately; preference is not an HD guarantee.
- Keep AV1, encoder speed, bitrate, rate/CPU adaptation, peer pixel limits and
  latest-frame/no-upscale safety unchanged.
- VideoStats includes AV1 payload bytes admitted to the local sender, payload
  bytes delivered by VideoRecv before decode, and received units. Admission is
  NOT wire transmission/ACK. Frame/payload rates use elapsed counter deltas;
  initial/reset/new-call windows are unknown and idle time counts.
- Regenerate Kotlin bindings with the new core record; Gradle already does this.
  No new peer protocol, membership/blocking/device or endpoint-policy change.

## Validation on the source branch

Android native x86_64 and arm64-v8a release libraries and bindings built; real app
Kotlin compilation and JVM tests passed **62/62** in each HD/SD flag mode. Scoped
core video tests passed **16/16**. These are build/unit tests, not phone evidence.
No APK assembled/installed, real capture, account changes or data wipes.

```sh
# From repository root. Use the repository's build lock for heavy commands.
AOM_DIR=/home/abe/build-cache/p2p_chat/libaom/install-v3.14.1-host AOM_STATIC_REQUIRED=1 \
python3 scripts/buildlock.py --who video-quality-core -- \
 cargo test -p p2pcore --locked --offline --lib video_call:: -- --test-threads=1
python3 scripts/buildlock.py --who video-quality-jvm -- \
 python3 scripts/test_video_quality_candidate.py
(cd android && python3 ../scripts/buildlock.py --who video-quality-hd -- \
 ./gradlew --offline --no-daemon :app:testDebugUnitTest -PvideoHdCapture=true)
(cd android && python3 ../scripts/buildlock.py --who video-quality-sd -- \
 ./gradlew --offline --no-daemon :app:testDebugUnitTest :app:generateReleaseBuildConfig -PvideoHdCapture=false)
```

The focused commit intentionally excludes the separate simulation/native-loopback
research backlog and its unstaged evidence. Merge/integration validation must use
current dev, not infer readiness from the old-base source-branch runs.

## Dev integration validation

Merged against dev `2111352` (release 0.1.32, versionCode 580), preserving its version
changes. Focused JVM **11/11**, scoped core **16/16** (124 filtered), real app
Kotlin/JVM **62/62**, zero failures/errors/skips. Both Android release native ABI
libraries and bindings regenerated from integrated sources. Generated debug HD
flag true and release flag false verified. SDK/ktlint and existing deprecation
warnings only. Exact integration commands used:

```sh
python3 scripts/buildlock.py --who video-quality-dev-jvm -- python3 scripts/test_video_quality_candidate.py
AOM_DIR=/home/abe/build-cache/p2p_chat/libaom/install-v3.14.1-host AOM_STATIC_REQUIRED=1 \
python3 scripts/buildlock.py --who video-quality-dev-core -- \
 cargo test -p p2pcore --locked --offline --lib video_call:: -- --test-threads=1
(cd android && python3 ../scripts/buildlock.py --who video-quality-dev-android -- \
 ./gradlew --offline --no-daemon :app:testDebugUnitTest :app:generateReleaseBuildConfig -PvideoHdCapture=true)
```

Core tests 0.54s; Android build/test 6m13s (including native compilation). Logs
`/tmp/video-quality-dev-{jvm,core,android}.log`. Buildlock waited for other owners'
checks/Android/desktop/group-audio builds without interruption. No phone access,
install or claim that the user's blur is fixed. Native test research remains
separate/uncommitted in the source worktree; this merge contains only the app
candidate, diagnostics, their tests/runner and this integration note.

## Device gate

Before any install: audit current-app integration and use explicit device/install
permission and isolated test data/accounts. Do not replace a daily-account app
with this older-base worktree build. Compare identical scenes/lighting/motion on
identified phones; record actual capture/send/receive size, payload rate, frame
cadence, encode cost, audio and thermal behavior. If camera is HD but transmission
is downscaled, diagnose rate versus CPU limits before altering adaptive policy.
