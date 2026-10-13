# Browser QA: refined design pages

Pass 2 (2026-10-11). Chromium (headless, `/usr/sbin/chromium`) via Playwright, using `qa/lib.js`. Pages: `home`, `calls`, `chat`, `groups`, `privacy`, `devices`, `mobile`.
Re-run with `qa/run-all.sh` (needs the static server on `127.0.0.1:8768` serving `docs/design`). Each suite writes `qa/<name>-results.json`; those files are the record of what ran and passed.

## Latest recorded results (pass 2)

From the `qa/*-results.json` files written on 2026-10-11 after the pass-2 page changes. Every recorded check passed.

| Suite | Script | Checks | Notes |
|---|---|---|---|
| Calls | `flow-calls.js` | 68/68 | Includes harness, call-screen layout and the `?call=` deep links |
| Chat | `flow-chat.js` | 58/58 | Includes harness, bubbles, menus and the in-conversation call |
| Groups | `flow-groups.js` | 48/48 | Includes the in-page group call and `?embed=1` |
| Privacy | `flow-privacy.js` | 39/39 | Includes harness and the singular/plural banner grammar |
| Home | `home-test.js` | 186/186 | See [home-verification.md](home-verification.md) |
| Group conversation polish | `group-polish-test.js` | 141/141 | See [group-polish-verification.md](group-polish-verification.md) |
| Mobile chat and embeds | `mobile-chat-test.js` | 148/148 | See [mobile-chat-verification.md](mobile-chat-verification.md) |
| Setup and devices | `devices-test.js` | 296/296 | See [devices-verification.md](devices-verification.md) |

`run-all.sh` runs the calls, chat, groups, privacy and home suites (399 checks) plus `baseline.js`, `shots.js` and `links.js`. The other three suites are run on their own. A link check (`node qa/links.js`) run on 2026-10-11 against the pass-2 pages found 19 local links across `index`, `home`, `review`, `calls`, `chat`, `groups`, `privacy` and `devices`, none broken.

## What the pass-2 suites check

### Cross-page
- Harness: one slim bar, Simulation drawer closed by default, no disclaimer text visible, theme toggle icon-only with an accessible name, no default-blue links (chat, calls, privacy, devices, groups polish).
- No horizontal overflow at the sizes each suite uses (1440x1000, 390x844, 320x740; light and dark where the suite loops themes).
- No console errors, page errors or failed/4xx requests (excluding `support.js` and external fonts, which only the source boards use).

### Calls (audio graph, not human listening)
`AudioContext.createOscillator` is instrumented (`lib.js`), counting start, stop and ended.
- Nothing is created on page load; sound switch defaults off; with sound off, scenarios create no oscillators and the caption says what would play.
- Sound on: connecting is silent, ringback starts only once the remote confirms ringing, one `AudioContext` is reused.
- Cancel, Decline, Answer, scenario switch, Stop sound and replay leave zero live oscillators (cancel: every oscillator stopped and ended within about 850 ms; none live after a further second).
- Incoming ringtone repeats cycles until acted on. Previews of Ringtone and Ringback play, then Stop sound silences them. Replay does not stack.
- Call screen: timer is tabular and does not shift layout; Mute | End | Output cluster is centred.
- Flows: answered, declined, no answer, offline-then-retry (reaches connected), mic denied, incoming timeout (missed record), hangup (history record), platform switch mid-ring.
- Deep links: `?call=arjun|meera|dev|sana` at 390 and 1440 land on outgoing ringing with the right name and avatar hue, Android at 390 and Desktop at 1440, with no `AudioContext`; sound starts only after a gesture and Cancel stops every oscillator; `?call=group:hike` shows the group screen silently and reaches the active group call via the host; an unknown id stays idle with no audio.
- Route dialog: focus moves in, Escape and Done restore focus to the route button. Mute is `aria-pressed`. Keyboard Enter on scenarios and Space on the sound switch. Theme toggle.
- Not verified: audibility, loudness, perceived character. Real device audio focus/DND is out of scope for the page.

### Chat
Harness and no hint row; bubbles (meta inline in the last line, sent uses `--bub-out`, no delivery prose in bubbles); grouping (2 px inside a group, tight stack-facing corner); no persistent menu buttons; desktop hover chevron and anchored popover with Reply/Copy/Edit/Delete/Message info; right-click opens it; Escape returns focus; Message info shows sent time and per-device delivery. In-conversation call: in-page overlay with no navigation and no audio, connects after about 2.5 s with an MM:SS timer, Mute, Escape minimises to the strip, return to call from the header and from the strip, chatting and switching chats while minimised, contact-sheet Voice call row, End with "Call ended" and focus on the phone button. Dev (message-only): call button disabled but focusable, no banner, contact sheet line plus Request calling that flips to Requested. Draft kept per chat and shown in the list; Shift+Enter newline vs Enter send; sending to delivered; one-shot "Next send fails" with Retry; contact offline queues and delivers on return; simulated incoming; search no-match; settings/new chat/attach/info dialogs (focus in, Escape closes, focus back on trigger); Reset and empty-list states; mobile list/conversation/Back with focus on the opened row; 44 px targets.

### Groups
In-page group call (44 px phone button left of Members, overlay, connects without ringing, timer, Mute, Escape minimises to a 40 px strip, strip persists across chats, other group's button returns to the same call, End with toast and focus return; no call button in a removed group; disabled while hosts are degraded; overlay and strip inside an `?embed=1` frame). Phone preview link shown at 1440, hidden under 860 px and in embed mode. New group (empty name blocked, selected count, create, honest host state); invitation review discloses that hosts can read, Escape restores focus, Join moves it to Groups; chat request Accept; send in group reaches delivered; hosts offline shows the thin strip with the title unchanged and the waiting state; Members dialog focus; Leave confirm; empty account; mobile list, conversation, Back, new-group sheet, 44 px targets.

### Privacy
Harness; tabs (roles, one selected, ArrowRight); who-has-it dialog; interval radio group updates the next change; jump to next change; asleep/overdue then wake; change failed then Try again now; some contacts offline; singular and plural banner grammar ("1 contact hasn’t got your address yet"); block dialog (consequences, honest limits, Escape cancels and restores focus); confirm block lists the person with date and removes call; unblock states QR reconnect is required; recovery QR dialog and focus return; recovery toggles have accessible names; mobile overflow on all tabs; block dialog size and 44 px targets; chip row at 390 and 320 scrolls with every chip reachable and unclipped.

## Pass 1 record (2026-10-10, superseded)

Kept for history; these numbers describe the pass-1 pages, not the current ones.

- Final pass-1 rerun: 146/146 (calls 48, chat 33, groups 30, privacy 35), with Chromium's normal autoplay policy. Setup/devices separately passed 292/292. Home passed 123/123. 13 in-page links resolved.
- Found and fixed in pass 1: calls `goActive()` without `UI.all()` left the stage on "Answered · connecting audio…" (also the retry scenario); chat and groups composers sat below the first mobile viewport; privacy focus fell to `BODY` after waking the phone and after Retry, and the mobile tab strip clipped the first tab label; mobile scenario start did not bring Answer/Decline into view.
- The calls repair agent also exercised active-state timers, group host loss without auto-redial, explicit redial through another eligible host, blocked caller refusal without retry, and pagehide closing the AudioContext. These were supplementary checks, not part of a repeatable suite.
- Pass-1 observations that no longer apply: the calls theme button labelled "Dark mode" (the toggle is now icon-only with an accessible name); the hosts-offline banner overlapping the transcript (now a thin strip under the header). The pass-1 note that Tab from chat's Send left the document was not rechecked in pass 2; the pass-2 suite checks only that Tab from a filled composer reaches Send.

## Screenshots

`qa/screenshots/` (about 490 PNGs). Baselines: `baseline-AndroidCalls.png`, `baseline-AndroidPrivacy.png` (source boards render; only `support.js` and Google Fonts are missing, both ignored). `shots.js` captures `index`, `calls`, `chat`, `groups` and `privacy` at 1440x1000 and 390x844, light and dark; the suites capture their own flow states.
