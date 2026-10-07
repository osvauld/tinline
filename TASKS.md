# Tasks

Heavy builds (cargo, gradle) go through `scripts/buildlock.py --who <name> -- <cmd>`: one at a
time machine-wide, and only when load/memory allow.

Owner: `lead` = main session; `sonnet:<name>` = Sonnet subagent. A task is done only when the lead
has re-run its acceptance command.

| # | Task | Owner | Owns | Status |
|---|------|-------|------|--------|
| 1 | Workspace skeleton, iroh+opus cross-compile for Android | lead | `Cargo.toml`, `crates/core` | done |
| 2 | `proto`: identity, contact tickets, grants, handshake messages + tests | sonnet:proto | `crates/proto`, `docs/protocol.md` | done |
| 3 | `audio`: Opus 1.6 codec, packet format, jitter buffer/PLC/FEC, tone tools + tests | sonnet:audio | `crates/audio` | done |
| 4 | Android Gradle skeleton: Compose app, cargo-ndk + UniFFI bindings, installs on emulator | sonnet:android | `android/`, `scripts/build_android.py` | done |
| 5 | `core`: iroh endpoint, contacts store, call state machine, UniFFI API | lead | `crates/core` | done (desktop↔desktop verified) |
| 6 | Host test peer CLI (headless, tone in / wav out) | lead | `crates/peer` | done |
| 7 | Android app per `docs/android-app.md` | sonnet:android-app (worktree) | `android/` | done, merged, re-verified |
| 7b | Desktop app (iced) + cpal/APM audio | sonnet:desktop (worktree) | `crates/desktop` | done, merged, re-verified |
| 8 | E2E on emulator: add contact, call both ways, background/doze/reboot | lead | `scripts/e2e_*.py` | done (real-mic content on emulator blocked, see README) |
| 9 | Compact QR ticket (`OSVC2:` base32) | sonnet:ticket (worktree) | `crates/proto` | done |
| 10 | Review of `crates/core` | sonnet:review (worktree, read-only) | — | done, fixes merged |
| 11 | Idle network measurement (`scripts/measure_idle.py`) + iroh timer audit | sonnet (worktree) | `scripts/` | done: ~330 bursts/h idle; own relay deferred |
| 12 | Passphrase vault in core: Argon2id-sealed secrets, device-unlock key, migration | sonnet:vault (worktree) | `crates/core`, `crates/peer`, `scripts/e2e_*` | done, merged, tests re-run |
| 13 | Android: passphrase onboarding/unlock/migration + Keystore-wrapped unlock key | sonnet (worktree), after 12 | `android/` | in progress |
| 14 | Desktop: passphrase onboarding/unlock/migration | sonnet (worktree), after 12 | `crates/desktop` | in progress |
