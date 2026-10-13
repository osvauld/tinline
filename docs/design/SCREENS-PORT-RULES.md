# Screens port: rules for area agents

Started 2026-10-13. Port the playground screens into the Android app (Compose) and the desktop app
(iced 0.14) **exactly**: same geometry, spacing, type, colour, motion, copy and behaviour. Integration
branch: `design/screens` (from dev 1fa2d7a). Each area agent works on `ds/<area>` in
`.claude/worktree/ds-<area>`. The lead reviews and merges.

## Read first
1. `docs/design/SCREENS-HANDOFF.md`: what the spec is, rules that must survive the port, where each
   foundation lives. Search core is now on dev (`docs/search.md`, `Node::search`, `search_in`).
2. `docs/design/PHILOSOPHY.md` (12 rules) and `docs/design/playground/PLAN.md` §7 (decisions).
3. The component files for your area under `docs/design/playground/src/` (header comments state the
   intent), its drawer notes (Decided / Proposed / Core facts) and its `tests/*.flow.js`.

The look is a soft, gentle, feminine aesthetic: curves and softness. It is already in the playground;
port it, don't reinvent it. If you see a change that would increase that softness tremendously, don't
build it: write it under "Ideas for the user" in your report.

## Seeing the playground
A playground dev server usually runs at http://127.0.0.1:5178/ (the design session's). Use it read-only:
`?form=android|desktop&theme=light|dark#/<route>`. Never edit the playground. If it is down, run your
own copy from your worktree on port 5190+<n> (`npm install && npx vite --port <p>`).
Measure the playground's computed values (Chromium devtools / Playwright `getComputedStyle`,
bounding boxes) instead of eyeballing them. Durations and easing: `src/lib/fx/motion.ts`. Tokens:
`src/tokens.css`. Geometry: `src/lib/geom/GEOMETRY.md`.

## Use the foundations
Android: `Theme.kt` (tokens, `Depth`, `Modifier.depth`), `Geometry.kt` (`SmoothCornerShape`,
`bubbleShape`), `Mark.kt` (`TinMark`, `statusLook`, `TinFx`). Desktop: `ui.rs` (`Tok`, `Depth`,
`raised`, `pop_surface`, `sheet_surface`, `ui::logo*`), `geom.rs`, `shaped.rs` (tiny-skia curved
surfaces), `mark.rs`. Don't hard-code a value a token names. Missing token → add it next to the others
in Theme.kt / ui.rs (additive, small) and say so in your report.

## Ownership
Edit only your area's files (listed in your brief). Shared files (`Theme.kt`, `Components.kt`,
`P2pApp.kt`/`MainActivity.kt`, `ui.rs`, `app.rs`, `view.rs`, `main.rs`) get **minimal, additive**
edits only: new helpers in your own new files where possible, new enum variants/match arms appended,
no reformatting, no renames. The lead resolves overlaps at merge.

## Core
Don't change `crates/core`, `proto` or the FFI shape unless your brief allows it. Where a screen needs
core support that isn't there, build the UI behind today's behaviour and list the gap in your report.

## Building and testing (light: full testing happens once, before merge)
- Heavy commands go through `/home/abe/p2p_chat/scripts/buildlock.py` (absolute path; `--status`
  shows the queue). `scripts/build_android.py` takes the lock itself: never wrap it.
- Desktop: `CARGO_TARGET_DIR=/home/abe/build-cache/p2p_chat/design-apps/target` (shared; already warm).
  Your worktree's `target` symlink points there.
- Run only: `cargo check -p desktop`, `cargo test -p desktop <your filter>`, the Android build, and
  your own new unit tests. No full-workspace or full-crate suites, no core tests.
- Android device: **emulator-5554 only**, install/screenshot through
  `buildlock.py --device emulator-5554 -- ...` (other agents share it). Never the USB phone ZD22257P6G.
  Never start a second emulator. Never `pm clear` without need.
- Do not install over `~/.local/bin/tinline-desktop` or the user's phone.

## Checking your work (this is the job)
For each screen: app and playground side by side, same state, light and dark, true size and zoomed.
Look at spacing, alignment, optical balance, corner curves, text weight, colour, easing, cue timing.
Change every piece of text that can change (status, timer, count, error) and confirm nothing else moves.
Fix the small things before committing. Keep the screenshots you compared in
`/home/abe/build-cache/p2p_chat/screens-shots/<area>/` (not in git) and name the key ones in the report.

## Commits and report
Commit each verified piece on your branch with pathspecs (never `git add .` / `commit -a`), no push.
Message trailer: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
Finish with a report: what's ported per screen per platform, exact commands run and results,
remaining differences from the playground (with reason), core gaps, ideas for the user.

## Questions
Design questions: ask the lead in your final report, or, if blocked, stop and ask. The design session
(`p2p-chat-a0`) knows the playground's intent; the lead relays.
