# Group conversation polish: verification

Run: `cd docs/design/refined/qa && node group-polish-test.js` (server on :8768, Playwright + /usr/sbin/chromium).
Result: **90 / 90 checks pass** (`group-polish-results.json`). Existing `flow-groups.js` still passes 30 / 30 (run from a scratch copy so its screenshots were not overwritten).

Viewports: 1440x1000, 390x844 (touch), 320x740 (touch), each light and dark. Screenshots: `screenshots/group-polish-*.png`.

Covered: Weekend hike selected by default on desktop; no Reply/Discard text and no reserved action row when idle (row height equals bubble height); hover reveals the control beside the bubble without resizing it; Tab reaches it, Enter and Shift+F10 open the dialog sheet, Escape and Cancel restore focus; Reply, composer quote, Escape cancel, send with in-bubble quote; queued message shows Discard waiting message, undo toast; phone: 44 px targets outside bubbles, long-press opens the sheet and it fits the viewport, long-press cancelled by move, short tap, scroll and pointercancel; textarea selection unaffected; no overflow or clipping; `?group=hike` opens the phone conversation, invalid values (`nope`, markup, `../x`) fall back to the list, a removed group opens read-only without actions; no console errors or failed requests.

Defects found and fixed during the run: class name clash with the members list (`.mrow`), release of a long-press closing the sheet via the backdrop click, 320 px header wrapping (Members is icon-only below 360 px, name kept for assistive tech).
