# Groups: refined interaction spec

Prototype: `refined/groups.html` (self-contained, no network). Everything in it is simulated; no
step claims a real send, join, block or removal. Replies from other people are triggered by hand
under **Simulation**. Rules come from `rotation-open-decisions.md` section 0 (U3, U7, U9, U11-U15,
U19, U20); anything not decided there is marked *proposal* or *open*.

## What the prototype covers

| Flow | Where | Notes |
|---|---|---|
| Browse, search, empty | Groups list | On desktop it opens on Weekend hike. Groups and private chats come first; "Waiting for you" and three collapsed folds (Invitations, Chat requests, Sent requests) sit below, so pending items never bury the list. Search filters group names and private chats only (not message text). "Start with an empty account" shows the empty state. |
| Create group | New group sheet | Name required; contacts only; host choice. Closing keeps the draft. |
| Invitations | Folded list under the groups | Compact row, Review opens the join dialog with the consent lines (hosts can read everything; hosts see your group address and online times; members see name and messages only). Join, Not now (undo), expired row (Dismiss only). |
| Private chat request | Member sheet → Message privately | Pending, accepted, declined, expired, cancelled. Simulated answers. |
| Incoming request | Folded list under the groups | Review dialog: Accept / Decline / Block (confirm). Never rings. |
| Group conversation | Conversation pane | Quiet transcript (no per-message buttons), message actions via overflow control or long-press, drafts, queued vs delivered, members sheet. Deep link `?group=<id>` (validated against known groups) opens that conversation, e.g. `groups.html?group=hike` for the full-height phone view. |
| Flag / removal | Member sheet → Flag to remove | 3 flags remove. Late "written before removal" message. Removed view with flag details. |
| Leave | Members → Leave group | Confirm dialog, Cancel focused. There is no delete: a left or removed group stays read-only (U24, chats are never deleted). |
| No host reachable | Simulation → host desktops offline | Banner, Try again, auto-flush on return. |

## Rules represented (and what is not invented)

- **Roles:** Owner, Admin, Host as pills; a member can hold several. There is no moderator tier
  beyond these. Admins see per-member flag counts; others do not.
- **Removal (U9, U15):** any 3 flags remove, not necessarily from 3 admins; the same admin may flag
  again. Flags are final and visible to the flagged member. Removal is announced to the removed
  member and to the group as one neutral line.
- **Host vs member removal (U12, W2-M Q8):** removing a *member* makes only the hosts rotate their address, 2 days later; other members do nothing. Removing a *host* is an early group-level rotation after a signed revocation: every member and the remaining hosts rotate, members do nothing and the hosts reconnect them. The hosts note in Members says both.
- **Late posts (U11, U12):** a post whose signed frontier predates the removal still arrives; it is
  labelled "Written before Leo was removed". New sends are refused immediately (so a removed member cannot ask anyone for a private chat through a host; the removed screen says so); hosts rotate their
  address after 2 days, after which the removed phone cannot reach them.
- **Hosts (U7, U14):** hosts are trusted members' desktops that hold the whole group. Members only
  talk to hosts, never to each other, and never see each other's addresses. Hosts are read-only in
  the members sheet; adding/removing hosts stays on the Hosts board. Hosting works best on a desktop that stays on; the prototype never says a phone cannot host.
- **Group calls (U21):** planned, not drawn in this prototype (no call button; a scope choice, not a rule that groups never have calls). They would run through one call host, which can hear the call; no end-to-end or mesh claim, and the call ends if the call host drops.
- **Hosts can read (U14):** shown in the create sheet, the invite and its Review dialog, the invite-more sheet and the host dialog. The word "end-to-end" is never used for group content.
- **Private chat (U3, U13, U19):** a request goes through the host sealed to the recipient, so the host cannot read the note, but it can see that a request was made, who asked whom and when (the dialog says so); if accepted the pair
  talks directly with **messages only**; calls need an explicit "Allow calls". Block is the
  contact-level action; "Remove" only ever means removing a group member. A personal block never
  changes group membership.
- **No delete:** there is no Delete group or Delete chat anywhere (U24). Leaving or removal keeps the chat read-only. Per-message delete is not drawn.
- **Delivered:** means a host has the message, not that anyone read it. There are no read receipts.

*Proposals (not decided):* invite lifetime 7 days, one chat request per person per day, request
expiry 7 days, cancelling a request does not notify the recipient, who-can-invite editing.

## Interaction and sound states

**Message states** (outgoing, group): `sending` ("Sending…", about half a second) → `delivered`
(check + "Delivered"). Offline: `queued` ("Waiting for a host · saved on this phone"), drawn with a
dashed outline and clock icon so it never relies on colour. Queued messages keep send order and
can be discarded (toast with Undo). When a host returns they flip to delivered together and one
system line says so.

**Retry:** "Try again" disables itself, reads "Trying…", marks the pane busy, announces the start,
and after about a second announces either "N waiting messages delivered" or "Still no host
reachable". It shows the time of the last attempt. There is no automatic countdown in the
prototype; production should also retry on network change.

**Composer:** Enter sends, Shift+Enter inserts a newline, Escape cancels a pending reply. Empty
send does nothing but announces "Write a message first" and keeps focus. After sending, focus stays
in the field. Draft text and reply target are stored per conversation and survive switching chats.
Reply shows a quoted chip, only while replying; the quote inside a message is a button that scrolls to and outlines the
original.

**Message actions (polish pass):** the transcript shows no Reply/Discard rows and reserves no action area.
Each bubble has a small overflow control *beside* it (never inside, never changing bubble size). Desktop
(fine pointer): hidden until hover or keyboard focus, 32 px. Touch: a quiet "..." icon, 44 x 44 px target.
It opens a native `<dialog>` action sheet (bottom sheet on phones) with **Reply**, **Discard waiting
message** (only for a still-queued own message; nothing sent is ever deleted) and **Cancel**. Focus lands on Reply;
Escape/Cancel return focus to the control. Shortcuts: long-press on a bubble (touch/pen, 500 ms; cancelled by
movement over 8 px, scroll, release or pointercancel; no effect in the composer), right-click, and Shift+F10
on the focused control. Same-sender runs share one name label; system events are quiet centred text; the
canvas is a warm neutral with a per-sender name accent (not colour-only: names are always printed).

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
and name the action on the confirm button ("Leave group", "Flag and remove", "Block").
Review dialogs for invitations and requests follow the same rules. Phones use bottom sheets; wide screens use a right drawer for members. Toasts are announced in a
polite live region and carry Undo only for reversible things (dismiss invite, discard queued
message, cancel a sent request).

## Edge cases

- **Empty states:** no groups (create / wait for invites), no search hits (names the query, offers
  Clear), group with no messages, group with no host (banner offers "Ask a member to host"), no one
  left to invite ("share your ticket or QR first").
- **No host reachable vs no host yet:** two different banners. The first is temporary and retries;
  the second needs a person to say yes and shows a pending request in Simulation.
- **Sole host leaves:** the leave dialog says nobody can host until another member does.
- **Expired invite:** disabled card, says who to ask, Dismiss only.
- **Request raced by block:** blocking from a request removes it and tells the sender (*proposed*: notice wording same as a blocked contact, open 8b-4); the sender was never a contact, so nothing is "removed from contacts". Group membership is untouched.
- **Removed while offline:** a message whose signed frontier predates the removal can still arrive (U11) and is labelled, not hidden. Anything new is refused immediately; after the 2-day host rotation the phone cannot reach the hosts at all.
- **Cancel a sent request:** removed from Sent requests; recipient is not told (proposal).
- **Long names / text:** wrap or ellipsise; nothing is truncated without an accessible name.
- **Dark mode / reduced motion:** palette follows `prefers-color-scheme` with a visible toggle;
  all animation is removed under `prefers-reduced-motion`.

## Open questions for the user

1. May the owner be flagged? (Prototype hides Flag for the owner.)
2. What do other members see of a personally blocked member's group messages? Open (G-BLOCK 1): the prototype leaves them visible and says block never changes membership. Proposed: collapsed for you, display only.
3. Does Leave by an owner transfer ownership or leave the group ownerless?
4. Are invite lifetime, request rate limit and request expiry (all 7 days / 1 per day) acceptable?
