# Interaction completeness audit — design boards

Date: 2026-10-10 · Branch `design/refinement` · Scope: design only. No app code, no commit.

> **Initial audit snapshot.** This was written before the refinement and browser repair passes below were complete. Its “missing” and “not drawn” findings are the input backlog, not a statement that the final prototypes still have every listed defect. For current interactive coverage read the study specifications and [final browser verification](verification.md); setup/devices has [separate verification](devices-verification.md). Native implementation remains outside scope.

## 0. How to read this

Evidence tags used throughout. Nothing is claimed beyond them.

| Tag | Meaning |
|---|---|
| **[B]** | Text of a board or doc was read for this audit (markup/text only; boards were **not rendered**, so visuals, spacing and animation are unchecked). |
| **[D]** | A decision recorded in `rotation-open-decisions.md` §0 (U1–U24) or a numbered open question there. A decision is not a shipped behaviour. |
| **[P]** | Proposed by this audit. Not decided, not designed, not built. |
| **[?]** | Not seen. Either a file I did not open or a thing no board shows. |

**Implemented vs prototype vs proposed.** I did not open any Kotlin, Rust or Python source. Git history shows `ui-android` commits ("placeholder groups/privacy API", "groups, requests, blocking, permissions and address-change screens (placeholder)", "Groups tab … (placeholder preview)", "gate the preview behind a debug-only developer switch"). I only know those exist from commit subjects. **I make no claim about what they cover.** Every row below describes the *design*; "build status" is `[?]` unless a design doc states it (only `media-status.md` and `device-linking.md` do, and they are quoted as their own claims, dated 2026-10-08/09).

## 1. Source inventory

Main boards, `docs/design/` (all read **[B]** unless noted):

- Tokens/flow: [Main.dc.html](../Main.dc.html) **[?]** not read (tokens only), [Flow.dc.html](../Flow.dc.html), [canvas.json](../canvas.json) (partial), [README.md](../README.md)
- Onboarding/unlock/accounts: [AndroidOnboarding](../AndroidOnboarding.dc.html), [AndroidUnlock](../AndroidUnlock.dc.html), [AndroidAccounts](../AndroidAccounts.dc.html), [DesktopMore](../DesktopMore.dc.html)
- Linking/devices: [AndroidLinkDevice](../AndroidLinkDevice.dc.html), [AndroidLinkedDevices](../AndroidLinkedDevices.dc.html), [AndroidMultiDevice](../AndroidMultiDevice.dc.html), [DesktopLinkDevice](../DesktopLinkDevice.dc.html); docs [device-linking.md](../device-linking.md), [multi-device.md](../multi-device.md)
- Contacts/home/chat: [AndroidHome](../AndroidHome.dc.html), [AndroidAddContact](../AndroidAddContact.dc.html), [AndroidContact](../AndroidContact.dc.html), [AndroidChats](../AndroidChats.dc.html), [AndroidConversation](../AndroidConversation.dc.html), [AndroidChatFiles](../AndroidChatFiles.dc.html), [DesktopMain](../DesktopMain.dc.html), [DesktopChat](../DesktopChat.dc.html); doc [media-status.md](../media-status.md)
- Calls/settings: [AndroidCalls](../AndroidCalls.dc.html), [DesktopCalls](../DesktopCalls.dc.html), [AndroidSettings](../AndroidSettings.dc.html)

Groups/blocking/rotation, `docs/design/groups/` (all read **[B]**): [Main](../groups/Main.dc.html) (chats list, group chat, new group, invite), [AndroidGroupInfo](../groups/AndroidGroupInfo.dc.html), [AndroidRemoval](../groups/AndroidRemoval.dc.html), [AndroidChatRequest](../groups/AndroidChatRequest.dc.html), [AndroidBlocking](../groups/AndroidBlocking.dc.html), [AndroidPrivacy](../groups/AndroidPrivacy.dc.html), [DesktopGroups](../groups/DesktopGroups.dc.html), [README](../groups/README.md).

Decision source (outside this worktree, read **[B]**): `/home/abe/p2p_chat/.claude/worktree/endpoint-rotation-blocking/docs/design/rotation-open-decisions.md` (coordinator digest, 2026-10-10; U1–U24, §1–§15 open questions).

**Not read** (so no claims): the `rotation-*.md`, `group-*.md` contract docs and `crates/**` tests that the digest cites; Kotlin/Rust sources; `Main.dc.html` tokens; rendered visuals; `refined/index.html` (another agent's file — present, not reviewed). The digest cites those contracts for evidence; I rely on the digest's summary of them, not on the contracts.

**Refined files seen so far:** `refined/index.html` only. No `calls/privacy/groups/chat.html` yet. Section 11 lists what to re-check when they land.

## 2. Headline findings (prioritised)

P0 = contradicts a recorded decision or leaks/misleads on privacy; fix before refined boards are treated as spec. P1 = a state a user will certainly hit that no board shows. P2 = polish/consistency. P3 = later.

### P0 — contradictions and misleading promises

1. **Unblock copy contradicts U20.** Blocked-people card (AndroidBlocking) says unblocked people "can message you again and get your current address … They're told they're unblocked" **[B]**. U20 says every block also asks their app to delete our contact entry, so after unblock *they must re-add us by QR/ticket*; the unblock notice only clears their "blocked you" state **[D]**. Board promises a result the design cannot deliver. Also the Block dialog never says "their app will delete your contact" nor "to talk again they must re-add you".
2. **"Remove contact" still exists; U19 says contacts are Blocked, never Removed.** Present in AndroidContact (dialog "Remove Arjun? … call history is deleted from this phone"), DesktopGroups contact panel (Block *and* Remove), AndroidPrivacy contact page ("Remove"), and AndroidContact verify-failure advice ("remove Arjun and add him again") **[B]**. U19 and U24 (chats never deleted) contradict deleting history **[D]**. Needs one action vocabulary: Block / Unblock for contacts, Remove only for group members. (Note digest §15 Q1 lean was "separate"; U19 supersedes it.)
3. **"Groups have no calls" and "Phones can't host".** Group chat board desc says "No call button: groups have no calls"; Hosts screen says "Phones can't host" **[B]**. U21 decides group calls through a call host, which "must be able to run on a phone when needed" **[D]**. Boards are stale vs U21; the host rule is "preferably desktop", not "never phone".
4. **Encryption claims on groups.** Chats empty state: "end-to-end encrypted. Nothing is stored on a server" **[B]**. Under U14 group hosts are trusted members that keep a full readable copy, protected hop-by-hop and at rest **[D]**. Group surfaces must not say end-to-end; the invite card and new-group screen do not currently say that hosts can read the group (only Hosts screen does) **[B]**. A joining member sees "Members will see your name and your messages" but not "the host can read everything".
5. **Single admin can remove a member; copy implies a panel.** Flag dialog: "Any admin can flag; the same admin flagging again counts" and removal details list "Flagged by Sam, Meera and Priya" **[B]**. U15 confirms any 3 flags, even from one admin **[D]**. The dialog ("Yours will be flag 2 of 3") must say when the flags are all the viewer's, and the removed person's details must render "3 flags from Sam" without implying three people. Groups with <3 admins, flagging the owner, flagging admins are open (digest §8 Q2–Q3, Q9) and have no board.
6. **Pronouns baked into copy.** Boards use "He can't message or call you … He is told you blocked him", "Calls himself Arjun", "her", "his" **[B]**. Pronouns of real people must not be guessed. Copy should use the name or "they"; strings need placeholders. Applies to Block dialog, blocked chat, AndroidContact, DesktopMain.
7. **"Blocking applies to all your linked devices" / "takes effect at once on all your devices".** Block dialog desc and Privacy desc **[B]**. The digest measures a sync lag where a sibling still leaks/acts until it hears of the block (W2-B Q5, W2-C Q4: "best effort") **[D]**. Say "as your devices sync" or decide immediate enforcement.
8. **Verified-state policy conflict.** AndroidMultiDevice: a contact adding a device "resets verified … Compare safety numbers again" **[B]**. Digest §4.1 / R1a Q8 / W2-D Q5 are open on whether verification survives authenticated rotation/device-list updates (safety number is DID-only) **[D]**. Rotation would clear it every few days under the board's rule. Needs a decision before the Privacy/Contact boards can be final.
9. **Restore "replaces whatever account is on this phone".** AndroidUnlock **[B]** vs multi-device.md "import adds an account rather than replacing" and Accounts board "Restore with recovery phrase" as an add-account route **[B]**. Two different promises for the same action; the destructive one needs an explicit confirm naming what is deleted.

### P1 — missing states users will hit

10. **Block of a group member who is not a contact.** Member sheet offers "Block Leo — Just for you" **[B]**. A non-contact has no private endpoint; U5/U20 notice and deletion request are contact-level **[D]**. What is sent, through which path (host?), and what does Leo see? Undefined **[?]**. Also: collapsed messages ("His group messages show collapsed") are promised in the Block dialog **[B]** but **no board shows a collapsed message, how to peek, or how to un-collapse** (digest Q1 lean "collapsed, display-only").
11. **Blocked person's view after U20.** Their contact entry for us is deleted but history stays read-only **[D]**. The blocked-person board still shows a contact header with crossed call icon **[B]**. Needs: where the read-only chat lives (not in Contacts), what the header says, what happens on tap of the call icon, what appears after we unblock ("Meera unblocked you. Add Meera again to talk" + a re-add path), and queued messages written before the block (U24: delivered after unblock + re-add).
12. **Request edge states** (AndroidChatRequest): requester when target has blocked them; target offline for the 3-step exchange (Q12 lean: endpoint only after accept) → "Accepted, connecting…" interim; request cancelled by requester (what does the recipient's card do); request from a removed member; spam from one person across groups; pending badge/count; notification content. Decline wording "Leo declined" is assumed explicit, but digest Q11 (explicit vs silent) is open **[D]**. "One request per person per day" and "expires after 7 days" are invented numbers (Q14, Q15 open) → mark as placeholders **[P]**. Also the sheet says the host "can't read" the note but the host does learn who asked whom and when (Q16) — say so in one line.
13. **Linking error states.** Boards show only the happy path plus a one-line "Code doesn't match? Cancel" **[B]**. `device-linking.md` §8 itself lists wrong secret (burns QR), expired QR, reused QR, second scanner racing, confirmation declined, E locked mid-link, crash after import, DID mismatch, account switch during link **[B]**. None has a screen. Also: camera permission denied, no network/no route, 8-device cap (core cap per doc; board cap is open §9), cancel during sync, partial sync failure, "Link it again" after unlink.
14. **Unlink when target is offline.** Dialog says "If it's online, it removes your account from itself" **[B]**; no pending state ("Will be removed when it next connects"), no way to cancel/retry, no list marker. U8 also pushes a new device list to contacts on unlink **[D]** — no board mentions that other people's apps learn of it.
15. **Host failure modes.** "Waiting for a host" appears **[B]**; missing: how long messages queue and the cap (Q7 open), expiry of queued messages ("Not sent"), what a removed member's "sending until Oct 12" becomes after hosts rotate (silent loss), host unreachable for days (escalate to "No host for 3 days — ask an admin"), stop-hosting vs only-host (warning exists in DesktopGroups **[B]**; whether stopping also rotates the group is undecided **[?]**), host appointment pending/declined/timeout on the admin side, "Make host" for a member with no linked desktop (disabled-with-reason).
16. **Group lifecycle gaps:** no **Leave group**, mute, or group-notification control on any board **[B: none seen]**; owner leaving/transfer, last-admin rules (digest §8, §11 Q3, §6 D-Q7); "Delete group"/"Keep as archive" after removal (AndroidRemoval) vs U24 "chats are never deleted" **[B][D]** — decide whether groups are an exception; role demotion ("Make admin" has no un-do or confirm); Invite management ("who can invite" is named in Edit but no screen); invite card states Joining…/Joined/Expired/Revoked/Already a member; invite when inviter is offline.
17. **Rotation UI.** AndroidPrivacy "Change my address" **[B]**: no off/never option and no min/max (digest §2 Q1 open); "a contact offline longer than two of your intervals may need to re-share" is an invented rule (grace undecided, §2 Q6) **[P]**; no notice that an active call can drop at rotation (§3b Q3 lean: acceptable for v1, schedule around calls if cheap); "Contacts told 11 of 12" should state that blocked people are excluded from the count (R1a Q5); no "rotation failed / degraded" state (§3b Q2, §2 Q7); no "lost contact" detection timeout (R1a Q7). Lost-contact board is good but its trigger and the group variant ("no group path left", §7 Q22) are not shown.
18. **Message/Call switches** (AndroidPrivacy, DesktopGroups) **[B]**: switch semantics are under-specified: what does turning **Message you** off do (silent drop? refusal reason? notice?). U13 defines tokens but not a "messages off" UX **[D]**. Caller's refusal reason for calls to a message-only contact is shown ("hasn't allowed calls from you") — digest W2-C Q6 / R1a Q4 are open on showing this to every authenticated contact. Switch controls should be labelled with their name + state (see §7), not "On"/"Off".
19. **Recovery phrase restore errors** (AndroidUnlock): "Word 14:" is shown **[B]** but no board for an invalid word, wrong checksum, wrong order, paste of 24 words, lockout/backoff. Unlock: "deliberately slow" is stated, but no lockout count, no "too many attempts" **[B]**.

### P2 — consistency and polish

20. Two vocabularies for the same thing: "address" (groups/Privacy boards) vs "path/line" (calls) vs "endpoint" (digest). Standardise: *address* for what changes, never show keys or endpoint IDs.
21. "Available for calls" status vs the new "Message you / Call you" permission: define whether "Not available" also silences messages (today the doc says availability is calls-only; `Unavailable` mode vs ticket redemption by blocked DID is open, §7 Q9).
22. Call history per contact is "deleted" on remove (AndroidContact) but U24 says history stays **[B][D]**.
23. Tray/desktop copy: "Quitting stops incoming calls" — also stops hosting; add the hosting warning when this computer hosts any group ("Quitting stops hosting 2 groups") **[P]**.
24. Desktop Esc declines / Enter answers on the popup (DesktopCalls **[B]**): a stray Enter keypress while typing elsewhere can answer a call because the popup is always-on-top. Add an activation guard (see §8).
25. File cancel/pause, 25 MB setting, zoom, in-app PDF on desktop are already acknowledged as "not built" in `media-status.md` / DesktopChat / AndroidChatFiles **[B]**; not re-audited here.

### P3 — later

26. Group calls UI (U21), group media/search, notification content settings, abuse reporting, per-group request spam control, "peer app attestation" (digest §8b.6, lean: not a gate).

## 3. Product constraints the screens must respect

Derived from the digest **[D]** (each is a decision or a stated invariant; verify against the latest digest before shipping):

- No server, no lookup; devices announce their own address changes (U1). Private vs group addresses are separate; group traffic must never reveal the private one (U2).
- Members know only hosts; hosts know all members' group addresses (U7). UI must never show another member's address/device, and must say the host sees who is online (shown in the Hosting dialog **[B]**).
- 1:1 from a group is a *request* via the host, sealed to the target; starts messages-only (U3, U13).
- Blocking is **visible** to the blocked person (U5 revised); their shipped app enforces it and deletes our contact entry (U20); we still refuse and exclude them from address changes. Personal block never changes group membership; blocking never triggers rotation (U5, U23).
- Contacts: **Block**; groups: **Remove** (3 flags, any 3, final, visible) (U9, U15, U19). Immediate refusal; pre-removal posts honoured until hosts rotate at +2 days (U11, U12). Removed person is told explicitly.
- Chats are never deleted; media can be (U24). Messages before a block are delivered after unblock + re-add.
- Message and call are separate tokens; new group-originated contacts start message-only; "Allow calls" is explicit (U13, U15).
- Group content is not end-to-end encrypted against hosts (U14). Group calls are relayed by one call host, up to 2–3 loudest speakers forwarded, no live failover (call ends; redial) (U21).
- Every device verifies every signature/op (U10). Apps are trusted for **display** only; enforcement is on the receiving side (R1a Q9).
- Product: open source, no ads, no tracking, no analytics SDKs (memory: publishing-decision). One account online per install (memory: accounts-online-model). Linking transfers the phrase, unlink ≠ revocation (device-linking.md §0, §5).

## 4. Proposed corrections (concise)

All **[P]**; none applied to any board in this audit.

| # | Where | Change |
|---|---|---|
| C1 | Block dialog | Replace "He is told…" lines with neutral, pronoun-free wording and add: "{Name}'s app will remove you from its contacts. If you unblock, {Name} has to add you again." |
| C2 | Blocked list "Unblocking" card | "{Name} can message you again once they add you back. Calls come back only if you allow them. They are told you unblocked them." Drop "get your current address". |
| C3 | Contact page (Android/Desktop/Privacy) | Remove "Remove contact". Bottom action = Block. Verify-failure advice: "Block {Name}, then ask them to add you again in person." |
| C4 | Group chat/Hosts | Replace "groups have no calls" / "Phones can't host" with "Group calls: coming" (or hide) and "Hosting works best on a desktop that stays on." |
| C5 | Invite card, New group, Group info | Add one line: "The group's hosts can read everything shared in it." Never use "end-to-end" for group content. |
| C6 | Flag dialog | When the viewer already holds flags: "You have flagged {Name} {n} time(s); this is flag {k} of 3." Removal details: "3 flags from: Sam ×2, Priya". |
| C7 | Block (group member) | Separate "Hide {Name}'s messages for me" (display-only, collapsed) from "Block {Name}" (contact-level, needs a contact). Show a collapsed row `Hidden message · Show` once. |
| C8 | Privacy/Contact | Replace "applies at once on all your devices" with "applies on all your devices as they sync". |
| C9 | Address changes | Add: "Never" is not offered; show min/max; one-line "A call in progress may drop when your address changes" if decided; "Contacts told N of M (people you blocked are not counted)". Mark grace text as placeholder until §2 Q6 is decided. |
| C10 | Unlock/Restore | Confirm dialog naming the account and "contacts and chats on this phone will be deleted" when restoring over an existing one; or switch to add-account. |
| C11 | Notifications (all) | See §9. |

## 5. Flow matrix

Columns: entry · back/cancel · error · recovery. "Shown" = text seen in a board **[B]**; "gap" = not seen **[?]** / proposed **[P]**.

| Flow | Entry | Back / cancel | Error | Recovery |
|---|---|---|---|---|
| First run (Android) | Welcome → name → passphrase (optional) → phrase → terms → permissions | Back arrows on 2–5; Skip on passphrase and permissions. Back from phrase to passphrase: whether it regenerates the phrase is gap | Terms unchecked: gap; mic/notification denied: shown only later in Home banners & Calls "Microphone needed" | Banners in Home catalogue; Settings › Background & battery |
| Unlock | Cold start / lost key | none (no cancel; Forgot link) | "That didn't match" shown; lockout count gap | Forgot → Restore |
| Restore from phrase | Welcome link; Unlock "Forgot" | Back shown | Invalid word/checksum/count: gap | Link to another device vs Start empty (shown) |
| Accounts (switch/add) | Settings › Accounts sheet | Back; "You're not on a call" shown, in-call switch ends call (text says "would end it") — confirm dialog for that case: gap | Wrong passphrase on switch: gap | Stay on current account |
| Link a device (either direction) | Settings › Link a device / Welcome › Link | Back, Close on camera; cancel on code mismatch (text) | Expired/burned/raced QR, camera denied, offline, cap reached, E locked: all gap | "Scan instead"/"Show QR instead" shown; retry flow gap |
| Unlink | Linked devices › device › Unlink | Confirm dialog with passphrase | Target offline: gap | "Link it again" on the unlinked device (shown) |
| Add contact | FAB/home/empty state | Back; new code cancels old (shown) | "Couldn't add" with reasons (shown) | Try again / Scan different (shown) |
| Verify | Contact › Verify | Back | "They don't match" advice (conflicts with U19, see C3) | Add again face to face |
| Outgoing call | Contact / recents / chat | Cancel call (shown) | Couldn't reach; Mic needed; message-only contact sheet (shown) | Try again; Ask to allow calls (shown) |
| Incoming call | Full-screen / heads-up / popup | Decline/Answer; Esc/Enter (desktop) | Busy while in call; "answered elsewhere"; blocked caller never rings (shown in desc) | Missed-call notification with Call back (shown) |
| Chat | Chats list → conversation | Back | Not connected: messages wait (shown) | Auto-send when both online (shown) |
| Group create | New › New group | Close | Name empty, no host chosen: "Ask later" shown; zero-desktop variant of "Use my desktop": gap | Add host later via Hosts |
| Group invite | Card in 1:1 chat | "Not now" shown | Expired shown in desc; revoked/already-member/offline: gap | Ask inviter again [P] |
| Group info / member sheet | Header ⓘ → tap member | Back/Cancel in dialogs | n/a | n/a |
| Flag / removal | Member sheet › Flag | Cancel in dialog; flags final (shown) | Not an admin: not shown; owner flagged: gap | Rejoin only by new invite (shown) |
| Chat request | Member sheet › Message privately | Cancel; Cancel request (shown) | Declined/expired (shown); blocked-by-target, host down, duplicate: gap | Wait/Dismiss (shown) |
| Block / unblock | Contact page; request card; member sheet | Cancel in dialog; Unblock in chat and list | Block on a non-contact: gap; sibling lag: gap | Unblock → requires re-add [D, not shown] |
| Permissions switches | Contact page | switch toggle | Reason shown to caller (partly) | Ask to allow / Allow button (shown) |
| Address changes | Settings › Privacy | Back | Contact missed (shown count); lost contact (shown); rotation failed: gap | Show QR / Scan (shown) |
| Hosting (desktop) | Settings › Hosting; request dialog | Not now / Stop hosting confirm | Only host warning (shown) | Ask a member to host (shown) |
| Voice/file | Conversation attach | slide to cancel (shown) | failures shown in AndroidChatFiles | retry (shown); cancel transfer = not built per source docs |

## 6. Nuanced missing states (cross-cutting checklist)

- **Group, with a host:** all hosts offline; one host removed mid-send; member removed while composing (composer swaps to read-only; draft kept?); group renamed while offline; offline-queue cap; host rotates while member is away (member "waiting to reconnect"); group created with 0 hosts.
- **Requests:** target blocks requester while pending; requester removed from the group while pending; the shared group is deleted/archived; two requests crossing each other; request to a person already in contacts (shown: opens existing chat).
- **Blocks:** block during an active call (does the call end? who is told?); block while a file transfer is running; block of a person in a call group; unblock when they already blocked you; both block each other; block a linked device's own account (not applicable); notice delivery pending because they are offline (blocker's UI should say "will be told when online"? or not mention at all — decide).
- **Devices:** unlinked device with unsent messages; linked device offline > N days (shown only as "Last seen 23 days ago"); two own devices change the list at the same time (digest §12 Q2); the sibling that hasn't heard of a block.
- **Time:** timers on QR ("Expires in 4:52") with no refresh; 5-minute link window vs a slow passphrase entry; 2-day removal window shown with absolute dates (shown) — add time zone/relative form.
- **Accounts:** account switch during link; locked second account receives a call (shown as "ring only on your other devices").
- **Large text/Locale:** every dialog with 4–5 consequence rows (Block) needs to scroll; RTL for names mixed with LTR; long names truncating inside "Flagged by A, B and C".

## 7. Accessibility

Findings are about what the markup text shows **[B]**; a rendered/TalkBack/keyboard pass was **not** done.

1. **Labels:** icon buttons carry `aria-label`s (Back, Mute, End call…) — good. Toggles are labelled only "On"/"Off" (Privacy, Hosting, Settings): label them by purpose with state ("Call you, off"), role=switch.
2. **Dialogs:** only the Block dialog is marked `role="dialog"` in the files I read. Others (Flag, Host request, Unlink, Remove) need the same, with initial focus on the safe action (Cancel / Not now) for destructive ones and focus return on close.
3. **Touch targets:** text buttons in rows are 40 px tall ("Unblock" in Blocked list, Hosts "Remove") — below the 48 dp guideline.
4. **Colour-only state:** host status dot and red "Blocked" use colour plus text (good); the three-segment flag meter needs a text equivalent ("2 of 3 flags" — present as aria-label **[B]**). Disabled/crossed call icon must also expose "unavailable: {reason}" as text, not only a greyed icon.
5. **Live regions:** announce, politely, "Waiting for a host", delivery changes (Not delivered), request arrival and "Messages only"; announce assertively only for incoming call and call-ended reasons. Do not announce the QR countdown every second; announce at 1:00 and expiry, and offer "New code".
6. **QR/camera:** every QR needs a non-camera path (shown: paste card; add: text/6-digit code). Link flow has no text alternative to scanning on the *approve* side other than the 6-digit code; ensure the code is selectable and is read grouped ("4 8 2, 1 9 6").
7. **Secrets:** recovery phrase screen is `FLAG_SECURE` **[B]**; decide whether a screen reader should read 24 words aloud (default off with a warning) **[P]**. Treat the link QR as a secret (bearer for 5 minutes, device-linking.md §1) → same capture protection **[P]**.
8. **Keyboard (desktop):** Enter answers / Esc declines is shown **[B]**; risk of a stray Enter (see §8). Dialog and members pane need full Tab order; right-click member menu needs a keyboard equivalent (context-menu key / Shift+F10).
9. **Motion/time:** respect reduce-motion for ring pulses, meters and progress rings; auto-dismiss (call-ended closes after 4 s **[B]**) should be pausable or extended with screen readers on.
10. **Text scaling/contrast:** fixed 412×915 frames and a 13 px hint size hide scaling problems; check 200% scaling and contrast of `--ln2 #8E9A94` hint/disabled text against `--bg #F5F6F3` (borderline) and error colours on dark.
11. **Pronoun-free, plain language:** reading level; avoid "address/endpoint/key" jargon for the user-facing strings.

## 8. Sound lifecycle (proposal; boards show only: desktop looping ringtone stopped by popup or toast; Android heads-up/full-screen; a "quiet tone, no full-screen" second-call banner)

Everything in this table is **[P]** except the two behaviours quoted above. Implementation state is unknown **[?]**.

| Moment | Sound | Starts | Stops / must not leak |
|---|---|---|---|
| Incoming call | Looping ringtone + vibration (respects ringer mode, DND, silent) | On first ring signal accepted by the guard rules (not blocked, token allowed, available) | Answer, decline, caller cancel, ring timeout, **answered/declined on another device**, account switch, availability turned off, app killed; never keep ringing after UI is gone |
| Blocked / message-only / unavailable caller | **Silence.** No ring, no vibration, no notification | – | Missed-call counter for blocked is a quiet line only (shown) |
| Second call while in call | Single short tone, not a ring (shown: "quiet tone") | On banner appear | On any user action or banner timeout; no audio-focus steal from the active call |
| Outgoing ringback | Soft ringback while "Ringing…" | When remote device acknowledges ring | Answer, cancel, no-answer end, busy |
| Busy / failed | One short distinct tone, then silence | On "busy"/"couldn't reach" | After tone; visual stays 4 s (shown) |
| Connected / ended | Optional short tick (off by default; never audible to the other side) | – | – |
| Reconnecting | No loop; one tone when audio pauses, one when restored | – | Repeats suppressed within 10 s |
| Audio route change (BT, headset unplug, speaker) | Pause/mute briefly, never switch to speaker automatically on headset unplug (privacy) | – | – |
| Voice-message playback / recording | Plays through earpiece/speaker per setting | On tap | **Stops when a call rings or starts**; recording stops/discarded with a prompt on incoming call |
| Chat message | Short, per-chat mutable; none for blocked/hidden/collapsed; none for requests | – | Muted groups silent; Do-not-disturb honoured |
| Request received | Silent notification by default (never rings) | – | – |
| Group-removed / blocked-by events | **No sound, no push** for the person who blocked/removed; blocked/removed person sees it in-chat only (reduces harassment amplification) | – | – |
| Group call (U21) | Reuse call ring? Decide: group call "invite" = quiet ring, not full-screen | – | Host failover: the call ends with the ended tone, redial manual (U21) |

Rules: one audio owner at a time; request audio focus for rings and release on stop; every start has a named stop event; a ring must be bounded by a timeout (the 30 s mentioned for desktop "red in Recents after 30 s" **[B]** suggests this; confirm).

## 9. Privacy-safe copy rules

1. Never display, log or put in notifications: addresses, endpoint IDs, device keys, device labels of other people, host addresses, other members' devices. Boards follow this in the places read **[B]**; keep it for toasts and error strings.
2. No pronouns for real people (see P0 #6). Use `{Name}`.
3. Accurate claims only: "end-to-end encrypted" for 1:1 chat/calls; for groups say "private to the group's members and hosts". "The host can read the group" must appear where people join or create groups (C5). The chat-request sheet says sealed from the host — but add that the host sees that a request happened.
4. Say what a block does and does *not* do, in one place: they're told; they can't message/call; they lose your address at your next change (not immediately — shown **[B]**); shared groups unaffected; they must re-add you if you unblock (missing).
5. Don't promise immediacy the design doesn't have (C8) or durations nobody decided (7 days, 1/day, "two of your intervals").
6. Notifications on the lock screen: default to sender name only for calls; message text hidden by default or behind a setting [P]; requests: "New request" without the group name or note; removal/block events: no notification.
7. Counters ("tried to message you 2 times") are local only, never push, never reveal that the blocked person is "typing" (digest U24: counter comes later with UI wiring).
8. Error copy must not reveal that someone is online/offline when a block, permission or silent refusal is the real reason; where the design chooses to reveal a reason (U5 revised), show the *same* reason on every later attempt (R1a: "blocked").
9. Analytics/crash: none. No third-party SDK on any new screen (memory: publishing-decision).
10. Recovery phrase never copied to clipboard by default; clipboard use for card paste is read-once.

## 10. Priorities for refined boards (what other agents should cover)

Ordered by value: (1) blocking set incl. unblock-needs-re-add, blocked person's read-only state, hide-vs-block for group members; (2) permissions/privacy: switch semantics, address-change states, lost contact (both private and group); (3) groups: leave/mute, invite states, flag semantics for duplicate flags, host offline/queue/expiry, removal (what the removed sees after hosts rotate); (4) calls: sound lifecycle, accessible answer/decline, group-call placeholders; (5) chat: contact-less read-only state, request lifecycle; (6) link-device error states; (7) accessibility pass items in §7.

## 11. Re-check list for newly landed refined files

When `refined/calls.html`, `privacy.html`, `groups.html`, `chat.html` appear, open each and verify, for each item, whether it is **shown / partially / absent**:

- P0 #1–#9 corrections (esp. no "Remove contact", pronoun-free copy, unblock-needs-re-add, no "no group calls", host-can-read line).
- All rows tagged "gap" in §5.
- Sound table (§8) addressed in calls.html (stop events, silence for blocked/message-only).
- Accessibility items 1–5 (switch labels, dialog roles, 48 dp targets, live regions).
- Whether placeholders (7 days, 1/day, 3 days) are labelled as placeholders.

Status of this review: `refined/index.html` exists (another agent's, not reviewed). No other refined files existed when this audit was written.
