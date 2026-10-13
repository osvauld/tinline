# Home / opening / tactile feedback verification

Executed in headless Chromium via Playwright using `node qa/home-test.js`. Latest recorded run (pass 2): **186/186 checks passed**. Results are in `qa/home-results.json`; screenshots in `qa/screenshots/home-*`. The pass-1 run passed 123/123; its note that screenshots were visually reviewed applies to pass 1.

## Checked

- Root preview (`index.html`) resolves to the actual home screen; design review is a separate page.
- Five layouts: 320×740 light, 390×844 light and dark, 1440×1000 light and dark. No horizontal overflow. Opening releases controls; six conversation rows render.
- Pass-2 component checks at every layout: no designer notes in the app frame; filter chips are `.t-chip` with `aria-pressed`; unread badges use `.t-badge`; the muted chat shows the bell-slash glyph before its time; avatars use the shared hue hash; no default-blue links.
- Phones (320 and 390): no selected-row tint; "+" is a 56 px button at the bottom right, 16 px above the tab bar; bottom navigation visible; tapping a chat opens the conversation directly and its Back returns Home.
- Desktop (1440, light and dark): empty detail pane when nothing is selected; selecting a chat embeds `chat.html` with `embed=1` in a titled iframe and keeps you on Home; selecting a group embeds `groups.html` with `embed=1`; selected-row tint; iframe seam is a left border only, no radius; opening a chat marks it read.
- All four tabs with focus retained after switching, search no-match/clear with focus return, unread filtering, settings sheet within the viewport with Escape focus return, and the account sheet's Phone preview link (`mobile.html?screen=home`).
- Availability pause/resume banner, offline explanation and simulated reconnect, expired-ticket error and confirmation before adding the sample contact, empty account with an add path.
- Loading skeletons with `aria-busy` and preserved row geometry, completion, failure/retry, and cancellation of stale load callbacks when switching tabs.
- Opening appears when explicitly replayed, can be skipped, releases inert controls, and does not repeat in the same session.
- Both system and in-preview reduced motion skip the opening; system reduced motion removes entry animations; reduced-motion controls stay still when pressed.
- Primary controls have a visible pressed transform and a layered shadow.
- No page runtime errors during the layout and tab checks. Local destination links are checked separately by `qa/links.js`.

## Feedback fixes (pass 1)

The first browser run caught a duplicate SVG-symbol/input ID that interfered with search, and checks were rerun after correcting it. Trigger focus is restored after re-rendered rows and nested ticket steps. Loading clears stale toasts. Pagehide releases the opening overlay; browser-history return resolves any interrupted simulated list load.

## Boundaries

This is a design prototype, not a native splash or haptic implementation. Calls and messaging are separate studies with their own sample data; on desktop Home embeds the real chat and group pages. Animation character is reviewed with captures and timing/DOM checks, not on a physical device. Actual finger feel, OS animations, real assistive technology, virtual keyboard resize and native startup latency still need device review. No vibration or tap sound is produced.

The shared surface stylesheet is also applied to the calls/chat/groups/privacy/devices studies. Their verification is separate; see [verification.md](verification.md) for the latest cross-study results.
