# Controlled 1:1 HD-capture candidate

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

## Device gate

Before any install: audit current-app integration and use explicit device/install
permission and isolated test data/accounts. Do not replace a daily-account app
with this older-base worktree build. Compare identical scenes/lighting/motion on
identified phones; record actual capture/send/receive size, payload rate, frame
cadence, encode cost, audio and thermal behavior. If camera is HD but transmission
is downscaled, diagnose rate versus CPU limits before altering adaptive policy.
