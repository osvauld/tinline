# Setup & devices — refined interaction spec

Prototype: [`devices.html`](./devices.html). Interactive design with simulated data; no camera, clipboard, network, storage or sound is used, and nothing is created or linked for real.

Sources: `device-linking.md` (§0–§9), `multi-device.md`, the Android onboarding / unlock / accounts / link / linked-devices boards, and `endpoint-rotation-blocking` decisions section 0 through U24 (the ones that touch devices: U8, U10, U13, U18).

This spec keeps two things apart:

- **Established** — already agreed in the docs above. The UI may state these plainly.
- **Proposed** — introduced by this study. They are design suggestions and need owner sign-off before they are built or promised in copy.

## 1. Established product guarantees the UI relies on

| # | Guarantee | Source |
|---|---|---|
| E1 | Recovery phrase = identity. Restoring gives the same DID with a **new device key and empty data**. | multi-device, linking §6 |
| E2 | If no other device survives, there is no data to restore (P2P cost). Say so at onboarding. | linking §6 |
| E3 | Linking transfers the phrase over an authenticated link. Every linked device is a full peer of the identity. | linking §0 |
| E4 | QR is a bearer secret: 5-minute expiry, single use, wrong secret burns it. Either device may show it; the one without a camera should. | linking §1–2 |
| E5 | Both screens show a 6-digit confirmation code; the **existing** device confirms and shows the new device's label; then local authorisation (passphrase, or device auth with no passphrase). | linking §2 |
| E6 | Abort before import leaves nothing on the new device. A crash after import leaves a valid account not yet in the registry; it appears at the next own-device sync. | linking §2 |
| E7 | Device labels are private to own devices. Contacts never see them. | linking §3 |
| E8 | Unlink = tombstone, stop sync and ringing, refuse self-sync, best-effort `Unlinked` message. Needs local authorisation. **Not** cryptographic revocation: a phrase holder or the device's unlock can still act as you. Only a new identity fully cuts off. | linking §5, multi-device 7 |
| E9 | Linking adds an account; it never replaces one. Only the one unlocked account on the existing device is linked. | linking §2 |
| E10 | Only the selected account is online per install; each account has its own storage and passphrase; switching drops the old account's secrets. | accounts model |
| E11 | The passphrase cannot be recovered by the app; recovery is by phrase. | passphrase design |
| E12 | U8 (decided, may not be built yet): when the device list changes, the signed list is pushed to unblocked contacts; offline contacts catch up when reachable. | rotation decisions U8 |

## 2. Proposed by this study

| # | Proposal | Why |
|---|---|---|
| P1 | Recovery words are **hidden by default**, revealed by an explicit toggle, auto-hide after 30 s, and hide when the page/app leaves the foreground or the step changes. | Shoulder surfing; screenshots. |
| P2 | No copy button for the phrase anywhere; nothing is read from the clipboard. Android sets FLAG_SECURE on the phrase and phrase-entry screens. | Keep the secret off clipboards and recents. |
| P3 | Two-word check (#4 and #17) before finishing create; unlimited retries, never wipes. | Catches transcription errors cheaply. |
| P4 | Restore entry validates count, characters and checksum with specific messages. | Today's boards show only a generic error. |
| P5 | Camera-denied fallbacks: show a QR here instead, type the link code, open settings. Scanning is never the only route. | Permission denial must not dead-end. |
| P6 | New-device "Waiting for approval" screen with cancel; explicit refused / timed-out state. | The original boards stop at the existing device. |
| P7 | Failure copy that states **what was and wasn't shared** for each reason (see §4). | Prevents false reassurance and false alarm. |
| P8 | Wrong passphrase: specific retry text, focus returns to the field. After 3 misses a 30 s wait that grows; never wipes data. | Brute-force friction without data loss. |
| P9 | Offline unlink wording; "not seen for a long time" nudge at 30+ days. | Stale devices are the real risk. |
| P10 | Account-switch draft rule: drafts are saved **per account, on this device only**, return on switch back, and are not synced to other devices. | Drafts are device-local by default; not covered in the sync table. |
| P11 | Switching during a call requires explicitly ending the call. | Resolves "reject or explicitly end". |
| P12 | Linked-device cap UI (5 proposed; 8 is the technical limit today). Not shown in the prototype. | Open question 2 in linking §9. |

## 3. Screens and states

### 3.1 First setup

Steps: Welcome → Name → (Create: Phrase → Check) / (Restore: Enter) → Protect → Done. Link-to-existing leaves for the Link flow with "new device" preselected.

| State | Behaviour |
|---|---|
| Welcome | Three equal choices. States E2 in one sentence before anything is created. |
| Name | Account name and device name required, ≤32 chars. Inline error is announced (`role=alert`) and focus moves to the offending field. Suggestion chips fill the device name only. |
| Phrase, hidden | Slots show masked bullets with a visually-hidden "hidden". Ribbon reads **Example only · not a real phrase**. Continue needs the "written down" box, otherwise an error. |
| Phrase, shown | Toggle is `aria-pressed`. Countdown visible, not announced every second. Hides on timer, on background, and when leaving the step. |
| Check | Wrong word names which word failed. No lockout. "Show the words again" goes back and keeps entered values. |
| Restore enter | Errors: empty, wrong count (reports how many entered), non-letters, failed checksum. Textarea is not auto-filled from clipboard; the "example words" button is the only shortcut. |
| Protect | Passphrase (≥8, repeat must match) or device lock (Android). Copy repeats E11. |
| Done (create) | States the phrase is the only backup of the account itself. |
| Done (restore) | States only identity returned; offers link to bring contacts and chats (E1, E2). |

### 3.2 Unlock

| State | Behaviour |
|---|---|
| Idle | Passphrase field, show/hide toggle (`aria-pressed`, 44 px target), device-lock button, "Forgot passphrase?". |
| Wrong | Message with tries left; field cleared; focus back in the field; one short shake only when motion is allowed. |
| Waiting (P8) | After 3 misses: field and button disabled, remaining seconds visible, text says nothing is erased. "Skip the wait" exists only in the prototype. |
| Device lock | Simulated sheet. Cancel returns with a note that the passphrase still works. |
| Forgot | Dialog explains no reset exists; options are device lock and restore from phrase. Does not offer to delete anything. |
| Unlocked | Simulated confirmation, then "Lock again". |

### 3.3 Link a device

Roles: **E** = existing device, **N** = new device. Method: whichever device has no camera shows the QR.

```
choose ─▶ qr (show | scan) ─▶ verify ─▶ [E] authorize ─▶ transfer ─▶ done
                                      └▶ [N] waiting  ─┘
any step ─▶ failed(expired | used | mismatch | declined | dropped | cancelled)
```

- **Choose.** Two radio groups (which device you hold; which has the camera). Diagram highlights "You are here". The directions panel lists "On this device" and "On the other device" and updates as radios change. Validation asks for a role and a method; N must name the device.
- **QR (show).** Simulated QR labelled "not scannable". Timer `m:ss` plus bar; only a one-minute warning and the expiry are announced. Expired: QR greyed, "Show a new QR code" regenerates it and resets the timer. Cancel is always visible.
- **QR (scan).** Permission states: not asked → simulated system prompt (dialog); allowed → simulated viewfinder; denied → three fallbacks (P5). Manual code accepts `OSVL1-…`; empty and malformed codes get distinct errors.
- **Verify.** Large code and the other device's code (simulated) side by side; E also sees N's name. "Codes match" is a button, **never automatic**. "They don't match" burns the code and shows the mismatch state (a photographed QR raced by a third device, E5). If a user taps "match" while codes differ, the prototype continues: a human check cannot be enforced in UI, and the spec says so.
- **Authorize (E only).** Plain warning that approving sends the phrase. Wrong passphrase: error, retry, focus returns. Cancel is "Don't link".
- **Waiting (N only).** Says nothing has been copied yet; cancel available.
- **Transfer.** Four steps with text status (done / in progress / waiting) so colour is not the only cue. Cancel opens a dialog whose copy depends on whether the account has already been sent (P7).
- **Done.** Says "Simulated". States that sync starts when both devices are online together and that contacts catch up as they get the updated list (E12).

### 3.4 My devices

| Element | Behaviour |
|---|---|
| Row | Icon, name, "This device" tag, status as **dot + text** ("Online" / "Offline") and "Last seen …". Rename and Unlink buttons carry the device name in their accessible label. |
| Rename | Dialog; empty and duplicate names are rejected in place. Focus returns to that row's Rename button. |
| Unlink (other device) | Dialog: what happens, offline/online variant, a warning that **this is not a lock**, passphrase confirmation. Wrong passphrase keeps the dialog open with an error. Success removes the row and announces it. No undo; re-linking is the way back. |
| Remove from this device | Different title and consequences (local data deleted). If it is the only device, an extra "I have my recovery phrase" acknowledgement is needed. Result is an empty state with a way to restore the demo or go to setup. |
| Single device | Info note: link another to keep a copy; the phrase restores identity but not chats (E1, E2). |
| Stale device | Warm note at 41 days (P9). |

Unlink copy must never say "secure", "removed from your account permanently", "revoked" or "no longer able to access". The approved shape: stops syncing and ringing; not a lock; a lost or stolen device is only fully cut off by a new identity.

### 3.5 Accounts

- List shows the active account (online here) and others (not online here, so calls don't ring this device; E10). A draft shows a "Draft saved" cue.
- Switch dialog states: one account online at a time; calls to the previous account won't ring this device; drafts are kept per account on this device (P10); target has its own passphrase. Wrong passphrase leaves you on the current account with an error.
- Call active (checkbox in prototype): the dialog adds a warning and the action becomes "End call and switch" (P11).
- Add account: never replaces (E9); routes to restore/create or link.

## 4. Failure copy matrix (link)

| Reason | What happened | What is safe |
|---|---|---|
| expired | Code older than 5 minutes. | Nothing shared. |
| used | Code already consumed; maybe not by you. | Nothing shared; make a fresh code. |
| mismatch | You said the codes differ. QR burned. | Nothing shared; retry somewhere private if unexpected. |
| declined | Existing device refused or timed out. | Nothing copied. |
| dropped | Connection lost. | Before account transfer: nothing copied. After: account may already be on the new device; it appears at next sync; unlink if unwanted (E6). |
| cancelled | You stopped. | Same split as dropped. |

## 5. Interaction, motion and sound

- **Sound:** none in this study. Device ringing is covered by the calls study. A future "linked" or "unlinked" tone is not proposed. Confirmation is visual plus the live region.
- **Haptics (proposal for Android):** one light tick when the verify screen appears; one error tick on a wrong passphrase. Never the only cue.
- **Motion:** only a progress bar width, a pulsing "waiting" dot and a 350 ms shake on a wrong passphrase. All disabled under `prefers-reduced-motion`.
- **Live feedback:** a persistent `role=status` region announces results ("Home desktop unlinked (simulated)"); errors use `role=alert` in place. The countdown text itself is not live.
- **Focus:** each step change focuses the step heading; validation errors focus the field; dialogs trap focus, close on Escape and backdrop click, and return focus to the opener (or the heading if the opener is gone).
- **Targets:** buttons and chips are ≥44 px; radios are full-width cards.
- **Theme:** follows `prefers-color-scheme` until the visible toggle is used.
- **No browser dialogs** (`alert`/`confirm`); all confirmations are in-page dialogs.

## 6. Edge cases to hold the implementation to

1. App backgrounded while the phrase is shown → hidden on return; user must re-reveal.
2. Screen rotation or process death mid-link → return to the start of linking; the old QR is dead.
3. QR scanned while the existing device is locked → E must unlock first; show "Unlock to continue", not a generic failure.
4. Same device scanned twice or two scanners race → first wins; the second sees "already used".
5. New device name collides with an existing label → allow, but E shows both so the user can tell them apart (labels are not identity).
6. Existing device loses connection after the phrase is sent → treat as `dropped` after-transfer.
7. Unlinking a device that is currently ringing or in a call with you → the call is not cut; the device leaves the list for future calls (to be confirmed in the call-fanout spec).
8. Unlinking the last other device while offline → list updates immediately; the offline device keeps its data until it reconnects.
9. Unlinking this device when it is the only one → needs the phrase acknowledgement; no data survives.
10. Switching accounts while a send is queued → the queue belongs to the account it was written in (open question for the chat team).
11. Draft textarea content must never be included in logs or notifications.
12. Right-to-left and long names: device names are truncated visually at about 40 characters but exposed in full to assistive tech.

## 7. Simulation inventory

Everything below is a stand-in:

| Real thing | Prototype |
|---|---|
| Recovery phrase | 24 plainly fictional words, labelled as an example |
| QR code | Generated pattern, labelled "not scannable" |
| Camera and permission | Fake viewfinder and fake system prompt |
| Other device | A "Simulation" strip with buttons to scan, expire, approve, refuse |
| Passphrase | `demo-pass`, printed beside every field |
| Network drop, expired or used codes | Radio switches in "Simulation controls" |
| Clipboard | Not used |

## 8. Open questions

1. Should the existing device require the 6-digit code typed (stronger, slower) rather than a "match" button? (linking §9.3)
2. Cap on linked devices, and how to message it.
3. Do drafts and unsent queued messages ever sync between a user's own devices?
4. How long may a verify screen wait for approval before the session closes? (2 minutes is a suggestion; not in the prototype.)
5. Should the new device be told when it has been unlinked while online, and what does it show?
6. Does the "not seen for a long time" nudge belong in settings only, or also as a notification?
