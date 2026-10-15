# Tinline refactor plan — synthesis of 9 audits (2026-10-14, dev @ 214a664)

Sources: 01-core-tests, 02-proto, 03-group-sim, 04-core-chat-sync-search, 05-core-group-gcall,
06-core-toplevel, 07-desktop, 08-android, 09-media-scripts (docs/refactor/audit/). The lead re-checked
the starred (★) claims against the code.

Size: ~155k tracked lines of code. ~39k are test code (Rust and Kotlin).

## Numbers (loc 0.5 + scratchpad/metrics.py over tracked .rs/.kt/.py; full output in metrics.txt/.json)
- loc: 135.5k code lines: Rust 110.6k (269 files), Kotlin 17.2k (86), Python 3.5k (23).
- Raw lines 156.5k in 384 files. **94 files ≥500 lines hold 104k lines (66%)**. 26 files ≥1200 lines hold 51.6k.
- Tests: 30% of lines (42.4k in test files + 6.1k inline `#[cfg(test)]` in 50 prod files), plus ~9.5k
  bare `#[test]` code mixed into 14 group-sim model files. 1,146 Rust `#[test]` fns.
- 93 of 172 Rust src modules have no unit test of their own. Android: 6 of 70 sources have a matching *Test.kt.
- Functions ≥80 lines: 136 (≥150: 32, ≥300: 8). Worst: desktop `update_inner` 999, proto adversarial test 905,
  group-sim flags test 896, desktop `update_chat_inner` 426, `load_demo` 415.
- Per area: prod / test-file / inline lines: core 32.2k/17.5k/3.4k (38 files ≥500), group-sim 22.2k (≈9.5k of it
  tests), desktop 20.0k/1.3k/1.5k (15), android 19.3k/1.2k (10), proto 2.6k/15.6k/0.4k (86% tests), video 4.7k/1.0k/0.5k.

## Structure standards (user 2026-10-14: no big files, tests in their own files, new crates OK)
- Prod file: target ≤400 lines, hard cap 600. Function: target ≤60, cap 120 (match arms → per-variant fns).
- Rust unit tests: never inline beyond a few lines. `foo.rs` declares `#[cfg(test)] mod tests;` and the tests
  live in `foo/tests.rs` (identity, cryptography, video already do this). One test file per module, named after
  what it tests. Large ones become `foo/tests/<topic>.rs`.
- Integration tests: `crates/<c>/tests/<feature>.rs` thin entry files plus a shared `tests/support/` (or `testkit` crate).
  No copied helpers.
- Kotlin: logic moves to Rust. What stays gets `FooTest.kt` beside its source package. UI kits are consolidated.
- group-sim: each model becomes `model/<name>/mod.rs` + `model/<name>/tests.rs`, and the shared kit goes in `model/common/`.

## New crates (proposals)
- `testkit` (publish=false, dev-dep): seeded Rng, temp dirs, ident(seed), eventually/until, pct.
- `present`: shared UI logic, UniFFI-exported (Phase 1). Possibly part of `core` behind a module if UniFFI
  multi-crate setup costs too much — check first.
- Candidates for splitting core (32k prod): `gcall` engines (pure, trait-based: rank/board/host/forwarder),
  `search` (index/scan/text), `chat-doc`/`group-doc` (CRDT docs + validation). `node`/UniFFI façade stays in core.
  Each split needs a dependency check (store, Error, uniffi types). An agent should map it before anyone moves code.
- proto: split lib.rs (1.4k) by wire domain. The rotation permit model goes to `tests/support/permit_model/`.

## Phase 0+ — mechanical file moves (zero behaviour change, do FIRST, before dedup)
- Move the 20 inline test blocks ≥100 lines (4.7k lines) to `foo/tests.rs`. Move the group-sim tests (~9.5k) out of the model files.
- Split the 26 files ≥1200 lines along the seams named in reports 04–08 (node.rs, chat/engine, group/engine,
  search/index, gcall/net, app.rs, view.rs, GroupsLive.kt, proto lib.rs, media.rs, …).
- Pure moves → reviewable by `git diff --stat` + `cargo test` count unchanged (record #[test] count before/after).

## Guiding rule
Non-UI logic lives in Rust, exported once via UniFFI. Desktop (iced) and Android (Compose) are thin:
rendering plus platform glue. Android already reaches core only through UniFFI ★ (`core/src/lib.rs:4`).
The only hand JNI is `video_jni.rs`, which stays for zero-copy planes. So moving logic to Rust costs
only `#[uniffi::export]`.

## Bugs / drift found (fix while consolidating)
- ★ Call-quality bars differ: desktop `view.rs:~160` uses 100/200/350 ms, Android `Support.kt:76` uses 120/250/450 ms.
- ★ Avatar colour hash differs: Kotlin `h*31+c` (`Components.kt:185`), desktop `DefaultHasher`
  (`ui.rs:316`; that hasher is not stable across Rust versions either). The same contact gets a different colour per app.
- Initials: desktop takes the first two words, Kotlin the first and last. End-reason and link-failure copy differ, and Kotlin covers more reasons.
- ★ Two `tie_hash` functions (`gcall/board.rs:12` JSON vs `gcall/rank.rs:113` spec formula). RESOLVED with user
  2026-10-14: the starter ranks hosts and connects to the winner, which becomes the call host (spec §2.3).
  Board's hash is only the fallback in §5 split-after-redial (after the backup-index rule). Fix: board calls
  `rank::tie_hash`; delete the JSON copy. Pre-launch, so no compatibility concern.
- `decode_vv` has 4+ copies that handle empty input differently. `day_valid` has a lax and a strict copy.
  ★ `UPDATES_BEFORE_SNAPSHOT` is u32 in chat and u64 in group. `GroupStore::open` retries 50 times vs 20 elsewhere.
- ★ `DIAL_TIMEOUT` is defined in 9 files: 20 s ×6, 30 s (node), 10 s (sync), and the gcall answer timeout.
- ★ Phrase→key derivation has 3 copies (`group/store.rs:72`, `rekey.rs:36`, `search/index.rs:80`).
  Unify only behind byte-identical golden tests, because a drift silently changes store keys.
- `*_for_test` hooks: 11 in node.rs and 14 in the exported UniFFI `Node`. They ship in the Kotlin bindings, so gate them behind `test-hooks`.

## Phase 0 — mechanical, low risk (parallelisable, ~1 day of agents)
| # | Work | Files (owner boundary) | Est. lines |
|---|------|------------------------|-----------|
| 0a | ★ `impl From<StorageError> for Error`, delete 5 `io()` + 36 ad-hoc map_errs | core: store, rekey, chat/store, group/store, search/index | −150–200 |
| 0b | Promote `sync/frames.rs` to `core::frame`, drop chat/wire + group/wire codec copies; shared `dial()` helper | core wire files | −120 + dial |
| 0c | **core/tests/support/mod.rs**: `Quiet`, `Tap`, `LinkTap`, RAII `TempDir`, `Peer{new,link_fresh,restart,restore_in_place,messages…}`, `pair`, `online`, `eventually`; default no-op bodies on `NodeEvents`/`LinkEvents` | core/tests/* | **−3.5–4.5k** |
| 0d | proto `tests/support/permit_model.rs` (Claims/Permit/View ★ ×7, verify_history_page, bind_live, Checkpoint) + fixture_seed/key | proto/tests/* | −1.0–1.3k |
| 0e | `scripts/lib/e2e.py` (`Phone`, `Checks`, `until`, `peer`, `build_peer`, `null_sink`, adb/PKG consts) | scripts/e2e_* | −400 |
| 0f | Workspace `testkit` crate (publish=false): seeded `Rng` (10 copies), `pct`, `ident(seed)` (~19 copies), temp dirs | new crate + dev-deps | −250 |
| 0g | Opus Encoder/Decoder: core/voice.rs → reuse `audio`; drop `opusic-sys` from core | core/voice.rs, audio | −120 |

## Phase 1 — ONE place for app logic (user 2026-10-14: "whatever we can abstract from desktop and android, we do")
Mechanism (verified): uniffi 0.29, Android bindgen runs `generate --library libp2pcore.so`. Library mode emits
bindings for EVERY uniffi crate linked into the .so. So a new crate with its own `uniffi::setup_scaffolding!()`,
depended on and re-exported by core, reaches Kotlin with no JNI work. Desktop depends on it directly, which is
fast to build and has no iroh. To do: add the crate to the gradle `rustInputs`, set the Kotlin package in `uniffi.toml`, and keep it
linked (`pub use` in core). Source inventory: 07-desktop §A (21 items) + 08-android §1.2 (27 items).

### New crate `crates/present` — pure, deterministic, no clock reads (callers pass now/tz), golden-table tests
| Module | Replaces (desktop ↔ Android) | Notes |
|---|---|---|
| `names` | first_name ×3 desktop + ChatUi.kt; initials (ui.rs:652 ↔ Components.kt:187 — DIFFER); display_name | |
| `avatar` | ui.rs:316 DefaultHasher ↔ Components.kt:185 h*31 (DIFFER = bug); authorColor | returns slot index; each UI maps it to its palette |
| `time` | ago/day_label/when_short/long/in_words/clock/duration_words: ~8 Kotlin + ~4 desktop variants | one canonical copy per VOICE.md |
| `call` | end reasons (reason.rs ↔ Support.kt:92 — DIFFER, Kotlin has more), quality (DIFFER), stats line ×3, call_ui_model(state)→{title,subtitle,buttons} | |
| `link` | failure text (link.rs:79 ↔ Link.kt:166 — DIFFER), QR text check, `LinkFlow` reducer (link.rs ↔ Link.kt:60-190) | reducer is a uniffi Object |
| `onboarding` | step graph/progress/phrase-confirm (flow.rs/gate_view ↔ Onboarding.kt:63-185, :413) | platform keeps the permission steps |
| `block` | composer/call-gate policy + copy (blocks.rs ↔ Blocking.kt) | pure fn of core's block facts; keeps policies separate per AGENTS.md |
| `chat` | message row grouping, day dividers, bubble run position (geom.rs Pos ↔ Conversation.kt:186-215), previews (×3), compose/draft state (compose_state.rs ↔ ConversationDrafts.kt), emoji (emoji.rs ↔ Emoji.kt) | |
| `group` | system lines, list preview, rejection text, role→permissions (group_live.rs ↔ GroupsCore.kt:130-480) | seed: desktop `GroupsLive` is already UI-agnostic |
| `notify` | idle/status text, message preview (tray.rs ↔ Notifications.kt) | builders stay native |
| `search` | highlight/marked spans (shell_search.rs ↔ Search.kt/FindBar.kt — Kotlin re-folds text itself) | calls core `search::text::folded`; Kotlin keeps only the UTF-16 offset mapping |

### Into existing crates
- `tin-icons` (or a new `tin-geom`): mark spring model + tiers + status_look (mark.rs 971 ↔ Mark.kt 337), smooth-corner
  outline + bubble radii (geom.rs ↔ Geometry.kt), all icons (retire PgIcons/Glyphs/ChatGlyphs/CallGlyphs.kt + ui.rs Lucide strings).
  Output plain path commands; tiny-skia and Canvas only draw them.
- `audio`: waveform bars (voice.rs:408 ↔ VoiceMessage.kt:431), resamplers; Opus codec shared with core.
- `identity`: `bip39_suggest/valid_word` → delete android `assets/bip39_english.txt` (2048 lines).
- `core`: `friendly_error(&Error)` (app.rs:521 ↔ Support.kt:19); file-open policy `open_plan(name,mime)` (FileOpen.kt; desktop
  lacks it — security, single audit point); `GroupCallConditions::from_inputs` (hostcond.rs ↔ HostConditions.kt; sensors stay
  native); account add/switch/restore sequencing behind a `KeyStore` trait (flow.rs ↔ P2pApp.kt:246-335); terms-version
  acceptance (LegalStore) into profile kv; drop the Kotlin I420 fallback (VideoIo.kt:126-214).
- Later, its own design doc: `core::app::Session` (dispatch/snapshot/one event sink) replaces the per-app event reducers
  (chat.rs/group_live.rs/blocks.rs ↔ ChatSource/GroupsSource/Blocking/Devices). After that the UIs are pure views.

### Stays native (platform layer)
Telecom/CallSession, permissions, notification builders, camera (CameraX)/mic/speaker I/O and routes, Keystore/keyring,
battery/network/thermal sensors, rendering, tray.

### Order & acceptance
1. Leaf fns with twins: names, avatar, call quality/end reasons, friendly_error, emoji, bip39, geometry.
2. time words (requires one canonical copy table), link/block/compose/host-conditions state machines.
3. chat row grouping, call_ui_model, onboarding, group presentation, mark/icons.
4. file-open policy + account sequencing (security review), then the app::Session design.
Each step deletes BOTH old copies in the same change; the Kotlin and desktop unit tests for that logic move to one
Rust golden test (`present/<module>/tests.rs`). Where the copies DIFFER, the agent lists both versions and the VOICE.md choice
for user review. It does not silently pick one.
Estimate: −1.5–2.2k lines in the apps plus the 2k-line asset; Android src/main logic shrinks toward UI + platform only.

## Phase 2 — dedup inside core (behaviour-preserving)
- sync/engine `self_loop`: a `WholeDoc` table replaces 4 copies (−90–120). push_groups/push_invitations merge.
- chat/engine: helpers for close-sessions, dm-peers, pending, and the ingest+announce tail; then split along banners.
- chat/doc + group/doc: shared `apply_remote` validation prefix; move day/time helpers to a neutral module.
- group: `HostKind` helper for the 6 appoint/revoke/accept fns, `GroupCore::am_host`, `relay_targets()`
  (enforcement rule duplicated 3× — drift is a security risk).
- search: merge `search`/`search_in`; move the tests and `Scan` out of index.rs; bench behind a feature.
- node.rs split (mod/types/api_*/persist/lifecycle/accept/call/devices/testkit/util); `run()`/`linger()` helpers.
- gcall: board uses `rank::tie_hash` (single §2.3 formula); add a split-after-redial test where neither host is in backups.
- Dead code: `HostEngine::is_limited`, `lower_priority`, `CallBoard::previous`; StaticDirectory/StaticAuthority → cfg(test).

## Phase 3 — test-suite consolidation (after 0c/0d)
- Merge overlaps: device_list_characterization↔device_list_push (identical test name ★), grant_characterization↔
  call_tokens_acceptance, the restore family (4 files), table-drive chat_shards skew tests.
- 15 `#[ignore]` stubs in rotation_acceptance ★ + 2 stale ignores in blocking_acceptance: implement or delete (decision).
- Replace 45+ fixed sleeps with `Node::wait_online` / outbox-flush hooks; move latency asserts to a release bench.
- Split offline-deterministic from network tests (23/32 core test files need a real relay).
- group-sim: shared `common` kit (SimRng ×4, World/Op/Board…) −1.8–2.3k; merge `seal/open` with chat_request.
- proto: use the real `sign_block_notice` in 3 tests; drop hand base64.

## Phase 4 — UI apps (coordinate with the active design/screens port FIRST)
- Desktop: gate `groups_fake` (1.7k) + preview stack behind debug/test-hooks; `mod voice` is unwired;
  split `App::update_inner` (~1000 lines) and the large view fns; spacing/radius tokens; shared dialog.
- Android: ~2.5k placeholder lines ship in src/main; UI kits from 4 generations → 1 (~−1k);
  drop `assets/bip39_english.txt` (use the `bip39` crate via core); mic-permission and query-pattern helpers.

## Decisions for the user
1. ~~tie_hash~~ resolved: one function, rank's spec formula (see Bugs).
2. Archive group-sim `group_call.rs` (3.5k) and `policy_candidates.rs` (1.8k) after a GC-S coverage check?
   Retire proto `rotation_succession_model.rs` + `support/rotation_network.rs`?
3. The 15 rotation_acceptance `#[ignore]` placeholders: keep them as a spec checklist or move them to docs?
4. Remove the placeholder/fake group UIs from release builds (desktop + Android) now, or wait for the port?
5. Phase 1 `app::Session` façade: worth a design doc now?
6. `Backing::Json` legacy store path: sunset?

## Doc fixes collected (for the later docs pass)
protocol.md: the block-notice wire is `/1`, not `/0` (line 482); the domain summary is missing devices/v1 and block/v1; there is no gcall wire section;
rotation/permit exists only as test models, not as an implementation. Code headers say chat/3 and self/2, but the ALPNs are chat/4 and self/3.
gcall/mod.rs + net.rs say membership is "not in production", but calldir wires it. group-policy-options.md still says
"not approved" for the decided U11. group-sim-coverage-audit.md covers 24 of ~200 tests. android-app.md is missing Groups,
Search, VideoIo, VoiceMessage, HostConditions, CallVideo, Notifications, and the UniFFI/JNI split. chat.md code map is missing
own.rs, presence.rs, engine/synth.rs. Every report has a per-file map to seed the docs.

## Rough totals
Dedup without behaviour change: ~10–12k lines. Archivable models and placeholder UI: a further ~10k.

## Dependency upgrades (checked 2026-10-14 vs crates.io / Google Maven / Maven Central; details in deps/*.md)
Rust: 61 direct deps → 36 current, 4 compatible bumps, 21 breaking. Lock: 998 crates, 106 with >1 version.
Order (each its own change; golden/compat tests land BEFORE the bump):
0. **Freeze golden vectors on current versions**: ed25519 sign/verify, x25519, HKDF, HMAC, AES-GCM, Argon2 vault key,
   BIP-39 seed for a fixed mnemonic, bincode bytes of signed bodies/invite links. There are none in the repo today.
1. Quick: `cargo update` compatible (tokio 1.53.3, serde_json, cc, notify-rust); base64 0.23; png 0.18 (tray.rs:75);
   **libaom 3.14.1 → 3.15.2** (security: CVE-2026-56208..56211 / 13906 class; scripts/build_libaom.py:25 + SHA);
   pin `rust-toolchain.toml` 1.99.0 (coordinate; skip 1.97/1.98).
2. **redb 2.6.4 → 4.x — user data.** Existing DBs are file format v2 (verified: 2.6.4 `default_to_file_format_v3: false`);
   redb 4 returns `UpgradeRequired(2)`. Keep `redb2 = { package = "redb", version = "=2.6.4" }`, back up the file,
   `Database::upgrade()`, then open with redb 4. Test on real v2 fixtures (desktop + Android data dirs). Ship alone.
3. uniffi 0.29.5 → 0.32.2: the custom `FfiConverter for Bytes` (video_call.rs:97), 9 `with_foreign` sites, Kotlin exception
   names, async constructors. No data risk. Do before Phase 1 `present` crate work (or together, decide).
4. Crypto group together (digest 0.11 + rand_core 0.10): rand 0.10, ed25519/x25519-dalek 3, sha2 0.11, hmac/hkdf 0.13,
   aes-gcm 0.11, argon2 0.6, bip39 3. First add one RNG helper (~26 `OsRng` sites). Golden vectors from step 0 must pass unchanged.
   Aligns with iroh (already on dalek 3/sha2 0.11/rand 0.10). rand 0.8/sha2 0.10 remain via loro/zbus.
5. Desktop: drop gtk via tray-icon 0.26 `ksni` backend (gtk3 unmaintained RUSTSEC-2024-0413; gtk 0.19 conflicts) — test KDE+GNOME.
   resvg/usvg 0.48 (pixel baselines), rtrb 0.4 (read changelog), jni 0.22 (video_jni.rs 2 fns).
   **keyring 4: WAIT** — all our features removed, unverified that 3.6 entries stay readable; needs cross-version test on 3 OSes.
6. **bincode: superseded 2026-10-15 by postcard (`cryptography::codec`); was: stay on 1.3.3.** 3.0.0 is a tombstone (4 KB, compile_error); 2.x changes the encoding of signed/wire/persisted
   bytes. Moving off is a separate protocol project (RUSTSEC-2025-0141 is informational "unmaintained").
No live security advisories against our current Rust versions except that informational bincode one.

Android (deps/android.md): targetSdk 36 meets the Play 2026-08-31 deadline. arm64 .so already 16 KB aligned (2027-02-01 rule). NDK r28 ok.
0. LocalLifecycleOwner → androidx.lifecycle.compose (4 files), zxing core 3.5.4.
1. Gradle 9.6+ / AGP 9.4.1 / Kotlin 2.4.21 together: install build-tools 36.0.0; remove the kotlin-android plugin (built-in);
   rewire `jniLibs.srcDir(Provider)` (app/build.gradle.kts:63) + `kotlin.srcDir(uniffiOut)` via androidComponents generated dirs.
2. Compose BOM 2026.09.00, activity 1.13, lifecycle 2.11, core-ktx 1.19.1, coroutines 1.11 (LocalClipboardManager in Conversation.kt).
3. JNA 5.19.1 (not 5.19.0). 4. CameraX 1.6.2 on a real phone (VideoIo.kt:49-53 strides/rotation). 5. Optional: replace the
   unmaintained zxing-android-embedded (Camera1) with a CameraX analyzer + zxing core.
Each step: build, unit tests, release build `zipalign -P 16` check, emulator smoke (call, video, QR link).

## Wave 1 (started 2026-10-14 on branch `refactor/main`, worktree `.claude/worktree/refactor`)
Scope: Rust core/proto/group-sim/crypto + a new unwired `present` crate. **No desktop or Android edits in wave 1.** The UI port
(design/screens, ds/*, ui-parity, signup-port) is active on those files. Desktop/Android splits and wiring `present`
into both apps come after the port settles, coordinated with the port owners.
Deferred to wave 2: dependency bumps (Cargo.lock churn; libaom touches the shared libaom cache other sessions build
from), `testkit` crate (wave 1 uses per-crate support modules), cross-folder dedups, scripts/lib/e2e.py,
archiving superseded group-sim models (user decision pending).

| Agent branch | Owns | Task |
|---|---|---|
| rf/core-tests | crates/core/tests/** | shared `tests/support/mod.rs`; remove copied helpers; no test semantics change |
| rf/proto | crates/proto/** | tests/support permit model + fixtures; split src/lib.rs (1.4k) by domain; inline tests → files |
| rf/group-sim | crates/group-sim/** | tests out of model files into `<model>/tests.rs`; shared `common` kit; split files >600 |
| rf/core-chat | crates/core/src/{chat,sync,search,profile}/** | inline tests → `x/tests.rs`; split chat/engine.rs, search/index.rs, sync/engine.rs |
| rf/core-group | crates/core/src/{group,gcall}/** | same for group/engine, calls, state, gcall/net; board uses rank::tie_hash + split-after-redial test |
| rf/core-node | crates/core/src/*.rs top level, src/video_call/, src/bin/ | split node.rs (3.8k) per audit 06; inline tests → files; voice/video_call/store |
| rf/golden | NEW test files only in crates/{cryptography,identity,proto}/tests/golden_*.rs (+ core/tests/golden_wire.rs) | freeze golden vectors on current versions before any crypto/bincode/redb upgrade |
| rf/present | NEW crates/present/** + workspace members line | pure fns + golden tables from audit 07 §A / 08 §1.2; record differing copies for user review |

Rules: moves are behaviour-preserving. Per-crate `#[test]` count must not drop (baseline at 84bc7d4: core 458,
proto 240, group-sim 183, cryptography 17, identity 18). Files ≤600 lines (target 400), unit tests in `foo/tests.rs`.
All builds through `/home/abe/p2p_chat/scripts/buildlock.py`. Each agent commits on its own branch; the lead merges into
`refactor/main`. Nothing goes to `dev` without the user.
