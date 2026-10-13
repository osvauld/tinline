# Screens handoff: from the playground to the apps

For the agent building Tinline's screens in the Android app (Compose,
`android/app/src/main/kotlin/com/osvauld/p2p/`) and the desktop app (iced 0.14, `crates/desktop/src/`).
Written 2026-10-13. The design is decided in the playground; this file says where each screen is
specified and what must carry over exactly.

## 1. What is the spec

1. **The playground** (`docs/design/playground/`, Svelte). Run it and look:
   ```sh
   cd docs/design/playground && npm install && npm run dev   # http://127.0.0.1:5178/
   ```
   Use Chromium. Header toggles **Android / Desktop** and **light / dark**. The sliders icon opens the
   **Simulation** drawer, which plays the other side (incoming calls, people coming online, flags,
   network loss). Deep links: `?form=android|desktop&theme=light|dark#/<route>` (routes in §4).
2. **Decisions**: `docs/design/playground/PLAN.md` §7 (decisions log, plus the core follow-ups they imply).
   Each drawer section ends with notes marked **Decided**, **Proposed** or **Core facts**: read them
   for every screen you build.
3. **Principles**: `docs/design/PHILOSOPHY.md` (12 rules). The ones that most often break during a port
   are listed in §2 below.
4. **Behaviour tests as spec**: `docs/design/playground/tests/*.flow.js` (Playwright) state the
   expected behaviour step by step (e.g. `waiting.flow.js` is the call-waiting spec,
   `layout.flow.js` lists every place where nothing may move).
5. **Older design files** (`docs/design/*.dc.html`, `docs/design/groups/*.dc.html`,
   `docs/design/refined/`) are earlier studies. **Where they differ from the playground, the
   playground wins.**

**Proposed** items: build them as drawn (the user accepted the current answers for now,
2026-10-13), but keep them easy to change. When one needs core support that doesn't exist, build the UI
behind the existing core behaviour and list it in your report. Never show "Proposed" or simulation
text in the app.

## 2. Rules that must survive the port

- **Nothing moves when text changes.** Statuses, timers, errors, counts, badges, hints and
  "typing…" lines get reserved space (fixed-height rows, stacked-in-one-cell cross-fades,
  tabular digits, one-line ellipsis). Things the user caused (opening a sheet, sending) may move;
  text-driven changes may not. The playground measures this (`tests/layout.js`); on Android,
  compare screenshots before and after with the status changing.
- **Transient alerts replace, never insert.** An incoming call while the app is open takes over the
  header's brand area in place (`lib/calls/RingHead.svelte`). There is no floating banner.
  `IncomingAlert.svelte` is no longer used in the app; keep it only as an outside-the-app
  reference. The find-in-chat bar covers the conversation header in the same box.
- **Honest copy.** Use the playground's wording verbatim. Never claim encryption before a call
  connects. Never say "straight to their device" (a relay may carry the call). No anonymity
  claims. Unlinking is "not a lock".
- **No rings around avatars** (calls, speaking): speaking is a soft glow plus a 4 % lift.
- **Symmetry on focus screens** (calls, welcome, empty states): mirror-balanced, the main action
  on the centre axis. For example, the call screen's top row has a chevron on the left, the live
  logo in the centre and an info button on the right, all the same size. Lists and headers follow
  platform convention.
- **Reduced motion and Quiet** are honoured everywhere. Touch targets are at least 44 px. Contrast
  is at least 4.5:1. Every control has a screen-reader label. RTL works (use start/end, never
  left/right).

## 3. Foundations (done, on `design/apps`; use them, don't redo them)

| Foundation | Android | Desktop | Spec |
|---|---|---|---|
| Smooth corners (bubbles 18 outer / 6 stacking side, smoothing 0.6, grouped single/first/middle/last; runs = same sender, same day, within 5 min) | `Geometry.kt`: `SmoothCornerShape`, `bubbleShape(pos, mine)` | `geom.rs` (`run_pos`, `gap_above`), `shaped.rs` (widget that paints the smooth surface, rasterised with tiny-skia) | `playground/src/lib/geom/GEOMETRY.md` |
| Depth ladder flat / lift / pop / sheet | `Theme.kt`: `Depth`, `Modifier.depth(level, shape)` | `ui.rs`: `ui::Depth`, `raised`, `pop_surface`, `sheet_surface` | `PHILOSOPHY.md` §9, `playground/src/tokens.css` |
| Colours (bub-in, hairline, canvas, raised, danger, gold-ink) aligned with tokens.css | `Theme.kt` | `ui.rs` `Tok` | `playground/src/tokens.css` |
| Logo Mark D, live (idle, ringing sway, connected taut, send pluck) and `statusLook` | `Mark.kt`: `TinMark`, `statusLook()`, `TinFx.messageSent()` | `mark.rs`, `ui::logo` / `ui::logo_live`, `status_look()` | `playground/src/lib/brand/` |
| Icons (launcher, themed, notification, tray, window, packaging) | `res/` | `assets/`, `packaging/linux/` | `docs/design/brand/` |

Notes from the port:
- Android menus have plain round 14 dp corners: Material 3 1.3.1 menus only take theme `Shapes`, which must be
  plain round corners. The dialog's shadow needs room inside its window (`TinDialog` keeps a 72 dp margin).
- Desktop iced MSAA on this surface format is coarse, so curves (bubbles, logo) are rasterised with tiny-skia
  and cached; draw new curved surfaces the same way.
- Desktop tray (user, 2026-10-13): available = taut, unavailable = taut at 45 %, in a call = taut plus a
  centred accent dot (`crates/desktop/assets/gen_tray.mjs`).

Wiring left for the screens (the logo foundation exposes it; the screens hook it up):
- Android call screen: the live mark (`TinMark(28.dp, live = true, state = Ringing | Taut)`) is centred in
  the top row, between the minimise chevron and the info button, like the playground. Incoming
  screen: a ringing mark replaces the `Glyphs.Cans` icon.
- Send pluck: call `TinFx.messageSent()` when a send succeeds (`send()` in `Conversation.kt`). The
  home header mark uses `live = true` so it plucks.
- Message spacing: 2 px between bubbles in a run, 10 px between runs (Android still uses 4 dp
  everywhere). Emoji-only messages have no bubble surface, as in the playground.
- Desktop: the same states from `mark.rs`, in the sidebar brand row and the call screen.

Colours, type sizes, radii and spacing come from `playground/src/tokens.css`. Durations and easing
come from `playground/src/lib/fx/motion.ts`. Don't hard-code values that a token already names.

## 4. Screens

Paths are under `docs/design/playground/src/`. Each component's header comment states its intent
and rules; read it first. *Drawer* is the Simulation section that drives the screen.

### Shell and navigation
- **Android shell** (`screens/shell/AndroidShell.svelte`): a stack. The home screen has 4 tabs
  (Chats, Groups, Calls, Contacts) and a 56 px app bar with the logo. Other routes cover it
  full-screen. **Desktop shell** (`DesktopShell.svelte`): sidebar | list | detail pane, the same
  routes. The selection highlight glides between rows.
- Routes (`lib/nav/router.svelte.ts`):
  - **Tabs**: `#/chats`, `#/groups`, `#/calls`, `#/contacts`.
  - **Chats**: `#/chat/<id>`, `#/chat/<id>/info`.
  - **People and search**: `#/contact/<id>`, `#/search/<q>`.
  - **Settings and profile**: `#/settings[/notifications|sounds]`, `#/me`,
    `#/devices[/link]`, `#/privacy[/blocked|calls|phrase|passphrase]`.

### Sign-up and unlock (drawer: *Sign-up and unlock*)
`screens/gate/*`. On Android it is a full-screen stepped flow; on desktop, a centred card with the
same steps.
- **Welcome** offers Create, "Link to a device" and "Restore from recovery phrase".
- **Confirmations are optional**: "Check my copy" for the phrase; the passphrase has show/hide and
  no confirm field.
- **Lockout**: after 5 wrong passphrases, Unlock waits 30 s. This is a core gap.
- **Restore on a device that already has an account** adds a second account, reading "Added
  <name> to this device". This is also a core gap.
- Failure screens: `Failed.svelte`.

### Chats and conversation (drawer: *Chat*)
- `screens/home/ChatsList.svelte`: rows 72 px; the draft shows in the list; new-chat button.
- `screens/chat/Conversation.svelte`, `MessageRow.svelte`, `Composer.svelte`, `Layer.svelte`.
- **Bubbles**: grouped corners from the geometry foundation.
- **Composer**: grows up to 140 px. Reply and edit show a context chip. A reply survives leaving
  and reopening the chat; an edit restores the draft instead.
- **Offline strip**: overlaid, with matching top padding, so messages don't jump.
- **Header**:
  - direct chats: Call, Video, Find in this chat, Info;
  - groups: the group-call button, Find, Info.
  - A call button never starts a 1:1 call while a group call is running.
- **Modals** (`Layer.svelte`): a bottom sheet on Android; a popover or centred dialog on desktop.
- **Send pluck** on the logo: silent, nothing under reduced motion.

### Search (drawer: *Search*; core API: `docs/search.md` on branch `search`, not merged into dev yet)
`screens/search/*`.
- **Android**: a search pill plus the new-chat button in one row **at the bottom, above the tab
  bar** (user decision). It opens a full-screen search with the field at the bottom.
- **Desktop**: the field at the top of the list pane; Ctrl+K focuses it; Esc clears, then leaves.
- **Results**:
  - "People & groups" is one fixed-height strip of tiles. Then "Messages": fixed 64 px rows,
    one-line snippets with the match kept in view (`Marked.svelte`, lead 18), newest first,
    20 a page.
  - Rows are keyed by position: a new query changes the rows' text, it doesn't push them around.
  - Hints sit in reserved space.
- **Find in this chat** (`FindBar.svelte`), on Android and desktop: covers the header in the same
  box.
  - The count n/m runs newest first: 1/5 is the newest match.
  - Up / Enter go to older matches, Down / Shift+Enter to newer; Esc closes.

### Calls, 1:1 (drawer: *Calls*, *Video calls*)
`lib/calls/*`.
- **Call screen** (`CallScreen.svelte`): a cloud colour field behind everything, then the avatar
  (no ring), name, status and timer.
  - **Top row**: the minimise chevron, the live logo in the centre, and an "About this call" info
    button.
  - **Android controls**: Mute · Camera · End · Flip · Speaker. Flip is dimmed when the camera is
    off.
  - **Desktop controls**: Mute · End · Camera.
  - **Status copy**: "Calling…" until their device answers the signal, then "Ringing…". An
    offline person stays on "Calling…" until the 20 s timeout, which ends as "No answer" or
    "Couldn't connect". "End-to-end encrypted" appears only after connecting. Busy has no
    copy of its own: the call ends as "Call ended" and is recorded as Declined.
- **Call background** (`field/CloudField.svelte`): five hue-separated masses that move with the
  call sound's notes. Both textures (Fog and Clouds) are in the drawer; the user picks after
  seeing them. When building it, check how much CPU it uses.
- **Ringing while the app is open**: `RingHead.svelte`.
  - Android home: the avatar replaces the logo; the name sits over "Incoming call"; Decline and
    Answer take the icon slots.
  - Android screens with their own header: Back stays.
  - Desktop: the sidebar's brand row.
  - It cross-fades in and out; nothing moves.
- **Minimised call**: `CallStrip.svelte`. It is in the layout flow (the app moves down: you
  caused it). Tapping it restores the call.
- **Call waiting**: `WaitingCard.svelte` plus `tests/waiting.flow.js`.
  - One more call can ring over an answered call. The options are Decline, or End & answer.
    There is no hold.
  - A third caller gets busy and is left as a missed call.
  - The waiting cue: two soft tones every 3 s.
- **Video** (`lib/calls/video/*`):
  - Camera off by default; front camera first; the self-view mirrored and draggable, snapping to
    a corner (arrow keys move it too).
  - Controls auto-hide after 4 s, during video only.
  - The "Video paused, audio continues" chip has a fixed width.
  - Minimised on Android: a small floating tile plus the strip. On desktop: the strip only.
  - Calling back from a video-call record starts a video call.
- **Outside the app** (`SystemLayer.svelte`, `LockScreen`, `HeadsUp`, `Shade`, `Notices`,
  `DesktopSystem`):
  - The lock screen shows the caller's name, using a full-screen intent.
  - Heads-up when unlocked.
  - An ongoing-call notification, plus a status-bar chip with the timer.
  - Missed calls fold into one notification per person ("2 missed calls · Sana Rao"); a missed
    call adds one unread.
  - Busy still shows calls.
  - Desktop: an OS toast, plus an always-on-top mini window while the main window is minimised.

### Groups (drawer: *Groups*, *Group calls*)
`screens/groups/*`.
- **Groups list**: invitations to me, chat requests (incoming and sent), active groups, and a
  folded "Left or removed" section. Sections animate in place (`Collapse.svelte`).
- **Group info**: invites, hosts, members, mute, leave. What you can do depends on your role.
  The owner can't leave.
- **Member sheet**: per-member actions, plus confirm steps for flag and chat request.
- **Chat requests** work like Instagram: one per person at a time. After a decline, or a cancel,
  you can ask again only after 7 days.
- **Flags**: `FlagsOnMe.svelte` shows each flag on me as it happens (who, when, n of 3).
  `Removal.svelte` is the removed person's view.
- **Group calls**: `GroupCallScreen.svelte`, `GroupCallStrip`, `GroupCallAnnounce`,
  `groups/OngoingCall.svelte`.
  - **No ringing**: a quiet banner that settles after 9 s, a heads-up or toast outside the app,
    and nothing at all for muted groups.
  - **Ongoing call row**: "Call in progress · 3 joined · Join", fixed width.
  - **Roster**: speaking shows as a glow. Leave room for the glow so it isn't clipped. The
    call host is shown.
  - **Refusal notes**: "You're on a call", "This call is full".

### People, devices, privacy (drawer: *People and devices*)
- **Contacts and people**:
  - `screens/people/ContactsList.svelte`, `PersonRow.svelte` (64 px, fixed).
  - `NewChat.svelte`: the list area has a fixed height.
  - `AddContact.svelte`: scan, show my QR, or paste. It ends in a card saying how sure Tinline is
    about who this is.
- **Devices**:
  - `screens/devices/Devices.svelte`, `LinkDevice.svelte`.
  - The existing device approves the link: the new device shows the QR and the code, both show
    6 digits, then the passphrase.
  - Unlinking is honest: "not a lock".
- **Privacy**: `screens/privacy/*`.
  - Blocked: a block is visible to the person, and personal (no group effect).
  - Who can call me.
  - Recovery phrase: behind the passphrase, hidden until asked, hides again after 30 s, no copy
    button.
  - Change passphrase.
  - Read receipts and online visibility are not offered: the core has neither.

### Profile and settings (drawer: *Profiles*; core: `docs/design/profile-sync.md`, `profile-ui-handoff.md` on dev)
- **My profile**: `screens/profile/MyProfile.svelte`, `FieldRow.svelte`, `AddInfo.svelte`.
  - Each field saves on its own and patches one key.
  - A failed save keeps the draft.
  - Values from other app versions can only be removed.
- **Picture**: `PictureFlow.svelte`, `Cropper.svelte`.
- **Availability** (`Availability.svelte`): Busy for 30 min / 1 h / 4 h / until tomorrow morning /
  no end; the default is 1 h. Busy does not stop calls.
- **Person profile** (`PersonProfile.svelte`): read-only profile, then a separate block with my own
  settings for them: nickname (syncs to my devices only), calls, mute, block.
  - Links in a profile open outside Tinline (`OpenLink.svelte`).
- **Settings**: `screens/settings/*`: profile card, Notifications (with `PermBanner`, which animates
  its height), Sounds (Quiet plus the call sound set, with previews).

## 5. Core gaps the screens depend on (PLAN §7)

Build the UI. Where the core doesn't support something yet, keep today's core behaviour and list
it in your report.
- **Busy**: must keep admitting calls (today `set_available` refuses them).
- **Nickname**: needs a place in own-device sync.
- **Chat requests**: one pending per person, plus the 7-day cooldown.
- **Flags**: delivered to the flagged member before removal.
- **Unlock lockout**: 5 tries, then 30 s.
- **Restore**: adds a second account.
- **Contact verification**: "verified" flag after a pushed device list.

## 6. How to check your work

- Put the app and the playground side by side, same screen, same state, light and dark. Look at
  spacing, alignment, curves, easing and cue timing, not only "it works".
- For each screen, change every piece of text that can change (status, timer, count, error) and
  confirm nothing else moves.
- Builds go through `/home/abe/p2p_chat/scripts/buildlock.py`. `scripts/build_android.py` takes the
  lock itself. Emulator `emulator-5554` only; never the user's USB phone.
