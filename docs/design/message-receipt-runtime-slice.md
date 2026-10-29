# 1:1 message receipts: runtime issuance (M3) and chat/6 exchange (M4)

Status: **implemented in `feature/receipt-runtime`, core only, not merged**. 1:1 (DM)
conversations only. Group message receipts are NOT done (see "Not done" and the UI handoff
below). Own-device receipt replication landed later (`fix/own-device-receipts`, step 6).
The chat wire bump to `tinline/chat/6` needs the user's OK before it reaches `dev`.

## What now happens in the real core runtime

1. **Delivered.** When a batch or a history snapshot from the contact is accepted,
   the chat engine writes, in the SAME transaction as the message rows, an intent row
   naming the exact incoming versions it made durable (`chat_commit` /
   `chat_apply_history` -> `Inner::chat_delivered_intent`). A batch relayed by one of
   our own devices adds no intent: that device accepted it from the contact and issues.
2. **Read.** `mark_read` writes, in the transaction that moves the read cursor, an
   intent for exactly the incoming, undeleted versions shown on this device whose `at`
   lies in that step (old cursor, new cursor]. A cursor that arrives from another of our
   devices (`SelfMsg::Read`) issues nothing on this device. Late messages at or below
   the cursor stay without a read receipt (a hole, not a claim).
3. **Signing.** `MessageReceipts::pump` (after each commit point, and once when the chat
   store opens) signs one `Receipt { kind, scope = (Direct(pair), day), recipient = our
   DID, device = this install, messages = exact sorted version digests }` per <=256
   versions with the account DID key, appends it to the per-day receipt ledger and, in
   the same transaction, deletes the intent row and marks the day in the receipt outbox.
   Failure leaves the intent queued; a retry re-signs nothing already covered by ANY
   device of our account (one device suffices for the account).
4. **Exchange (`tinline/chat/6`).** After `Hello`, each side sends `ReceiptHello` (its
   receipt-ledger VV for recent days we hold plus unacknowledged outbox days). Each side
   answers with `Receipts { day, update }`: original native ledger ops the peer lacks
   (bounded exporter, 256 KiB / 64 ops per frame, 16 frames per pass). New statements
   are pushed live to the contact's sessions. The receiver imports through
   `MessageReceipts::import_direct`: Loro structure, DID signature, writer = issuer's
   writer ID, scope = this conversation's pair and day, kind Delivered/Read only, issuer
   is the session's DID or our own (statements a peer relays back). It replies
   `ReceiptAck { day, vv }`; an ack covering this install's statements clears the
   outbox mark. A refused receipt frame is logged ("rejected receipts of ...") and not
   stored; the chat session itself continues.
5. **Projection.** `Node::chat_message_receipts(peer_did, message_id)` (additive UniFFI)
   returns `MessageReceiptStatus { message_id, version, recipients: [RecipientReceipts {
   did, level: Unconfirmed|Delivered|Read, proofs: [ReceiptProof { kind, signer_did,
   signer_device, receipt_id, scope_day, covered_versions }] }] }` from committed ledger
   entries only. For our message: the contact's statements. For theirs: our account's
   own statements. "You" has no recipients. After an import commits statements covering
   our shown outgoing version, the engine raises `ChatEvents::on_message_changed`.

6. **Own devices (`cap/dm-receipts/v1` on `tinline/self/5`).** The ledgers replicate across
   our own devices as the original ops: `DmReceiptHello` at session start (held days with
   receipts, <= 1024), `DmReceipts { doc, vv, update }` with what the peer lacks (same
   bounded exporter), `DmReceiptAck`; pushed live via the `!receipts/{doc}` dirty mark when
   a contact frame, our pump or another own device adds statements. Admission is
   `import_direct` with the conversation's contact (so the same DID/writer/kind/day rules)
   plus a contact in good standing; a refused frame is logged and dropped. Our own
   statements replicate too (another device relays them to the contact; the pump sees them
   as account coverage). Then the same `on_message_changed` events. docs/chat.md section 8.

## Exact version identity

`receipts::store::messages::dm_version(doc, rec)` = BLAKE3 (receipt message domain) over
`tinline-receipt-dm-version-v1\0 ‖ len‖doc ‖ len‖id ‖ len‖author ‖ dev ‖ len‖did_sig`.
`did_sig` is the author's DID signature that every replica verified over the version's
full content and its exact op span before storing it, so equal digests name the same
signed version on both sides. Every edit/delete is a new signed version, therefore a new
digest: delivery is re-confirmed by the recipient's next acceptance; an earlier read does
not cover the edit. Group records will use `group_version(stored SignedRecord bytes)`
(defined, not yet wired).

## What the legacy indicators still mean

`Message.delivery` / `DeliveryState` (the contact's unsigned transport `Ack` VV) and
group `GroupDelivery` (host storage acks) are **unchanged** and are not recipient
evidence. `ReceiptLevel` never reads them. Apps must not merge the two; see handoff.

## Policy decisions applied (user, 2026-10-15) and their limits

- One device of the recipient account suffices; the projection reports which device.
- Receipts are never removed; reopening does not re-judge accepted statements against
  current contact/block state (structural admission only).
- 1:1 admission is DID-level: chat content has no device-list authority either (U18), so
  any install holding the account key can sign; this is the shared-mnemonic limit.
- Delayed statements from removed devices: **open** (see roadmap). 1:1 has no device
  admission, so nothing here selects that policy for groups.
- Read receipts are always issued on `mark_read`; whether users can turn them off is an
  **open product question** (not asked yet).

## Not done (honest)

- **Desktop and Android displays** were added afterwards
  ([status display](message-receipt-ui-handoff.md)). Desktop was demonstrated against a
  real p2p-peer (Waiting -> Delivered -> Read screenshots, outside Git); Android was
  built and unit-tested but not run on an emulator or phone. The 1:1 product gate is
  therefore met on desktop only; Android needs a device/emulator demonstration.
- Group message receipts: no issuance, exchange or visibility yet (needs group store
  transaction seam, member admission at first acceptance, group/2 frames via hosts).
- Own-device replication only lists the newest 1024 held days with receipts at session start;
  an older day that gains a statement while no session is up waits until it changes again
  with a session up (statements still arrive from the contact itself).
- Closed days (older than 8 days, snapshot blob only) are skipped by read intents.
- No per-session aggregate scheduler/quota; ledgers and outbox have no retention quota
  (by decision: keep all copies until M7 shard signing).

## Validation

See the roadmap M3/M4 entries for exact commands and results of each run. Unit tests
(`receipts::store::messages::tests`) use real DID-signed chat versions and sealed redb
stores; the network test `crates/core/tests/message_receipts.rs` runs two real nodes over
iroh. Neither is a desktop or Android UI demonstration.
