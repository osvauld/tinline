# Signed message receipts: desktop/Android status display

Status: **built in `feature/receipt-runtime` (2026-10-15), not merged**. Desktop (iced) and
Android (Compose) draw the 1:1 bubble mark from the core's `ReceiptTick` and show signed
provenance in the message menu / action sheet. Group receipts are not built.

## What was built

- Core decides the mark: `MessageReceiptStatus.tick: ReceiptTick { None, OnlyHere, Waiting,
  Delivered, Read }`. `Delivered`/`Read` come only from committed recipient-signed
  statements for the exact shown version; the legacy transport ack can at most move
  `OnlyHere` to `Waiting`. Both apps only map tick -> glyph (Rust-first).
- Marks: clock = `OnlyHere`/`Waiting` (never looks delivered), check = signed Delivered,
  new `read` glyph (a second check beside the first, tin-icons `read`, Android
  `Pg.TickRead`) = signed Read, brighter tint. The 13 x 11 slot is always reserved
  (Android `MetaTick.Blank` while loading), so no layout shift.
- Provenance: desktop message menu header ("Read on bob’s device" / "On bob’s device" /
  "Waiting for bob’s device to confirm" / "Only on this computer so far"), then "Signed by
  bob" and one line per signing device ("Device dbebb7e4 · delivered, read"). Android
  action sheet: the same headline in the "Sent hh:mm · ..." hint plus one line per proof.
- Loading: per day page `chat_day_receipts`; single message re-read on send, edit/delete,
  `on_message_changed` (raised when signed receipts commit) and `on_delivery_changed`.
  `chat_list_receipts` exists for list rows; neither app's list draws a delivered mark today,
  so it is unused by the UIs.
- Desktop test hook `P2P_MENU_FIRST=1` opens the oldest loaded message's menu for screenshots.
- No read-receipt off switch (open product decision).

Files: `crates/desktop/src/{chat.rs, chat_view.rs, chat_fake.rs}`, `crates/tin-icons`
(+ generated), `android/.../{ChatSource.kt, ChatUi.kt, ChatParts.kt, ChatGlyphs.kt,
PgIcons.kt, Chats.kt, Conversation.kt}`. The desktop fake chat never reports Delivered/Read
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
