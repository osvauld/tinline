# Tasks

Owner: `lead` = main session; `sonnet:<name>` = Sonnet subagent. A task is done only when the lead
has re-run its acceptance command.

| # | Task | Owner | Owns | Status |
|---|------|-------|------|--------|
| 1 | Workspace skeleton, iroh+opus cross-compile for Android | lead | `Cargo.toml`, `crates/core` | done |
| 2 | `proto`: identity, contact tickets, grants, handshake messages + tests | sonnet:proto | `crates/proto`, `docs/protocol.md` | done |
| 3 | `audio`: Opus 1.6 codec, packet format, jitter buffer/PLC/FEC, tone tools + tests | sonnet:audio | `crates/audio` | done |
| 4 | Android Gradle skeleton: Compose app, cargo-ndk + UniFFI bindings, installs on emulator | sonnet:android | `android/`, `scripts/build_android.sh` | done |
| 5 | `core`: iroh endpoint, contacts store, call state machine, UniFFI API | lead | `crates/core` | done (desktop↔desktop verified) |
| 6 | Host test peer CLI (headless, tone in / wav out) | lead | `crates/peer` | done |
| 7 | Android app per `docs/android-app.md` | sonnet:android-app (worktree) | `android/` | in progress |
| 7b | Desktop app (iced) + cpal/APM audio | sonnet:desktop (worktree) | `crates/desktop` | in progress |
| 8 | E2E on emulator: add contact, call both ways, background/doze/reboot | lead | `scripts/e2e/` | todo |
