# Playground screen port — integrated partial milestone

Integration branch: `design/screens`; destination: `dev`.
Recovery evidence: `/home/abe/build-cache/p2p_chat/screens-resume-sol61-YwvBIJ/`.

## Scope

This is **not the complete playground UI on either platform**. It recovers interrupted work and combines the useful Android and Iced milestones. The playground remains the specification; no core, protocol, FFI or security-policy changes are included.

- Android: shell/home/lists/search, direct conversation/composer/find, onboarding/unlock/link, group list/info/member surfaces and a 1:1 call-layout milestone.
- Iced: shell/search, voice calls and development-only Fog/Clouds previews, in-session conversation draft/reply/edit handling and a centered gate frame. Desktop groups are unchanged.
- Unfinished: exact visual/motion/accessibility parity; broad profile/settings/privacy/device screens; desktop conversation find and exact search-hit jumps; complete group calls, video, waiting and OS surfaces. Android group-message search jumps are unfinished. Fog versus Clouds remains undecided; no production texture default is selected.

## Recovery evidence (before combined integration)

Android area snapshots: 17 targeted tests passed; 28 debug emulator previews opened without crashes, plus two Find captures; 30 offline equal-bound checks passed. Latest call glyph and unlock-IME corrections were unverified at handoff. These are debug fixtures, not real-peer end-to-end validation or exact playground parity.

Iced area snapshots: 8 field tests, 3 call-UI tests and 4 search tests passed. Call screenshots: 20 native states; initial shell screenshots: 6. Golden field model covers 728 frames. Continuous field animation, GPU performance and final shell/chat/gate changes were not verified at handoff.

## Integration repairs

- Deduplicated Android colour/motion tokens; disambiguated groups buttons/press and people sheets.
- Reconciled Android Home, routes and gallery additions; connected direct search hits to conversation routes and draft previews to the single draft store.
- Prevented disposed Android screens from republishing drafts after account/gate clearing.
- Prevented indexed Find from resetting its cursor when loading a hit's day; scrolling reads recomposed rows.
- Reconciled Iced shell/call/gate modules; retained minimized-call controls in empty detail and visible-chat behavior during minimized calls.
- Added account-epoch rejection of late desktop text submissions before draft/message/notice mutation.
- Corrected the desktop gate's `Length::Shrink` reference.

## Combined verification

PASS for the integrated source: normal and test-hooks `cargo check -p desktop`; 23 targeted desktop tests; Android x86_64 debug assembly and 18 targeted JVM tests; whitespace checks. Read-only sol6.1 platform reviews and Rust 2024 syntax parsing also completed. Exact commands/results: `/home/abe/build-cache/p2p_chat/screens-resume-sol61-YwvBIJ/integration/merge-state.json` and adjacent logs. Android used the unchanged warm debug core/FFI; this is not a fresh Rust cross-build, combined screenshot comparison or real-peer E2E validation.

Heavy checks use `/home/abe/p2p_chat/scripts/buildlock.py`. CPU admission has been blocked above its 40% threshold; no bypass or unrelated-process termination is authorized.

Original `ds-*` working trees remain intact and uncommitted; integration copies and backups are external. No push or installation to the user's phone is authorized.


## Uncommitted recovery follow-up — 2026-10-13

Coordinator: goose UI recovery. Original parity-android-aauim450 and
parity-desktop-aauim450 worktrees remain read-only, both based on 15ad3cd.
Recovered 37 platform files into dev after checking every destination against the
base; preserved external source snapshots and hashes. No commits, staging, phone
installation, account changes or data clearing performed. Unrelated checkout work
is excluded. External evidence:
/home/abe/build-cache/p2p_chat/goose-ui-recovery-20261013-160014/.

Pre-fix baseline from recovered current source: normal/test-hooks desktop checks
passed; 6 profile helper tests passed (0 ignored, 129 filtered); Android debug and
release Kotlin compilation plus 14 gate/geometry JVM tests passed (0 skipped).
Android used existing warm JNI/bindings with cargoNdkBuild/uniffiBindgen excluded,
not a fresh Rust Android build. Compiler warnings remain.

Follow-up safeguards added (pending post-fix validation):
- Production Busy cannot call core unavailable: explicit unsupported feedback,
  no call-admission policy change; debug fixtures remain simulation-only.
- Profile writes and account transitions have reciprocal desktop UI guards;
  legacy name/availability writes share the profile worker barrier.
- Desktop invalidation clears decoded pictures; reads recheck selected hash and
  readiness, and owner changes invalidate profile presentation/generations.
- Android failed metadata reads/parses retain accepted data; matching request
  slots are released; locked identity and debug-fixture disposal are corrected.
- Android settings reads and Allow calls writes are session-bound worker tasks.
- Groups notifications are recorded as unsupported instead of an ineffective
  switch; Quiet suppresses message/missed notification sound without changing
  OS channel preferences. Availability feedback uses reserved single-line space.
- Added targeted Busy/session/transition regression tests. The attempted red run
  was delayed by memory admission until after safeguards existed; no genuine
  test-first red result is claimed.

Post-fix validation is serialized in external followup.sh after the existing
queued test job. Memory admission is currently below the 4 GiB threshold; do not
bypass it or duplicate jobs. results.txt, command logs and followup.done contain
the eventual outcomes. New fixes are NOT accepted until checks complete.

Visual acceptance remains incomplete: existing native/Svelte images predate these
fixes; fresh paired measurements, native flow checks, account-switch races,
accessibility/motion/RTL, and the full screen/state matrix still need validation.
Desktop's original 335-state inventory is not proof of parity. No whole-app or
real-device integration claim. See external acceptance.md for blockers.
