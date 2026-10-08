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
| 18 | Fix desktop review findings (Windows build, test-hooks feature, single instance, device loss) | sonnet (worktree) | `crates/desktop` | done, merged (Windows build unverified: webrtc-audio-processing C++ fails under mingw; needs Windows CI) |
| 19 | Rename to Tinline (`com.osvauld.tinline`), Terms/Privacy acceptance, reset-ticket UI | lead/sonnet, after 16–18 | all | Android id + name done; desktop rename (after 18), Terms/Privacy, reset-ticket todo |
| 20 | CI: GitHub Actions for Android AAB/APK, Linux deb/rpm/AppImage, Windows installer | todo | `.github/` | todo |
| 21 | Core for the design: call history, contact alias + verified, safety number, availability, reconnecting, end reasons | sonnet:core-design (worktree) | `crates/core`, `docs/protocol.md` | done, merged; 26 core tests re-run |
| 22 | Android redesign per `docs/design/` (tokens, fonts, icon, all screens on today's API) | sonnet:android-design (worktree) | `android/` | done, merged (agent: e2e both emulators + release onboarding walk; lead: centring fix) |
| 24 | Desktop redesign per `docs/design/Desktop*` + Tinline rename, on the new core API | sonnet:desktop (worktree), after 18 | `crates/desktop` | done, merged; lead re-ran desktop tests + desktop_e2e tone |
| 23 | Wire 21 into the Android UI (recents, rename, verify, availability, end reasons) | sonnet:wire (worktree) | `android/` | done, merged; lead re-ran e2e_android + phone_phone |
| 25 | Chat core (1:1): copied osvauld storage + sealed vault records, encrypted iroh-blobs, Loro day-shards with signed updates, files (2 GB), voice messages (hold to record), API + events, e2e_chat.py | sonnet:chat (worktree) | `crates/core`, `crates/peer`, `crates/storage`, `docs/chat.md` | done (merged; per-message signatures added) |
| 26 | Chat design boards (Android chats/conversation/files+voice, desktop chat) | lead | design canvas | done |
| 28 | Optional passphrase (design change 2026-10-08): core identity without passphrase (keys protected by Keystore / OS keyring only), add one later in Settings; onboarding drops quick check, single passphrase field + Skip | sonnet:passphrase (worktree) | `crates/core` vault, `android/` onboarding/unlock/settings, `crates/desktop` | done (emulator-checked by lead) |
| 29 | Call waiting (Flow 4b): second incoming call rings as a banner over the current call; Decline → caller sees Busy; End & answer; ignored 30 s → missed | sonnet:callwait (worktree) | `crates/core` calls, `android/` call screens, `crates/desktop` | done (e2e_callwait) |
| 30 | Voice messages: Ogg Opus 16 kHz recorder/decoder + 64-peak waveform in core; Android hold-to-record + voice bubble; desktop recorder/player widget | sonnet:voice (worktree) | `crates/core/src/voice.rs`, `android/.../VoiceMessage.kt`, `crates/desktop/src/voice.rs` | done (emulator-checked by lead) |
| 27 | Chat UI on Android + desktop (Android and desktop agents, against the frozen chat API stub from 25) | after 25's API stub, 26 | `android/`, `crates/desktop` | done (both merged; Android tested emulator <-> peer) |
| 31 | See attachments: desktop image thumbnails + full-size viewer and Open for documents; Android Open for documents (PDF in-app, others via system viewer). Only safe types open; others Save only | sonnet:preview-android, sonnet:preview-desktop (worktrees) | `android/` chat files, `crates/desktop` chat | done (merged; emulator + demo screenshots; real phone↔laptop pending) |
| 32 | Reliable file transfer (small files; no folders, LAN discovery or preview — DashBeam is reference only, AGPL): stall watchdog on blob fetch (no progress for 20 s → fail), automatic retry with backoff while the session is up (not only on reconnect), receiver cancel, "Tap to retry" on failure; e2e with a peer killed/restarted mid-transfer both directions | todo | `crates/core/src/chat/{engine,blobs}.rs`, `scripts/e2e_chat.py`, then apps for retry/cancel UI | todo |

## Roadmap (after the redesign)

| # | Item | Notes |
|---|------|-------|
| R1 | 1:1 chat on Loro — FIRST | design in `docs/chat.md` §2, tests C1–C8 written first |
| R2 | Time-based sharding | one doc per conversation per time window; recent shards sync first, old ones on demand |
| R3 | File sending with iroh-blobs | messages carry BLAKE3 hash + name + size; content streams device to device, resumable |
| R4 | Group chats — after R1 (admins may create admins) | shared doc with a DID-signed member list; group encryption and member add/remove need a design doc first |
| R5 | Offline delivery: members relay for each other (pure P2P, no node) | an online member who holds an update forwards it; delivery needs some member overlap |

Reuse first (user, 2026-10-07): `~/osvauld2/courier` has the permits (node-rooted delegation tokens `token.rs`, `policy.rs`, `role.rs`, per-doc read/write rules `access.rs`) and the Loro sync engine (`sync.rs`, `subscribe.rs`, `publish.rs`); `~/osvauld2/docs/design/group-chat-sync.md` (shards, permissions, ephemeral; steps 0-6 built) and `app-permissions.md`, `loro-notes.md` are the design. Courier is transport-free, so it can ride Tinline's iroh endpoint.

Decided (user, 2026-10-07): **pure P2P, no sovereign node (no kunki)**. A group's permit chains root at the creator's DID (`verify_chain` takes the root as a parameter); the permit set lives in the group doc so every member holds it; every Loro update travels with its author's permit chain + a signature by the author's attested device, and each peer verifies (identity via proto attestations, chain, `access.rs` rules) before importing. Open design points: concurrent revocation/removal semantics, member relay, shard holding.
