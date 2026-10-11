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

## QA duties per packet

Static server is already running at http://127.0.0.1:8768/ (root `docs/design/`).
Use `qa/lib.js` (playwright at `/home/abe/project/sveditorjs/node_modules/playwright`,
chromium `/usr/sbin/chromium`). After editing: update the flow tests you own so they
pass, and screenshot your screens at 390×844 and 1440×1000, light + dark, and LOOK
at them. Do not edit `tokens.css`, `SPEC.md`, `interactions.css`, `qa/lib.js`, or
files owned by another packet. Do not run `git commit`.
