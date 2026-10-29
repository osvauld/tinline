# Repository instructions

> **Every build, test and Android build goes through the task manager LoadTable (`loadtable`), and by
> default to whichever machine is free.** Use
> `loadtable run --who YOUR_AGENT --anywhere -- <direct command>`
> (`/home/abe/.local/bin/loadtable`; Claude sessions can use the LoadTable MCP tools (server `tasks2`)
> `submit`/`wait`/`logs`/`status` instead). The manager syncs your checkout to the peer
> over iroh (no SSH), runs it under that machine's admission, streams the log back and
> returns the exit code. The old `tasks` (v1) is retired and removed: do not use
> `tasks`, `ssh`, `tasks-offload`, `scripts/buildlock.py` or
> `scripts/build_android.py`; they bypass the `loadtable` queue.
> Placement: `--anywhere` lets the manager pick a machine, `--peer nixos` names the
> worker, `--pin` keeps the job on this laptop (GUIs, dev servers). Without a flag the
> job runs on this laptop. Device jobs use `--device SERIAL` and stay on the machine
> the device is attached to. Never use the personal phone `ZD22257P6G`.
> **Builds: `--anywhere`, not `--pin`.** The manager runs a build on the nixos worker
> when it is free, otherwise on this laptop. `.tasks/run-policy.toml` sets
> `[placement] os = "linux"`, so an `--anywhere` job never goes to the Mac (it lacks
> the Linux libraries, and its binaries do not run here). Pin only what must run here:
> the desktop GUI, device jobs, dev servers.
> **Builds you need back here: add `--fetch` for each file** (a path from the repo root,
> repeatable, up to 2 GiB each; MCP `submit` takes `placement: "anywhere"` and
> `fetch: [...]`). The log says `[loadtable] fetched PATH (size, sha256)`; when the job ran
> on this laptop the file is already in place.
> Android (run from the repo root; the nixos worker has the SDK/NDK):
> `loadtable run --who YOUR_AGENT --anywhere --fetch android/app/build/outputs/apk/debug/app-debug.apk -- ./android/gradlew -p android --no-daemon --console=plain :app:assembleDebug`
> (`assemblePlayDebug` -> `apk/play/debug/app-play-debug.apk`). An APK built on nixos
> is signed with the worker's debug key and will not install over a laptop build, so
> always re-sign it with the laptop's debug key (harmless when it was built here; the
> password is Android's public default, `android`):
> `P=android ~/Android/Sdk/build-tools/<newest>/apksigner sign --ks ~/.android/debug.keystore --ks-pass env:P APK`.
> Release signing stays on CI.
> Desktop (`p2p-desktop`):
> `loadtable run --who YOUR_AGENT --anywhere --fetch target/debug/p2p-desktop -- cargo build -p desktop --bin p2p-desktop -j 6`
> (optimised: add `--release` and fetch `target/release/p2p-desktop`). A binary built on
> nixos points at `/nix/store` and will not start here until it is patched, so after
> every build: `B=target/debug/p2p-desktop;`
> `readelf -l $B | grep -q /nix/store && patchelf --set-interpreter /lib64/ld-linux-x86-64.so.2 --remove-rpath $B`,
> then `ldd $B | grep 'not found'` must print nothing (if it names a library, the two
> machines' versions differ: report it and build with `--pin`). Run `$B` directly, not
> `cargo run`, which would rebuild. This is for testing on this laptop only; release
> tarballs, `.deb` and AppImage are built by CI on Ubuntu 24.04, never from nixos.
> Tests and checks: `--anywhere`, no fetch.
> **iOS (on the Mac peer `mac`): run a suite, never a `bash -c "a; b; c"` chain:**
> `loadtable suite run ios-sim --who YOUR_AGENT` (core -> xcodegen -> simulator launch) or
> `ios-ui` (core -> xcodegen -> XCUITests); MCP: `run_suite`. Each step is one fixed
> command with its own history, so it gets an estimate; a chain is a new command every
> time and gets none. `loadtable policy show` lists the suites (`.tasks/run-policy.toml`).
> For a single step, submit that one script directly (`--peer mac -- ios/scripts/build_core.sh`).
> The Mac starts peer jobs only while it is on AC power, unless its owner allows battery:
> a job that waits with `mac is on battery` is held, not stuck. Jobs on a peer are confined:
> they can read and write only the build cache, the synced checkout and the toolchains,
> not the rest of that machine's home folder.
> **Cargo:** pass `-j 6` at most (`-j 2` for jobs that stay on the laptop). `loadtable`
> sets sccache itself, so do not set RUSTC_WRAPPER. **Name your test targets:**
> `cargo test -p X --lib FILTER` / `--test NAME`, never a bare filter
> (`cargo test -p p2pcore video_call` builds all ~37 p2pcore test binaries, ~45 min;
> `--broad-test` builds everything deliberately).
> Check `loadtable list --active` and `loadtable status` before starting; do not bypass
> pressure gates, duplicate a job after a timeout or interrupt jobs. `loadtable logs ID`
> reads a job's output, `loadtable cancel ID` stops your own job only. If `loadtable` fails,
> report the exact error and job id; do not work around it. Coordinator source is
> `/home/abe/task-manager` (branch `dev`). Required feedback: your task report plus your
> own new file under `/home/abe/task-manager/feedback/`.

This repository contains Tinline: Rust core/protocol/media crates, an Iced desktop
app, and a Kotlin/Compose Android app. The Svelte playground under
`docs/design/playground/` is a design reference, not the shipped application.

## Shared checkout safety

- Verify the working directory, branch, HEAD and `git status --short` before work.
  The integration checkout normally uses `dev`; do not switch branches or reset it.
- Other agents may be editing or building concurrently. Coordinate exact file
  ownership before parallel edits; do not overwrite another agent's changes.
- Do not modify or remove other worktrees. Do not delete build outputs while a
  build/test or device session may be using them.
- Do not stage, commit, merge, push, install on the user's phone, or wipe app data
  without explicit authorization. Never use `git add .` to collect unrelated work.
- Keep generated screenshots, test logs and local build artifacts out of Git.
  Preserve intentional visual baselines and required runtime/package assets.

## Builds and tests

### Task-manager trial: required use and feedback

The shared Rust coordinator LoadTable (`loadtable`) is an **experimental trial**, not a finished
system. Its source is the separate local repository `/home/abe/task-manager`
(branch `dev`; design and regression docs under `docs/`).
Use `loadtable` for new direct heavy workloads as below; do not bypass admission or
silently fall back to the old `tasks` when it misbehaves. Inspect the existing ID/logs first and
report blockers before requesting an authorized workaround.

After meaningful use in each agent session, provide feedback in your task report
AND a new uniquely named `/home/abe/task-manager/feedback/<agent>-<job-id>-<topic>.md`
using that directory's template. Include job IDs, expected/actual behavior,
queue/log/approval usability, any suggested improvement and an acceptance test.
If no problem was observed, say so briefly; do not invent suggestions or timings.
Redact secrets/private command inputs. Write only your own feedback file; do not
edit other agents' reports or stage/commit them. Already-running agents should
reread this section before their next heavy job; old helper jobs may finish.

### Admission and execution

- Heavy builds/tests must use the shared task manager LoadTable (`loadtable`) (installed at
  `/home/abe/.local/bin/loadtable`):
  `loadtable run --who <unique-agent-name> [--anywhere | --peer nixos | --pin] -- <direct command>`.
  Use `--device <serial>` only for an explicitly authorized device job.
  `loadtable run` submits the command, follows its log and returns its exit status;
  `--detach` (or `loadtable enqueue`) submits and prints the id without following.
  Ctrl-C detaches the client; query/cancel the existing job, never duplicate it.
  `--priority background` for work that may wait; `--priority blocking --reason TEXT`
  only when someone is waiting on the result now.
  Query `loadtable list --active`, `loadtable status`, `loadtable logs ID`; cancel only your
  own work with `loadtable cancel ID`.
  Jobs run in your live checkout (synced to the worker for `--peer`/`--anywhere`);
  keep that checkout stable during compile/run, don't put secrets in argv, and
  inspect failures before retrying. Use `env NAME=value ...` for needed non-secret
  build inputs (e.g. VPX_DIR), not cache-policy overrides.
  The legacy Python helpers (`scripts/buildlock.py`, `scripts/build_android.py`) are
  retired for builds: run the underlying command (`cargo ...`, `./gradlew ...`) under
  `loadtable run --anywhere` instead, so it is tracked, placed and visible. Do not wrap
  any command in your own lock.
- Build output and caches live on each machine's cache disk `~/build-cache`: `loadtable`
  creates missing per-checkout `target/`, `android/build` and `android/app/build` links
  under `~/build-cache/loadtable/checkouts/` without migrating/deleting existing outputs,
  sets `GRADLE_USER_HOME=~/build-cache/gradle` when that directory exists, and runs
  every cargo build with sccache (`~/build-cache/sccache`). Don't override
  `RUSTC_WRAPPER`/`CARGO_TARGET_DIR`/`GRADLE_USER_HOME` or point several worktrees at
  one `target/` (branches overwrite each other's binaries). `loadtable cache` shows the
  cache setup and free disk.
- Inspect `loadtable status` and `loadtable list --active` before starting work. Do not
  bypass admission, launch duplicate jobs after a timeout, or terminate another
  agent's processes. Use the existing device lock for emulator/phone operations.
- Run targeted checks for owned changes; report exact commands, results, ignored
  tests and limitations. Do not claim models prove production integration.
- Timing-sensitive video tests need an appropriate release profile. Separate
  network/device-dependent validation from deterministic offline tests.
- Format only owned files, not the entire shared workspace.

## Protocol and product constraints

- Read the relevant current protocol/subsystem docs before changing behavior.
  Important references: `docs/protocol.md`, `docs/chat.md`, `docs/group-core.md`,
  `docs/vault.md`, `docs/video.md`, and `docs/design/rotation-open-decisions.md`.
- The account DID is stable; device IDs are owner-replaceable. Endpoint rotation
  is periodic/configurable, not triggered by blocking. Enforce verified-DID policy.
- Personal blocking, group delegation/membership and device succession/revocation
  are separate policies; do not implicitly change one through another.
- No anonymity promise. Shared account mnemonics do not provide strong
  cryptographic revocation of a hostile linked device.
- Do not invent offline-discovery guarantees or resolve open security/product
  decisions silently. Propose alternatives and acceptance tests.
- Preserve author proofs and user data. Use synthetic identities in tests.

Follow more specific `AGENTS.md` instructions within subdirectories. Current UI
integration is active: coordinate before removing its design references or previews.
