# Repository instructions

> **Every build, test and Android build goes through the task manager, and by default
> to whichever machine is free.** Use
> `tasks run --who YOUR_AGENT --anywhere -- <direct command>`
> (`/home/abe/.local/bin/tasks`). The manager syncs your checkout to the peer over
> iroh (LAN, no SSH), runs it under that machine's admission, streams the log back
> and returns the exit code. Never use `ssh`, `tasks-offload`, `scripts/buildlock.py`
> or `scripts/build_android.py`: they bypass the queue, so the job is invisible in
> `tasks list`/`tasks ui`, cannot be placed on the idle machine and holds the laptop's
> compile slot. Android builds:
> `tasks run --who YOUR_AGENT --peer nixos --fetch android/app/build/outputs/apk/debug/app-debug.apk -- ./android/gradlew -p android --no-daemon --console=plain :app:assembleDebug`
> (run from the repo root; the wrapper is `android/gradlew`. The nixos worker has the
> SDK/NDK; `--fetch REL` brings the output back).
> Fetched files may be up to 2 GiB each (full APKs are fine).
> Without `--anywhere`/`--peer`, only cargo check/test/clippy/nextest are placed
> automatically; gradle, npm and scripts stay on the laptop. `tasks place -- <cmd>`
> shows where a command would run without running it. Dev servers and device jobs
> stay local (`--pin`, `--device SERIAL`).
> **Cargo:** pass `-j 6` at most (the worker refuses `-j 8`; use `-j 2` for jobs that
> must stay on the laptop). It has sccache, so do not set RUSTC_WRAPPER. **Name your
> test targets:** `cargo test -p X --lib FILTER` / `--test NAME`, never a bare filter
> (`cargo test -p p2pcore video_call` builds all ~37 p2pcore test binaries, ~45 min;
> `--broad-test` builds everything deliberately).
> Check `tasks list --active` and `tasks admission` before starting; do not bypass
> pressure gates, duplicate a job after a timeout or interrupt jobs. `tasks logs ID`
> is plain text (`--json` for scripts). If `tasks` fails, report the exact error; do
> not work around it. A job that failed with "incomplete or oversized response" was
> never run: resubmit it. Coordinator source is `/home/abe/task-manager`; read
> `/home/abe/task-manager/TRIAL.md`. Required feedback: your task report plus your own
> new file under `/home/abe/task-manager/feedback/`.

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

The shared Rust coordinator is an **experimental trial**, not a finished system.
Its authoritative source is now the separate local repository
`/home/abe/task-manager` (not Tinline's historical tools/task-manager copies).
Read `/home/abe/task-manager/TRIAL.md` before first use in an agent session.
Use `tasks` for new direct heavy workloads as below; do not bypass admission or
silently fall back when it misbehaves. Inspect the existing ID/logs first and
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

- Heavy local builds/tests must use the shared Rust task manager (installed at
  `/home/abe/.local/bin/tasks`):
  `tasks run --who <unique-agent-name> -- <direct command>`.
  Default admission is compile; use `tasks run --who <name> --run -- <already-built command>`
  for run-only work, or `--device <serial>` for an explicitly authorized device job.
  `tasks run` explicitly submits/approves/starts that command and returns its exit
  status. Ctrl-C detaches the client; query/cancel the existing job, never duplicate it.
  For approval-separated work: `tasks enqueue ...`, read `submitted_id`, then
  `tasks approve-command ID` and `tasks start-worker`.
  Agents share the default native daemon/state; do not set TASK_MANAGER_STATE_DIR
  except for deliberately isolated tests. Query `tasks list`, `tasks admission`,
  `tasks logs ID`; cancel only your own work with `tasks cancel ID`.
  Commands use a trusted LIVE cwd (not an immutable source snapshot); keep that
  checkout stable during compile/run, don't put secrets in argv, and inspect
  failures before retrying. Use `env NAME=value ...` for needed non-secret build
  inputs (e.g. VPX_DIR), not cache-policy overrides.
  The legacy Python helpers (`scripts/buildlock.py`, `scripts/build_android.py`) are
  retired for builds: run the underlying command (`cargo ...`, `./gradlew ...`) under
  `tasks run --anywhere` instead, so it is tracked, placed and visible. Do not wrap
  any command in your own lock.
- All build output and caches live on the cache disk `/home/abe/build-cache`: tasks
  (and legacy buildlock) create missing per-checkout `target/`, `android/build` and
  `android/app/build` links there without migrating/deleting existing outputs, run
  Gradle with `GRADLE_USER_HOME=/home/abe/build-cache/gradle`, and every cargo build shares compiled crates
  through sccache (`/home/abe/build-cache/sccache`; machine-wide via `~/.cargo/config.toml`,
  set up by `scripts/setup_build_cache.py`). Don't override `RUSTC_WRAPPER`/`CARGO_TARGET_DIR`/`GRADLE_USER_HOME`
  or point several worktrees at one `target/` (branches overwrite each other's binaries).
- Inspect `tasks admission` and `tasks list` before starting work. Do not
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
