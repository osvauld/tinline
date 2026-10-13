# Home, opening motion and touch feedback

This is the app-shaped entry missing from the first refinement pass. `index.html` now opens [home.html](home.html); the previous design-review landing page is [review.html](review.html). Design only; no native application changes.

Home is the app itself, so it has no harness bar and no notes inside the frame. Design-only controls (preview states, phone preview, the list of studies) sit at the bottom of the account sheet.

## Home

- Mobile: Tinline identity, availability, account/settings, searchable list, filter chips (`.t-chip`, `aria-pressed`: All / Unread / Groups on Chats, All / Missed on Calls), unread `.t-badge` counts (muted variant plus a bell-slash glyph before the time for muted chats), draft and waiting markers, and persistent Chats / Groups / Calls / Contacts navigation with the badge at the top right of the icon. "+" is a 56 px floating button 16 px above the tab bar. Content scrolls independently above navigation and safe-area padding. No selected-row tint on phones.
- Desktop: navigation rail, list and a detail pane. Selecting a private chat or a group loads the real conversation page in the pane (`chat.html?chat=<id>&embed=1` or `groups.html?group=<id>&embed=1`), so the conversation is fully usable there, including its in-page call. With nothing selected the pane reads "Choose a chat" / "Messages stay on your devices." The selected row is tinted. No oversized welcome dashboard in place of conversations.
- Switch tabs immediately; keep the shell stable. No fake network delay on normal navigation.
- Search is local, case-insensitive and escaped before rendering. Clear returns focus to the input. Empty results say "No matches"; the add-contact sheet explains that Tinline has no public directory.
- Availability has explicit Save / Cancel and pauses calls, not messages. Offline is distinct from paused. Becoming available does not imply that connectivity or OS notification permissions are fixed.
- Add contact: local ticket example, invalid/expired recovery, then a consent step. Ticket contacts can message and call; nothing real is transmitted, scanned or copied.
- On phones, tapping a private chat opens that conversation directly using `chat.html?chat=<id>&from=home`, and tapping a group opens `groups.html?group=<id>&from=home`; Back returns to Home. Calls-tab rows open a small sheet with Message and Call; Call opens [calls.html](calls.html).
- The account sheet links to [Phone preview](mobile.html?screen=home) (Home inside a phone bezel) and to all design studies. `mobile.html` also takes `?screen=chat&chat=<id>` and `?screen=groups&group=<id>`, loading the conversation pages with `?embed=1`, with a 390/320 px width switch and a "Desktop layout ↗" link.
- The theme switch and Reduce motion in the account sheet also apply to an embedded conversation in the detail pane.
- Native dialogs trap keyboard focus and support Escape. Closing restores the relevant trigger, including rows that were re-rendered and nested steps in the add-contact flow.

## Opening animation

A quiet, **silent** brand flourish: two rounded endpoints appear and the gold connecting line draws between them, with "Tinline" and "Your people. Your line." It lasts 650 ms, followed by a 160 ms fade. Skip is always available. No startup chime, no synthetic haptics, no looping splash.

- Once per browser tab session using a single sessionStorage flag; no message, contact or secret is stored.
- Normal return/navigation does not replay it. Account sheet → Preview states → Replay opening makes it easy to review.
- System reduced-motion or the Reduce motion switch skips the flourish. No animation is required to understand any state.
- The app is temporarily inert while covered, so hidden controls cannot be accidentally activated. It is released on skip, completion, visibility change and pagehide. Returning from browser history cannot strand the page behind an inert splash.
- This is **not** a real loading-progress indicator. The home data is simulated and available immediately. A native implementation should tie any longer startup state to genuine initialization, never extend this brand flourish to cover failures.

## Loading and transitions

Preview states (in the account sheet) contains Offline and No contacts yet switches and separate slow-load and failed-load simulations.

| Moment | Feedback | Recovery |
|---|---|---|
| Normal navigation | 180 ms content entrance, 5 px maximum movement, short bounded stagger | Immediate interaction; no waiting spinner |
| Slow list load | Row-shaped skeletons; shell/navigation stays usable; `aria-busy` and status announcement | Simulated completion after 950 ms |
| Load failure | Text explanation with Try again; not an endless skeleton | Retry or leave the section |
| Change tab while loading | Old loading timer cancelled | No stale result overwrites the new tab |
| Browser-history return during load | Restores the local list | No stuck loading state |
| Reduced motion | Static skeletons, no entrance movement or shimmer | Identical text and controls |

Loading simulations are deliberately invoked, not added to every screen. Offline, paused calls, load failure and empty lists each have their own copy and action.

## Surface and touch language

Shared `interactions.css` adds feedback to home and the existing studies, without flattening their individual layouts.

- **Lists / transcripts:** mostly flat. A tinted selection (desktop only) or pressed wash, not a shadow around every row.
- **Primary controls:** two low-opacity shadow layers create slight elevation. Press scales to 97% and flattens the shadow. Release returns over 100–160 ms without moving surrounding layout.
- **Sheets / dialogs:** larger, soft two-layer shadow and a dimmed backdrop. A 160 ms entrance; no bouncy movement.
- **Touch vs hover:** touch gets immediate press feedback. Hover elevation is only applied on a fine pointer that supports hover—no sticky mobile hover dependence.
- **Keyboard:** clear focus outline, native Enter/Space activation, input focus ring and correct focus restoration. Selection is not indicated by shadow alone.
- **Disabled:** never receives the active scale/elevation response.
- **Reduced motion:** transforms and transitions off; color, outline and pressed contrast remain.
- **High contrast:** shadows replaced with system-color borders; pressed outline remains visible.

No vibration or tap sounds are simulated or claimed. Those need explicit platform implementation and user preferences.

## Verification

See [home-verification.md](home-verification.md) and [mobile-chat-verification.md](mobile-chat-verification.md). Representative captures are in `qa/screenshots/home-*`. The original boards remain at `../AndroidHome.dc.html` and `../DesktopMain.dc.html` for comparison.
