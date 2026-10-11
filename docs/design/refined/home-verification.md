# Home / opening / tactile feedback verification

Executed in headless Chromium via Playwright using `node qa/home-test.js`: **123/123 checks passed**. Results are in `qa/home-results.json`. Screenshots were captured and visually reviewed, not just generated.

## Checked

- Root preview resolves to the actual home screen; design review is a separate page.
- 320×740, 390×844 and 1440×1000; light and dark. No horizontal overflow. Phone bottom navigation remains within the viewport. Sheets fit the screen.
- All four tabs, selected/focused navigation, search no-match/clear, unread filtering, contact details and Escape focus return.
- Availability pause/save/resume, offline explanation and simulated reconnect, expired-ticket error and confirmation before adding the sample contact.
- Loading skeletons with `aria-busy`, completion, failure/retry, and cancellation of stale load callbacks when switching tabs.
- Opening appears when explicitly replayed, can be skipped, releases inert controls, and does not repeat on reload in the same session.
- Both system and in-preview reduced motion skip the opening. Reduced-motion controls remain still when pressed.
- Normal primary controls have a visible pressed transform and a layered shadow. Settings-sheet elevation, brand opening, skeletons and phone/desktop layouts were screenshot-reviewed.
- No page runtime errors during the viewport/tab checks. Local destination links checked separately by `qa/links.js`.

## Feedback fixes

The first browser run caught a duplicate SVG-symbol/input ID that interfered with search, and checks were rerun after correcting it. Trigger focus is restored after re-rendered rows and nested ticket steps. Loading clears stale toasts. Pagehide releases the opening overlay; browser-history return resolves any interrupted simulated list load.

## Boundaries

This is a design prototype, not a native splash or haptic implementation. Calls and messaging are separate studies with their own sample data. Animation character is reviewed with captures and timing/DOM checks, not on a physical device. Actual finger feel, OS animations, real assistive technology, virtual keyboard resize and native startup latency still need device review. No vibration or tap sound is produced.

The shared surface stylesheet is also applied to the existing calls/chat/groups/privacy/devices studies. Their earlier verification remains separate; see `verification.md` for the latest cross-study regression run.
