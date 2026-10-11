# Setup & devices — browser verification

Run: `node qa/devices-test.js` (server `http://127.0.0.1:8768/refined/devices.html`, installed Playwright 1.41.2, `/usr/sbin/chromium`). Results: `qa/devices-results.json`. Screenshots: `qa/screenshots/devices-*.png` (179 files). Last run: **292/292 checks pass**.

## Coverage
- **Viewports:** 390x844 and 320x740 (light + dark), 1440x1000 (light + dark), plus 195x422 and 720x500 as a 200 % zoom reflow stand-in (CSS px halved). 21 states per viewport (every tab, key error/expired/transfer/dialog states): no horizontal overflow, no console/page errors, no browser dialogs, no network requests beyond the page.
- **Not covered:** real text-only 200 % font scaling (the page uses px sizes; reflow at half width was tested instead), real screen readers, real camera/clipboard (none used by the page), touch gestures beyond clicks.
- **Flows:** create (name validation, hidden/reveal/hide, auto-hide, hide on visibility change / tab switch / back, check words, protect, done), restore (empty, count, characters, checksum, example fill), unlock (toggle, wrong, 3-miss wait, device-lock sheet, forgot), link (validation, QR show, natural expiry, regenerate, cancel keep/stop/Escape, verify, mismatch, wrong passphrase, transfer, cancel after send, connection drop, success, camera ask/deny/allow, manual code errors, expired code, waiting, declined), devices (rename empty/duplicate/Escape/save, unlink Cancel/Escape/backdrop/wrong passphrase/success, online vs offline wording, remove this device, only-device acknowledgement, restore demo), accounts (switch, wrong passphrase, per-account drafts, call variant, add), theme toggle, tab keyboard use, focus restoration. QR/phrase timers are sped up 60x in the test only.

## Defects found and fixed (in `devices.html`)
1. Recovery words stayed revealed after leaving First setup for another tab and returning (violated P1). `go()` now hides them.
2. Dark theme via system preference: destructive buttons (Unlink / Remove account) rendered white text on pale pink (low contrast); the rule only applied with the manual toggle. Now variable-driven for both.
3. Overflow at ~195 px width (header title, device rows, link diagram) fixed; narrow-width padding reduced so radio cards stay readable.
4. Active tab could be scrolled out of view in the mobile tab strip after switching; it is now scrolled into view.

## Secrets / guarantees (all passed)
- Phrase words are absent from the DOM while hidden; no copy/share/export control exists; clipboard APIs, copy/cut events and `execCommand` were never called in a full session; nothing is written to localStorage, sessionStorage or cookies; URL carries only the tab hash; passphrase fields are `type=password` unless explicitly toggled.
- No copy says unlink is secure, revoked, permanent or guaranteed. Every unlink dialog and the My devices page say it is **not a lock**, that a phrase holder or the device's unlock can still act as the user, and that only a new identity fully cuts a device off.
- Link failure copy never claims "nothing copied" after the account was sent; it says it may already be on the new device.

## Residual notes (not changed)
- The status toast is fixed-position and can briefly cover the bottom of a screen (9 s); it is `role=status`.
- Post-run copy-only refinement: the online-device unlink dialog now explicitly says Tinline will **try** to ask it to lock/delete; delivery is not guaranteed and modified apps can ignore the request. The 292-check run above preceded this wording-only edit.
