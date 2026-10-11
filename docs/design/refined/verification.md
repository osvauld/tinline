# Browser QA: refined design pages

Run: 2026-10-10, Chromium (headless, `/usr/sbin/chromium`) via Playwright. Pages: `index`, `calls`, `chat`, `groups`, `privacy`.
Re-run with `qa/run-all.sh` (needs the static server on `127.0.0.1:8768` serving `docs/design`).
The server on 8768 had died mid-run; QA restarted `python3 -m http.server 8768 --bind 127.0.0.1` from `docs/design`.

## Home and motion follow-up

The main entry now opens [Home](home.html); the previous landing page is [Design studies](review.html). Opening motion, explicit slow/error loading previews, layered surface shadows and touch feedback are covered by **123/123 home checks** in [home-verification.md](home-verification.md). Shared tactile styles are in `interactions.css`. The calls/chat/groups/privacy regression suite was rerun after that stylesheet and the home navigation links were added; the devices suite's earlier results predate this shared-CSS-only addition.

## Result of the final run
**146/146 checks passed** in the final primary rerun: calls 48, chat 33, groups 30, privacy 35. This rerun used Chromium's normal autoplay policy (the earlier QA-only bypass flag was removed). Setup/devices separately passed **292/292**: see [its verification](devices-verification.md).

The mobile scenario-start defect found in the initial run is fixed: starting a scenario now focuses the call heading and scrolls the call panel into view, bringing Answer/Decline within reach. The relevant regression check passes. Group flow checks were rerun once more after preventing the offline banner and message bubbles from shrinking over one another: **30/30 passed**.

## Additional targeted repair checks
The calls repair agent also exercised outgoing and incoming active-state timers, group host loss (ends the call without auto-redial), explicit redial through another eligible host, blocked caller refusal without retry, and pagehide closing the AudioContext. These checks are supplementary to the repeatable 146-check suite, not included in that count.

## Found and fixed during this QA (rechecked in the final run)
- calls: after an outgoing call was answered, the log said "Connected" but the stage stayed on "Answered · connecting audio…" (`goActive()` was called without `UI.all()`), so mute/route/end-call never appeared. The retry scenario had the same symptom.
- chat (mobile): the conversation composer sat below the first viewport (bottom 919 of 844).
- groups (mobile): composer clipped at the bottom (858 of 844).
- privacy: focus fell to `BODY` after "Simulate waking the phone" and after Retry; mobile tab strip clipped the first tab label.

## What was checked
Screenshots: `qa/screenshots/` (about 250 PNGs). Baselines: `baseline-AndroidCalls.png`, `baseline-AndroidPrivacy.png`
(source boards render; only `support.js` and Google Fonts are missing, both ignored).
Every page at 1440x1000 and 390x844, light and dark (`shots.js`).
Flow screenshots were read and inspected, not only captured.

### Cross-page
- No horizontal overflow (`scrollWidth <= clientWidth`) on any page at either size, light or dark, and after the flow states.
- No console errors, page errors or failed/4xx requests (excluding `support.js` and external fonts, which only the source boards use).
- 13 in-page links resolved (`qa/links.js`).

### Calls (audio graph, not human listening)
`AudioContext.createOscillator` is instrumented (`lib.js`), counting start, stop and ended.
- Nothing is created on page load; sound switch defaults off; with sound off, scenarios create no oscillators and the caption says what would play.
- Sound on: connecting is silent, ringback starts only once the remote confirms ringing, one `AudioContext` is reused.
- Cancel, Decline, Answer, scenario switch, Stop sound and replay all leave zero un-ended oscillators (ended within about 1.5s; the delay is notes scheduled up to 0.7s ahead; the gain ramps to 0 at once).
- Incoming ringtone repeats cycles until acted on. Previews of Ringtone and Ringback play, then Stop sound silences them. Replay does not stack.
- Flows: answered, declined, no answer, offline-then-retry (reaches connected), mic denied, contact has calls off, second call during a call, incoming timeout (missed record), hangup (history record).
- Route dialog: focus moves in, Escape and Done restore focus to the route button. Mute is `aria-pressed`. Keyboard Enter/Space on scenarios and the sound switch. Theme toggle.
- Not verified: audibility, loudness, perceived character. Real device audio focus/DND is out of scope for the page.

### Chat
Draft kept per chat and shown in the list; Shift+Enter newline vs Enter send; Sending to Delivered; one-shot "Next send fails" gives Not sent with Retry, retry delivers; contact offline queues and delivers on return; simulated incoming; search no-match empty state; settings/new chat/attach/info dialogs (focus in, Escape closes, focus back on trigger); message actions dialog; Reset and empty-list states; mobile list/conversation/Back with focus on the opened row.

### Groups
New group (empty name blocked with a message, selected count, create, new group appears with honest host state); invitation review discloses host visibility, Escape restores focus, Join moves it to Groups and decrements the count; chat request Accept moves to Private chats; send in group; hosts-offline simulation shows "Waiting for a host" and a saved-on-phone message; Members drawer, Escape focus; Leave confirm (Escape does not leave; confirm leaves, composer disabled); empty account state; mobile list, conversation, Back, new-group sheet.

### Privacy
Tabs (roles, one selected, ArrowRight); who-has-it dialog; interval change updates next change; jump to next change; asleep/overdue then wake; change failed then Retry; some contacts offline; block dialog (consequences, honest limits, Escape cancels and restores focus); confirm block lists blocked person with date and removes call; unblock states QR reconnect is required; recovery QR dialog and focus return; laptop/shared toggles have accessible names; mobile overflow on all tabs and block dialog size and 44px targets.

## Noted, not failing
- The hosts-offline banner previously overlapped the transcript. It now has non-shrinking layout with bounded scrolling; message bubbles no longer shrink below their content height.
- Calls theme button label stays "Dark mode" while `aria-pressed=true`; acceptable toggle pattern.
- After sending in chat, the Tab order from Send leaves the document (focus goes to BODY then the first link); no trap.
