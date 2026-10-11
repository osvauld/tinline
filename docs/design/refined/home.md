# Home, opening motion and touch feedback

This is the app-shaped entry missing from the first refinement pass. `index.html` now opens [home.html](home.html); the previous design-review landing page is [review.html](review.html). Design only; no native application changes.

## Home

- Mobile: Tinline identity, availability, account/settings, searchable list, filter chips, unread/draft/waiting metadata, and persistent Chats / Groups / Calls / Contacts navigation. Content scrolls independently above navigation and safe-area padding.
- Desktop: navigation rail, list and selected-conversation preview. No oversized welcome dashboard in place of conversations.
- Switch tabs immediately; keep the shell stable. No fake network delay on normal navigation.
- Search is local, case-insensitive and escaped before rendering. Clear returns focus to the input. Empty results explain that Tinline has no public directory.
- Availability has explicit Save / Cancel and pauses calls, not messages. Offline is distinct from paused. Becoming available does not imply that connectivity or OS notification permissions are fixed.
- Add contact: local ticket example, invalid/expired recovery, then a consent step. Ticket contacts can message and call; nothing real is transmitted, scanned or copied.
- On phones, tapping a private chat opens that person’s conversation directly using `chat.html?chat=<id>&from=home`; the chat Back control returns to Home. Desktop selection keeps the preview pane and its Open conversation action preserves the person. Group and call studies remain separate previews. Explicit [Mobile chat preview](mobile.html?screen=chat&chat=arjun) makes the phone layout visible even in a desktop browser.
- Native dialogs trap keyboard focus and support Escape. Closing restores the relevant trigger, including rows that were re-rendered and nested steps in the add-contact flow.

## Opening animation

A quiet, **silent** brand flourish: two rounded endpoints appear and the gold connecting line draws between them. It lasts 650 ms, followed by a 160 ms fade. Skip is always available. No startup chime, no synthetic haptics, no looping splash.

- Once per browser tab session using a single sessionStorage flag; no message, contact or secret is stored.
- Normal return/navigation does not replay it. `Preview states → Replay opening` makes it easy to review.
- System reduced-motion or the in-preview Reduce motion setting skips the flourish. No animation is required to understand any state.
- The app is temporarily inert while covered, so hidden controls cannot be accidentally activated. It is released on skip, completion, visibility change and pagehide. Returning from browser history cannot strand the page behind an inert splash.
- This is **not** a real loading-progress indicator. The home data is simulated and available immediately. A native implementation should tie any longer startup state to genuine initialization, never extend this brand flourish to cover failures.

## Loading and transitions

`Preview states` contains separate slow-load and failed-load simulations.

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

Shared `interactions.css` adds feedback to home and the five existing studies, without flattening their individual layouts.

- **Lists / transcripts:** mostly flat. A tinted selection or pressed wash, not a shadow around every row.
- **Primary controls:** two low-opacity shadow layers create slight elevation. Press scales to 97% and flattens the shadow. Release returns over 100–160 ms without moving surrounding layout.
- **Sheets / dialogs:** larger, soft two-layer shadow and a dimmed backdrop. A 160 ms entrance; no bouncy movement.
- **Touch vs hover:** touch gets immediate press feedback. Hover elevation is only applied on a fine pointer that supports hover—no sticky mobile hover dependence.
- **Keyboard:** clear focus outline, native Enter/Space activation, input focus ring and correct focus restoration. Selection is not indicated by shadow alone.
- **Disabled:** never receives the active scale/elevation response.
- **Reduced motion:** transforms and transitions off; color, outline and pressed contrast remain.
- **High contrast:** shadows replaced with system-color borders; pressed outline remains visible.

No vibration or tap sounds are simulated or claimed. Those need explicit platform implementation and user preferences.

## Verification

See [home-verification.md](home-verification.md). Representative captures are in `qa/screenshots/home-*`. The original boards remain at `../AndroidHome.dc.html` and `../DesktopMain.dc.html` for comparison.
