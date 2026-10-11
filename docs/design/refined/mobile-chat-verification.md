# Mobile chat verification

Run: `node qa/mobile-chat-test.js` (Playwright Chromium, server `localhost:8768/refined/`). Latest recorded result (pass 2): **148/148 checks passed** (`qa/mobile-chat-results.json`). Screenshots: `qa/screenshots/mobile-chat-*.png`. The pass-1 run passed 124/124.

Covered:
- 1440px `mobile.html?chat=arjun`: the 390px iframe loads `chat.html?chat=arjun&embed=1` and shows the mobile conversation and composer for Arjun, with no in-frame Back and no harness. The 320px width switch works. The "Desktop layout ↗" link opens the split view. Invalid `chat` values fall back to Arjun on `mobile.html`, and to the list on `chat.html`, with no errors.
- Embed contract: the app fills the frame with no border or radius; the 44px call button is present; the call overlay fills the iframe and leaves the outer page unchanged; the minimised strip sits above the header inside the frame; ending the call works.
- Native 390px and 320px chat: no horizontal overflow, selected person correct, 44px targets. Draft preview and restore, send, empty send, long draft, attach sheet and attachment, a keyboard-height viewport, and the details sheet all behave.
- Message menu on touch: no visible per-message buttons; a 500ms long-press opens the bottom sheet docked to the bottom with Reply, Copy, Edit, Delete and Message info; a move over 10px or a short press does not open it; the context-menu event and the hidden keyboard menu button open the same sheet; Escape and scrim tap close it with focus back on the message; Message info opens.
- Home to Arjun at 390px and 320px: goes straight to `chat.html?chat=arjun&from=home` with no modal. Back, Escape and browser Back return to Home Chats. Group rows open `groups.html`.
- 1440px Home: clicking a chat stays on Home and embeds `chat.html` with `embed=1`, with no back button or harness in the pane.
- `chat.html`'s "Phone preview" link resolves to `mobile.html?screen=chat&chat=arjun` and loads Arjun; `mobile.html` at 390px and 320px fills the viewport with no overflow.
- No runtime errors anywhere.

Pass-1 screenshot feedback, fixed before the pass-1 rerun:
- Informational toasts no longer cover the composer in a mobile conversation. In pass 1 they moved below the header; in the pass-2 page they sit just above the composer (84px from the bottom). They do not intercept touches, and only the latest is shown. No pass-2 check covers toast position.
- Initial deep-link focus still announces the conversation title, but does not paint an oversized selection outline on first load. Normal focus styling returns on keyboard/pointer interaction.
