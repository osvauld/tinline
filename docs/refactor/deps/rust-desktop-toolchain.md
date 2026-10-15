# Rust desktop deps + toolchain + libaom: upgrade research (2026-10-14)

Read-only research; nothing edited/built. Source confidence noted where a fetch was partial.
Cargo.lock facts below were grepped from /home/abe/p2p_chat/Cargo.lock.

## Top-line recommendations
| Item | Do | Effort |
|---|---|---|
| libaom 3.14.1 -> 3.15.2 | UPGRADE NOW (security CVEs fixed in 3.15.0; oss-fuzz fixes in 3.15.2) | S |
| rustc | move to 1.99.0 on next convenient cycle; add rust-toolchain.toml pin (1.99.0 or 1.97.1+) | S |
| png 0.17 -> 0.18 | upgrade now (desktop only, 3 call sites + tray decode); removes dup | S |
| rtrb 0.3 -> 0.4 | upgrade now, low risk (verify changelog first) | S |
| tokio/serde_json/notify-rust/cc | take with next `cargo update` | S |
| resvg/usvg 0.45 -> 0.48.1 | later (M); tiny-skia 0.11->0.12 bump is the real change | M |
| keyring 3 -> 4 | WAIT; feature flags all change; needs on-device entry-compat test | M-L |
| tray-icon 0.21 -> 0.26 | later; combine with dropping gtk via `ksni` backend, prototype first | M |
| gtk 0.18 -> 0.19 | DON'T: blocked by muda/libappindicator (still gtk ^0.18); gtk3 is unmaintained anyway | - |
| jni 0.21 -> 0.22.4 | later (M); 2 functions, real API break; netdev still pulls 0.21 on Android | M |
| iced 0.14 | latest stable; stay | - |

## Duplicates in Cargo.lock
- png: 0.17.16 (used by tray-icon, muda, tiny-skia/resvg stack, our desktop dep) and 0.18.1 (via image 0.25.10). Bumping our `png` to 0.18 and tray-icon 0.26 (needs png ^0.18) removes most of 0.17; tiny-skia 0.11 (resvg 0.45) still pins 0.17.16 until resvg 0.47+ (tiny-skia 0.12). So full dedupe needs png + tray-icon + resvg together.
- windows-sys: 0.45.0 (jni 0.21 <- netdev and p2pcore), 0.52.0, 0.59.0, 0.60.2 (tray-icon 0.21, muda, keyring 3), 0.61.2. tray-icon 0.26 accepts `>=0.60,<=0.61` so cargo can unify onto 0.61.2. 0.45.0 goes away only when jni 0.21 has no users: netdev also uses jni 0.21 (Cargo.lock ~5114/5141), so it persists until netdev moves. 0.52/0.59 are from other transitive crates (not analysed).
- jni: 0.21.1 AND 0.22.4 already both in the lock (0.22.4 via cpal/android-activity/others; 0.21.1 via our p2pcore and netdev). Upgrading p2pcore removes only one of two 0.21 users.

---
## 1. gtk 0.18.2 -> 0.19.0
1. Changed: 0.19.0 published 2026-09-08 by gtk-rs/gtk3-rs ("Rust bindings for GTK+ 3"); 0.18.2 is marked deprecated "UNMAINTAINED ... use gtk4 instead" (https://crates.io/crates/gtk). Release notes for 0.19 not retrieved.
2. Advisories: RUSTSEC-2024-0413 (atk) and siblings (gdk 0412, gdk-sys 0418, gdkwayland-sys 0411, gdkx11 0417, atk-sys 0416...) all "unmaintained" informational, no patched version; recommendation gtk4-rs (https://rustsec.org/advisories/RUSTSEC-2024-0413). Exact ID for `gtk` itself not confirmed but same family. No exploitable vuln, just unmaintained warnings in cargo-audit.
3. Our use: crates/desktop/Cargo.toml:40 (`gtk = "0.18"`, linux only). crates/desktop/src/tray.rs:154 `gtk::init()`, :171 `gtk::glib::timeout_add_local`, :173 `ControlFlow::Continue`, :175 `gtk::main()`. Purpose: tray-icon on Linux (libappindicator + muda menus) requires a GTK main loop on the tray thread. Not rfd.
4. Key blocker: tray-icon 0.26.1 -> muda 0.21 depends on gtk ^0.18 (crates.io deps pages), and libappindicator 0.9 is gtk 0.18-based. Our own gtk 0.19 would conflict with gtk-sys 0.18 (native `links`) -> cargo resolve error. So gtk 0.19 cannot be adopted independently.
5. Recommendation: do NOT bump. Better path: tray-icon 0.25.0+ added a GTK-free `ksni` StatusNotifierItem backend (https://github.com/tauri-apps/tray-icon CHANGELOG). Using it would let us delete the `gtk` dep and the gtk main loop in tray.rs (need a replacement loop/poll for refresh()). Risks: GNOME needs the AppIndicator shell extension either way; KDE/most others support SNI natively; menu/events behaviour must be tested. Effort M, prototype first. Acceptance: tray shows on KDE Plasma and GNOME(+extension), Show/Quit work, status text/icon update, no GTK libs needed at runtime, `cargo tree -i gtk` empty.

## 2. tray-icon 0.21.3 -> 0.26.1
1. Changes (CHANGELOG, https://github.com/tauri-apps/tray-icon/blob/dev/CHANGELOG.md): 0.22 MSRV 1.73; 0.25.0 adds ksni backend, libappindicator/ksni features separated from muda features, muda features renamed with `muda-` prefix; 0.26.0 removes `muda-libxdo` feature (default), deprecates `*_icon_as_template`, `set_icon` now clears template flag, KSNI no longer fails when no StatusNotifierWatcher; 0.26.1 KSNI sandbox no D-Bus name ownership. MSRV 1.90 from 0.25.0 (OK with rustc 1.92). Released 2026-10-07 (very fresh).
2. Advisories: none found for tray-icon; transitive gtk3 unmaintained warnings (above).
3. Our use: crates/desktop/Cargo.toml:34 `tray-icon = { version = "0.21", default-features = false }`. NOTE: with default-features off, 0.26 has no default `libappindicator` feature; Linux then has NO backend unless we enable `libappindicator` or `ksni`. Current lockfile shows libappindicator being pulled in, so verify what features 0.21 actually resolves (0.21 defaults differ). Call sites: tray.rs:9-11 imports (Menu, MenuEvent, MenuItem, PredefinedMenuItem, Icon, TrayIcon, TrayIconBuilder), :106-130 build, :134 refresh (`set_text`, `set_tooltip`, `set_icon`), :74 png decode. API of these is stable across the range; the break is Cargo features, plus png 0.18 for our own decode (below).
4. User-visible: Linux backend choice changes tray behaviour on GNOME/KDE; menu event handling on ksni differs (predefined items without muda action render disabled; we only use custom items). Windows tray unaffected API-wise.
5. Effort M. Recommendation: later, together with the gtk-removal decision; wait a week or two since 0.26.x is days old. Acceptance: tray on Linux (KDE + GNOME ext), Windows tray, icon flips by State/dark, tooltips, Quit, no panic when no tray host (spawn returns false path tray.rs:154-165 / 5s START_TIMEOUT).

## 3. keyring 3.6.3 -> 4.2.0
1. Changes (https://github.com/open-source-cooperative/keyring-rs/releases, docs.rs features): 4.0.0 (2026-04-26) split library into `keyring-core` 1.0 + separate store crates; the old API is retained behind default feature `v1` (4.1.0) which pulls apple-native, windows-native, and zbus-secret-service stores. 4.1.3 yanked; 4.1.4 fixes Entry::new bug; 4.1.6 fixes concurrent init of default v1 store. MSRV 1.88 (OK).
2. Advisories: none found for keyring 3.6.3. Lock shows 3.6.3 pulls openssl, dbus-secret-service and secret-service (both).
3. Our use: crates/desktop/Cargo.toml:32 `features = ["sync-secret-service","crypto-rust","vendored","apple-native","windows-native"]` — the 4.2 feature list is {default=v1, v1, windows-native-keyring-store, zbus-secret-service-keyring-store, apple-native-keyring-store, android-native-keyring-store, dbus-secret-service-keyring-store, linux-keyutils-keyring-store, db-keystore, cli}. All five of our feature names are gone -> cargo will error. Code: crates/desktop/src/keystore.rs:15 `SERVICE = "com.osvauld.tinline"`, :39 `Entry::new(SERVICE,user).and_then(set_password)`, :42 `get_password`, :45-46 `delete_credential`. The v1 feature keeps the same Entry API so source likely compiles unchanged, but errors types/`default store init` semantics changed (4.1.6 "lets callers check the result of default store creation").
4. DATA RISK (unverified): I could NOT confirm from docs whether entries written by 3.6.3 (service/username attributes, "application=rust-keyring" label on Secret Service; Keychain; Credential Manager target "user.service") are read identically by the 4.x zbus-secret-service / apple / windows stores. Release notes do not state compatibility or migration. Also 3.x used `sync-secret-service` (dbus-secret-service crate with crypto-rust) vs 4.x zbus store; backend change could differ in attribute naming or collection (default collection "login"). Account keys are the only thing at stake, plus migration code in keystore.rs:179.
5. Effort M-L (mostly testing). Recommendation: WAIT / do only after a real compatibility test: on Linux (GNOME Keyring + KWallet), macOS, Windows, create entry with a build using 3.6.3, upgrade, confirm get_password returns it, and also reverse (downgrade) case; ensure keystore.rs fallbacks (unlock.key / passphrase) never silently regenerate keys. If compat fails, keep 3.6.3 (no advisories) or pin and write migration read via old crate. Acceptance: round-trip across versions on all 3 OSes; keystore_tests.rs passes; no-keyring detection still returns Err("no secure keyring") (keystore.rs:56).

## 4. resvg/usvg 0.45.1 -> 0.48.1
1. Changes (https://github.com/linebender/resvg/blob/main/CHANGELOG.md): 0.46 MSRV 1.87 + edition 2024, adds `Tree::from_data_nested`; 0.47.0 (2026-02-05) tiny-skia 0.11 -> 0.12 (major); 0.48.0 (2026-07-31) MSRV 1.89, text shaping moved rustybuzz -> harfrust and ttf-parser -> skrifa (possible small render differences + binary size), new `svgz`/`writer` features; 0.48.1 (2026-08-02) MSRV lowered to 1.85. OK on 1.92.
2. Advisories: none known for resvg 0.45.1.
3. Our use: crates/desktop/Cargo.toml:21-22 (default-features=false both). `use resvg::tiny_skia as sk;` at crates/desktop/src/shaped.rs:18 and mark.rs:14. Break surface: anything touching tiny_skia 0.11 APIs through that re-export (Pixmap, Transform, Paint, PathBuilder); tiny-skia 0.12 changes unverified in detail - compile and see. With default-features=false there is likely no text, so the text-shaper swap probably doesn't affect us; confirm feature flags in the 0.48 Cargo.toml (new feature gates may need `svgz` off).
4. User-visible: generated icons/mark rendered by tiny_skia — anti-aliasing pixel differences could change visual baselines (AGENTS.md says preserve visual baselines) and screenshot QA.
5. Effort M. Recommendation: later (after png, as a single "render stack" PR); removes png 0.17 dup entirely together with tray-icon 0.26. Acceptance: run design/refined QA screenshots & mark/shaped tests, diff baselines, confirm `cargo tree -d | grep png` clean.

## 5. png 0.17.16 -> 0.18.1
1. Changes (https://github.com/image-rs/image-png/blob/master/CHANGES.md): 0.18.0 breaking: `Decoder` needs reader `Seek + BufRead` (`Decoder::new(Cursor)` OK; Cursor is both), `output_buffer_size()`/`output_line_size()` now return `Option<usize>`, removed `Info::encode`, `Encoder::set_srgb`, decoder event/streaming changes (not used), bitflags 2. MSRV unchanged (no change noted).
2. Advisories: none found.
3. Our use: crates/desktop/Cargo.toml:19 `png = "0.17"`. Call sites: tray.rs:74-82 (Decoder::new(Cursor).read_info(), `reader.output_buffer_size()` at :75 -> must `.unwrap_or/ok()?`; next_frame; ColorType matches); app.rs:2086-2088 Encoder::new/set_color/set_depth (unchanged); media.rs:369-371 same; chat_fake.rs:150-152 same. Only tray.rs:75 breaks (`vec![0; Option<usize>]`). Note `png::Decoder::new(BufRead+Seek)` at tray.rs:74 passes Cursor fine.
4. Risk: none user-facing.
5. Effort S. Upgrade now. Acceptance: `cargo check -p desktop`; tray icon and window icon (tray.rs window_icon) still decode; screenshot encoding test.

## 6. rtrb 0.3.5 -> 0.4.0
1. Changes: 0.4.0 published 2026-08-17 (MSRV 1.38); note 0.3.5 published a day later (2026-08-18) so 0.3.x backports continue. I could not retrieve the changelog (404 on GitHub raw; docs.rs gave nothing) so breaking details are UNVERIFIED.
2. Advisories: none found.
3. Our use: crates/desktop/Cargo.toml:37. crates/desktop/src/audio.rs:18 (`Consumer, Producer, RingBuffer`); voice.rs:19,126,156,235,262: `RingBuffer::new(n)` returns (prod, cons), `prod.push()`, `cons.pop()`, `prod.slots()` (voice.rs:285,295). These are the likeliest renamed/changed methods; compiler will say.
4. Risk: real-time audio path — behaviour (capacity/wraparound) must remain wait-free; run voice/audio tests.
5. Effort S. Read the 0.4.0 changelog on GitHub first, then upgrade now. Acceptance: audio.rs/voice.rs unit tests; call loopback smoke test.

## 7. jni 0.21.1 -> 0.22.4
1. Changes (https://github.com/jni-rs/jni-rs/blob/master/CHANGELOG.md): 0.22.0/0.22.1 yanked; use >=0.22.2. `JNIEnv` replaced by `EnvUnowned` (native-method argument) and `Env`; env is borrowed from an attachment guard; `with_env`/outcome error policy for native methods; `get_array_length` deprecated -> `JPrimitiveArray::len()`; `set_*_array_region` -> `JPrimitiveArray::set_region`; GlobalRef->Global; jni-sys 0.4; strings MUTF8; signatures typed. `get_direct_buffer_address` changes not documented in what I read.
2. Advisories: none found.
3. Our use: crates/core/Cargo.toml:61 `jni = { version="0.21", default-features=false }`; crates/core/src/video_jni.rs:13-15 imports `JNIEnv`, `JByteBuffer, JClass, JIntArray`, `sys::{JNI_FALSE, JNI_TRUE, jboolean, jint, jlong}`; :20-26 `region()` uses `env.get_direct_buffer_address/capacity`; :45 `Java_com_osvauld_p2p_VideoJni_pushYuv(env: JNIEnv<'l>, ...)`; :86 `pullRgba`; :95 `env.get_array_length(&info)`; :106 `env.set_int_array_region`. Both `extern "system"` signatures change (EnvUnowned) and must be wrapped in `with_env`, and the raw-pointer `JNIEnv` lifetime `'l` pattern goes. Gate: android only. `jni 0.22.4` is already in the lock (cpal etc.), so this just unifies one of the two; netdev still uses 0.21 so the dup (and windows-sys 0.45) persists until netdev updates.
4. Risk: JNI symbol names/ABI to Kotlin VideoJni must be unchanged; wrong signature = crash only at runtime on device, not at compile.
5. Effort M (small code, device-testing). Recommendation: later; no benefit now. Acceptance: Android build via buildlock, emulator video call push/pull frames path (existing video JNI test if any), device lock respected.

## 8. Compatible bumps
cc 1.6.0->1.8.0 (crates/video/Cargo.toml:33 `cc = "1"`; build script only; release notes not found — check cc-rs CHANGELOG when updating), notify-rust 4.18.1->4.18.2 (used at crates/desktop/src/app.rs:488 `Notification::new()`; notes not found; smoke-test a desktop notification on GNOME/KDE), serde_json 1.0.151->1.0.154, tokio 1.53.2->1.53.3. All semver-compatible: `cargo update -p <name>` is enough. Effort S each. Recommend taking them now.

## 9. iced
0.14.0 (2025-12-07) is the latest stable on crates.io and GitHub releases; no 0.15 pre-release or announcement visible (https://github.com/iced-rs/iced/releases, crates.io). Stay on 0.14. (Release notes page checked up to first 100k chars only.) Note: iced 0.14 uses wgpu/winit stacks that pin their own png/windows-sys; they will affect dedupe regardless.

## 10. Rust toolchain
- Current stable as of 2026-10-14: 1.99.0 (2026-10-01); 1.98.1 (2026-09-03), 1.97.1 (2026-07-16). Source: https://github.com/rust-lang/rust/blob/master/RELEASES.md
- Since 1.92 relevant to us (edition 2024, desktop + Android): 1.95 `if let` guards stable, `cfg_select!`; 1.96 `core::range` types; 1.96.1 includes libssh2 CVE patches in Cargo (security relevant for cargo fetches over ssh only); 1.97 v0 symbol mangling default, linker output warnings on by default (may create new build noise with linking vendored libs), `build.warnings` stable, `pin!` no deref coercions; 1.98: stricter ambiguous glob import errors (2024 edition may now hard-error), `repr(transparent)` stricter, Windows thread-local destructors via FLS; 1.98.1 and 1.97.1 are miscompilation fixes (LLVM vtable generation / LLVM opt) — so 1.98.0 and 1.97.0 should be avoided; 1.99: `no_mangle_generic_items` hard error, `static_position_independent_executables` on gnu targets, new `debug` cargo profile, incremental off in CI.
- Dependency MSRV picture: tray-icon 0.26 needs 1.90, keyring 4 needs 1.88, resvg 0.48 needs 1.85 (0.48.0: 1.89), rtrb 1.38. All satisfied by 1.92 already; 1.99 not required by any.
- Pin? No rust-toolchain file exists (verified). Recommend adding rust-toolchain.toml with `channel = "1.99.0"` (plus components rustfmt, clippy, and Android targets listed under `targets`) so multi-agent worktrees, buildlock jobs and CI agree, and bump deliberately each quarter. Pin to a patched point release, not 1.98.0/1.97.0. Caveat: rustup auto-installs the pinned toolchain on first cargo call in each worktree -> download on shared build-cache partition; coordinate with other agents (AGENTS.md shared checkout) before committing it. Effort S. Acceptance: `cargo build` of desktop+core under buildlock with 1.99.0; fix any new lints (glob imports, mangling) ; `cargo test -p` targeted crates.

## 11. libaom 3.14.1 -> 3.15.2
1. Changes (https://aomedia.googlesource.com/aom/+/refs/heads/main/CHANGELOG): 3.15.0 (2026-08-25) "includes the fixes for all the security issues found by AI tools recently": bounds-check bypass in AV1E_SET_SVC_LAYER_ID (CVE-2026-56209, bug 538651949), heap OOB read in av1_int_pro_motion_estimation (CVE-2026-13906), heap double-free in multithreaded encode (bug 513342555), heap overflow in av1_update_and_record_txb_context with SVC, SIGFPE in SVC encoder, SEGV in encoder_destroy after failed init. 3.15.1 (2026-09-21) Tune-VMAF regression fix. 3.15.2 (2026-10-06) key-frame force, oss-fuzz fixes (bad-cast in compute_gradient_info_sb, integer overflow in linsolve_wiener), IntraBC motion search stale data. All ABI compatible.
2. Advisories on 3.14.1: Debian/SUSE trackers list CVE-2026-56208 (heap overflow in LAP/lag_in_frames >=1 first-pass ring buffer), 56209, 56210 (OOB heap read), 56211 (OOB layer selection, "RCE") — https://security-tracker.debian.org/tracker/CVE-2026-56209 etc. Which tags fix which CVE is not fully clear from sources (3.15.0 notes say all fixed; Debian shows 3.12.1/3.13.1 vulnerable, 3.14.1 status not shown). Treat 3.14.1 as likely affected.
3. Our use: scripts/build_libaom.py:25 `VERSION = "3.14.1"`. We use libaom as encoder (rav1d decodes, from memory notes). I grepped crates/video/src and found no references to SVC layer id / lag_in_frames, so SVC-specific CVEs (56209/56210/56211) are probably not reachable from our config, but lag_in_frames (56208) depends on encoder config in crates/video — check the `g_lag_in_frames` setting (RT mode normally 0). The encoder processes only our own captured frames, not remote input, which lowers exposure; still, double-free in multithreaded encode and general fuzz fixes are worth taking.
4. Risk: video encoding output/rate-control behaviour may shift slightly between 3.14 and 3.15 (RT speed presets); timing-sensitive video tests need release profile (AGENTS.md).
5. Effort S (bump VERSION + any sha/hash/tarball pin in the script + cached build artifact invalidation). Recommend upgrade now to 3.15.2. Acceptance: rebuild libaom via script under buildlock, run video crate tests in release profile + the encoder/decoder simulation round trip, check encode speed/bitrate benchmarks not regressed vs 3.14.1, Android build of libaom for all ABIs.

## Suggested order
1. libaom 3.15.2; `cargo update -p tokio -p serde_json -p notify-rust -p cc`.
2. png 0.18 (one-line fix tray.rs:75) + rtrb 0.4 after reading its changelog.
3. Toolchain pin (1.99.0).
4. Prototype ksni tray (drops gtk + unmaintained advisories); then tray-icon 0.26.
5. resvg 0.48 PR with baseline review.
6. keyring 4 only after cross-version round-trip test on all OSes.
7. jni 0.22 with Android device test, when netdev also moves.
