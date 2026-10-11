# Chat: refined conversation design

Prototype: [chat.html](./chat.html). Back to [Home](./home.html). Phone layout from a desktop browser: [mobile.html?screen=chat](./mobile.html?screen=chat&chat=arjun).
Boards it refines: `AndroidChats`, `AndroidConversation`, `AndroidChatFiles`, `AndroidSettings`, `DesktopChat`. Tokens come from [tokens.css](./tokens.css); the binding component rules are in [SPEC.md](./SPEC.md).

The page is an interactive design study with simulated data: no network, nothing is sent. That disclaimer, and every scenario control, sits in the Simulation panel (sliders icon in the harness bar, closed by default). Nothing inside the app frame talks about simulation.

## Harness and embed

- One slim bar above the app: `← Home · Chat`, a "Phone preview ↗" link (opens `mobile.html` for the open chat), an icon-only theme toggle and an icon-only Simulation toggle.
- `?chat=<id>` opens that conversation (`arjun`, `meera`, `dev`, `sana`; anything else falls back to the list). `&from=home` makes Back return to `home.html#chats`.
- `?embed=1` removes the bar and the panel and lets the app fill the frame with no border or radius. It is used by `mobile.html` (phone bezel) and by the desktop detail pane in `home.html`. `?theme=light|dark` and `?motion=reduce` are honoured so the embedding page can keep themes in sync.

## What the prototype covers

| Area | Behaviour shown |
|---|---|
| Chat list | Sorted by latest activity; unread `.t-badge` (muted variant plus a bell-slash glyph before the time for muted chats); `Draft:` preview; `Not delivered ·` for a failed last message; clock or check glyph before your own last message; search over names and messages on this phone |
| Empty states | Search with no match ("No chats match “…”" / "Search covers names and messages on this phone." / Clear search); no chats at all ("No chats yet" / "Messages stay on your devices." / Start a chat); empty conversation ("Say hello to {name}" / "Messages go straight to their phone."); desktop with no chat chosen ("Choose a chat" / "Messages stay on your devices.") |
| Bubbles | One system shared with `groups.html` (see below): received white with a hairline border, sent soft mint with dark ink; time, `edited` and the status glyph sit inline at the end of the last line |
| Compose | Attach (paperclip) · auto-growing field (up to 140 px) · paper-plane Send. Empty draft shows Send at reduced opacity and disabled; it is never hidden. Enter sends, Shift+Enter adds a line on fine pointers; on touch the default flips (Enter adds a line, Ctrl/⌘+Enter sends). The setting is in Chat settings. IME composition never sends. No hint row under the composer |
| Drafts | Kept per chat while you switch, shown in the list, cleared on send. Reply or edit context is per chat too; editing stashes the draft and restores it on save or cancel |
| Send states | Clock glyph while sending or waiting for the contact to be online, check glyph once delivered. Each glyph carries a visually hidden label ("Sending", "Waiting for {name} to be online", "Delivered"). Failed: a compact `Not delivered · Retry` row below the bubble |
| Message menu | On demand only, no persistent per-message buttons. Touch or narrow screens: long-press (500 ms, cancelled by a 10 px move or scroll) or the context-menu event opens a bottom sheet. Fine pointer at 860 px and wider: a chevron appears beside the bubble on hover or focus and opens an anchored popover; right-click opens the same popover. Every message also has a visually hidden, focusable menu button for keyboard and screen-reader users. Actions: Reply, Copy, Edit (own text), Delete (own), Message info |
| Message info | Sent or received time, edit time, and per-device delivery rows ("Arjun’s phone · Delivered 12:40", "Sana’s laptop · Waiting · laptop offline"). Delivery detail lives here, not in the bubble |
| Delete (proposal) | Per-message deletion is not a decided feature; the Simulation panel says so. Dialog "Delete message?" with Cancel focused first, "Delete for me", and "Delete for everyone" (asks the other phone to remove it; copies elsewhere can’t be recalled). Offline wording says it reaches their phone when you’re both online. Failed or unfinished sends offer only "Delete for me" ("It was never delivered, so it’s only on this phone."). Whole-chat deletion does not exist (U24) |
| Attachments | Sample or real file (only name and size are read). Progress bar, Cancel, waiting when offline, Retry from the start. Name is `dir=auto` and ellipsised |
| Offline and linked devices | Thin strip under the header ("Meera is offline. Messages send when you’re both online."), presence line in the header ("Online", "Offline", "Last seen 17:42"), Sana’s laptop-offline strip, chat details with devices and a link to `privacy.html` |
| In-conversation call | Phone button in the header opens a silent in-page call overlay; see [In-conversation calls](#in-conversation-calls) |
| Settings sheet | Notification preview, message sound with audible Play (synthesised tones), vibrate, Enter key, appearance (match device, light, dark) |
| Simulation panel | Contact online (per open chat), next send fails, simulate incoming message, empty the chat list, reset demo |

## Bubbles and grouping

- Received: `--bub-in` fill, hairline border. Sent: `--bub-out` soft mint with `--bub-out-ink`, no border. Never solid teal. Conversation background is `--canvas`.
- Meta (`edited`, time, status glyph) floats right at the end of the last text line and drops to its own short line only when it does not fit.
- Messages from the same author on the same day within 5 minutes form a group: 2 px apart (10 px between groups), with the stack-facing corners tightened.
- Reply quote inside the bubble: accent left bar on `--surface-2`. Emoji-only messages (1 to 3 emoji) have no fill and larger glyphs.
- A new own bubble rises 4 px over 120 ms; the clock swaps to a check without layout shift. A floating scroll-to-latest button with a count appears when a message arrives while you are scrolled up.

## In-conversation calls

The header phone button (between the name block and the info button) starts a call without leaving the conversation. Calls here are silent visual simulations; [calls.html](./calls.html) is the only page that makes sound.

- Overlay fills the app frame (or the iframe in embed mode): 96 px avatar, name, "Ringing…", then "Connected" after about 2.5 s with an MM:SS timer. Controls: Mute (toggle, `aria-pressed`) and a solid danger End button.
- The down-chevron at the top left ("Back to chat — call continues") or Escape minimises the call to a 40 px accent strip above the headers: "● {name} · MM:SS". The strip stays visible in the list and in any conversation. Tapping it returns to the overlay. Chatting and switching chats work while minimised.
- While a call is live, the header phone button (and the "Voice call" row in chat details, which becomes "Return to call") returns to the call; it never starts a second one.
- End closes the overlay and strip, shows the toast "Call ended" and returns focus to the phone button.
- Message-only contact: Dev Patel. His call button is disabled but focusable; activating it opens chat details, which shows "Messages only — calling needs their permission" with a "Request calling" button that becomes "Requested". There is no banner in the conversation.

## Delivery language (no misleading receipts)

Tinline has direct device-to-device delivery. The only fact the sender can know is that the message reached the other phone, so the UI says exactly that.

- Never "Read", "Seen", double blue ticks or typing indicators. Chat details says "A check means it reached {name}’s phone. Nobody is told when a message is read." Message info repeats it.
- Every own message shows its status glyph in the inline meta. There is no delivery text in the bubble; per-device detail is in Message info.
- Glyphs are `aria-hidden`; a visually hidden text label beside each one carries the status for assistive technology.
- "Delivered" on a linked-device contact means at least one of their devices has it. Message info lists which devices are still waiting.

## Sound and interaction states

| Event | Sound | Notification preview | Badge |
|---|---|---|---|
| Message in a non-open, unmuted chat | per setting (chime, pop, silent) | per setting: name and message, name only, hidden ("New message") | yes |
| Message in the open chat | none; announced in the live region | none | none |
| Message in a muted chat | none | none | quiet badge |
| Message sent or delivered | none (no whoosh) | n/a | n/a |

- Tones play only from a user action (Play, choosing a sound, the incoming simulation), which also satisfies browser autoplay rules. Short (under 1 s), low volume, no looping. If audio is unavailable the UI says so in text.
- Vibration is a checkbox; browsers that cannot vibrate say they cannot preview it instead of pretending.
- Calls are separate from chat sound. The settings sheet says "Muted chats never make a sound. Calls ring separately."
- Toasts are polite live-region messages, auto dismiss after about 4 s (simulated notifications after 6 s), and never carry the only copy of important information (every failure also stays visible in the thread).
- Reduced motion (system setting or `data-motion=reduce`) removes animations, the long-press press scale and smooth scrolling.

## Edge cases handled

- Send while offline: message shows the clock glyph, flushes in order when the contact returns.
- "Next send fails": message shows `Not delivered · Retry`; the first retry succeeds. Files fail around 50% and restart from the beginning (resume is a proposed later improvement).
- Cancel a transfer: the row reads `Cancelled · Retry`, a toast says nothing was left on the other phone, and focus moves to Retry.
- Delete a message that is the target of a reply or edit: the reply or edit strip clears safely (an edit restores the stashed draft).
- Edit while offline: the bubble shows `edited` and a clock glyph; a toast says the edit reaches {name} when you’re both online; Message info shows the edit time and that it reaches them when online.
- (Proposal) Delete for everyone while offline: tombstone "You deleted this message" with a clock glyph until the contact is back.
- (Proposal) Delete for me removes the row from this phone; the dialog says copies elsewhere cannot be recalled.
- Policy: U24 means no "Delete chat" action; chat history is kept, and only media/files may be deleted to free space. U20 contact-deletion requests remove the contact entry only, never history. A blocked contact stays as a chat. Per-message deletion needs a user decision; if rejected, restrict Delete to local media.
- Delete on a failed message removes it from this phone and says it was never delivered.
- Long names, long unbroken text and bidi file names wrap or ellipsise rather than overflow.
- Search looks only at what is on this phone and says so when empty.
- Dialogs: native `<dialog>`, Escape closes, focus returns to the opener (or to the message’s menu button if the opener was re-rendered), backdrop click closes. On mobile they are bottom sheets.
- Back on mobile returns to the list and focuses the row you came from (or to Home when opened with `from=home`). Escape outside the field does the same on phones.
- Dark mode follows the device by default; the header toggle and the Appearance setting override it. Choices persist in `localStorage` in this prototype only.

## Accessibility notes

- Message list is `role="log"` with a label; each message has a menu button whose name includes author, time and a text excerpt ("Actions for Arjun’s message at 12:44: …").
- Radio groups are real `fieldset`/`legend`; the Simulation switches are `role="switch"` with `aria-checked`.
- Targets are at least 44 px; focus ring is a 2 px `--accent-ink` outline; status never relies on colour alone.
- Progress uses `role="progressbar"` with `aria-valuenow`.
- The call overlay is `role="dialog"`; focus moves to it on open, to the strip on minimise, and back to the phone button on End.

## Audit: overlooked daily flows (not built in the interactive UI)

Findings from reading Main, Flow, the Android and desktop boards, onboarding, linking and accounts. Proposed details are deliberately short; most need a product decision before a board.

| Gap | Observation | Proposed detail |
|---|---|---|
| Conversation search | The chats search only matches names and previews; nothing finds a message inside a long chat | In-chat search (desktop Ctrl+F, Android overflow); results list with jump; same "only on this phone" wording |
| Read-receipt language | Boards say "on Arjun’s phone 12:42", which is correct but unnamed as a rule | Adopt the delivery-only wording above everywhere, including desktop and notifications |
| Failed send | Boards show Waiting but no Not sent / Retry for text; only files show retry in the Save-only board | Use the retry pattern here; also define when Waiting turns into Not delivered (proposed: never time out, only on an explicit error such as a changed identity) |
| Draft persistence | Not drawn | Per-chat drafts including across restarts, shown in list; wiped by account removal |
| Notification content | AndroidSettings has Background and battery only; no preview or sound preference | Add this settings group with a lock-screen-safe default of "Name only" for new installs |
| Per-chat mute | Missing | Chat details toggle; duration options (1 h, 8 h, always) as a later addition |
| Message sound | No decision on the sound identity | One short soft chime, no custom tones at launch; silent option; respect system Do Not Disturb |
| Linked-device offline | Linking boards explain linking, not what a chat looks like when one device lags | Strip "{name}’s laptop is offline"; catch-up wording from this prototype; link to privacy page |
| Unlink is not revocation | Per the device-linking decision | Keep the distinction visible in chat details when a linked device exists: "Unlinking a device does not remove what it already received" (copy to be confirmed with the privacy page) |
| Contact identity change | Group board covers address change; 1:1 chat has no inline notice | System line in the thread "Arjun’s device changed" with Verify; sending paused until acknowledged only if the identity key changed |
| Blocked or unknown sender | Groups boards cover requests | 1:1 chat from a stranger uses the same request card, composer disabled |
| Quote and edit history | Edited label exists, no way to see the original | Not needed at launch; keep "edited" only. Document that originals are not retained |
| Attachments | Cancel is designed only; large-file wait limit is not in any settings screen | Settings row for the auto-download limit; copy "Files over 25 MB wait for you to tap" |
| Onboarding | Does not mention that history is per device and not recoverable from the phrase | One line on the passphrase or recovery step: "Your recovery phrase restores your account, not your messages" |
| Account switch | Only one account is online per install | Chat list header shows the account name; switching clears the open chat and keeps per-account drafts |
| Desktop | Hover-only actions need a keyboard path | Built: the chevron appears on focus, Enter opens the same menu; Esc cancels reply or edit |
| Empty states | First chat empty and no-match were not drawn | See the prototype strings |
| Long-press | Android action sheet is long-press only | Keep long-press. The original proposal to keep a visible `…` is superseded by SPEC.md (no persistent per-message button); the hidden focusable menu button and the desktop chevron cover keyboard and screen-reader access |

## Open questions

0. Is per-message deletion wanted at all, or only local media deletion (U24)? Everything marked proposal above depends on this.
1. Should "Delete for everyone" be available after a time window? The prototype leaves it unrestricted, as in the existing board.
2. Is the waiting state ever shown as a count in the list ("2 waiting") when several are queued?
3. Does muting apply per account or per contact when the same contact exists in two accounts?

## Mobile layout note

At phone widths the open conversation fills the whole viewport: header with the Back button, scrolling messages, composer pinned to the bottom with safe-area padding. Back returns to the list with focus on the chat that was opened, and its unsent draft is kept. The list view scrolls as a normal page. The harness bar and Simulation panel are hidden while a conversation is open and return with the list.
