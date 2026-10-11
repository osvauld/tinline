# Mobile chat verification

Run: `node qa/mobile-chat-test.js` (Playwright Chromium, server `localhost:8768/refined/`). Result: **124/124 checks passed** (`qa/mobile-chat-results.json`). Screenshots: `qa/screenshots/mobile-chat-*.png`.

Covered:
- 1440px `mobile.html?chat=arjun`: 390px iframe shows the mobile conversation, composer and Back for Arjun. The 320px toggle works, and the Desktop layout link opens the split view. Invalid `chat` values fall back to Arjun on `mobile.html`, and to the list on `chat.html`, with no errors.
- Native 390px and 320px chat: no horizontal overflow, selected person correct, 44px targets. Draft preview and restore, send, empty send, long draft, attach sheet and attachment, message menu, details sheet, and a keyboard-height viewport all behave.
- Home to Arjun at 390px and 320px: goes straight to `chat.html?chat=arjun&from=home` with no modal. Back, Escape and browser Back return to Home Chats. Group rows stay on Home.
- No runtime errors anywhere.

Screenshot feedback fixed and the 124-check suite rerun successfully:
- Informational toasts now appear below the header in a mobile conversation, not over the composer. They do not intercept touches, and only the latest is shown.
- Initial deep-link focus still announces the conversation title, but does not paint an oversized selection outline on first load. Normal focus styling returns on keyboard/pointer interaction.
