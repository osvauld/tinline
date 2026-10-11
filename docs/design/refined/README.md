# Tinline · interaction refinement

Design-only work on branch `design/refinement`, based on `ui/android` so both original Claude Design canvases are present. No native Android or desktop application code changed. Original boards are preserved with targeted annotations/corrections; the remote Claude canvases have not been updated.

## Open the studies

From this worktree:

```sh
python -m http.server 8768 --bind 127.0.0.1 --directory docs/design
```

Open http://127.0.0.1:8768/refined/ for the **app home screen**. The previous review landing page is now [Design studies](review.html), separate from the app UI. These local HTML pages and their shared `interactions.css` need no package installation or external fonts. All people, messages, permissions, tickets and network outcomes are simulated. Browser audio is real, but it is not an implementation of native ringing.

| Study | Interaction detail | Specification |
|---|---|---|
| [Home](home.html) | Chats, groups, calls and contacts navigation, search, availability, add contact and settings | [home.md](home.md) |
| [Conversations](chat.html) | Drafts, message states, reply/edit, attachments, notification preferences | [chat.md](chat.md) |
| [Calls](calls.html) | User-initiated synthesized ringtone/ringback, answer/end, quiet mode and failures | [calls.md](calls.md) |
| [Groups](groups.html) | Hosting, invitations, member boundaries, requests, removal and offline states | [groups.md](groups.md) |
| [Privacy](privacy.html) | Visible blocking, permissions, rotation audiences, device state and recovery | [privacy.md](privacy.md) |
| [Setup & devices](devices.html) | Pairing, expiry, cancellation, recovery and unlinking | [devices.md](devices.md) |

## Mobile chat

Open [Mobile chat preview](mobile.html?screen=chat&chat=arjun) or [Mobile group chat](mobile.html?screen=groups&group=hike) to see and interact with the phone conversation at 390 or 320 CSS pixels, even on a desktop browser. Use **Desktop layout** to compare. On a phone, tapping a private conversation on Home now opens that person directly; it no longer presents an extra study-selection sheet or another chat list.

Group conversations now use compact bubbles and an on-demand message menu—not persistent Reply rows. On a phone, long-press a message or tap its small overflow control; on desktop, hover/focus reveals the control. The reply strip appears only after choosing Reply.

## Home, opening and tactile details

Start at [Home](home.html), not the [study index](review.html). Use **Preview states → Replay opening**, **Slow load**, or **Load failure** to review motion and recovery. The account sheet contains theme and reduced-motion preferences. [Home / motion specification](home.md) and [verification](home-verification.md) cover the entry animation, skeletons, surface elevation and press feedback.

## Review approach

- Start on a 390px viewport, then compare 1440px and dark mode.
- Open a sheet with the keyboard, cancel using Escape, check where focus returns.
- Try empty, offline, pending and failed states, not only success.
- Start a ringtone explicitly; answering, declining and leaving the page must stop it. Quiet mode must retain a visual equivalent. Automated graph checks do **not** constitute a human listening review or device/OS audio testing.
- Check [browser verification](verification.md), [setup/devices verification](devices-verification.md), and the [screenshot review gallery](qa/gallery.html) for actual executed checks and remaining limitations. [Audit](audit.md) distinguishes broader design gaps from interactive coverage; not everything in that inventory is implemented.

## Visual direction

Keep the existing forest-teal identity, warm neutral surfaces and restrained lavender accents. Avoid changing the brand purely for novelty. The landing-page mark simplifies the existing two-ended line into rounded forms; it is a design exploration, not a replacement launcher asset. Use calm hierarchy, small contextual status labels and clear verbs instead of icon-only mystery actions. Gold is a transient/warning accent, not another primary action color. Animation must never carry essential meaning.

## Policy precedence

The original rotation brief has superseded decisions. The source for this study is `endpoint-rotation-blocking:docs/design/rotation-open-decisions.md`, section 0 **through U24**, plus the two original canvas collections. In particular:

- Blocking is **visible**, with a signed notice; it does not trigger private-address rotation. The shipped recipient app removes the contact entry, keeps read-only history and stops sending/calling/retrying. Modified clients cannot be forced to delete anything.
- Unblock does not recreate a deleted contact: QR/ticket re-add is required. Contact action is **Block**, not Remove; chats are not deleted.
- Private and group addresses rotate independently per device. Group traffic must not expose private addresses. Rotation is not IP anonymity.
- Any three admin flags remove a group member; they need not be from distinct admins. New traffic is refused immediately; host-address retirement is announced for two days later. Eligible earlier posts may still arrive.
- Group hosts can read group content. Group-call hosts can hear calls; losing that host ends the call, rather than providing seamless handoff.
- Contacts acquired by a group chat request start message-only; calling requires separate permission.

Proposed interface timings, preview outcomes and design conveniences must not be mistaken for protocol or platform guarantees. See each study's specification for scope.
