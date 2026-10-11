# Tinline refined studies — binding component spec (pass 2)

This file is law for every page in `docs/design/refined/`. Where a page disagrees
with this spec, the page is wrong. Tokens come from `tokens.css` — load it before
the page's own `<style>` and **delete** page-local palette values it replaces.
Principles: minimal, calm, every interaction deliberate, **zero designer notes
inside app frames**. Light + dark + 390px + 1440px must all be checked.

## Harness chrome (study pages only)

One slim bar above the app frame, visually receding (no card, no pills, transparent
on `--bg`): left `← Home · <Study name>` (13px, muted; "← Home" is a link in
`--accent-ink`); right: icon-only theme toggle (sun/moon, 36px) and, where the page
has scenario controls, an icon-only "Simulation" toggle (sliders icon) opening a
drawer **closed by default**. ALL disclaimers ("simulated data", "nothing is sent…",
platform notes) live inside that drawer or in the study's `.md` — never visible by
default, never inside the app frame. No "Dark theme" / "Dark mode" text buttons,
no blue default links anywhere (style every `a`).

### Embed contract
`?embed=1` on `chat.html` and `groups.html`: hide harness bar + drawer entirely,
zero page padding, app frame fills `100dvh`, no border/radius/shadow. Consumers:
`mobile.html` (phone bezel) and `home.html` (desktop detail pane iframe).

## Message bubbles (chat.html, groups.html — identical system)

Anatomy — meta flows INLINE at the end of the last text line (WhatsApp-style),
never a separate row:

```html
<div class="bub">Yes, after 6 works for me<span class="meta">edited 12:40 <svg class="tick">…</svg></span></div>
```
```css
.bub{max-width:min(78%,560px);padding:7px 12px;font-size:var(--fs-body);
     line-height:var(--lh-body);border-radius:var(--r-bub);overflow-wrap:anywhere}
.meta{float:right;margin:10px -2px -2px 10px;font-size:var(--fs-meta);line-height:1;
      font-variant-numeric:tabular-nums;display:inline-flex;gap:3px;align-items:center;
      user-select:none}
```
(With `white-space:pre-wrap` keep the meta span glued to the text node. The float
drops to its own right-aligned short line only when it doesn't fit — correct.)

- Received: `--bub-in` bg, `--bub-in-ink` text, 1px `--bub-in-line` border, meta `--bub-meta-in`.
- Sent: `--bub-out` bg, `--bub-out-ink` text, **no border**, meta `--bub-meta-out`. Never solid teal.
- Conversation scroll area background: `--canvas`.
- Status glyphs in meta (sent only): pending = 11px clock outline; delivered = 12px
  single check. No text like "Delivered to Arjun's phone" in the bubble — that
  detail moves to a "Message info" entry in the message menu.
- Failed: no tick; a compact 12px row *below and outside* the bubble, right-aligned:
  `Not delivered · Retry` (Retry = real button, `--danger` text). Nothing else.
- `edited` = lowercase word before the time, same meta size.
- Reply quote inside bubble: 3px `--accent` left bar, `--surface-2` fill, 12px radius,
  sender bold 12px, 2-line clamp, 6px below to text.
- Emoji-only messages (1–3 emoji): no bubble fill/border, 34px glyphs, meta below
  right at meta size.

### Grouping
Same author, same day, gap ≤ 5 min ⇒ one group. Margins: 2px inside a group,
10px between groups. Corners (side facing the stack, i.e. right for sent /
left for received): first in group `[outer, outer, tight, outer]`, middle
`[outer, tight, tight, outer]` (mirror for received), last restores the bottom
outer corner. `outer = --r-bub`, `tight = --r-bub-tight`. In groups.html the
sender name (13px bold, avatar-hue ink — same hash as §Avatars) appears only on
the first bubble of a group.

### Message menu — no persistent ⋮ ever
- Coarse pointer / <860px: **long-press** (500ms, cancelled by 10px move or scroll;
  also the `contextmenu` event) opens a bottom sheet: Reply · Copy · Edit (own) ·
  Delete (own) · Message info. Scrim tap and Escape close; focus returns to the
  message. Light haptic-style scale feedback on open (respect reduced motion).
- Fine pointer ≥860px: 20px chevron button appears beside the bubble on
  hover/focus-within only (`opacity:0` otherwise, `display:none` on coarse
  pointers), opening the same actions as an anchored popover.
- Accessibility: every message keeps a visually-hidden focusable menu button so
  keyboard/screen-reader users reach the same sheet. Arrow-key message focus
  optional; do not regress existing focus handling.

### Other conversation elements
- Day separator: centered pill, `--surface-2` bg, 11px `--muted`, padding 3px 10px,
  radius 999, margins 10px.
- Event lines ("Kavya joined…"): centered 12px `--muted`, no pill, 6px margins.
- New-message affordance: when scrolled up and a message arrives, floating
  scroll-to-bottom circle (36px, `--surface`, shadow-pop) with a `.t-badge` count.
- Send feel: new own bubble rises 4px/120ms fade-in (skip under reduced motion);
  pending clock swaps to check without layout shift (fixed-width glyph slot).

## Composer (identical in chat.html and groups.html)

`[ attach ] [ field                      ] [ send ]`
- Attach: 44px quiet icon button (paperclip), `--muted`, left of the field.
- Field: min-height 44px, max-height 140px auto-grow, radius `--r-field`,
  `--surface` bg, 1px `--hairline` border, padding 10px 16px. Focus: border
  `--accent`, plus 3px press-wash ring — no double heavy ring.
- Send: 44px circle, `--accent` bg, `--on-accent` paper-plane icon (same icon both
  pages, the `#i-send` plane). Empty draft ⇒ same button at 38% opacity, disabled,
  no shadow. Never hidden, never a different icon per page.
- Enter sends on fine pointers, Shift+Enter newline; on touch Enter newlines.
  **No permanent hint row** under the composer.
- Reply/edit context strip above the field: `--surface-2`, 3px accent bar, close
  button, 13px, 2-line title+snippet clamp to one line each.
- Sending a new message (not an edit) plays a short cue from `call-audio.js`
  (`pluck`, a bright one-shot — continuing the brand mark's string: ringback is
  the string slack, connect is it pulled taut, send is a pluck of the same
  string) the instant it's pushed into the thread, not tied to delivery. Same
  Quiet-mode switch as the call cues; silent when a call's ringback is
  interrupted, it resumes right after. An attachment plays it too; an edit
  never does.

## Lists, badges, chips, avatars (home.html primarily)

- Unread badge: `.t-badge`. Muted chat: `.t-badge.muted` **plus** a 14px mute
  (bell-slash) glyph before the timestamp. One system, both platforms.
- Filter chips: `.t-chip` with `aria-pressed`. Equal height, no half-styled states.
- Avatars: `.t-avatar` + hue class from the shared hash
  `[...name].reduce((a,c)=>a+c.codePointAt(0),0)%6` → `av0…av5`. Apply the same
  hash in every page that shows initials or colours a sender name. Group avatars:
  `--surface-2` bg + group glyph in `--accent-ink` (not a hue).
- Chat rows: 44px+ target, press wash per interactions.css. Selected-row tint
  exists **only** ≥860px (two-pane); remove on mobile.
- Mobile home: "+" is a 56px bottom-right FAB (accent, shadow-pop), 16px above the
  tab bar, not next to the heading. Desktop: small 36px "+" in the pane header.
- Tab bar: badge sits top-right of the icon (translate, 2px `--bg` ring), never
  overlapping the glyph; active tab = accent icon + label, inactive `--muted`;
  consistent 1.8px stroke icons.

## Headers (conversation)

- Direct: back (44px) · 36px avatar · name (17px semibold) · presence line 13px
  `--muted` — "Online", "Offline", or "Last seen 12:41". **Never "· direct".**
- Group: name + single line "8 members" (13px muted). Host status appears only
  when degraded, as a thin `--warn-bg` strip under the header ("Host offline —
  messages wait"), not in the title block.
- Info (ⓘ) 44px icon button right-aligned; same icon set both pages.

## In-conversation calls (chat.html, groups.html)

The phone button in a conversation header starts an **in-page call overlay** —
no navigation away, because the core loop is call ↔ chat:
- **Direct call**: overlay fills the app frame (and the iframe in embed mode):
  `--bg`, 96px `.t-avatar`, name 24px semibold, status 13px `--muted`
  ("Ringing…", then simulated connect after ~2.5s), timer MM:SS tabular-nums
  in a fixed-width slot. Controls centered: Mute 56px quiet circle (toggles),
  End 64px `--danger`. Top-left: 44px down-chevron, aria-label "Back to chat
  — call continues".
- **Group call**: the single avatar is wrong — show who's actually in the
  call. Header strip: group glyph avatar 44px, group name 17px semibold,
  "<n> in the call" 13px `--muted`, timer. Below it, a **roster**, not a
  bigger avatar: up to K speaking tiles (K = 3 desktop host / 2 phone host,
  matching the host's advertised K) at 72px, each a `.t-avatar` + first name
  under it + a mic-off glyph badge when that person is muted; the current
  speaker's tile gets a 2px `--accent` ring that only redraws on a level
  change (no continuous pulsing meter — these are still visual simulations).
  The host's tile carries a small "Hosting" `.t-badge.muted`-style tag. Tiles
  that are in the call but not currently forwarded sit in a quiet row below,
  40px avatar only, up to 6 shown then "+<n>"; tapping it expands the full
  roster in a sheet. Controls are the same centered row as the direct call,
  minus Camera (group video is out of scope). Grid reflows at 390px: speaking
  tiles wrap to two rows before the quiet row starts.
- Minimize → **ongoing-call strip**: one full-width 40px button above the app's
  header(s), `--accent` bg, `--on-accent` text: "● <name> · MM:SS" (dot pulses;
  static under reduced motion; timer ticks, tabular-nums). Visible across list
  AND conversation views; tapping returns to the overlay. End exists only inside
  the overlay. Chatting, browsing, switching chats all work while minimized.
- While a call is live, the header phone button means "return to call" (never a
  second call). Escape minimizes, never ends. Focus: overlay open → container;
  minimize → the strip; end → back to the phone button. Toast "Call ended" on End.
- **The call outlives the page.** A live call is app state, not page state:
  it persists across every navigation between `home.html`, `chat.html` and
  `groups.html` (incl. inside `mobile.html` and Home's desktop detail pane)
  via one shared `call-state.js` (sessionStorage, timer derived from the
  stored connect time so it never resets or jumps). Every app screen —
  Home on all four tabs, chat list, conversation, group list — shows the
  same 40px strip at the very top while a call is live; tapping it reopens
  the overlay for that call (from Home: opens the conversation with the
  overlay up). Exactly one strip is ever visible: inside Home's desktop
  detail pane the embedded page suppresses its own strip and Home's
  full-width strip is the one. Ending the call from any page clears it
  everywhere immediately.
- **Sound.** These overlays are no longer silent, but they are not a second
  audio implementation: `call-audio.js` is the one synthesized cue set,
  extracted verbatim from `calls.html`'s existing `PAT`/`Snd` (same
  frequencies, timing, labels) and loaded by `calls.html`, `chat.html` and
  `groups.html` alike, so the ringtone/ringback/connected/ended cues sound
  identical everywhere. It only ever starts after a user gesture (the call
  button itself is that gesture) and is fully torn down on End/minimize-
  survives/unmount, same discipline as `calls.html`'s own `pagehide`
  cleanup.
  - **Direct call**: the `back` (ringback) cue loops through "Ringing…",
    the `connected` cue plays once on connect, `ended` once on End. Mute
    silences your mic, never these cues.
  - **Group call**: per the rotation brief, **a group call never rings
    anyone's phone** (U21) — so the `ring`/`back` cues never play here.
    Starting or joining one is quiet: a single short `pulse` cue on joining
    (reusing the existing "reconnecting" cue's one-note shape), the
    `connected` cue once the roster has you in it, a soft version of `pulse`
    whenever someone else joins or leaves the roster (easy to miss, not a
    doorbell), `ended` once on leaving. No looping tone at any point.
  - A Simulation-drawer "Quiet mode" switch (same control and label as
    `calls.html`'s) silences all of the above without hiding the roster or
    timer; QA keeps it on by default so headless test runs stay silent.
- **Call record, left in the thread on End** (the WhatsApp convention —
  calls need history too, not just a toast):
  - Direct call (`chat.html`): a bubble, same side and timing as any other
    message from whoever ended it — in this simulation that's always you,
    since the header button only calls out. Content is `fileNode`'s icon
    + title + subtitle layout (`.file.callrow`, a button): phone icon,
    "Voice call", and either the MM:SS duration or, if End was pressed
    before the call connected, "No answer" in `--danger`. No ✓/clock
    delivery tick — a call already happened, it has nothing left to
    deliver. No reply/copy/edit — only Delete, same as any bubble. Tapping
    it calls that person back. Shows in the chat-list preview the same way
    a `📎` attachment does.
  - Group call (`groups.html`): an **event line** (existing rule, this
    section), since a group call belongs to the room, not one sender —
    "Call ended · MM:SS". A group call never rings (U21) so there is no
    "No answer" case here. Event lines never appear in the chat-list
    preview, same as every other one.
  - Quiet resets (Clear/Reset in the Simulation drawer) end the call
    without leaving a record — they're clearing the demo, not living it.
- **The brand mark, animated** (direct calls only — `chat.html`'s overlay and
  `groups.html`'s overlay when it's showing a direct call; group calls keep
  their roster and never show this, group video is out of scope the same
  way): the `mk-can`/`mk-str` mark from `brand.html`'s `#mark` sits between
  the name and the status line. The string (`.mk-str`, `--gold`, quadratic
  `M14 20Q20 28 26 20`) sags while ringing/connecting and straightens to
  `M14 20 L26 20` — **taut** — the instant `t.connected` flips true, the same
  read that flips the status text to "Connected" and the same moment the
  `connected` audio cue plays: one effect, seen and heard together, not two.
  260ms `d` transition. On End, before the overlay unmounts: the string goes
  slack again and the whole overlay plays a 200ms scale(.96)+fade release
  (`.ending` class) — `endCall()` waits for it before actually calling
  `TinCall.end()`. Minimize never triggers this (the call hasn't ended);
  reopening shows whatever taut/slack state is already current. `calls.html`
  shows the same sag/connecting/taut states (`view()`'s `v.mark`) but, since
  it fully redraws `#stage` on every phase change (nothing there persists
  across a render to transition from), the change there is a cut, not a
  tween — consistent with every other state change already being a cut on
  that page. Reduced motion (`prefers-reduced-motion` or `?motion=reduce`):
  no transition, no End delay — states still change correctly, just instantly.
- Deep links (`calls.html` only): `?call=<id>` / `?call=group:<id>` open an
  already-ringing screen — unaffected by the above. `calls.html` is unchanged
  apart from pulling its cues from the shared `call-audio.js`: it keeps its
  own Android/Desktop toggle, incoming-vs-outgoing states and failure cues,
  none of which the in-conversation overlay needs.
- Policy states: a message-only contact has a disabled call button (calling
  needs their permission — explained in the contact sheet, not in a banner);
  a group with no live host has a disabled call button; a removed/read-only
  group has none.

## Dialogs & sheets

≥700px: centered dialog, radius `--r-sheet`, shadow-sheet. <700px: bottom sheet,
top corners `--r-sheet`, 28px grab notch optional, safe-area padding. Scrim
`rgb(10 16 14 / .5)`. Escape + scrim close; focus returns to the opener.
Destructive confirm buttons: `--danger` bg, `--on-danger` text.

## Copy rules

Sentence case. No jargon ("direct", "stash"), no designer voice ("We tell you now,
not later."), no simulation talk inside app frames, no exclamation marks. Empty
states: 2 lines max — bold short line + one plain line (e.g. "Choose a chat" /
"Messages stay on your devices."). Fix: "1 contact **hasn't** got your address yet".

## Motion

120–180ms ease-out; entrances fade+4px rise; `prefers-reduced-motion` and
`data-motion=reduce` kill all of it (already in interactions.css — keep applying
the shared classes). Animation never carries meaning alone.

Press feedback (`interactions.css`, shared by every study) scales to `.97` on
`:active` for `.btn/.ib/.b/.chip/.new-btn`, and now `.cbtn`/`.cb` too — the
call-control buttons (Mute, End, and calls.html's Mute/End/route row) get the
same give as every other button, with End itself squishing a touch more
(`.cbtn.end`/`.cb.end`, `.93`) since it's the one irreversible action in that
row. Reduced motion zeroes the transform the same way it already zeroes
everything else here.

## QA duties per packet

Static server is already running at http://127.0.0.1:8768/ (root `docs/design/`).
Use `qa/lib.js` (playwright at `/home/abe/project/sveditorjs/node_modules/playwright`,
chromium `/usr/sbin/chromium`). After editing: update the flow tests you own so they
pass, and screenshot your screens at 390×844 and 1440×1000, light + dark, and LOOK
at them. Do not edit `tokens.css`, `SPEC.md`, `interactions.css`, `qa/lib.js`, or
files owned by another packet. Do not run `git commit`.
