# Groups: refined interaction spec

Prototype: [groups.html](./groups.html) (self-contained, no network). Everything in it is simulated; no
step claims a real send, join, block or removal. Replies from other people are triggered by hand
in the Simulation drawer (sliders icon in the harness bar, closed by default), which also holds the
disclaimer; nothing inside the app frame talks about simulation. Rules come from
`rotation-open-decisions.md` section 0 (U3, U7, U9, U11-U15, U19, U20, U21); anything not decided
there is marked *proposal* or *open*. Shared component rules: [SPEC.md](./SPEC.md).

## Harness and embed

- One slim bar: `← Home · Groups`, "Phone preview ↗" (shown at 860 px and wider; opens
  `mobile.html?screen=groups` for the open group), icon-only theme toggle, icon-only Simulation toggle.
- `?group=<id>` (validated: lowercase id of a known group) opens that conversation, e.g.
  `groups.html?group=hike`; invalid values fall back to the list. `&from=home` makes Back return to Home.
- `?embed=1` hides the bar and drawer and fills the frame. With a group id it shows one locked
  conversation (no Back, no list) at any width; this is what `home.html`’s desktop detail pane and
  `mobile.html` load.

## What the prototype covers

| Flow | Where | Notes |
|---|---|---|
| Browse, search, empty | Groups list | On desktop (900 px and wider) it opens on Weekend hike. Invitations and Chat requests are compact folded rows with `.t-badge` counts at the top of the list (expanded by default only when there are no groups); then Groups and Private chats; Sent requests is a quiet fold at the bottom. Search filters group names and private chats only (not message text). "Start with an empty account" in Simulation shows the empty state. On phones New group is a 56 px FAB. |
| Create group | New group sheet | Name required; contacts only; host choice ("My desktop" or "Ask a member later"). Closing keeps the draft. |
| Invitations | Invitations fold | Review opens the join dialog with the consent lines (the group’s hosts can read everything shared in it; hosts see your group address and when you’re online; members see name and messages, never your address). Join, Not now (undo), expired row (Dismiss only). |
| Private chat request | Member sheet → Message privately | Pending, accepted, declined, expired, cancelled. Simulated answers. |
| Incoming request | Chat requests fold | Review dialog: Accept / Decline / Block (confirm). Never rings. |
| Group conversation | Conversation pane | Same bubble, composer and message-menu system as `chat.html` (below). Drafts, waiting vs delivered, members sheet. |
| Group call | Header phone button | Silent in-page call overlay with minimise-to-strip; see **Group call (in page)** below. |
| Flag / removal | Member sheet → Flag to remove | 3 flags remove. Late "written before removal" message. Removed view with "Why you were removed". |
| Leave | Members → Leave group | Confirm dialog, Cancel focused. There is no delete: a left or removed group stays read-only (U24, chats are never deleted). |
| No host reachable | Simulation → "Weekend hike: host desktops offline" | Thin strip under the header, Try again, automatic flush when a host returns. |

## Rules represented (and what is not invented)

- **Roles:** Owner, Admin, Host as pills; a member can hold several. There is no moderator tier
  beyond these. Admins see per-member flag counts; others do not.
- **Removal (U9, U15):** any 3 flags remove, not necessarily from 3 admins; the same admin may flag
  again. Flags are final and visible to the flagged member. Removal is announced to the removed
  member and to the group as one neutral line.
- **Host vs member removal (U12, W2-M Q8):** removing a *member* makes only the hosts rotate their address, 2 days later; other members do nothing. Removing a *host* is an early group-level rotation after a signed revocation: every member and the remaining hosts rotate, members do nothing and the hosts reconnect them. *(Not currently shown in the page: the members sheet only says "Hosts keep a full copy of the group and can read it.")*
- **Late posts (U11, U12):** a post whose signed frontier predates the removal still arrives; it is
  labelled "Written before Leo was removed". New sends are refused immediately; the removed screen
  says the hosts refuse anything new and change address on a stated date, after which the removed
  phone cannot reach them.
- **Hosts (U7, U14):** hosts are trusted members' desktops that hold the whole group. Members only
  talk to hosts, never to each other, and never see each other's addresses. Hosts are read-only in
  the members sheet; adding/removing hosts stays on the Hosts board. Hosting works best on a desktop that stays on; the prototype never says a phone cannot host.
- **Group calls (U21):** drawn as a silent in-page overlay only (see below). They run through one
  call host, which can hear the call; no end-to-end or mesh claim, and the call ends if the call
  host drops. The call-host details (who hosts, host can hear, host loss) are drawn in
  [calls.html](./calls.html), not in this page.
- **Hosts can read (U14):** shown in the create sheet, the invite Review dialog, the ask-to-host
  dialog and the members sheet. The word "end-to-end" is never used for group content.
- **Private chat (U3, U13, U19):** a request goes through the host sealed to the recipient, so the host cannot read the note, but it can see that a request was made (the dialog says so); if accepted the pair
  talks directly with **messages only**; calls need an explicit "Allow calls" (a "Calls are off · Allow calls" line above the private chat’s composer). Block is the
  contact-level action; "Remove" only ever means removing a group member. A personal block never
  changes group membership.
- **No delete of chats:** there is no Delete group or Delete chat anywhere (U24). Leaving or removal
  keeps the chat read-only. The message menu does offer Delete on your own messages; it removes the
  message from this view with an Undo toast and makes no claim about other members’ copies. Whether
  per-message delete belongs in groups at all is undecided (see chat.md, open question 0).
- **Delivered:** means a host has the message, not that anyone read it. There are no read receipts.

*Proposals (not decided):* invite lifetime 7 days, one chat request per person per day, request
expiry 7 days, cancelling a request does not notify the recipient, who-can-invite editing.

## Interaction and sound states

**Message states** (outgoing, group): `sending` (clock glyph, about two-thirds of a second) →
`delivered` (check glyph; toast "Delivered"). With no host reachable a new message is `queued`:
clock glyph in the inline meta and no explanatory text in the bubble; the strip under the header
and Message info ("Waiting for a host. Saved on this phone; it sends when a host is back.") carry
the explanation. Queued messages keep send order. When a host returns they flip to delivered
together and one system line says so.

**Retry:** "Try again" in the strip disables itself, reads "Trying…", marks the pane busy, announces
the start, and after about a second either delivers the waiting messages ("N waiting messages
delivered") or announces "Still no host reachable. N message(s) stay saved on this phone." There is
no automatic countdown in the prototype; production should also retry on network change.

**Composer:** attach · field · paper-plane Send, identical to `chat.html`. Enter sends on a fine
pointer, Shift+Enter inserts a newline; on touch Enter inserts a newline. Escape cancels a pending
reply or edit. Empty Send is disabled. After sending, focus stays in the field. Draft text and
reply target are stored per conversation and survive switching chats. Reply shows a context strip
only while replying; the quote inside a message is a button that scrolls to and outlines the
original. Attach picks a real file and posts its name.

**Bubbles:** same system as `chat.html` and SPEC.md: received white with a hairline border, sent
soft mint, meta (`edited`, time, glyph) inline at the end of the last line. Same author, same day,
within 5 minutes groups bubbles (2 px gaps, tightened stack-facing corners); the sender name (13 px
bold, avatar-hue ink) appears only on the first bubble of a group. Day separators are pills; event
lines ("Kavya joined, invited by Priya") are centred muted text. A scroll-to-latest button with a
count appears when a message arrives while you are scrolled up.

**Message menu:** no persistent per-message buttons and no reserved action row. Fine pointer at
860 px and wider: a 24 px chevron appears beside the bubble on hover or keyboard focus (never
inside, never changing bubble size) and opens an anchored popover; right-click opens the same
popover. Touch or narrow screens: long-press on a bubble (500 ms; cancelled by a 10 px move,
scroll, release or pointercancel; no effect in the composer) or the context-menu event opens a
bottom sheet with a short message preview. The menu button stays focusable for keyboard and screen
readers. Actions: **Reply**, **Copy**, **Edit** and **Delete** (own messages), **Message info**.
Focus lands on Reply; Escape returns focus to the message’s menu button.

**Group call (in page):**
- The header phone button (44 px, left of Members) opens an overlay over the app frame: group glyph
  avatar, group name, "Connecting…" then "Connected" after about 2.5 s (group calls do not ring),
  MM:SS timer, Mute (toggle) and a solid danger End.
- The down-chevron or Escape minimises to a 40 px accent strip above the headers ("● Weekend hike ·
  MM:SS"), visible in the list and in conversations. Tapping it returns to the overlay. Chatting and
  switching chats work while minimised; while live, any group’s phone button returns to the same
  call. End removes overlay and strip, toasts "Call ended" and returns focus to the phone button.
- Disabled (`aria-disabled`, "Group call unavailable while no host is online") when no host is
  online; no extra banner. A removed or left group has no call button. Private chats opened from
  this page have no call button.
- The overlay is silent. [calls.html](./calls.html) is the only page with audio and supports
  `?call=group:hike`.

**Sound** (design intent; the prototype stays silent so it never surprises):

| Event | Sound | Notes |
|---|---|---|
| Incoming group message | Platform notification sound for the group's channel | Muted group = no sound, badge only. |
| Own message delivered | None | Visual tick only. Optional soft tick is a setting, off by default. |
| Chat request / invite | Silent notification, no ring | Requests are never call-like. |
| Removal | Normal message-level notification | Not an alarm. |
| Block / flag confirm | None | |

**Dialogs:** all native modal `<dialog>`; Escape closes and returns focus to the control that
opened it (falls back to the heading of what is underneath). Destructive dialogs focus Cancel first
and name the action on the confirm button ("Leave group", "Flag and remove", "Block Leo").
Review dialogs for invitations and requests follow the same rules. Phones use bottom sheets; wide
screens use a right drawer for members. Toasts are announced in a polite live region and carry Undo
only for reversible things (dismiss invite, delete a message, cancel a sent request).

## Edge cases

- **Empty states:** no groups ("No groups yet" / "Start one with people you already know." / New
  group), no search hits ("No results" / "Search covers group and chat names." / Clear search),
  group with no messages ("No messages yet" / "Say hello."), group with no host (strip "No host
  yet — messages wait" with "Ask to host"), no one left to invite ("Everyone you know is already
  here.").
- **No host reachable vs no host yet:** two different strips. The first is temporary and retries;
  the second needs a person to say yes and shows a pending request in Simulation.
- **Sole host leaves:** the leave dialog says nobody can host until another member does.
- **Expired invite:** "Expired · ask {name} again", Dismiss only.
- **Request raced by block:** blocking from a request removes it and tells the sender (*proposed*: notice wording same as a blocked contact, open 8b-4); the sender was never a contact, so nothing is "removed from contacts". Group membership is untouched.
- **Removed while offline:** a message whose signed frontier predates the removal can still arrive (U11) and is labelled, not hidden. Anything new is refused immediately; after the 2-day host rotation the phone cannot reach the hosts at all.
- **Cancel a sent request:** removed from Sent requests; recipient is not told (proposal).
- **Long names / text:** wrap or ellipsise; nothing is truncated without an accessible name.
- **Dark mode / reduced motion:** palette follows `prefers-color-scheme` with a visible toggle;
  all animation is removed under `prefers-reduced-motion` or `data-motion=reduce`.

## Open questions for the user

1. May the owner be flagged? (Prototype hides Flag for the owner.)
2. What do other members see of a personally blocked member's group messages? Open (G-BLOCK 1): the prototype leaves them visible and says block never changes membership. Proposed: collapsed for you, display only.
3. Does Leave by an owner transfer ownership or leave the group ownerless?
4. Are invite lifetime, request rate limit and request expiry (all 7 days / 1 per day) acceptable?
5. Should group members be able to delete their own messages, and with what reach?
