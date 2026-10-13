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
