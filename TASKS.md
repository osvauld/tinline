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
| 13 | Android: passphrase onboarding/unlock/migration + Keystore-wrapped unlock key | sonnet (worktree), after 12 | `android/` | done, merged (emulator-verified) |
| 14 | Desktop: passphrase onboarding/unlock/migration | sonnet (worktree), after 12 | `crates/desktop` | done, merged; audio decoupled from UI loop |
| 15 | Code review (4 parallel read-only reviewers: crypto, core, Android, desktop/release) | sonnet x4 | — | done; findings triaged into 16–18 |
| 16 | Fix core/proto review findings (tickets, handshake DoS, call states, perms, verify_strict) | sonnet (worktree) | `crates/core`, `crates/proto`, `crates/cryptography` | done, merged, tests re-run |
| 17 | Fix Android review findings (mic FGS, audio focus/routing, FLAG_SECURE, release/R8, targetSdk 36) | sonnet (worktree) | `android/` | done, merged; e2e on 2 emulators + R8 release smoke-tested (onboard, add, call) |
| 18 | Fix desktop review findings (Windows build, test-hooks feature, single instance, device loss) | sonnet (worktree) | `crates/desktop` | in progress |
| 19 | Rename to Tinline (`com.osvauld.tinline`), Terms/Privacy acceptance, reset-ticket UI | lead/sonnet, after 16–18 | all | Android id + name done; desktop rename (after 18), Terms/Privacy, reset-ticket todo |
| 20 | CI: GitHub Actions for Android AAB/APK, Linux deb/rpm/AppImage, Windows installer | todo | `.github/` | todo |
| 21 | Core for the design: call history, contact alias + verified, safety number, availability, reconnecting, end reasons | sonnet:core-design (worktree) | `crates/core`, `docs/protocol.md` | in progress |
| 22 | Android redesign per `docs/design/` (tokens, fonts, icon, all screens on today's API) | sonnet:android-design (worktree) | `android/` | in progress |
| 23 | Wire 21 into the Android UI (recents, rename, verify, availability, end reasons) | lead, after 21+22 | `android/` | todo |
