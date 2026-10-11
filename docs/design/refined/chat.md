# Chat: refined conversation design

Prototype: [chat.html](./chat.html) (interactive design · simulated data, no network, nothing sent). Back to the [index](./index.html).
Boards it refines: `AndroidChats`, `AndroidConversation`, `AndroidChatFiles`, `AndroidSettings`, `DesktopChat`. Tokens come from `Main.dc.html`.

## What the prototype covers

| Area | Behaviour shown |
|---|---|
| Chat list | Sorted by latest activity; unread badge (quiet grey when muted); `Draft:` preview in teal; `Not sent` in red; search over names and local messages |
| Empty states | Search with no match (explains only on-phone messages are searched, offers Clear); no chats at all (encryption line, Start a chat); empty conversation; desktop with no chat chosen |
| Compose | Auto-growing field; Enter sends, Shift+Enter newline (desktop default). On touch the default flips: Enter adds a line, Ctrl/⌘+Enter sends. Setting in the sheet. IME composition never sends. Empty send is announced, not silent |
| Drafts | Kept per chat while you switch, shown in the list, cleared on send. Reply or edit context is per chat too; editing stashes the draft and restores it on save or cancel |
| Send states | Sending, Delivered to {name}’s phone, Waiting for {name} to be online, Not sent + Retry/Remove |
| Message actions | `…` per message (hover or focus on desktop, always visible on touch): Reply, Copy, Edit (own text), Delete (Proposed, see below) |
| Delete (Proposed) | Per-message deletion is NOT a decided feature; the sheet is labelled Proposed and shows two outcomes. Default focus is “Keep message”. “Delete for both” is a request to the other phone, never a guarantee (modified apps, backups and screenshots keep copies); hidden on failed (never sent) messages; offline wording says it is pending. Whole-chat deletion does not exist (U24) |
| Attachments | Sample or real file (only name and size read). Progress, Cancel, waiting when offline, Retry from start, Remove. Name is `dir=auto` and ellipsised |
| Offline and linked devices | Offline banner, header status, chat details with devices and delivery explanation, link to `privacy.html`; Sana’s laptop-offline note |
| Settings sheet | Notification preview, message sound with real audible Play (synthesised tones), vibrate, Enter key, appearance (system, light, dark) |
| Simulation panel | Contact online, next send fails, incoming message, empty the list, reset |

## Delivery language (no misleading receipts)

Tinline has direct device-to-device delivery. The only fact the sender can know is that the message reached the other phone, so the UI says exactly that.

- Never “Read”, “Seen”, double blue ticks or typing indicators. The chat details sheet says so in one sentence.
- Status is shown only where it carries information: the latest own message, or anything not yet delivered. Old delivered messages show just the time.
- Text always accompanies the icon. Icons are `aria-hidden`.
- “Delivered” on a linked-device contact means at least one of their devices has it. Details names which devices are offline.

## Sound and interaction states

| Event | Sound | Notification preview | Badge |
|---|---|---|---|
| Message in a non-open, unmuted chat | per setting (chime, pop, silent) | per setting: name and message, name only, hidden (“New message”) | yes |
| Message in the open chat | none; announced in the live region | none | none |
| Message in a muted chat | none | none | quiet badge |
| Message sent or delivered | none (no whoosh) | n/a | n/a |

- Tones play only from a user action (Play, choosing a sound, the incoming simulation), which also satisfies browser autoplay rules. Short (under 1 s), low volume, no looping. If audio is unavailable the UI says so in text.
- Vibration is a checkbox; browsers that cannot vibrate say they cannot preview it instead of pretending.
- Calls are separate from chat sound (they ring per Background and battery). The sheet states it so nobody expects chat settings to silence a ring.
- Toasts are polite live-region messages, auto dismiss after about 4 s, never carry the only copy of important information (every failure also stays visible in the thread).
- Reduced motion removes toast pop, progress easing and smooth scrolling.

## Edge cases handled

- Send while offline: message goes to “Waiting”, flushes in order when the contact returns.
- “Next send fails”: message becomes Not sent with Retry and Remove; the first retry succeeds. Files fail around 50% and restart from the beginning (resume is a proposed later improvement).
- Cancel a transfer: becomes “Cancelled. Not sent.” with Retry and Remove. Focus moves to Retry.
- Delete a message that is the target of a reply or edit: the reply or edit strip clears safely (an edit restores the stashed draft).
- Edit while offline: saved locally, labelled “edited · waiting to reach {name}”.
- (Proposed) Delete for both while offline: tombstone “You deleted this message” with “Waiting to reach {name}’s phone”.
- (Proposed) Deleting only for me removes the row; both options state that a saved copy or screenshot cannot be recalled.
- Policy: U24 means no “Delete chat” action; chat history is kept, and only media/files may be deleted to free space. U20 contact-deletion requests remove the contact entry only, never history. A blocked contact stays as a chat. Per-message deletion needs a user decision; if rejected, restrict Delete to local media.
- Remove on a failed message removes it from this phone and says it was never sent.
- Long names, long unbroken text and bidi file names wrap or ellipsise rather than overflow.
- Search looks only at what is on this phone and says so when empty.
- Dialogs: native `<dialog>`, Escape closes, focus returns to the opener (or to the nearest equivalent if the opener was re-rendered), backdrop click closes. On mobile they are bottom sheets.
- Back on mobile returns to the list and focuses the row you came from.
- Dark mode follows the device by default; the header toggle and the Appearance setting override it. Choices persist in `localStorage` in this prototype only.

## Accessibility notes

- Message list is `role="log"` with a label; each message has a “…” button whose name includes author, time and a text excerpt.
- Radio groups are real `fieldset`/`legend`; the two switches are `role="switch"` with `aria-checked`.
- Targets are at least 44 px, focus ring is a 3 px teal-text outline, status never relies on colour alone.
- Progress uses `role="progressbar"` with `aria-valuenow`.

## Audit: overlooked daily flows (not built in the interactive UI)

Findings from reading Main, Flow, the Android and desktop boards, onboarding, linking and accounts. Proposed details are deliberately short; most need a product decision before a board.

| Gap | Observation | Proposed detail |
|---|---|---|
| Conversation search | The chats search only matches names and previews; nothing finds a message inside a long chat | In-chat search (desktop Ctrl+F, Android overflow); results list with jump; same “only on this phone” wording |
| Read-receipt language | Boards say “on Arjun’s phone 12:42”, which is correct but unnamed as a rule | Adopt the delivery-only wording above everywhere, including desktop and notifications |
| Failed send | Boards show Waiting but no Not sent / Retry for text; only files show retry in the Save-only board | Use the retry pattern here; also define when Waiting turns into Not sent (proposed: never time out, only on an explicit error such as a changed identity) |
| Draft persistence | Not drawn | Per-chat drafts including across restarts, shown in list; wiped by account removal |
| Notification content | AndroidSettings has Background and battery only; no preview or sound preference | Add this settings group with a lock-screen-safe default of “Name only” for new installs |
| Per-chat mute | Missing | Chat details toggle; duration options (1 h, 8 h, always) as a later addition |
| Message sound | No decision on the sound identity | One short soft chime, no custom tones at launch; silent option; respect system Do Not Disturb |
| Linked-device offline | Linking boards explain linking, not what a chat looks like when one device lags | Header detail “Laptop offline”; catch-up wording from this prototype; link to privacy page |
| Unlink is not revocation | Per the device-linking decision | Keep the distinction visible in chat details when a linked device exists: “Unlinking a device does not remove what it already received” (copy to be confirmed with the privacy page) |
| Contact identity change | Group board covers address change; 1:1 chat has no inline notice | System line in the thread “Arjun’s device changed” with Verify; sending paused until acknowledged only if the identity key changed |
| Blocked or unknown sender | Groups boards cover requests | 1:1 chat from a stranger uses the same request card, composer disabled |
| Quote and edit history | Edited label exists, no way to see the original | Not needed at launch; keep “edited” only. Document that originals are not retained |
| Attachments | Cancel is designed only; large-file wait limit is not in any settings screen | Settings row for the auto-download limit; copy “Files over 25 MB wait for you to tap” |
| Onboarding | Does not mention that history is per device and not recoverable from the phrase | One line on the passphrase or recovery step: “Your recovery phrase restores your account, not your messages” |
| Account switch | Only one account is online per install | Chat list header shows the account name; switching clears the open chat and keeps per-account drafts |
| Desktop | Hover-only actions need a keyboard path | `…` appears on focus, Enter opens the same sheet; Esc cancels reply or edit |
| Empty states | First chat empty and no-match were not drawn | See the prototype strings |
| Long-press | Android action sheet is long-press only | Keep, and keep the visible `…` for accessibility and discoverability |

## Open questions

0. Is per-message deletion wanted at all, or only local media deletion (U24)? Everything marked Proposed above depends on this.
1. Should “Delete for both” be available after a time window? The prototype leaves it unrestricted, as in the existing board.
2. Is the Waiting state ever shown as a count in the list (“2 waiting”) when several are queued?
3. Does muting apply per account or per contact when the same contact exists in two accounts?

## Mobile layout note

At phone widths the open conversation fills the whole viewport: sticky header with the Back button, scrolling messages, composer pinned to the bottom with safe-area padding. Back returns to the list with focus on the chat that was opened, and its unsent draft is kept. The list view scrolls as a normal page. The design-page chrome (title, simulation controls) is hidden while a conversation is open and returns with the list.
