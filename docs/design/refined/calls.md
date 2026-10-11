# Calls: refined design spec

Prototype: [calls.html](calls.html) (self-contained, simulated data, light/dark, checked at 390, 320 and desktop widths).
Source boards: `../AndroidCalls.dc.html`, `../DesktopCalls.dc.html`. Multi-device call rules: `../multi-device.md`. Call permission: `../groups/AndroidPrivacy.dc.html`.

**Scope.** One-to-one voice calls, plus a small group-call and blocked-caller layer (sections below, decisions U19 to U21). Video is not drawn. Wherever this document or the prototype says "end-to-end encrypted" or "an encrypted relay can't hear you", it is true **only for one-to-one calls**. Group calls are hop-by-hop encrypted and the call host can hear them.

**The audio is a browser demo.** It exists to judge the character, level and timing of the sounds. It is not a native fix. Android ringing (ringtone/notification channel, audio focus, Do Not Disturb, silent/vibrate) and desktop ringing are separate engineering work, and nothing in the prototype claims them.

## Small-screen layout (390 x 844, 320 x 740)

- Compact chrome at 700 px and below: one-row header (back, theme), smaller title and note, the "Call screen" heading is visually hidden, the platform switch is full width, card padding is 12 px.
- The phone panel is content-sized with a viewport-relative minimum (`clamp(380px, 100svh - 250px, 600px)`), with a smaller avatar. At 390 x 844 and 320 x 740 the idle actions, incoming Answer/Decline and the active Mute / output / End call controls are all above the fold, with no page scroll needed to use them.
- Idle offers two direct actions in the panel: "Call Arjun (simulated)" and "Incoming call (simulated)", so an outgoing or incoming flow starts without opening the scenario list. Both begin a scripted scenario, so sound lifecycle is unchanged (cues follow the sound switch, stop on every phase exit).
- Desktop width keeps the two-column layout and the 390 px phone / 460 px window.

## Phases and what each one shows

| Phase | Status text | Sound | Actions |
|---|---|---|---|
| Connecting (outgoing) | "Connecting… Finding a path." | Silent. Remote is not ringing yet. | Cancel |
| Remote ringing | "Ringing on Arjun's device" | Ringback loop, starts only when the remote device confirms ringing | Cancel call |
| Incoming | "is calling · end-to-end encrypted" | Ringtone loop | Decline, Answer |
| Joining | "Answered · connecting audio…" | Silent | End call |
| Active | timer, Direct or Relayed, quality | Connected cue once | Mute, audio output, End call |
| Reconnecting | "Reconnecting…", "Gives up in N s" | One soft cue at entry, then silent | Mute, audio output, End call |
| Ended | reason as title | Ended cue (normal) or failed cue (failures) | Depends on reason, below |

Connecting and ringing are deliberately different: connecting is quiet with a dashed avatar ring; ringing has the gold pulse and the ringback. A caller who hears nothing for several seconds is told "Not ringing yet" in text.

## Ending reasons

| Reason | Title | Sound | Actions |
|---|---|---|---|
| Remote hung up | "Arjun hung up" | Ended | Call again, Close |
| You hung up | "You hung up" | Ended | Call again, Close |
| You cancelled while connecting or ringing | "You cancelled the call" | Ended | Call again, Close |
| Declined | "Arjun declined" | Failed | Try again, Close |
| No answer (timeout) | "No answer" | Failed | Try again, Close; text explains messages cannot be left |
| Could not reach | "Couldn't reach Lena" | Failed | Try again, Close |
| Connection lost (reconnect timed out) | "Connection lost" | Failed | Try again, Close |
| Missed: caller cancelled or ring timed out | "Missed call from Arjun Oommen" | None | Call back, Dismiss; history gets a Missed tag |
| Answered on another device | "Answered on another device" | None | Close; **no missed record**, this device stops ringing |
| You declined | "You declined" | None | Close; caller is told you are busy |

Failures never auto-close. The native normal-ended screen closes itself after 4 s (existing board); the prototype keeps it open so it can be inspected, and says so on screen.

Retry reruns the dial from the start. In the "Can't reach, then retry" scenario the second attempt gets through; that outcome is scripted, not a promise.

## Group call through one call host (U21)

Scenarios "Group call, then the host drops" and "Chosen host can't host calls". Intentionally small: the same call screen with contextual pills and one line of copy, no member grid or roster.

- **Shape.** Members connect only to the selected call host, never to each other (no member sees another's address, U7). There is no ringing: the screen goes Connecting ("Connecting to Kofi's desktop, the call host") to Joined to Active. Connecting is silent.
- **Honest claims.** Active shows "Group · via Kofi's desktop" and a "Host can hear" pill; the copy says "Encrypted hop by hop … not end-to-end. The call host can hear the call." Nothing says end-to-end, mesh, or live failover.
- **Separate permission.** Hosting a call needs `host_call`, which chat hosting does not imply. If the selected host lacks it ("Raj's laptop"), the call does not start: state "Can't start through Raj's laptop" with a "No call-hosting permission" pill and an explicit action "Start via Kofi's desktop" (an eligible host). Failed cue once.
- **Host lost ends the call.** "Call host drops" shows "The call host dropped" (failed cue once). No Reconnecting state, no automatic retry, no hand-off. The one primary action is "Redial via Mina's phone", the next host that has `host_call`; it is the user's explicit choice and the host is named. If no other eligible host remains, only Close is offered. "Couldn't reach the call host" (host unreachable while connecting) behaves the same way.
- A member's own network loss still uses the normal Reconnecting state, since the call host is still there.
- Not drawn: starting a group call from the group screen, host selection UI, member list and speaker limits (2 to 3 loudest forwarded), how members learn a call is running. Group-call spec: G6 (`group-call-spec.md`, not present in this worktree when this was written).

## Blocked caller (U19, U20)

Scenario "You were blocked". The other person blocked you; the signed notice made your shipped app disable calling.

- **Visible state.** Title "Leo has blocked you", status "Calls and messages are off", a red "Blocked by Leo" pill, and a disabled "Call unavailable" button (real `disabled`, so it is not a retry affordance). Nothing is dialled and Leo's phone does not ring. No sound: a call that cannot start makes no tone.
- **Modified or older client.** Remote control "Dial anyway" shows Connecting for about 1.4 s, then returns to the same screen with a banner "Refused: blocked … It will not be tried again". Failed cue once; the control is then removed so it cannot be repeated. Cancel while dialling returns to the blocked screen.
- **No retry until unblocked AND re-added.** "Leo unblocks you" moves to "Leo unblocked you · Calls still off · Add Leo again": block deletion removed your contact entry, so calling stays off. Only after "Add Leo again (scan QR, simulated)" does "Call Leo" appear and dial. Old chat history stays readable throughout (U24).
- Different from "Contact has calls off": that is a permission the contact can grant (offers "Ask Leo to allow calls"); blocking has no ask action.
- Not drawn: the blocked person's chat screen (see privacy design), the blocker's own view.

## Pre-call states

- **Microphone denied.** Title "Tinline can't use the microphone", copy from the existing board. Actions: Open settings, Not now. In the prototype, Open settings cannot leave the page, so it reveals a clearly labelled "Simulate: I allowed the microphone" button which continues to dialling. The real app only continues once the OS reports the permission.
- **Contact has calls off.** Tapping call never dials and never rings the contact's phone. Offers "Ask Leo to allow calls", which sends a normal message (per the privacy board). After asking, the button is replaced by a confirmation and the actions reduce to Close, so the request cannot be sent twice.
- Address change or lost contact map to Connecting then Couldn't reach; the explanation text is the same.

## Second call while in a call

Banner over the active call: Decline, End & answer. No hold, no call waiting (existing board). Decline tells the caller you are busy and the current call continues. If the second caller hangs up first, the banner goes away and a Missed record is added. Sound: one ringtone cycle only, never a loop over a live conversation (proposal).

## Multi-device (from multi-device.md)

- Incoming copy notes that linked devices ring too and the first answer wins.
- "Answered on another device" ends ringing here without a missed-call record. History keeps one entry per call.
- Not drawn: the caller-side view of several devices ringing. The source defines the coordinator rule but no caller UI for it.

## Audio output and mute

- Mute is a toggle button (`aria-pressed`), shown as the dark filled control plus a "Muted" pill so it is not colour alone. It stays available while reconnecting. In the prototype it only changes state; nothing is captured.
- Output opens a dialog with a radio group. Android: Phone earpiece, Speaker, Pixel Buds Pro. Desktop: Laptop speakers, Jabra Evolve2 65.
- Route change: if the selected external device disconnects, audio moves to the private default (earpiece / laptop speakers), never to the loud speaker, and the user is told in text with a reminder to check volume. When the device returns it becomes available again but audio is **not** switched back automatically. These two rules are proposals; confirm them before native work.
- Direct to Relayed switches update the pill and the explanation without interrupting the timer.

## Sounds

All pitches are sine with a quiet second harmonic, low-passed at 3.2 kHz; fast attack (20 ms) except ringback (180 ms) so it stays soft.

| Sound | Character | Timing |
|---|---|---|
| Ringtone | Rising four-note chime G5 B5 D6, then G6 | Cycle about 3.2 s, loops |
| Ringback | Calm two-tone hum G4 + B4, distinct from the ringtone | 1.2 s on, about 2.8 s off, loops |
| Connected | Two rising notes C5 G5 | Once, about 0.8 s |
| Ended | Two falling notes G5 C5 | Once |
| Failed | Two low falling notes B3 G3 | Once |
| Reconnecting | One low note | Once |

Level: master gain is volume × 0.25 and each note peaks at 0.3 to 0.5 of that, so even the slider at maximum stays well below full scale. Default slider is 50. The slider adjusts a playing sound immediately.

## Sound controls and rules

- **Nothing plays on load.** The AudioContext is created or resumed only inside a click handler: a preview button, Replay, or turning on "Play sounds during scenarios".
- **Scenario sounds switch** (`role=switch`, default off). Off means every cue is described in text instead: the sound panel says "Sound off · would play: Ringtone, soft rising four-note chime…" and the event log records the same. The pulsing halo, the status line and the live region carry the same information.
- **Preview buttons** (`aria-pressed`) toggle: press to play, press again to stop. Loops preview for two cycles, then stop themselves. Starting any sound stops the one before it, so sounds never layer.
- **Replay current sound** plays the cue for the current phase (ringtone while incoming, ringback while ringing, else the last cue). **Stop sound** is always present. **Replay scenario** restarts the whole scripted flow.
- **Stops**: answer, decline, cancel, end, timeout, remote cancel, answered elsewhere, new scenario, turning the switch off, Stop sound. Also on `visibilitychange` (tab hidden: sound and clock ticker stop, a text note says so and Replay is offered; ringing is **not** auto-resumed on return) and `pagehide` (all timers cleared, AudioContext closed).
- Each note's oscillators are scheduled with explicit stop times and the gain is ramped to zero in about 12 ms on stop, so there are no clicks and no leftover oscillators.
- Reduced motion removes the halo pulse; the text status and badge remain.
- Reconnecting and the ringing countdown are shortened for the demo (reconnect gives up in 12 s, incoming stops ringing in 20 s, no-answer in 9 s). Real timeouts are open product decisions.

## Accessibility

- One polite live region announces every phase change once ("Ringing on Arjun's device.", "Reconnecting. Your network changed."). The visible event log is not live, to avoid double speech. The countdown and call timer are plain text updated in place, not announced.
- All controls are real buttons at least 44 px high; mute and sound toggles use `aria-pressed` / `role=switch` with fixed labels.
- When a re-render removes the focused button, focus moves to the same action if it still exists, otherwise to the call title (`tabindex=-1`).
- The output dialog is a native modal `<dialog>`: Escape closes, Tab is trapped, focus returns to the Output button, and clicking the backdrop closes it. No `alert`/`confirm`.
- Desktop: with focus not on a control, Enter answers and Esc declines an incoming call (existing board hint).
- Dark mode follows `prefers-color-scheme` and the visible toggle overrides it.

## Sound cleanup on the new terminal states

Every new state reaches silence the same way as the old ones: starting any scenario stops the current sound first; host lost, host unreachable, host not allowed and dial-anyway refused each play the failed cue exactly once; the blocked screen and "unblocked" step are silent; Close, unblock, re-add and cancel call the stop. Checked in Chromium at 390 and 1440 px: zero live oscillators after each end, after a ringtone is interrupted by switching scenario, and no console errors.

## Edge cases the prototype exercises

1. Answer and remote cancel race: whichever lands first wins; later transitions are ignored because each requires the matching phase.
2. Reconnecting then End call: ends normally with the elapsed time.
3. Cancel during Connecting or Ringing: "You cancelled the call"; ringback stops at once and the ended cue plays once.
4. Switching platform view mid-call keeps call state, resets the output to that platform's default.
5. Leaving the page or hiding the tab during ringing silences it immediately.
6. Web Audio unavailable: panel reads "Sound unavailable" and every sound stays described in text.

## Not decided here

Real timeout values; ringtone choice and system ringtone use; behaviour with Do Not Disturb; whether a missed-call notification also has a sound; automatic switch back to a reconnected headset; group-call entry points, host selection and member UI (see above); real redial timing; whether a blocked person sees the block time.

## Prototype note: state transitions

Every timed step in the prototype (`later()`) now re-renders the call screen after it runs. Before this, the log said "Connected" while the screen stayed on "Answered · connecting audio…", and a retry that succeeded looked stuck. This is a prototype rendering fix only. It does not change the call policy above.
