# Playground screen port — integrated partial milestone

Integration branch: `design/screens`; destination: `dev`.
Recovery evidence: `/home/abe/build-cache/p2p_chat/screens-resume-sol61-YwvBIJ/`.

## Scope

This is **not the complete playground UI on either platform**. It recovers interrupted work and combines the useful Android and Iced milestones. The playground remains the specification; no core, protocol, FFI or security-policy changes are included.

- Android: shell/home/lists/search, direct conversation/composer/find, onboarding/unlock/link, group list/info/member surfaces and a 1:1 call-layout milestone.
- Iced: shell/search, voice calls and development-only Fog/Clouds previews, in-session conversation draft/reply/edit handling and a centered gate frame. Desktop groups are unchanged.
- Unfinished: exact visual/motion/accessibility parity; broad profile/settings/privacy/device screens; desktop conversation find and exact search-hit jumps; complete group calls, video, waiting and OS surfaces. Android group-message search jumps are unfinished. Fog versus Clouds remains undecided; no production texture default is selected.

## Recovery evidence (before combined integration)

Android area snapshots: 17 targeted tests passed; 28 debug emulator previews opened without crashes, plus two Find captures; 30 offline equal-bound checks passed. Latest call glyph and unlock-IME corrections were unverified at handoff. These are debug fixtures, not real-peer end-to-end validation or exact playground parity.

Iced area snapshots: 8 field tests, 3 call-UI tests and 4 search tests passed. Call screenshots: 20 native states; initial shell screenshots: 6. Golden field model covers 728 frames. Continuous field animation, GPU performance and final shell/chat/gate changes were not verified at handoff.

## Integration repairs

- Deduplicated Android colour/motion tokens; disambiguated groups buttons/press and people sheets.
- Reconciled Android Home, routes and gallery additions; connected direct search hits to conversation routes and draft previews to the single draft store.
- Prevented disposed Android screens from republishing drafts after account/gate clearing.
- Prevented indexed Find from resetting its cursor when loading a hit's day; scrolling reads recomposed rows.
- Reconciled Iced shell/call/gate modules; retained minimized-call controls in empty detail and visible-chat behavior during minimized calls.
- Added account-epoch rejection of late desktop text submissions before draft/message/notice mutation.
- Corrected the desktop gate's `Length::Shrink` reference.

## Combined verification

PASS for the integrated source: normal and test-hooks `cargo check -p desktop`; 23 targeted desktop tests; Android x86_64 debug assembly and 18 targeted JVM tests; whitespace checks. Read-only sol6.1 platform reviews and Rust 2024 syntax parsing also completed. Exact commands/results: `/home/abe/build-cache/p2p_chat/screens-resume-sol61-YwvBIJ/integration/merge-state.json` and adjacent logs. Android used the unchanged warm debug core/FFI; this is not a fresh Rust cross-build, combined screenshot comparison or real-peer E2E validation.

Heavy checks use `/home/abe/p2p_chat/scripts/buildlock.py`. CPU admission has been blocked above its 40% threshold; no bypass or unrelated-process termination is authorized.

Original `ds-*` working trees remain intact and uncommitted; integration copies and backups are external. No push or installation to the user's phone is authorized.


## Uncommitted recovery follow-up — 2026-10-13

Coordinator: goose UI recovery. Original parity-android-aauim450 and
parity-desktop-aauim450 worktrees remain read-only, both based on 15ad3cd.
Recovered 37 platform files into dev after checking every destination against the
base; preserved external source snapshots and hashes. No commits, staging, phone
installation, account changes or data clearing performed. Unrelated checkout work
is excluded. External evidence:
/home/abe/build-cache/p2p_chat/goose-ui-recovery-20261013-160014/.

Pre-fix baseline from recovered current source: normal/test-hooks desktop checks
passed; 6 profile helper tests passed (0 ignored, 129 filtered); Android debug and
release Kotlin compilation plus 14 gate/geometry JVM tests passed (0 skipped).
Android used existing warm JNI/bindings with cargoNdkBuild/uniffiBindgen excluded,
not a fresh Rust Android build. Compiler warnings remain.

Follow-up safeguards added (pending post-fix validation):
- Production Busy cannot call core unavailable: explicit unsupported feedback,
  no call-admission policy change; debug fixtures remain simulation-only.
- Profile writes and account transitions have reciprocal desktop UI guards;
  legacy name/availability writes share the profile worker barrier.
- Desktop invalidation clears decoded pictures; reads recheck selected hash and
  readiness, and owner changes invalidate profile presentation/generations.
- Android failed metadata reads/parses retain accepted data; matching request
  slots are released; locked identity and debug-fixture disposal are corrected.
- Android settings reads and Allow calls writes are session-bound worker tasks.
- Groups notifications are recorded as unsupported instead of an ineffective
  switch; Quiet suppresses message/missed notification sound without changing
  OS channel preferences. Availability feedback uses reserved single-line space.
- Added targeted Busy/session/transition regression tests. The attempted red run
  was delayed by memory admission until after safeguards existed; no genuine
  test-first red result is claimed.

Post-fix validation is serialized in external followup.sh after the existing
queued test job. Memory admission is currently below the 4 GiB threshold; do not
bypass it or duplicate jobs. results.txt, command logs and followup.done contain
the eventual outcomes. New fixes are NOT accepted until checks complete.

Visual acceptance remains incomplete: existing native/Svelte images predate these
fixes; fresh paired measurements, native flow checks, account-switch races,
accessibility/motion/RTL, and the full screen/state matrix still need validation.
Desktop's original 335-state inventory is not proof of parity. No whole-app or
real-device integration claim. See external acceptance.md for blockers.

## Sign-up, Add contact and the first screen — desktop (ui-parity/signup-port, 2026-10-13)

Spec: `SIGNUP-HANDOFF.md` §§ 1–6f, `SIGNUP-PORT-BRIEF.md`, `VOICE.md` (design session, uncommitted at port time).
Code: `crates/desktop/src/signup.rs` (state/updates), `signup_view.rs` (screens), `cues.rs` (generated `line`
cue), `mark.rs` (spring parameters, tug events, line colour, logo glow), `ui.rs` (`line_logo`, dark rim).

What could not be matched exactly, and how it differs (never silently):
- **Desktop has no Permissions step**, so the progress counts **5** ("1 of 5"), not the playground's 6. Notifications,
  microphone and camera need no prompt on Linux, and "Allow" buttons that do nothing would not be honest. Open question
  for the design session (macOS/Windows may want it back).
- **Step transitions** (36px slide + fade, 200/140 ms) are a cut, as with reduced motion: iced has no per-widget
  opacity for a whole step. Header and footer already stay still.
- **Balanced titles** (`text-wrap: balance`): greedy wrapping. Every desktop sign-up title fits one line at 518px.
- **Busy spinner** on "Encrypting…" is not drawn (label only, same 220 box).
- **Dark rim** on `ui::raised` surfaces is a straight 1px white-8 % line between the corner curves (inset by 0.75 ×
  radius); CSS's inset shadow follows the corner for a few px. The underglow's −28 spread is a box inset by 28 that
  casts the 80px blur; iced shadows have no spread. Logo glow: three box blurs (σ = 11px) of the mark's alpha,
  accent-ink 34 %, 6px down, drawn beyond the widget (no layout space).
- **Faded line colour** mixes gold 55 % into `--line` in sRGB (CSS: oklab).
- **"Other" add errors** (anything the core reports besides Unreachable / Refused / Blocked / Offline) show the
  Timeout line ("Couldn't reach …"). Errors are mapped by `AddError` kind only (see § 6f below).
- **Own code / expired code on paste**: now from core `peek_code` (superseded, see § 6f below).
- **The Code body** is now 328 high (playground change, 2026-10-13), which fixes the clip of the paste hint's second line.
- **"Your code" line**: "It works until {date}." now that core codes are reusable (superseded, see § 6f below).
- **Owner side of § 6f**: ported as the request sheet (see § 6f below).
- **Appearance**: desktop Settings has no Appearance row yet; the sign-up theme switch writes `appearance` in
  `desktop-settings.json` (None = follow the system), which the app then honours.
- **Terms "agreed"** is not recorded anywhere (the playground keeps it in memory only).
- **Quiet (Settings > Sounds)** doesn't exist on desktop; the `line` cue always plays (suppressed in screenshot runs).
- **tinmark.js bug, not ported**: once `tugs` have run out, the resting string counts as a new tug every frame and
  re-plucks, so the twang never ends (the playground keeps animating on Done). `mark.rs` only counts a tug while the
  cans move into the string (v > 0).
- The **default** spring is now the playground's (hitOver 1.6, recoil 0.5, settle 2 Hz), so the sidebar/call marks
  bounce a little when they go taut; `Spring::dead_stop()` keeps the old one.
- Restore and Link keep their existing screens (reached through Choose), and finish without Done ("Open Tinline").
- "Add an account" Welcome shows Cancel as a third, narrower footer button (the playground's three 220 buttons
  would not fit 520px).

## Sign-up, Add contact and the first screen — Android (same branch)

Code: `Onboarding.kt` (Welcome/Choose/Done steps, copy, arrow footers, strength meter, phrase copy-in-place),
`PeopleKit.kt` (`GNext` 56 arrow, `GNextRow`, theme switch and step-0 count in `GateHead`, `GateStep(fill)`),
`Mark.kt` (`MarkSpring`, tug reporting, `lineColor`, `glow`), `Cues.kt` (the `line` cue, AudioTrack), `Theme.kt`
(`Appearance`, dark rim in `Modifier.depth`), `AddContactSheet.kt` (new), `EmptyLine.kt` (new), `HomeLists.kt`/`Home.kt`
(empty states), `MainActivity.kt` (Add contact as a sheet over Chats, Done → Add my first contact), debug gallery entries
`g-<step>` and `first-*`.

Differences and gaps:
- **Rejected reasons**: now `AddException.Refused` / `.Blocked` / `.Unreachable` / `.Offline` (superseded, see § 6f below).
- **Permissions "Off for now"**: the Android rows only know granted / not granted (no denied state), so the muted
  "Off for now. You can turn it on in system settings." line is not shown yet.
- **Logo glow** needs Android 12+ (`RenderEffect` blur); older versions show no glow.
- **Faded line colour** mixes in sRGB (CSS: oklab). Rim: a 1px white-8 % rect clipped to the element's outline.
- **Appearance**: Settings has no Appearance row; the sign-up switch writes `appearance` prefs, which the root theme reads.
- **Quiet (Settings > Sounds)** doesn't exist yet; the `line` cue always plays. Notification-channel WAVs are not generated.
- The old full-screen Add contact (`AddContactScreen`, "Couldn't add …") is no longer routed to (kept for the gallery).
- Verified only through the debug gallery on emulator-5554 (keyboard up on field steps). The real create flow was not
  run end to end: it needs an install without an identity, and the emulator's app data was not wiped.

Open items that waited on the core are built now (requests, reusable codes, `peek_code`, the request event): see § 6f below.
- Still open from the brief: centred alignment for Welcome/Done; "New group · You'll be the owner" on a phone vs Done's
  "Groups are hosted on a computer"; the greyed call buttons in a new chat (a code-added contact may call: no greying was
  added here, existing logic untouched); the empty chat's shield.

## Adding one contact, both sides (§ 6f) — core + desktop + Android (ui-parity/signup-port, 2026-10-13)

Spec: `SIGNUP-HANDOFF.md` § 6f (main), § 6b, § 6e; playground `AddContact.svelte`, `LineJoin.svelte`,
`people.svelte.ts`, `ContactsList.svelte`, `EmptyLine.svelte`. Protocol: `docs/protocol.md` "Contact requests" and
"Tickets in the core". Core: `crates/core/src/contact_req.rs` (new), `node.rs` (`accept_hello`, `add_contact_outcome`,
live tickets), `store.rs`; proto: `ContactPending` / `WelcomeTaken`, `redeemed_key`, `hello_invite`.

Core decisions (why, and what the apps call):
- **Reusable codes (ask 0).** A code adds anyone until it expires (7 days) or New code; each joiner DID once
  (`redeemed_key`). *Why:* the user's decision; replay protection moves from "nonce spent" to "this joiner already
  redeemed this nonce", which the hello's binding already proves. `set_name` (and a synced rename, a relay change, the
  last day) mints a new code and keeps the old one good until its `exp` (the suggestion in § 6f): a code someone was
  sent yesterday keeps working. `reset_ticket` and `block` cut every live code at once (block did so before).
- **Requests (ask 0b).** A ticket hello is held (`contact_requests`), answered `ContactPending`; Accept dials the joiner
  back with the Welcome (outbox, retried 7 days). *Why a dial-back and not a new ticket:* the hello already carries the
  joiner's attested device and relay, and the joiner's `PendingContact` checks the Welcome exactly as before. Existing
  contacts re-adding, group-request tickets and the test switch are still welcomed at once.
- **Typed results (ask 1 + errors).** `peek_code` → NotACode / Own / Expired{name} / Found; `add_contact_by_code` →
  `AddOutcome::Added|Requested` or `AddError::{Unreachable, Refused, Blocked, Offline, …}` (flat error, distinct
  variants: Kotlin `AddException.Blocked` etc.). `my_code` → code + `expires_at` for "It works until Mon 19 Oct".
  Iced: `Reach::of(AddError)`; Compose: `when (e) { is AddException.Refused -> … }`. Never read display text.
- Events: `ContactRequestEvents.on_contact_request` (once per joiner) and `on_request_accepted` (joiner side).

Both apps (what differs from the playground, and why):
- **Not now** in the sheet closes it and the request stays in Contacts › "Waiting for you" (the § 6f table). The
  playground's `answerRequest(id, false)` removes it from the list, which contradicts the table; the table wins.
  "Not now" on a Contacts card hides that card **for this session only** (the core keeps the request; it comes back
  after a restart). *Iced:* `App::hidden_requests`; *Compose:* `hiddenRequests` in `HomeScreen`. Open question.
- **When they accept and my sheet is still on them**, the line goes taut with "Your line to Tara is open. Say hello."
  (the playground's in-sheet copy); the § 6f table says "Tara accepted. Your line to Tara is open. Say hello." — that
  wording is used only when the sheet reopens by itself ("New contact", chip "Accepted your request"), as the
  playground does.
- **Sheet opening by itself** only over Home (or the Add contact sheet itself), never over a call, the image viewer, the
  new-chat picker or a groups dialog (desktop `sheet_free`; Android: top of the stack is Home and no call). Otherwise it
  is dropped and the request waits in Contacts (the playground drops it too).
- **One mark instance** per face: the LineJoin row keeps the same widget/composition position across slack → ringing →
  taut, so iced keeps the canvas spring state and Compose keeps the `TinMark` state. The asking → joined change on the
  owner animates the same way.
- **Tabs hidden but keeping height** when the sheet is a request: Iced puts a 32-high `Space` in the chips' slot;
  Compose keeps the chips and sets `alpha(0)` (taps ignored).
- **knock cue**: generated tin (five partials + 4 ms tap) in both `cues.rs` and `Cues.kt`. Desktop plays it and also
  posts a desktop notification "Maya wants to add you as a contact" (the window may be in the tray). Android plays it
  only while an activity is resumed; in the background it posts a notification on a new "Contact requests" channel
  (system default sound: the generated-WAV channel sound is still not done), and a tap opens the request sheet.
- **Date format**: "EEE d MMM" / `%a %-d %b` in local time, so "Mon 19 Oct" in every locale (the playground uses
  `toLocaleDateString`, which reads "Mon, Oct 19" in en-US).
- Contacts › "Waiting for them" rows use a 40 avatar and wrap the sub line (playground: same); on desktop the row is
  12/8 padded instead of `min-block-size: 64` (iced has no min height): 64 for one line, taller for two.
- The empty Contacts list no longer shows "0 contacts" on desktop (the playground's foot is empty then).
- Desktop: the dead single-use views (`code_view`, `my_code`, `their_card`: "Works once…") were removed.
- Call buttons: unchanged logic — both apps gate on core `can_call`, and a code contact now holds a call token both ways.

Design decisions applied in this pass (user, 2026-10-13):
- **No underglow anywhere** (dark mode): the sign-up card's `--underglow` and the Welcome/Done logo glow are removed in
  both apps (Iced: `mark.rs` glow raster and `ui::line_logo`'s glow flag gone; Compose: `TinMark(glow)` and its
  Android 12+ `BlurEffect` gone). Dark depth is the rim on raised surfaces (`ui::raised`, `Modifier.depth`).
  Earlier bullets about the logo glow / underglow above are superseded.
- **Chips are "Code" | "QR"** (not "By code"); Code stays first and the default.
- **Desktop call marks keep the slight bounce** (the default spring); `Spring::dead_stop()` is not used for them.
- **Add contact body 328 high** in both apps.

## Full port pass — 2026-10-14 (ui-parity/signup-port, uncommitted)

Compared against the ui-parity playground at the same size: desktop window interior 1278×798 (headless sway,
`P2P_DEMO=pg-*` with the seed's data; `P2P_CHAT_FAKE=1` for messages), Android at 2.625× against the emulator
gallery. Shots: `/home/abe/build-cache/p2p_chat/screens-shots/port/{pairs,android}/`.

Ported this pass:
- **Calls, signal and path (§ 6g), both apps:** bars replace the "i" (fades, reconnecting wave), "End-to-end
  encrypted · Direct/Relayed", About this call with the connection row (reserved 95 px) and numbers; recent loss
  over ~5 s; one set of thresholds (desktop `call_ui::strength`, Android `qualityBars`; words Strong/Good/Fair/Weak).
  Android About is an in-window overlay (a dialog window read zero insets). Desktop keeps the device pickers in it.
- **Desktop lists:** Groups (New group row, "Your groups · n", "Left or removed" fold, invitation cards; a "Join a
  group" row kept: the playground has no paste-a-link path), Calls (avatar = profile, row = chat, call-back button
  dimmed with the reason). Scrollbars show only while the pointer is over a list.
- **Desktop conversation (1:1 and group):** 60 px header (head opens details; call, video, find, details), day chips,
  "n new messages" divider, call records as pills on the caller's side, bubbles with the time inline on the last line
  when it fits (greedy wrap measured), reply quote with the accent bar, hover chevron + pop menu (Reply, Copy, Edit,
  Delete), "Delete message?" dialog, find bar (Ctrl+F, covers the header, n/m, Enter/Shift+Enter, ring on the match),
  offline / waiting-for-host strip over the list, the composer (field, round send; reply/edit chip). Video button
  dials and turns the camera on once negotiated (desktop and Android).
- **Desktop settings:** Notifications (Messages / Calls / Groups switches, Permissions), Sounds (Quiet, the three
  call sound sets, previews), Devices (+ actions, rename, honest unlink dialogs), Privacy (landing, Blocked with
  block/unblock dialogs, Who can call me, Recovery phrase behind the passphrase with 30 s hide, Passphrase change).
- **Sounds, desktop:** all call sounds now generated from audio.ts PAT/PALETTES (`cues::call`): ring, ringback,
  connected, ended, failed, call waiting; `sent` (faint pluck) and `received` (tin tap, app open). Quiet silences all.
  Notifications › Calls off = no ringing and no call notifications.
- **Android conversation:** playground header (status "Online/Offline · messages only", call, video, find, details),
  the unread divider and call pills.

Deviations kept on purpose (tell the user):
- Desktop composer keeps the mic in the send slot when empty (voice messages; the playground has none).
- Passphrase change follows sign-up's "any length works" (the playground page asks for 8+), with the repeat field.
- "Delete for me" is not offered: the core deletes for both sides only.
- Who can call me has no "New contacts can call me" default: no core setting.
- Linked devices other than this one use a generic glyph: the core has no device kind.

Later the same day:
- **Group calls, both apps:** start / join / return from the group header (dimmed with the reason when the group has no
  call host), "Call in progress · Started by …" / "You're in the call" banner with Join / Open, the call screen (roster
  grid sized by count, status or timer, count line, "Call host: …" opening About this call, Mute / Leave (+ Speaker on
  the phone)), the strip when minimised (in flow), the host's limited-hosting note, ended holds (0 / 0.9 s / 2.6 s).
  Groups-list rows say "Call in progress" / "You're in the call". Driven by the core's group-call events.
- **Desktop calls:** incoming rings in the sidebar's brand row (RingHead: caller, Decline, Answer; the full screen on
  tap); the call strip spans the list and detail columns; an ended call holds on the call screen (the reason, e.g. "Arjun
  is busy", where the status was) and then goes, instead of a separate page.
- **Desktop gate:** unlock and restore (24-box words with paste, suggestions and the word-list check; then nickname,
  passphrase "Restore account", device, terms, Done) in the sign-up card; the link steps in the playground's words
  ("Link to your account", "Same code on both?", "Couldn't link" + restore instead); the remaining screens around it
  (key lost, set passphrase, link passphrase, sync, unlinked) use the same card, fields and buttons.
- **Desktop group conversation and group info page** (the info page replaces the side members panel).
- **Android group conversation** on the shared bubble parts (header with call + info, day pills, divider, runs, quotes,
  waiting strip, composer); day pills read "Thursday 8 Oct" on both apps.
- **Group notifications** (both apps): posts from others in a group that isn't in front of me, and new invitations,
  behind Notifications › Groups. Android has a "Groups" channel.

Not yet ported / gaps:
- Group calls: who is speaking, other people's mute and line strength, how many joined a call I'm not in, and the
  host's device kind aren't in the core (§ 6h ask); those parts stay empty or generic ("Arjun's device").
- Icons: the playground now draws its tin set by default; the native apps still use icons.ts until the dev merge
  (tin-icons generator) lands in this branch.
- Chat mute (core has none): muted badges/icons in lists.
- Group find and the group call button in the group header (wait for the group call screen).

### Tinline line icons (after merging dev's tin-icons, 2026-10-14)

- Desktop: `main.rs` includes `crates/tin-icons/generated/tin_icons.rs`; `pg_icons::pgi` and `ui::icon` draw the
  tin glyph when one exists (`ui::glyph2`), else the old path. `Icon::PhoneOff` → `callend`. The `voice_demo`
  example includes the module too.
- Android: `app/build.gradle.kts` adds `crates/tin-icons/generated` as a Kotlin source dir (TinIcons.kt, no copy).
  `PgIcons` is now the tin set at the 20 dp tier; 184 Material references with a clear tin equivalent were
  switched to `PgIcons.*` (Bluetooth, Headphones, Science, BugReport, flashlight, etc. stay Material: no tin glyph).
  `CallGlyphs` uses tin `call`/`callend`/`mic`/`vol`/`video`/`camoff`; hang-up is `callend`, no rotated handset.
- Not yet: the gold string (`body` + `string` in two colours) is drawn in one colour on both apps.
- Checked: desktop `cargo test -p desktop` 144 passed, 1 failed (`motion_bezier_matches_css_endpoints_and_midpoints`,
  failing before this pass); Android `testDebugUnitTest` 107 passed; gallery and demo captures under
  `screens-shots/port/{android,pairs}/icons/`.

### Minimised call and messaging during a call (decided 2026-10-14, in-call lab `?lab=incall`, middle phone)

- Android `CallStrip.kt` (new): the 1:1 strip (there was none: minimising closed the call screen) and the group strip,
  at the BOTTOM of MainActivity, above the navigation bar / keyboard (`windowInsetsPadding(navigationBars ∪ ime)`; the
  app above consumes those insets while a strip shows). No End/Leave. Slims to 26 dp while the keyboard is up.
  Grows in 160 ms after 80 ms. Tap: CallActivity / the group call screen. The old top `GroupCallStrip` is gone.
- Android call screen: ← back (was a chevron) and a Message button beside the signal bars (voice and video);
  Message opens their chat in MainActivity (`EXTRA_PEER`) and closes the call screen.
- Android chat header: the phone turns green (accent-soft) and returns to the call while my call with them runs.
- Calls tab rows call them (both apps); someone I can't call: the row opens their profile.
- Desktop: strip unchanged (top, with End); the chat header's phone turns green during their call.
- Gallery: `--es screen '<screen>+strip'` draws a made-up 1:1 call strip under any screen (`CallController.showForGallery`);
  GalleryActivity now resizes for the keyboard like MainActivity.
- Open (handoff): the slim strip is 26 dp, under the 48 dp touch minimum; check on a device.
- Not done: the minimised video tile (MiniVideo), RingHead on Android, GroupCallAnnounce, WaitingCard restyle.

### Photos (Framed) and photo sizes in the core; Appearance (2026-10-14)

- Core (`tinline/chat/5`, `tinline/self/4`): a picture's file record carries `width`, `height` (EXIF orientation
  applied) and a 4x3 RGB `preview`, signed by the author (`chat/photo.rs`, `doc.rs`); `Attachment` exposes them.
  `/4` and `self/3` peers are refused (426, update, no wipe). Tests: `photo` unit tests,
  `photo_size_and_preview_are_signed_and_older_records_keep_their_input`, `link.rs`
  `a_photo_arrives_with_its_size_and_preview_at_every_device`; core lib 188 passed.
- Android `Conversation.kt` PhotoBox and desktop `chat_view.rs` image_view: Framed (3 rim, inner corners follow the
  bubble's), true shape from the size, preview until here, Download / ring / waiting chip, caption and time under.
  Desktop has no ring (a percentage chip with Cancel) and no blur on the preview (iced: smoothed scale only).
- Appearance: Settings landing on both apps (Android `Appearance.follow`, desktop `Msg::Appearance` + `sys_dark`).
