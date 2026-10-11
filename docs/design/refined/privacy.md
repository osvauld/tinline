# Privacy, address changes and blocking: refined design

Prototype: [`privacy.html`](./privacy.html) (interactive, simulated data, no network).
Sources: `groups/AndroidPrivacy.dc.html`, `groups/AndroidBlocking.dc.html`, and the decisions in
`endpoint-rotation-blocking:docs/design/rotation-open-decisions.md` section 0 (U1-U24),
`endpoint-rotation-blocking.md`, `rotation-time-contract.md`, `rotation-storage-failure-contract.md`.
Items marked **Proposed** are design ideas, not agreed protocol behaviour.

## 1. Principles

1. **Block and address change are two different things.** A block is immediate: the blocked DID is
   refused on every device of the blocker. Blocking never triggers an address change. The blocked
   person drops out of address updates at the *next scheduled* change, and only then does the old
   address stop working for them (U5, U23). The UI always shows both steps as a two-step timeline.
2. **Refusal does not wait for rotation.** If the scheduled change is overdue or failed, the block
   still holds; the timeline says so ("the old address is still in use, X is still refused").
3. **No anonymity claims.** Copy says what rotation does (old addresses stop working) and does
   not (does not hide you from people you talk to; an allowed contact could pass an address on;
   identity is unchanged).
4. **Two audiences, two addresses (U2, U7).** Contacts get the contact address. Group hosts get a
   separate group address. Members never receive anyone's address. Primary UI says "contact
   address" / "group address"; "endpoint", "iroh", "DID", "token" do not appear.
5. **Blocking is visible to the blocked person, and only to them.** Decisions U5 (revised), U19, U20:
   a DID-signed notice tells the blocked person. Their shipped app then disables sending and calling,
   stops background retries, and deletes our contact entry (always requested, never their chat history).
   Modified clients cannot be forced to delete anything, so our devices keep refusing them. No one else
   is told: not group members, not hosts, not other contacts. Nothing in group UI reflects a personal
   block, except a local display choice. Copy never describes a block as silent or invisible.
6. **Block, not Remove, for contacts (U19).** Remove exists only for group members.
7. **Honest simulation.** Every simulated action is labelled "Simulate" or sits in the dashed
   Simulation panel. No QR is real. Nothing claims a real send.

> **Superseded brief.** An early brief said never to expose blocked status to the remote user. That was
> replaced by the user's decisions (U5 revised, 8b item 1, U19, U20, U24). The prototype follows the
> decisions and shows exactly what the blocked person sees. Older "silent block" wording (U5 original,
> W2-B/W2-Q "indistinguishable from offline") is rejected policy and must not appear in copy.
>
> Other constraints applied: no "Delete chat" anywhere (U24; media only); a contact has Block, not
> Remove (U19); private and group addresses are separate and a block never rotates either (U2, U5);
> group hosts can read group content (U14), so privacy copy never calls group content end-to-end.

## 2. Screens in the prototype

| Tab | Content |
|---|---|
| Address changes | Interval choice, last/next change, contacts told N of M with a "who" dialog, who gets which address, per-device status, banners for overdue / failed / waiting contacts |
| People and blocking | Contact list, per-contact permissions, connection state, Block dialog, post-block timeline, "What X sees" card, Unblock dialog, Reconnect flow |
| Can't reach someone | Both sides changed address while apart: via a shared group host, host offline, or no shared group (QR) |

Simulation controls: address situation (normal / phone was asleep / change failed / some contacts
offline), jump to the next scheduled change, laptop offline, share a group with Sam, reset.

## 3. Address change states

| State | Trigger in prototype | UI | Rule |
|---|---|---|---|
| On schedule | Normal | Last/next time, N of M told | Block/unblock never touch the schedule (time contract T1) |
| Contacts waiting | "Some contacts offline" | Info banner, "Waiting" per contact | Whichever side's address is still live retries; nothing for the user to do (U1) |
| Overdue (phone slept) | "Phone was asleep" | Warning banner; "Simulate waking the phone" | **One** change on wake, no catch-up changes; next = wake + interval (candidate ResumePlusInterval, open) |
| Failed | "Change failed" | Error banner; "Try again now" fails once more, then succeeds | Current address is not extended silently; fail-closed vs short grace is open (C2). Banner says the length is undecided |
| Interval changed | Radio group | Live message with the new next date | Counted from last change with a floor; shortening can only bring the next change forward; overdue/failed changes are not postponed (T3, FromLastRotation) |
| Both sides rotated while apart | Tab 3 | See section 5 | U1 |

Not offered, on purpose: a general "Change address now" button (not in the agreed decisions, and
it would invite use as a block substitute), a per-contact "re-send address", and any display of an
actual address or key.

Open values shown as **Proposed**: interval choices (Day / 3 days / Week), default, limits, jitter,
floor length. Per-device status needs devices to share status: **Proposed**.

## 4. Permissions and blocking

**Contact permissions (U13).** Message is always on for a contact (the way to stop it is Block), shown as
status, not a switch. Call is a switch. Defaults: QR/ticket contacts message + call; from a group chat
request, message only. Turning calls off applies at once; with a linked device offline the toast says
that device applies it when back (sibling sync is best effort, open question W2-C 4). A refused call
never rings for the receiver. What the caller sees is **Proposed** ("hasn't allowed calls", not
"blocked"; exact wording open).

**Block dialog** lists consequences in two groups:
- *Right away:* cannot message or call on any of your devices; they are told; their shipped app stops
  sending, calling and retrying and is asked to delete your contact entry (always sent, U20; a modified
  app can ignore it, your devices still refuse); chat history stays on both sides, read-only on theirs.
  Chats cannot be deleted (U24), only media.
- *At your next scheduled address change:* date shown (or "when it succeeds" if failed); they are not
  sent the new address; the old one stops working for them.
- *Shared groups* (only if any): you both stay; nobody there is told; their group messages show
  collapsed for you (**Proposed**, display only, open question G-BLOCK 1).
- If a linked device is offline: banner that it applies the block later and may accept messages until then.

**After a block** (contact page): two-step timeline (refusal done; address step pending/done),
an explanation that refusal does not depend on the address change, the "What X sees" mock (header "Blocked you",
signed notice, crossed-out call, no composer), and Unblock.

**Unblock dialog.** They are told the block is lifted (clears "blocked you" on their kept history). They must
re-scan because their contact entry for you is gone (U20). Unblocking alone never restores messaging and does
not send an address; nothing is delivered until the QR/ticket re-add completes. Contact state becomes
*Reconnect needed* until then (simulated). Messages written before the block are delivered after the re-add
(U24); the rule for messages written during the block is open.

**Not shown:** the "tried to message you N times" counter (U24: comes later with UI wiring); the old board
line is annotated.

**Chat requests via host (U3).** A blocked requester gets the same refusal as a blocked contact (open 8b-4). Not
prototyped.

## 5. Recovery: can't reach someone

Detection of "lost" (timeout, number of tries) is open (R1a 7); the prototype only presents the result.

| Situation | UI |
|---|---|
| Shared group with a reachable host | "Reconnecting through <group>": automatic, nothing required; QR remains possible |
| Shared group but host offline | Warning: passes on when host returns; QR buttons offered |
| No shared group | Warning with Show my QR / Scan; messages show "Waiting to reach Sam" |
| Recovered | Success banner; "Delivered"; history kept; keeping the verified flag is **Proposed** (R1a assumes yes) |

Dialogs: Show my QR (sample pattern, labelled not real), Scan (no camera in prototype). Both have a
Cancel and a "Simulate" completion; Escape and backdrop click cancel.

## 6. Interaction, accessibility and sound states

- **Dialogs:** native `<dialog>` modal; Escape and backdrop click close; focus lands on the title; on close,
  focus returns to the control that opened it, or to the contact heading if that control no longer exists
  (e.g. Block was replaced by Unblock).
- **Focus after a rerender:** if the control you used disappears ("Simulate waking the phone", "Try again now" once
  the banner resolves), focus moves to the "Status on this phone" heading, where the result is shown; the polite live
  region still announces it. Focus is never left on the page body.
- **Live feedback:** every action posts a sentence to a polite live region and a visible toast (6 s). Banners
  are persistent state; toasts are not the only record.
- **Keyboard:** tablist with arrow keys / Home / End; radios and switches are native radio / `role=switch` buttons;
  buttons >= 44 px. Status is never colour-only (icon + words).
  The three tabs are a wrapping three-column segmented control (labels wrap to two or more lines), so none is
  scrolled off or clipped at 390 px, 320 px or 200% text.
- **Motion:** only a 150 ms switch slide and toast fade; both removed under `prefers-reduced-motion`.
- **Theme:** follows `prefers-color-scheme`, plus a visible toggle (`aria-pressed`), remembered in
  `localStorage` (`tinline-design-theme`) when available.
- **Sound:** none is added here. By design: a blocked caller and a caller without call permission never ring the
  receiver; address changes, blocks and unblocks are silent (no notification sound, no vibration). A queued
  message that becomes undeliverable ("blocked you") is a status line on that message, not a notification.
- **Empty / error:** the Blocked people list shows an explanatory empty state until someone is blocked; failed change has an explicit error banner and retry; nothing is ever shown as sent unless
  simulated.

## 7. Edge cases checklist

1. Block while the address change is overdue or failed: step 2 says old address still in use, still refused.
2. Block while a linked device is offline: block holds on this device now; the other device applies later.
3. Interval shortened while a change is pending: next date cannot move later; overdue stays overdue.
4. Contact in two groups with the blocker: both listed; membership untouched.
5. Contact is only a group member, not a contact: block there is personal, blocks private chat requests,
   leaves membership alone (U5 revised). Removal from a group is a different action (flags, U9).
6. Blocked person's modified client: it may ignore the notice, keep our contact entry and still know an address.
   Our devices' refusal and the exclusion from later address changes are the backstops, which is why copy never
   says "they can never reach you" or "they will delete you"; it says "is asked to delete".
7. An unblocked contact could pass your current address to a blocked one (open G-BLOCK 7): refusal still applies.
8. Both rotated while apart with no shared group: only manual re-share (U1); never silent failure.
9. Unblock then reconnect: history stays; call and message permissions reset to QR defaults (**Proposed**).
10. Group hosts and members never see contact addresses; group copy says so. It also says hosts can read the
    group (U14) and, for group calls, a call host can hear the call (U21): no end-to-end, mesh or live
    failover claim in privacy copy.
11. Group removal is a different action from a block: 3 flags (any three, even from one admin, U15) remove a
    member at once for new traffic and calls; hosts rotate their group address 2 days later, and posts written
    before the removal are kept (U11, U12). A block never does any of this.

## 8. Open questions this design depends on

Default and range of the interval; jitter; fail-closed vs short grace on failed change and what the
user sees (C2 Q2); overdue policy (ResumePlusInterval vs aligned grid); lost-contact detection;
messages written during a block; whether Verified survives reconnect; display of a blocked member's group
messages; wording of the call-refused reason; whether the block notice is shown immediately or on first attempt (8b-2).
