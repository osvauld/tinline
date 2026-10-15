# Signed message receipts: desktop/Android status display

Status: **built in `feature/receipt-runtime` (2026-10-15), not merged**. Desktop (iced) and
Android (Compose) draw the 1:1 bubble mark from the core's `ReceiptTick` and show signed
provenance in the message menu / action sheet. Group receipts are not built.

## What was built

- Core decides the state: `MessageReceiptStatus.tick: ReceiptTick { None, OnlyHere, Waiting, Delivered,
  Read }`. `Delivered`/`Read` come only from committed recipient-signed statements for the exact shown
  version; the legacy transport ack can at most move `OnlyHere` to `Waiting`.
- **Ticks are the mark's string** (user pick from `?lab=ticks`, branch lab/ticks, preset "crisp",
  STRING variant at every size): K1 (6,12), K2 (19.5,4), sag 7.5, swing -35 deg, stroke 2.1, second
  tick dx 5, knot gap 1, slot `20 + max(dx, 6)` = 26 units wide on the 24 grid. Geometry (40-point
  polylines for the slack and the taut string) and the morph curves (the mark's own spring, sampled at
  120 Hz: pull `1 - s/s0`, pluck `1 - 2s/s0`) are generated from Rust: `crates/tin-icons/src/ticks.rs`
  -> `generated/tin_ticks.rs` (iced) and `generated/TinTicks.kt` (Compose). Checked against the lab in
  node: tick points identical to 3 decimals, pluck identical, pull within 0.005.
- States: OnlyHere = slack string (waiting to send), Waiting = one tick (left your device), Delivered
  = two ticks, Read = two ticks in lavender ink, Not delivered (blocked) = slack string in the danger
  ink. Groups: waiting for a host = slack, a host has it = one tick (no member delivery/read yet).
- Tokens: desktop `Tok.tick_read` / `tick_bad`, Android `TinColors.tickRead` / `tickBad`. Light: read
  #374582, bad #A12B23 on the sent bubble #CDEBE3 = 7.10 / 5.75 : 1. Dark: read #DFE3FF, bad #F2B8B5
  on #0E4E43 = 7.57 / 5.62 : 1 (WCAG). The dark danger ink already clears 3:1, so no stronger tint
  was needed; it does read as pink, which is the token's hue.
- Transitions, one shot, only when the state of a message already on screen changes (never on page
  load or scroll-in): slack -> one = pull; one -> two = the second string pulls out of the first;
  two -> read = one pluck + colour ease (240 ms, ease-out); waiting -> not delivered = colour ease;
  waiting -> two = both pull together (not in the lab, which steps one state at a time). Reduced
  motion shows the end state. The slot never changes size.
- Provenance: desktop message menu header ("Read on bob's device" / "On bob's device" / "Waiting for
  bob's device to confirm" / "Only on this computer so far"), "Signed by bob", one line per signing
  device ("Device dbebb7e4 · delivered, read"). The core has no display name for a contact's install
  (receipts carry the scoped install ID; device labels exist only for our own linked devices), so the
  short ID stays. Android action sheet: same headline plus one line per proof.
- Desktop menu rows are now vertically centred in their 44 px rows (the gap under the last row).
- Test hooks (desktop, test-hooks builds only): `P2P_MENU_FIRST`, `P2P_SHOT_EVERY=dir` (window to
  dir/NNN.png twice a second), `P2P_TICK_AT_MS=n` (freeze every transition at n ms).
- Open: whether users can turn read receipts off.

Files: `crates/desktop/src/{ticks.rs, chat.rs, chat_view.rs, chat_fake.rs, group_live_view.rs,
ui.rs, app.rs, main.rs}`, `crates/tin-icons` (+ generated), `android/.../{ReceiptTicks.kt,
ChatSource.kt, ChatUi.kt, ChatParts.kt, ChatGlyphs.kt, PgIcons.kt, Chats.kt, Conversation.kt,
GroupsLive.kt, Theme.kt}`. The desktop fake chat never reports Delivered/Read
(it has no signatures): at most Waiting.

The sections below are the original handoff contract (kept for reference).

## Core contract (additive; nothing existing changed shape)

- `Node::chat_day_receipts(peer_did, day) -> Vec<MessageReceiptStatus>`: one call per
  rendered day page (same order as `chat_day`). Blocking: call on the IO thread.
- `Node::chat_message_receipts(peer_did, message_id) -> MessageReceiptStatus`: one row
  (message detail / info sheet).
- `MessageReceiptStatus { message_id, version (hex), recipients: [RecipientReceipts] }`;
  `RecipientReceipts { did, level: Unconfirmed | Delivered | Read, proofs: [ReceiptProof
  { kind: Delivered | Read, signer_did, signer_device (hex), receipt_id, scope_day,
  covered_versions }] }`. For "You" `recipients` is empty.
- Refresh trigger: `ChatEvents::on_message_changed(message)` is now also raised when the
  contact's signed statements for that message's shown version were committed. Re-read
  `chat_message_receipts` for that id (or the day). No new callback was added (adding one
  would break every existing `ChatEvents` implementation).

## What to show (proposal; presentation is the UI owner's/user's call)

Keep the meanings distinct. Never derive Delivered/Read from `Message.delivery`.

| State | Source | Suggested look (outgoing bubble) |
|---|---|---|
| Only here | `delivery == Pending` | clock (offline) / one tick — unchanged |
| Reached their device (unsigned) | `delivery == Delivered`, level `Unconfirmed` | two ticks, as today (legacy, transport ack) |
| They confirmed | level `Delivered` | two ticks + small "signed" affordance, or unchanged two ticks with detail-sheet provenance (UI decision) |
| They read it | level `Read` | distinct read mark (e.g. filled/accent ticks) |

Why: today's two ticks are the contact device's transport ack — honest "on their phone",
but unsigned. Signed Delivered is the same meaning with portable proof, so it may share
the two-tick look; Read is new and needs its own mark. Host storage (groups) is a third,
different meaning and must never look like Read/Delivered by a person.

Detail sheet (Android: the long-press info line at `Conversation.kt` ~L860; desktop: the
message menu/info): list proofs as "Delivered to <name>'s device <short device> · signed",
"Read on <name>'s device · signed". Copy voice: see `docs/design/VOICE.md` ("you/your" in
text). Do not say "seen by <name>" as a fact about a person: a signature proves the
device's claim, not human attention.

Edits: after an edit the shown version is new, so the level drops to `Unconfirmed` until
the contact's device confirms the edit; an earlier Read does not carry over. Show this as
"edited" plus the current level; do not animate it as a failure. No layout shift: reserve
the tick slot (user rule `ui-no-layout-shift`).

Incoming messages: `recipients[0].did` is our own DID; `level` says what OUR account has
signed (Delivered on arrival, Read after `mark_read`). Optional to show; useful in a
debug/provenance view only.

## Implementation hints

- Iced (`chat_view.rs`): extend the page state with a `HashMap<message_id, ReceiptLevel>`
  loaded by a `Task` calling `chat_day_receipts` when a day page loads, and updated by
  the `on_message_changed` subscription path. The tick render (`fn ticks`, the `.meta`
  row near L610) takes the level as an extra input. Keep `Icon` choices in `pg_icons`.
- Compose (`Conversation.kt`): load `chatDayReceipts` alongside each day page on the IO
  dispatcher into a `SnapshotStateMap<String, ReceiptLevel>`; on `ChatEvent.Changed`
  re-query that id. `MetaTick` gains `Read`; the existing `Delivered` branch stays for
  the legacy tick. `Chats.kt` (list row) can stay legacy until a list-level API exists.
- Tests: an app-level demonstration must run two real installs (or the core test
  `crates/core/tests/message_receipts.rs` as the fixture), not fake data. Physical-device
  runs need explicit authorization and the device lock.
