# Sign-up handoff (2026-10-13)

The playground (`playground/src/screens/gate/*`) is the spec; this page tells the implementor of the Iced
desktop and Android ports **what is decided and why**, so the reasons survive the port. The agent doing the port starts from
`SIGNUP-PORT-BRIEF.md` (the advice: order, pitfalls, what to verify). Copy rules live in
`VOICE.md`; this page links to them rather than repeating them. Iterate with
`http://127.0.0.1:<port>/?form=desktop|android&step=<step>&theme=light|dark` (`?step=` jumps straight to a step).

> **Implementor:** when a detail here looks arbitrary (a 184px button, a hidden "1 of 6"), it is there to
> keep something from moving. Don't drop it to save code. If the target toolkit can't do it, write it
> down in `SCREENS-PORT-STATUS.md` rather than approximating silently.

## 1. The three rules everything below serves

1. **Nothing moves.** No resize, no re-centring, no jump when you change step, type, or something turns busy.
   *Why:* the user is handing over the keys to their account; a calm, steady screen reads as trustworthy.
   *How:* fixed card height, reserved space for text that comes and goes, fixed button widths.
2. **One voice** (`VOICE.md`): you/your in text, I/my on buttons, caring, never "we" for the app.
3. **Owning your account is said once, gently** (`VOICE.md` § Owning your account). There is no server to
   save the user, so they must understand that, but it must never feel like a threat.

## 2. Frame (every step)

| | Desktop | Android |
|---|---|---|
| Container | Card 600 wide, **fixed 640 tall** (`min(640px,100%)`), radius 24 (`--r-sheet`), centred on `--canvas` | Full screen |
| Header | Back slot 44 · brand (Mark 24 + "Tinline" 17) · progress at the end | Back slot · progress at the end |
| Body padding | 8 40 16 | 8 24 16 |
| Footer | Row, buttons end-aligned, primary rightmost, 16 40 32, gap 12 | Column, full width, 12 24 24, gap 8 |
| Title | 28/34, 700, balanced wrap | 26/32, 700, balanced wrap |

- **Fixed card height.** *Why:* the old card was 618, then 704 on the phrase step, then 750 with the words
  shown, and its top jumped from y=91 to y=25. Now a long step scrolls *inside* the card instead.
  *Iced:* give the card a fixed `height(640)`, clamped to the window, and put the body in a `scrollable`.
- **Balanced titles** (`text-wrap: balance`). *Why:* this avoids a single word left alone on the second line
  ("A direct line to your / people"). *Iced/Compose:* there is no balance primitive. Pick the break by hand
  per string and width (Compose: `LineBreak.Heading`), or accept greedy wrapping and note it.
- **Leads given as sentences** render one sentence per line, each wrapped on its own (`StepShell` `lead: string[]`).
  *Why:* this gives an even, symmetric shape instead of a ragged paragraph.

## 3. Progress header: present from Welcome on

- Welcome and Choose are **step 0**: the header shows all six segments empty, and "1 of 6" is **hidden but
  keeps its width** (`visibility: hidden`, min width 42, tabular figures).
  *Why:* if the header appeared only on step 1, the whole body would jump down by its height. Now the first
  Continue only fills a segment and fades in the count, in the same place.
- Segments 14×6, radius 3, gap 4, `--line`, then `--accent` when filled; colour change takes `--dur-switch`.
- The back slot is always 44 wide, even with no back button. *Why:* the brand never shifts sideways.
- **Theme switch** (40 round icon button, `--muted`) at the very end of the header on every sign-up step:
  a moon in light mode, a sun in dark mode. It flips the effective theme to an explicit light or dark.
  *Why:* the first screens are where someone decides if the app feels like theirs. *App:* it writes the same
  Appearance setting as Settings, so the choice carries into the app.
- Accessibility: `progressbar`, min 0, value text "Step n of N: <label>". The labels are in `Gate.svelte` `LABEL`.

## 4. Buttons and keys

- **Desktop button min width 220.** *Why:* it fits the longest, "Add my first contact" with its ↵ keycap, and the busy label (spinner +
  "Encrypting…"), so the button never changes width between steps or when busy.
- **Android "Continue" is a 56 round accent button with an arrow** at the end of the footer (`Btn next`).
  - The label stays the accessible name.
  - When busy, a spinner replaces the arrow, so the size is unchanged.
  - A secondary action ("Skip for now") shares its row at the start.
  - *Why:* it is smaller above the keyboard, sits where the thumb is, and is the familiar "next".
  - Steps whose button says what it does keep their words: Create my account, Copy my words / I've saved them,
    Open Tinline / Add my first contact, Not now. Terms keeps "I agree" as the accessible name and shows the arrow.
- **Enter continues on every desktop step.**
  - In a field, Enter submits the form natively. A disabled submit button means Enter does nothing.
  - Anywhere else, Enter clicks the step's `[data-enter]` button. Each step focuses its title on entry, so
    Enter works immediately.
  - A focused button or link keeps its own Enter. Modifiers and IME composition are ignored.
  - The button shows a **↵ keycap**: 22×20, radius 6, 13px, the button's own colour at 16% (10% when
    disabled). *Why:* the shortcut can be found without being taught.
  - *Iced:* subscribe to `keyboard::on_key_press(Enter)`, and skip it when a `text_input` has focus (that
    input's `on_submit` already handles it).
- **RTL:** the arrow mirrors.

## 5. Android keyboard

- Assume the soft keyboard is up on every step that has a field (the playground mocks it at 272).
  `adjustResize`: the footer rides on top of the keyboard. *Check each step with it up:* the primary
  action and the field must both stay visible.
- *Compose:* `imePadding()` on the step column. Don't put the footer inside the scrolling body.

## 6. Steps

### Welcome (step 0)
- Same layout as every step: title and lead at the top, **Mark 120 (line slack, live)** filling the free
  space, emergency note at the bottom of the body, footer buttons.
- Title "A direct line to your people". Lead: "Tinline connects your {computer|phone} straight to
  theirs." / "You just need a nickname. Encrypted end to end."
- **Desktop buttons: the same footer as every step**, right-aligned: "I have an account" then "Create my
  account" (primary, Enter) at the far right, both min 220. *Why:* "Create my account" sits exactly where
  Continue sits on the next step, so the click target doesn't move as you go on (user, 2026-10-13; a centred
  stack and a centred pair were tried and dropped).
  **Android:** they stack full width, "Create my account" on top (thumb reach).
- Text alignment is under trial: `?align=center` centres titles and leads on every step. Start-aligned is
  still the default until it is decided.
- Second button "I have an account" goes to **Choose**. *Why:* two choices instead of three equal ones
  keeps the main path obvious.
- **Line language:** the tin-can mark's string is the brand. It is slack on Welcome and **taut on Done
  ("Your line is open")**. Keep the two states; they are the visual story of sign-up.

### Choose ("Use your account here")
- Two option rows: Link my other device / Restore my account.
- **Android: the options sit at the bottom**, in thumb reach where Welcome's buttons were. Desktop: top.

### Name
- Label "Your nickname". *Why "nickname":* it says the name is casual and changeable, and that no real
  identity is needed. Same word everywhere in sign-up.
- Lead "Only the people you add will see it." *Why:* says who sees it in a positive way. The old "It never leaves
  your phone otherwise" was a negative and read awkwardly.

### Passphrase
- Optional; "Skip for now" shares the footer row on Android. The busy label is "Encrypting…" (with the fixed width).
- **Tone (2026-10-13):** this step had the most fear in it; it now says what the lock *gives* you.
  - Lead "Optional. A lock, so only you can open Tinline on this {phone|computer}." (was "Anyone with your phone
    could otherwise open your account.")
  - Info box: what it does, then the reassurance. Phone: "Encrypts your account on this phone. Your phone
    remembers it, so calls still ring after a restart. If you forget it, your recovery words let you set a new
    one." Desktop: "…on this computer. You type it each time Tinline starts. If you forget it, …". The old bold
    "**It can't be recovered**" is gone. *Why:* the fact is the same, but leading with the way back is calm.
  - Strength line: "Any length works. A few random words are easy to remember." → "Short. A few more words make
    it stronger." → "Fair" → "Strong". *Why:* "easy to guess" scolds; this says how to make it better.
  - *Implementor:* all four strength lines share one cell (the tallest sets its height), so a rating never moves
    the box below. In Iced, stack them in a fixed-height container; in Compose, measure the tallest once.

### Recovery words
- **Casual and calm, never alarming** (like the app's sounds). Title "Here are your recovery words". Lead:
  "If you ever switch devices, these 24 words bring your account along." / "Save a copy somewhere safe. Paper
  works great." There is no warning box.
- **The words show straight away**, with no blur and no "Show the words". *Why:* a reveal makes the moment
  feel dangerous; we want it to feel ordinary.
- **One primary slot, two moments.** First it says "Copy my words" (Enter). Once copied, the *same button* says
  "I've saved them" (Enter goes on), and the quiet option changes from "I wrote them down" to "Check my copy".
  Same size, so nothing moves (measured: x and width unchanged). `gate.phraseShown` remembers the copy, so
  coming back from Check my copy keeps the second state. *Why:* copying is the easy, likely path, so lead
  with it; writing on paper is still one tap away.
- **Clipboard (implementor).** Android: put the text on the clipboard with `ClipDescription.EXTRA_IS_SENSITIVE`
  (API 33+) so it isn't shown in the clipboard preview. Desktop: clearing it after a while is a reasonable
  extra, but say nothing alarming about it in the UI.
- Grid: 3 columns on desktop with rows 30 high and 4 gap; 2 columns on Android with rows 32. Numbers use
  tabular figures. Android order runs down the columns (1–12, then 13–24); desktop runs across the rows.
  Both forms fit without scrolling.

### Before you start (terms)
- Four points: Free of ads and tracking / Calls are end-to-end encrypted (relay can't hear) /
  **Your account is yours** / Not for emergency calls.
- **No checkbox.** One line sits right above the button: "By continuing, you agree to the Terms of Use and
  have read the Privacy Policy." The button ("I agree", the arrow on Android) is the agreement and sets `agreed`.
  *Why:* a checkbox and then a button is two taps for one decision. The line keeps the consent explicit,
  so don't move it away from the button.

### Permissions (Android)
- A permission you turned down reads "Off for now. You can turn it on in system settings." in **muted** ink (it was
  red, bold "Not allowed"). *Why:* you said no to something optional; that's fine, not a failure.

### Done
- Mark 96 enters **slack**, then **pulls straight** 450ms after the step arrives, in **two tugs** on the rhythm of
  the `line` cue (`LINE_SPRING` in `lib/brand/springs.ts`, shared with the empty state § 6e):
  - the line goes tight on the cue's first note, gives about 1.6px, and tugs again on the second note (~120ms
    later), then rests. Each tug twangs the string. The gap is read from the cue, so sound and motion stay locked;
  - the `line` cue (two plucks, § 6d) plays on the first tug, its notes landing on the two tugs;
  - **colour:** on sign-up marks (`lineColor`, Welcome and Done) a slack line is a faded gold (gold 55% into
    `--line`); it warms to full `--gold` on the first tug (`--dur-switch`). The colour says "connected" as much
    as the shape does.
  - *Why not more bounces:* a long bounce train looked artificial (user, 2026-10-13); two tugs read like someone
    on the other end. Calls keep the quick default spring.
  - Reduced motion: it settles taut, the colour still warms and the cue still plays.
  *Implementor:* the spring is one model with per-use parameters (`tugs`, `recoil`, `settleHz`); don't build a
  second animation. Done: settleHz = 1 / (2 × cue gap).
  Title "Your line is open, {first name}".
- New account only: lead "Scan their code or share yours, and you can call each other.", then two **tips**
  (not warnings). They are written in `VOICE.md`:
  - Two devices are better than one.
  - Groups are hosted on a computer (phone) / This computer can host your groups (desktop).
  - *Why here:* this is the happy moment, so it informs without intimidating. These facts must be said
    somewhere, and saying them once is enough.
- Buttons: **"Add my first contact"** (primary, Enter) and "Not now".
  - Add my first contact lands on **Chats** (its empty hero, § 6e) with **Add contact open over it**, on the
    Code tab. *Why:* a new account has nothing else to do yet. Adding someone pulls the hero's line tight behind
    the sheet, so closing the sheet shows "Your line is open · Message {name}", the next step already in place.
  - "Not now" lands on the same Chats hero.
  - Restore and Link keep "Open Tinline" (they already have contacts coming).

## 6b. Add contact (the first landing; redesigned 2026-10-13)
- **Two ways only: Code and QR.** A chip pair at the top ("Code" | "QR"; renamed from "Code" 2026-10-13 so the pair matches); **Code is first and the
  default** on both forms. *Why:* most first contacts happen at a distance (you send your code in another app);
  QR is for when you're together. One token, nothing else to choose. The chips are the decision for now
  (the old segmented pill is still at `?acswitch=pill` for comparison; don't port the pill).
- **Code**, top to bottom:
  - "Your code" / "Send it to anyone you'd like to talk to, in any app." / the code on **one line**, small mono,
    ellipsized at the end / **"Copy my code"** (primary; turns into "Copied" for 1.6s in the same button).
  - It copies the **bare code only** (`OSVC2:…`), no "Add me on Tinline:" prefix. *Why:* the user decided one
    plain token is least confusing, and pasting is what Tinline reads anyway.
  - An "or" divider, then "Their code" / **"Paste their code"** (it reads the clipboard and looks it up at once;
    any text containing a code works, core `find_card`). There is no paste textarea any more.
  - A reserved two-line hint below it (`min-block-size 38`), so the sheet never resizes.
- **QR**: my QR (176 in a 196 box) and "Let them scan this with Tinline." On a phone, "Scan their code" swaps
  the viewfinder into *the same box* and back ("Show my code"). Camera off: "The camera is off" + "Allow the camera".
- **Found / added:**
  - Scanned: chip "Scanned in person" + "You scanned it from their screen, so you know whose code this is."
  - Pasted: "Is this the {first} you expected? If you're not sure, check with them first."
  - Added: "{first} is in your contacts. Say hello." Footer: Done + "Message {first}".
- **Hints are not errors (tone, 2026-10-13).** The user read the old empty-clipboard line as *angry*: it was red,
  announced as an alert, opened with "Nothing" and gave two orders ("Copy their code first, then come back
  here"). Now:
  - Empty clipboard: "Your clipboard is empty right now. When their code arrives, copy it and paste it here."
  - No code in it: "That doesn't look like a Tinline code. Could they copy it again for you?"
  - Your own code: "That's your own code. Send it to them, and paste theirs here."
  - All of them are **muted ink, not red**, and `role="status"` (polite), not `alert`. Rule in `VOICE.md`:
    red only when something actually went wrong.
  - *Implementor:* Iced: use the muted text colour, never the danger token, for this line. Compose: no
    `isError` styling; use `LiveRegionMode.Polite` on the hint.
- *Implementor:* use `peek_card` for the preview (no dialling). The ticket changes after it is redeemed
  (core comment), so refresh "My code" when it does; don't cache the string in UI state.

## 6c. Dark-mode depth (decided 2026-10-13; compare at `?lab=dark`, zoom 2–3×)
- A shadow can't show on a near-black canvas, so dark mode gets depth from *light*.
- **Everyday screens: rim.** Raised surfaces are a lighter tone (as before) plus a 1px light edge along the top
  (`--rim`, 8% white, inset), as if lit from above. It lives *inside* the dark `--shadow-lift/pop/sheet`
  tokens, so every menu, dialog, sheet, floating button and switch thumb gets it with no per-component work.
- **No underglow anywhere, sign-up included (decided by the user 2026-10-13: "it isn't working").** It was tried on
  the first-time screens (a green light under the desktop card and behind the logo) and didn't read as depth. The rim
  is the one dark-mode depth everywhere. Remove any glow already ported.
- Light mode is unchanged.
- *Iced:* rim = a 1px line drawn just inside the top of the `shaped.rs` path. *Compose:* rim via `drawBehind`
  (`Modifier.depth()` gains a dark-mode rim).

## 6d. Sounds (a thoughtful toy, used sparingly)
- **Character:** Tinline is a thoughtful toy. Its sounds come from the brand: the *line* (a soft plucked string)
  and the *can* (a small struck tin), plus the existing call chimes. Mellow, never an alarm.
- **Only where a sound tells you something.** No sound on typing, steps, taps, copying or theme.

  | Moment | Cue | Sound | Level (vs Connected) |
  |---|---|---|---|
  | Done, a contact added | `line` | two plucks D5 → G5, soft G4 under, on the two tugs | equal |
  | Someone asks to add you | `knock` | two tin taps, the second a little lower | about −4 dB |
  | A message arrives (app open) | `received` | one tin tap | about −4 dB |
  | I send a message | `sent` | one high pluck, **faint** | about −13 dB |
  | Calls (ring, connected, ended…) | existing | unchanged | |

- **All generated, no sound files.** The tables are `PAT` in `playground/src/lib/fx/audio.ts` (frequency,
  start, length, peak, attack, instrument). Pluck = Karplus-Strong (noise burst, softened, in a delay one period
  long, averaged each pass, decay to −60 dB over the note). Tin = 5 inharmonic partials (×1, 1.51, 2.03, 2.47,
  3.31; higher ones die sooner) plus a 4ms tap. Each play is detuned by up to ±4 cents, so it never sounds stamped.
- One-shot cues **layer**; a message arriving mid-ring doesn't cut the ring.
- *Implementor:* one synth in Rust (`crates/audio`) used by both apps, so desktop and Android sound the same.
  Desktop plays it live on the call output. Android plays it live while the app is open; for sounds the
  **system** plays (notifications while the app is closed or the phone locked), generate the cue once to a small
  WAV with the same synth on first run and give that file to the notification channel (still generated, just
  kept). Quiet (Settings > Sounds) silences all of them.

## 6e. The first screen: one empty state (designed 2026-10-13)
A new account used to land on four different "nothing here" designs (blank Chats, "0 contacts", a phone icon on
Calls, "Choose a chat" with no chats). Now there is **one empty state** (`lib/ui/EmptyLine.svelte`) in two sizes,
and all its copy is decided in that one file.
- **Hero** (a new account's first screen): the mark at 88 with the faded-gold line (`lineColor`), a title,
  one line, one button. Where: Android **Chats** and **Calls** tabs; the desktop **detail pane** when there are
  no chats.
  - Nobody added yet: **"Your line is ready"** / "Add someone you'd like to talk to, and you can message and
    call each other." / **Add my first contact** (opens Add contact over this screen; no tab switch).
  - One contact, no chats: **"Your line is open"** / "Say hello to {first} whenever you like." / **Message {first}**.
  - Several contacts, no chats: "Your line is open" / "Pick someone and say hello whenever you like." / Start a chat.
  - **The story:** the line is slack while you have nobody; when your first contact arrives it **pulls tight in
    two tugs** (`LINE_SPRING`, the same pull as sign-up Done, now shared in `lib/brand/springs.ts`), and the words
    turn from "ready" to "open". This closes the sign-up story: Welcome slack → Done taut (your line works) →
    empty slack (nobody on it yet) → taut again when someone is.
  - **Sound:** the hero plays no cue itself. Adding a contact already plays `line` (people `addPerson`), so the
    sound and the tug land together. Don't play it twice.
  - **Nothing moves:** the title is one line, the body reserves two lines (`min-block-size 38`), the button has
    min width 220 (as the sign-up footer). Measured: the button stays at the same x, y and width from "ready"
    to "open" (Android 190,530 w220; desktop 916,534 w220).
- **Quiet** (an empty list in an app you already use): a small dimmed mark (40) and one muted line, no button.
  - Desktop Chats column (the pane has the hero): "Your chats show up here."
  - Unread filter with nothing unread: "You're all caught up."
  - Calls with contacts but no calls (and every desktop Calls column): "Calls you make and receive show up here."
  - Contacts with nobody: "The people you add show up here." It replaces "0 contacts". The Add contact row above
    is the action, so there's no second button.
  - Groups (text only, under its "Your groups" header): phone "When someone invites you to a group, it shows up
    here."; computer "Groups you start or join show up here."
- **Chats with no chats at all:** the All/Unread chips are hidden (filters mean nothing yet). On a phone the whole
  row goes, so the hero sits at the same height on Chats and Calls; on desktop the row stays for its + button.
- *Implementor (both):*
  - One function decides the copy from (contacts count, chats count, where, form). Port it as one function,
    not per-screen strings.
  - Keep **one mark instance** across "ready" → "open" so the state change animates. If the screen mounts
    already taut (e.g. on a phone you come back to Chats later), show it taut without replaying the tug.
  - Iced: `mark.rs` with `state` taut/slack and the `LINE_SPRING` params; the hero is a centred `column` with
    fixed-height text slots. Compose: `TinMark(state = …, spring = LineSpring)` in a `Column` with
    `Modifier.heightIn(min = …)` on the body text.
  - Don't port the old Calls empty (phone icon + "No calls yet") or "Choose a chat" when there are no chats.
- **Still open (not designed):** "New group · You'll be the owner" on a phone contradicts Done's "Groups are
  hosted on a computer"; the greyed call buttons in a new chat have no reason given; the empty chat still shows a
  shield ("Messages go straight to their phone").

## 6f. Adding one contact: both sides, from paste to the first call (2026-10-13)
Follow it live at `?lab=pair`: Maya (left, new account) and Tara (right, already on Tinline) in two linked windows.
Buttons above them play "the world" (Tara's device off, her code used by someone else, expired, Tara blocks Maya).
What the core does, and so what the UI must say (sources: `docs/protocol.md` "Add contact" and "Tickets in the core",
`crates/core/src/node.rs` `add_contact_from` / `accept_hello`, `caps.rs` `policy_for`):
- **On paste (no network):** `peek_card` finds the code in any text and reads the signed name, so the found screen can
  show "Tara Iyer" before anything is sent. It returns *None* for my own code, an expired code and junk alike.
- **On "Add Tara":** the app dials Tara's device (up to 30 s) and waits for her answer (up to 15 s). The button reads
  "Adding…" and the line under the name says "Reaching Tara's Tinline…"; after 2.5 s it becomes "Still trying. Tara's
  Tinline needs to be open and online for this." *Why:* 45 s of silence feels broken; saying what it needs is caring.
- **On Tara's side (core today):** her device accepts on its own, with no question, and only logs it.
  **Decided by the user 2026-10-13: it becomes a request.** Tara sees "Maya wants to add you as a contact" and
  chooses **Accept** or **Not now** (see "A request, with Accept" below). Core change: § 6f core ask 0b.
- **Both may call from the start** (`policy_for`: a ticket contact gets a call token both ways; a group-request
  contact is message-only). The chat's call buttons are therefore *enabled* for a code-added contact. (The earlier
  greyed buttons were a playground default, `pp.newContactsCall`, that only applies to requests now.)
- **The first message** arrives in Tara's Chats as a new chat from Maya with an unread badge. **The first call** rings
  on Tara's side like any other call, with Maya's name.

**A request, with Accept (decided with the user 2026-10-13).** Both sides see the same row: **me · the mark · them**
(`LineJoin.svelte`; I am always on the left). *Why a request:* codes are reusable for a week, so nobody should
become your contact without your yes.
| Moment | Maya (joiner) | Tara (owner) |
|---|---|---|
| Code recognised | line **slack**; "Is this the Tara you expected?…"; Back / Add Tara | (nothing) |
| Delivering (dial) | line **ringing**; "Reaching Tara's Tinline…" (then "Still trying…"); "Adding…" | (nothing yet) |
| Delivered, waiting | line **ringing**; "Request sent. Your line opens when Tara accepts."; **Done** | sheet **"Contact request"** opens by itself (not over a call or another dialog), the `request` knock plays; chip "Has your code"; "Maya wants to add you as a contact. Accepting lets you message and call each other."; **Not now / Accept**. Also a card in Contacts › "Waiting for you". |
| Accepted | line **taut**, two tugs, `line` cue: "Tara accepted. Your line to Tara is open. Say hello." (in the open sheet, or the sheet reopens by itself); Done / Message Tara | line **taut** at the same moment, `line` cue: "Your line to Maya is open. Say hello."; Done / Message Maya |
| Not now | nothing is sent; the request just waits (Contacts › "Waiting for them": "Request sent. Your line opens when Tara accepts.") | the request stays in Contacts › "Waiting for you" to answer later |
- **Ringing** is the call "connecting" sway (period 1.2 s, 6°, `status.svelte.ts`): "being connected" looks the same
  everywhere. **Taut** is `LINE_SPRING` (as sign-up Done and the empty state).
- While waiting, Maya's empty first screen says: "Your line is ready / Your request to Tara is waiting. Your line opens
  when they accept. / Add another contact".
- *Why no "declined":* "Not now" telling the joiner nothing is kinder to both, and doesn't reveal that Tara saw it.
  (Open: should an unanswered request expire with the code, after 7 days?)
- *Core ask 0b (needed):* the owner's device must **hold** the hello as a pending request (not accept it), raise an event
  ("request from Maya", name + DID), and on Accept send the Welcome back by dialling Maya (her hello carries her device
  and relay). The joiner's `add_contact` then returns "delivered" instead of a contact, and a later event says
  "accepted". The core already has a request/accept pattern for group chat requests (`req_tickets`), which may be reused.
  Acceptance tests: hello → pending on owner, nothing stored as contact; Accept → both have each other with call
  tokens; Not now → nothing sent; replayed hello → one request only; blocked joiner → refused as today.
- *Implementor:* one mark instance across slack → ringing → taut; the row is fixed 64 high; text slot reserved; the
  tab row is hidden but keeps its height when the sheet becomes a request, so nothing jumps.

**What the joiner sees when it doesn't work.** All muted, in the line under the name (three lines reserved, so the
face never moves), with a matching button:
| Core result | Line | Button |
|---|---|---|
| `Error::Timeout` | Couldn't reach Tara's Tinline. It needs to be open and online, so try again when Tara's around. | Try again |
| `Rejected("refused")` | Tara's code isn't valid any more. They may have made a new one, so ask Tara for their newest code. | Paste a new code |
| `Rejected("blocked")` | You can't add Tara right now. They've chosen not to be in touch here. | Close |
| my device offline (checked before dialling) | You're offline. Once you're connected, try again. | Try again |
| expired (on paste) | That code from Tara has expired. Codes last 7 days, so ask them for a fresh one. | (paste hint) |
- **Codes are reusable for 7 days (decided by the user 2026-10-13: "taking a code every time would be annoying").**
  "Your code" says: "Send it to anyone you'd like to talk to, in any app. It works until Mon 19 Oct." A quiet
  **New code** on the "Your code" line replaces it (core `reset_ticket`): the old one stops working at once.
  - *What makes reuse acceptable:* anyone holding the code can add you for a week without being asked, so (1) New
    code lets you cut an old one off, (2) every addition shows you the **New contact** sheet (needs the core events
    below), (3) you can remove or block them afterwards.
  - *Core today is single use* (`accept_hello` drops the ticket; the redeemed-nonce set refuses a second joiner).
    Until the core changes, the apps must keep the honest single-use line; don't ship "works until" on a
    single-use core.
- *Why "isn't valid any more":* the core's "refused" can't say why (they made a new code, changed their nickname, which
  re-signs the code, or the code was used up on today's core). "Isn't valid any more" is true in every case.

**Core asks (proposed, not decided; for the core owner):**
0. **Reusable codes** (user decision): a ticket stays redeemable by any number of joiners until `exp` (7 days) or
   `reset_ticket`. Redeeming no longer drops it; replay protection moves from "nonce spent" to "this joiner DID already
   redeemed this nonce" (the `binding` already ties the hello to the joiner's DID and device). Decide whether
   `set_name` should still drop the ticket (it re-signs the name; suggest: mint a new one but keep the old valid until
   its `exp`). Acceptance tests: two different joiners both succeed with one ticket; the same hello replayed is
   refused; a reset ticket is refused; an expired one is refused.
1. `peek_card` → a reason instead of `None` (not a code / my own / expired, with the signed name when readable), so
   the paste hint can be specific.
2. (Covered by 0b: the request event. Also a system notification for it when Tinline is in the background:
   "Maya wants to add you as a contact".)
3. Maybe: say on the found screen that adding needs Tara's device online ("Tara's Tinline needs to be on for this"),
   or queue the hello and retry. Today a code is only usable while both are online at once.

*Implementor:* keep the found-screen line in a fixed three-line slot; the button label sits in a fixed-width box
("Add Tara" / "Adding…" / "Try again"). Map errors by `Error` variant, never by display string. Desktop's current
"Couldn't add / Both computers need Tinline open…" screen becomes the table above.

## 6g. In a call: signal and path (decided with the user 2026-10-13)
Try it at `?lab=pair` (call from Maya, answer on Tara) or any call; the drawer's Calls › Network sets the signal,
the path and "Never finds a direct path". Playground: `lib/calls/link.svelte.ts`, `lib/calls/SignalBars.svelte`,
`lib/calls/CallScreen.svelte`.
- **Source:** core `CallStats { direct, rtt_ms, received, lost, reconnecting }` (`call_stats()`, poll ~1 s).
  `direct` = the selected iroh path `is_ip()`. Nothing new needed from the core.
- **Signal = four bars, top right, where the "i" was.** Tapping them opens About this call (its label says
  "About this call. Strong signal, direct."). Levels use the desktop's existing `quality()` thresholds so all apps
  agree: **Strong** rtt < 100 ms and loss < 1 % · **Good** < 200 / < 3 % · **Fair** < 350 / < 8 % · **Weak**.
  Compute loss over the last few seconds (not the whole call), or one bad minute colours the rest.
  - Bars: 24 box, 3 wide at x 3.5/8.5/13.5/18.5, heights 6/10/14/18 on y 20, radius 1.5; unlit bars stay as faint
    ghosts (ink 28 %) so the glyph never changes size; changes fade 300 ms.
  - Before connection: all ghosts. Reconnecting: a slow wave through the ghosts (1.6 s, 160 ms apart), none lit.
- **Path, on the main screen:** the line under the timer reads "End-to-end encrypted · Direct" or "· Relayed".
  *Why on the main screen:* it's honest, and the card explains it. A call normally starts relayed and turns direct
  a second or two later when the devices find each other; both ends share one connection, so both always agree.
- **About this call** adds one row: bars + "Strong signal · Direct" and one line:
  - direct: "Your call goes straight between your device and {first}'s."
  - relayed: "Your devices can't reach each other directly right now, so a relay passes the call along. It can't
    listen." (video: "listen or watch")
  - reconnecting: "The connection dropped for a moment. Tinline is finding a way back to {first}."
  - weak adds: "The line is weak, so {first}'s voice may break up."
  - then small numbers for the curious: "42 ms round trip · 0.2% lost".
  - The text block reserves title + four lines, so the card never grows when the wording changes.
- **A weak line is not announced on the main screen** (no banner, no word under the timer): the bars and the card
  are enough. *Why:* nothing alarming during a conversation.
- *Iced:* replace the "i" button in `call_screen.rs` with a canvas of the bars; the existing `call_about` already
  prints "Direct · Good": restyle it into the row above. *Compose:* a `Canvas` for the bars; read `CallStats` in the
  call ViewModel's existing poll.
- Desktop call marks keep the new slight bounce (default recoil), decided 2026-10-13.

## 6h. Group calls: everyone's line (decided with the user 2026-10-13)
Try it at `?lab=group`: Maya (phone), Arjun (laptop, the group's call host) and Sam (phone) in Weekend hike. The
lab's "line" buttons step each person Strong → Good → Fair → Weak; "talks" makes someone speak in every window.
Playground: `lib/calls/glink.svelte.ts`, `GroupCallScreen.svelte` (`SignalBars` from § 6g).
- **In a group call everyone connects only to the call host**, so "someone's signal" means their line to the host.
- **Mine, top right:** the § 6g bars (my line to the host); tap = **About this call**, which now starts with my row:
  "Good signal · Direct / Your line to Arjun's laptop is direct." (relayed: "…goes through a relay. It can't
  listen."; weak adds "It's weak, so you may break up for the others."). Text block reserves title + three lines.
  On the host's own screen there is no line to measure: the header keeps the "i".
- **Everyone else, on their tile:** a 16 px bars glyph right after the first name, in a fixed slot (ink 70 %;
  weak = full-strength gold ink, never red). The host's tile has none (it is the hub). Strength only: no path and no
  numbers for other people. *Why:* enough to explain "Sam keeps breaking up" without exposing how others connect.
  Screen readers: the tile label ends ", weak signal".
- Bars show all ghosts until connected; my own tile and header wave while reconnecting.
- **Core ask (proposed, needed for other people's bars):** today a client knows only its own connection to the
  host (`ClientStatus` has the roster and who is forwarded, no link quality). Proposal: the host measures each
  client's connection (rtt + recent loss on its iroh connection, the § 6g thresholds) and sends a small `Lines`
  message every ~2 s, `{ slot: u8 → strength 1..4 }`, on the existing call stream; clients expose it on
  `ClientStatus` as `lines: BTreeMap<u8, u8>`. Mine comes from my own connection (no message needed). A missing or
  stale entry (> 6 s) shows ghosts, not a guess. Acceptance tests: a client with injected loss shows strength ≤ 2 on
  the others' status; slots that leave drop from `lines`; an old client ignoring `Lines` still works.
- *Iced / Compose:* reuse the § 6g bars at 16; the tile's name row becomes name + glyph with a 4 px gap.

## 7. Transitions

- Between steps the header and footer stay still. Only the step content moves: it slides 36px and
  fades, cubic ease-out, `enter` 200ms and `exit` 140ms; back slides the other way; RTL mirrors.
  With reduced motion it is a cut.
- **Proposed, not built:** launch → Welcome with the line settling into its slack curve; the progress drawn
  as one thin line filling; the large Mark shrinking into the header Mark. Don't build these until they
  are decided.

## 8. Not to port

The keyboard mock, `?step=` / `?gate=chat` / `?depth=` switches, the `?lab=dark` depth lab, the `?lab=pair` two-window simulation (`lib/sim/pair.svelte.ts`), the tin icons trial (`?icons=`; still a study, port from `icons.ts`), the Simulation drawer, the fake QR or camera,
and the demo phrase. The **conversational sign-up** (`?gate=chat`, `ChatGate.svelte`) is a PROPOSAL and uses
the same copy. Port the stepped flow.
