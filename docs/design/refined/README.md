# Tinline · interaction refinement

Design-only work on branch `design/refinement`. No native Android or desktop
application code changed. Pass 2 rebuilt every study onto one shared system:
[`tokens.css`](tokens.css) is the single palette/type/shape source and
[`SPEC.md`](SPEC.md) is the binding component spec — where a page disagrees
with the spec, the page is wrong.

## Open the studies

From this worktree:

```sh
python -m http.server 8768 --bind 127.0.0.1 --directory docs/design
```

Open http://127.0.0.1:8768/refined/home.html for the **app home screen** — the
intended starting point. The older review landing page is kept at
[Design studies](review.html). Pages are self-contained HTML, no packages or
external fonts. All people, messages, permissions and network outcomes are
simulated; every simulation control and disclaimer lives in the closed-by-default
Simulation drawer (sliders icon, top right), never inside the app frame.

| Study | Interaction detail | Specification |
|---|---|---|
| [Home](home.html) | Chats, groups, calls and contacts navigation; the desktop detail pane is the real conversation page embedded | [home.md](home.md) |
| [Conversations](chat.html) | Bubbles, delivery states, reply/edit, message menu, in-conversation calling | [chat.md](chat.md) |
| [Calls](calls.html) | The only page with audio: synthesized ringtone/ringback, answer/decline/end, quiet mode, failures; `?call=<id>` deep links | [calls.md](calls.md) |
| [Groups](groups.html) | Hosting, invitations, member boundaries, removal, offline states, group calls | [groups.md](groups.md) |
| [Privacy](privacy.html) | Visible blocking, permissions, rotation audiences, device state and recovery | [privacy.md](privacy.md) |
| [Setup & devices](devices.html) | Pairing, expiry, cancellation, recovery and unlinking | [devices.md](devices.md) |

## The shared conversation system

`chat.html` and `groups.html` use one bubble system: sent = soft mint with dark
ink (never solid teal), received = surface white with a hairline border, time
and delivery state inline at the end of the last line, consecutive messages
grouped with tightened corners. Message actions are on demand — long-press on
touch, a hover/focus chevron on desktop — never a persistent button. Delivery
detail lives under **Message info** in that menu, not in the bubble.

## Calling from a conversation

The phone button in a conversation header starts an **in-page call overlay**
(silent visual simulation). The top-left chevron or Escape minimizes it to a
full-width ongoing-call strip above the headers; you can keep chatting, browse
the list and switch chats, then tap the strip to return. End exists only inside
the overlay; while a call is live the phone button means "return to call".
Policy states are demoed: Dev Patel is message-only (calling needs his
permission — see his contact sheet), and a group without a live host cannot
start a call. [Calls](calls.html) remains the only page that makes sound.

## Phone preview

Open [Mobile chat](mobile.html?screen=chat&chat=arjun),
[Mobile group chat](mobile.html?screen=groups&group=hike) or
[Mobile home](mobile.html?screen=home) for the real pages in a 390/320 px
phone frame. Studies link there via **Phone preview** in the harness bar, and
Home's account sheet has the same rows. Under the hood this is the
`?embed=1` contract from SPEC.md: the conversation pages drop all harness
chrome and fill the frame — the same mechanism Home's desktop detail pane uses.

## Review approach

- Start at 390 px, then compare 1440 px and dark mode (sun/moon toggle).
- Long-press a message on the phone sizes; hover one on desktop. Check the
  sheet closes on Escape and scrim tap, and focus returns to the message.
- Start a call, minimize it, switch chats, return, end — focus should land on
  the overlay, the strip, then back on the phone button.
- Try empty, offline, pending and failed states, not only success.
- On [Calls](calls.html), start a ringtone explicitly; answering, declining and
  leaving the page must stop it. Quiet mode keeps a visual equivalent.
  Automated checks are not a human listening review or device/OS audio testing.
- `qa/run-all.sh` plus `devices-test.js`, `mobile-chat-test.js` and
  `group-polish-test.js` is the executed evidence; the per-study `*.md` and
  `*-verification.md` files record scope and limitations.

## Visual direction

Forest-teal identity, warm neutral surfaces, restrained lavender secondary.
Dark teal is reserved for primary actions; sent bubbles are mint, never teal.
Gold is a transient/warning accent. Destructive call actions use the solid
danger fill tokens in both themes. Calm hierarchy, clear verbs, sentence case,
no icon-only mystery actions, and animation never carries meaning alone.

## Policy precedence

The rotation brief has superseded decisions. The source is
`endpoint-rotation-blocking:docs/design/rotation-open-decisions.md`, section 0
through U24, plus the two original canvas collections. In particular:

- Blocking is **visible**, with a signed notice; it does not trigger
  private-address rotation. The shipped recipient app removes the contact
  entry, keeps read-only history and stops sending/calling/retrying. Modified
  clients cannot be forced to delete anything.
- Unblock does not recreate a deleted contact: QR/ticket re-add is required.
  Contact action is **Block**, not Remove; chats are not deleted.
- Private and group addresses rotate independently per device. Group traffic
  must not expose private addresses. Rotation is not IP anonymity.
- Any three admin flags remove a group member; they need not be from distinct
  admins. New traffic is refused immediately; host-address retirement is
  announced for two days later. Eligible earlier posts may still arrive.
- Group hosts can read group content. Group-call hosts can hear calls; losing
  that host ends the call, rather than providing seamless handoff.
- Contacts acquired by a group chat request start message-only; calling
  requires separate permission.

Proposed interface timings, preview outcomes and design conveniences must not
be mistaken for protocol or platform guarantees. See each study's
specification for scope.
