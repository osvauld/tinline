# Chat — design

Status: 1:1 chat built (core + peer, 2026-10-08); groups still design. Order decided: **1:1 chat first, then groups**, then group calls.
Pure P2P: no node, no server of ours. Reuses osvauld2's `courier` (role tokens, chain
verification, the sync round trip) and its chat design (`~/osvauld2/docs/design/group-chat-sync.md`),
adapted from "the node is the referee" to "every device is its own referee".

## 0. The one rule everything rests on

There is no authority to reject a write and make the writer roll back. Every device validates
every update itself, so validation must be **deterministic**: two honest devices that receive
the same updates, in any order, must reach the same verdict. Hence:

- An update is judged against the permit state **as of the version it names** (its causal
  past), never against whatever the receiver knows now.
- A rejected update is **dropped and never forwarded**. Nothing is "replaced": there is no
  authoritative copy to replace it with.

## 1. Identity and authorship (exists)

`crates/proto` already gives: DID (the person) → DID-signed attestation → device key (the iroh
endpoint key, proven by the QUIC handshake). Chat adds one thing: every update batch is
**signed by the author's attested device key**, and the batch may only contain Loro ops from
that device's Loro peer id.

- Loro peer id per device, per doc: the first 8 bytes of `BLAKE3("tinline-loro-peer-v1" ‖
  device key ‖ doc name)`, set before the device writes. A receiver rejects a batch carrying ops
  from any other peer id. This is what makes "who wrote this op" checkable.
- Superseded for message content by P8/P9/P9b (section 7, "Signatures and writer IDs"): messages
  are signed by the DID key, and devices write under a writer ID derived from the DID and a random
  device ID, not from the device key (which is the network address and will rotate).

## 2. Phase 1: 1:1 chat

### Docs
- `dm/{pair}/{day}`: the conversation, one Loro doc per UTC day (`YYYY-MM-DD`). `{pair}` =
  hex of `BLAKE3(sorted(did_a, did_b))`, so both sides name it identically.
- UTC days, not local days: both sides must agree on the shard boundary regardless of time zone.
  The UI groups by local day; that is presentation only.
- No permits needed: the existing contact relationship (grant) is the permission. Removing a
  contact blocks the DID (as today) and deletes the conversation on this device, like call
  history.

### Message record (from group-chat-sync §3, trimmed)
```
messages/<id> = { id, author, text, at, edited_at?, deleted?, reply_to?, file? }
```
- `id`: random 128-bit, chosen by the author. `author`, `at`, `id`: immutable.
- Insert: `author` = the signer's DID; `|at − receiver's now| < 24 h` on arrival for live
  pushes (looser than the node's 300 s: phones' clocks drift and messages arrive late), and
  `at` must fall inside the shard's day ± 1 h.
- `text`, `edited_at`, `deleted`: author only. A deleted message keeps its id; text is cleared.
- Reactions (later): `reactions/<did>/<emoji>`, each DID writes only its own slot.

### Wire
New ALPN `tinline/chat/1` on the same iroh endpoint as calls. The connection opens with the
same proof as a call (attestation + grant, `proto`); the rest is framed like the call control
stream.

- `ChatHello { shards: [(day, vv)] }`: the last 7 days we hold. Both sides send it.
- `ChatSync { doc, vv, update, sig }`: courier's round trip, made symmetric: "here is my
  version and what you lack". The receiver validates, imports, and answers with its own
  `ChatSync` for what the sender lacks (an empty update is fine).
- `ChatPush { doc, update, sig }`: live, after a local write, while connected.
- `ChatAck { doc, vv }`: "I now hold up to here" — this drives the ticks.
- Older shards: `ChatHistory { before: day, limit }` → the peer lists `(day, vv, snapshot
  hash)`; we fetch only what the user scrolls to, closed days as blobs over iroh-blobs.

### Delivery and offline
1:1 has no third member to relay through, so **a message is sent when both devices are
online at the same time**. With the always-on service that is usually seconds; when it isn't:

- A local write goes to the outbox (it is already in the local doc). On every connection to
  that contact, `ChatHello` reconciles everything automatically. Retry with backoff while the
  app is in the foreground; the always-on service retries on network change.
- Ticks: one = written on this device; two = the peer's `ChatAck` covers it; read (later) =
  the peer's read cursor (`user/{did}` doc, own slot only) passes it.
- Notifications: the foreground service receives pushes and posts a messaging-style
  notification (`MessagingStyle`), quiet if the conversation is open.

### Storage (decided 2026-10-07: our vault + iroh-blobs)
Two stores, both in the app's private data dir:

- **Vault records**: osvauld2's `storage` (one redb file, sortable path keys) with `vault`'s
  sealed-record pattern (`put_doc` / `put_entry`: Loro-free, snapshots are opaque sealed
  bytes). Sealed with Tinline's existing data key (DEK, `crates/core/src/vault.rs`), not a
  per-login passphrase: the phone unlocks at boot from the Keystore-remembered DEK, so chat
  must be readable then too. Holds: open shards (Loro snapshot + appended updates), the shard
  index (`dm/{pair}/{day}` → vv, closed-snapshot hash), outbox, read cursors.
- **Blob store**: iroh-blobs, content-addressed by BLAKE3. Holds files/images/voice notes
  (§3) and **closed shards**: a past day's doc is compacted into one snapshot blob and the
  index records its hash. Messages and the index refer to content only by hash; the app loads
  by hash and, if it isn't local, fetches it from a peer that may read it. Scrolling back and
  receiving a file are then the same operation.
- **Blobs are encrypted, and the vault holds their keys** (decided 2026-10-07). The author
  encrypts each blob once with a fresh random AES-256-GCM key (chunked, so large files stream
  and resume), and stores the **ciphertext** in iroh-blobs: the hash is the ciphertext's.
  The referencing message carries `{hash, key, name, size, mime}` (end-to-end encrypted in
  transit, sealed in the vault at rest), and the vault index keeps `hash → key`. Every device
  stores and serves the same ciphertext, so fetching by hash works across devices, and a
  member who forwards a blob handles only ciphertext. Random keys, not content-derived ones:
  convergent keys would dedupe identical files but reveal that two people hold the same file.
- A closed shard that is edited later (an edit or delete of an old message) gets a delta in
  the vault; the next compaction makes a new snapshot blob and drops the old one.

## 3. Phase 2: files (iroh-blobs)

A message's `file = { hash (BLAKE3 of the ciphertext), key, name, size, mime }` (see §2
Storage). The recipient fetches the ciphertext from the author, or from any member who already
has it (groups), and decrypts with `key`. A device serves a blob only to a peer
that may read a doc referencing that hash. Auto-download limits per network type; resumable.

## 4. Phase 3: groups

### Genesis and permits
- The creator signs `{created_at, creator DID, nonce}`; the group id is its hash, so it can't be
  claimed or forged later.
- Permits are courier `Token`s rooted at the **creator's DID** (`token::verify_chain(leaf,
  root_did = creator, holder, now, revoked)`). Roles: `admin`, `member` (maybe `moderator`).
  **Admins may create admins** (decided); delegation depth is capped.
- `group/{gid}/meta` (not sharded): members, permits, revocations, name, avatar, history policy.
  Every permit in it is self-verifying; a write to meta is validated like any other.

### Messages
`group/{gid}/{day}` like DMs. Every batch carries `{update, author's permit chain, meta
frontier it was written against, sig}`. Validation = `access.rs`-style rules evaluated on the
meta doc **checked out at that frontier**; Loro can check out any past version, which is what
makes the verdict the same on every device.

### Removal
Removal = a revocation in meta. Posts **concurrent** with the removal are accepted; anything the
removed member writes with the removal in its causal past is rejected. ("Erase concurrent
posts too" is not done: later changes depend on earlier ones and would be lost with them.)

### Delivery
- v1 (groups up to ~50): direct member-to-member sync over authenticated connections,
  `ChatHello` reconciliation with every online member, and any member forwards what it holds.
  No group key: every hop is an end-to-end encrypted link between two verified members.
- Later, large groups: iroh-gossip per group for fan-out. Gossip may route via non-members, so
  that needs a group key rotated on every removal.
- Ticks: one = held by at least one other member (from then on it reaches the rest without
  you); two = held by all.

### History for new members
Per-group setting from courier's manifest: `history all` or `since_join` (day precision).

## 5. Phase 4: group calls

- Mesh up to ~5–6 people: everyone sends Opus to everyone (~32 kbps per stream up).
- Up to ~15–20: one member (best uplink, often a desktop) acts as host and forwards streams —
  an SFU that is a member, not a server.
- Beyond that is a broadcast/rooms product, out of scope.

## 6. Done means these pass (1:1, written before the code)

End to end, two desktop peers (`crates/peer`) and the Android emulators:

| # | scenario |
|---|---|
| C1 | A sends 3 messages while B is online; B has them within 2 s, in order; A sees two ticks |
| C2 | B offline; A sends 5; B comes online; both converge without anyone opening the chat |
| C3 | Both offline from each other write concurrently; on reconnect both show the same 2-way merge, ordered by `at` then id |
| C4 | A edits and deletes a message; B shows the edit and the deletion |
| C5 | A forged batch (B's peer id, or `author` = B, or signed by a third device) is rejected and not stored |
| C6 | Messages on two UTC days land in two shards; a fresh open loads only today; scrolling back pulls the older shard |
| C7 | A removes B: B's later messages are refused at A, and A's copy of the conversation is gone |
| C8 | Android: the app backgrounded, screen off: a message from desktop raises a notification |


## 7. As built (1:1, `crates/core/src/chat/`)

What differs from or settles the text above.

### Code map
`crypt.rs` chunked blob AEAD · `blobs.rs` iroh-blobs hub (store, gated serving, GC) ·
`doc.rs` Loro day shard + batch validation · `wire.rs` frames of `tinline/chat/1` ·
`store.rs` sealed records · `engine.rs` sessions, sync, outbox, transfers, history ·
`api.rs` the UniFFI surface. New crate `crates/storage` (redb + sealed records, copied from
osvauld2 `storage` @ 90d22ec with the `vault` entry idea).

### API (Kotlin-visible, additive; voice recording/encoding is the apps' job)
`Node.set_chat_events(ChatEvents)`, `chats()`, `chat_day(peer_did, day?)` -> `DayPage{day,
messages (oldest first), older_day}`, `fetch_older_history(peer_did, before_day)`,
`send_text(peer_did, text, reply_to?)`, `edit_message`, `delete_message`, `mark_read`,
`send_file(peer_did, path, mime, text?)`, `send_voice(peer_did, path, duration_ms, waveform)`,
`download_attachment(peer_did, message_id)`, `cancel_download(peer_did, message_id)`, `save_attachment(peer_did, message_id, dest)`,
`set_auto_download_limit(bytes)` / `auto_download_limit()` (default 10 MB).
Records: `Chat`, `Message`, `Attachment` (state `Remote|Downloading|Ready|Failed`),
`DeliveryState {Pending, Delivered}`. `ChatEvents`: `on_message_added`, `on_message_changed`,
`on_chat_changed`, `on_delivery_changed`, `on_transfer_progress`. Times are unix ms; a day is the
UTC `YYYY-MM-DD`. A voice message is a file message with `kind = Voice`, `duration_ms` and a
~64 byte waveform; the blob is whatever file the app recorded (Ogg Opus 16 kHz mono).

### Wire (`tinline/chat/1`)
One bidirectional stream, opened by the dialer; frames are u32 BE length + bincode (cap 16 MiB).
`Auth{attestation, grant, relay}` both ways first: the same proof as a call (a call hello with
id "chat" is built and checked by `proto::accept_call_hello`; `proto` is unchanged).
Then `Hello{shards:[(day, vv)]}` (last 7 days + days with unacknowledged ops; also acts as an
ack), `Sync{doc, vv, update, sig}`, `Push{doc, update, sig}`, `Ack{doc, vv}`,
`HistoryReq{before, limit}` / `HistoryResp{days:[{day, vv, hash, key, size}]}`.
`sig` = Ed25519 by the device key over `"tinline-chat-batch-v1" ‖ len(doc) ‖ doc ‖ update`.
A device only ever sends **its own** ops (export from the peer's vv with other peers' counters
masked), so a receiver can require a single Loro peer id per batch.
One session per contact: if both dial at once, the one dialled by the lower device key stays.
Blobs travel on the stock iroh-blobs ALPN of the same endpoint (iroh-blobs 0.103, iroh 1.x).

### Validation (one verdict on every device)
`Shard::apply_remote` imports into a scratch fork and rejects the whole batch (nothing stored,
session closed) if: the blob holds ops under a peer id it may not use (below, "Signatures and
writer IDs"; live: the signer's legacy peer id and the signer's proven writer IDs only); ops are not self-contained; the doc has anything but `messages`; a message has an
unknown or ill-typed field; a new message's `author` is not the signer; `at` is outside the
shard's day ±1 h (live pushes: also |at − now| < 24 h); an existing message changed `id`,
`author`, `at`, `reply_to` or `file`; someone but the author changed `text/edited_at/deleted`;
a message disappeared; a deleted message kept text. Unit tests: `chat::doc_tests`.

### On disk (app data dir)
`chat.redb`: sealed records (AES-256-GCM under `BLAKE3-derive-key("tinline chat store v1",
DEK)`, each bound to its path), layout in `store.rs` (conversations, shard snapshot + appended
updates + meta, acks, message counters, outbox markers, blob key index, read-permission refs).
`blobs/`: the iroh-blobs store (ciphertext only; tags `tl/<hash>`; GC every 30 s). Existing
`profile.json`/`state.json`/`calls.json` are untouched.

### Decisions where the text was silent
- Ticks track the *creation* op of a message; an edit does not take the second tick away.
- A message id -> day index (`mi/`) lets edit/delete take only the id.
- Conflicting concurrent edits of one message (same author, two devices) are last-writer-wins
  per field (plain map values, not Loro text).
- Outbox = the Loro doc plus an `out/{pair}/{day}` marker until the peer's vv covers our ops.
  Dial at start, on local write, on `network_changed`, every 30 s while something is unsent, with
  2..60 s backoff.
- Auto-download: incoming files up to the limit are fetched as soon as the message is applied;
  others on `download_attachment`. Retries on every new session.
- **Reliable transfer (task 32, details in `docs/iroh-blobs.md` section 2).** A fetch with no
  progress for 20 s fails; failed fetches retry by themselves after 2, 5, 15, 30, 60 s (state stays
  `Downloading` meanwhile), then `Failed` ("Tap to retry" = `download_attachment` again, which
  resumes from the bytes held). `cancel_download` returns an attachment to `Remote` and frees the
  partial bytes. Progress after an app restart starts at the bytes already on disk.
  `on_transfer_progress(.., 0, 0, outgoing)` = the transfer ended without completing (for the
  sender: the peer dropped, so leave "Sending n%").
- Serving a blob: only to a device that is a contact's (or in an authenticated session) AND only
  if a message of that conversation names the hash (`ref/{hash}/{pair}`). Push/observe refused.
- Closed days: days 8+ days old are compacted into an encrypted snapshot blob (`closed` in the
  shard meta) by a periodic task; opening one decrypts the blob. A peer's `HistoryReq` is
  answered with such a blob (made on demand, cached) and its key, inside the authenticated link.
- **Per-message author signatures.** Every message record carries `sig`, an Ed25519 signature by
  the author's attested device key over `"tinline-chat-msg-v1"` and the length-prefixed doc id
  (`dm/{pair}/{day}`), message id, author DID, `at`, current text, `edited_at`, `deleted`,
  `reply_to` and the file ref (hash, key, name, size, mime, kind, duration, waveform). Creating,
  editing and deleting all re-sign. Every apply path (live batch, history snapshot) verifies each
  new or changed message against the author's device key (the peer's from the session, ours from
  the profile); unsigned or mis-signed messages reject the whole batch. Because the doc id is
  signed a message cannot be replayed into another day or pair, and a changed message may not move
  `edited_at` backwards or un-delete, so an old signed version cannot roll back an edit.
- **Vouched import.** History snapshots (older days, or a day whose batch depends on ops of ours
  that we lost, e.g. after a reinstall) are accepted op-agnostically: ops of either party may
  appear, but each message must carry a valid signature of its author. A contact therefore cannot
  put words in our mouth; the worst it can do is withhold messages.
- Contact removal: the conversation records and all its blobs are dropped (bytes freed by the
  next GC run); the block already refuses later sessions.
- No `proto` change; `crates/audio` untouched (voice encode/decode left to the apps).
- Test knob: `P2P_CHAT_CLOCK_MS_OFFSET` shifts the chat clock (day-shard tests only).

### Signatures and writer IDs (P8, P9, P9b; byte formats in docs/protocol.md)
- **Dual signature (P8, U4 revised).** Every message version is signed by the author's **DID key**
  and, for apps that predate that, still by the writing device key (the record's `sig`). The DID
  signature rides in the commit message ("note") of the Loro change that writes the version, so the
  record keeps its legacy shape and older apps keep working. A version that carries a DID signature
  is judged by it alone (a valid device signature never rescues a bad one); a version without one
  is judged by the device key against the attested keys. A restore changes the device key but not
  the DID, so DID-signed messages verify on devices that never knew either key.
- **Writer ID (P9, U16).** Devices write under `writer_id(DID, device ID)` as the Loro peer id; the
  device ID is random per install (P9b: stored as `device_id` in `account.json`, new on create, link
  and restore, kept when only the device key changes). Every note carries the device ID, every
  receiver recomputes the hash; all replicas keep the sender's number. Compatibility: a device
  writes under its legacy (device-key) peer id for a contact until every known device of that
  contact has shown it judges writer IDs (`w9/`), because apps before P9 refuse other peer ids live.
- **Writer bound into the DID signature (P9b, U17 a).** The DID signature also covers where the
  version is written: the change's peer id, first counter and length. It proves "this DID wrote this
  version as this change under this writer ID", and a genuine note copied onto another change does
  not verify. Ops under a writer ID (remembered from earlier batches, ours, or claimed anywhere in
  the doc) are accepted only from proven writer changes: the note's device ID hashes with the noted
  message's author to the peer id, the version right after the change is the noted one and its DID
  signature verifies for that change, and the change writes nothing but that message. Ops under any
  other peer id: live, only the session's legacy peer id; in a vouched snapshot or own-device sync,
  a legacy peer id derived from a device key of the author we know, or else only the contact's own
  messages. So no one can place **our** messages, or copies of our notes, under a peer id we cannot
  account for.
- **Per-DID requirement (P9b, U17 b).** Once a DID has DID-signed a version (seen in any batch, or
  written by us), its versions from then on need a DID signature: `req/{did}` keeps the earliest
  version time seen DID-signed (a batch's own DID-signed versions count too), and a version at or
  after it with only a device signature counts as unsigned: a live batch is refused, a new message
  of the contact's in history stays hidden, anything else refuses the snapshot. Older versions
  (fetched history from before the upgrade) are still judged by the device key. The flag outlives
  removing the contact. Consequence: a device of the same account that is not yet upgraded is
  refused by upgraded devices (contacts' and our own) for everything it writes after that time, and
  if such messages sit unsent when it upgrades, its catch-up batch for that day is refused as a
  whole (live batches are all-or-nothing). Users must upgrade every device.
- **Limits.** (1) In a vouched snapshot, ops under a peer id we cannot account for are still
  accepted when they write only the contact's own messages, because that is how the contact's
  restored or never-met devices reach us (P1, P8; the first messages of every conversation are
  written in compatibility mode). Such a peer id may be a writer ID of one of our siblings that this
  device has not seen yet: the contact can occupy it with its own messages, and that sibling's real
  ops under it are then lost on this device (refused once the writer ID is known here). Policy
  options: share own devices' device IDs between them (account doc), or refuse unknown peer ids in
  history once device lists are pushed to contacts (P4). (2) The ops of a proven change are not
  signed one by one: a relaying contact could alter ops that leave the signed version unchanged.
  (3) A holder of the device key (not the DID key) can still backdate a device-signed edit to before
  the DID's cut-off time. (4) Same-DID copying by a holder of the recovery phrase is the accepted
  shared-mnemonic limit.

## 8. Own devices (task 34g)

A person's linked devices hold the same conversations. Pair ids are `BLAKE3(sorted DIDs)`, so
`dm/{pair}/{day}` is the same doc on every device of the account. The devices exchange shards over
the own-device link (`tinline/self/1`, `docs/protocol.md` "Own-device sync"); a contact never
learns how many devices we have.

### What is exchanged
- `Hello` lists every shard we hold (up to 4096, newest day first) with its version vector, next to
  the `account` doc. Docs the peer does not list it has none of: we send them whole. Then
  `Sync{doc, vv, update, sig}` carries the ops beyond the peer's vector, **all peers' ops** (ours,
  the other device's, the contact's). A write on any device marks its shard dirty and is pushed to
  every connected own device at once. Older (closed) days travel the same way, only when a vector
  differs; the cap bounds a session's work.
- A batch relayed by an own device is validated like a vouched history snapshot (`Ctx::history`):
  the QUIC-authenticated own device signed the batch; the doc must be a conversation of ours (a
  contact in good standing, or "You"); authors are only we and that contact; and **every new or
  changed message must carry a valid signature of its author's attested device key**: our devices
  (the registry, removed ones included so old messages stay valid) or the devices we know of the
  contact. So a relayed batch can neither forge the contact's words nor ours, and a message from
  the contact verifies against the contact's attestation, not against the relay. A rejected batch
  is dropped (nothing stored) and the session goes on; the usual per-field rules (immutable
  `id/author/at/reply_to/file`, no un-delete, no removal, day window) hold.
- A device still sends the **contact** only its own ops (`export_own_since`), and the contact
  accepts a batch from a session device only with that device's peer id. So each of our devices
  syncs its own writes with the contact itself; a contact keeps one session per *device* (a person's
  devices no longer replace each other's sessions). Messages the contact wrote reach our second
  device through the first, with the contact offline. Duplicates cannot happen: a message is a
  map entry keyed by its random id.

### Read state
`ConvMeta.read_upto` is a read cursor: everything incoming whose `at` is at or before it is read.
`mark_read` moves it to the newest incoming message applied here and zeroes `unread`. The cursor
is a max-register synced with `SelfMsg::Read{(pair, ms)}` (sent at session start and when it
moves, forwarded onward). A device receiving a larger cursor stores it and **recounts** unread
(incoming, not deleted, `at` > cursor), so reading on one device clears the other, and the order in
which messages and cursors arrive does not matter. A new message counts as unread only if its
`at` is above the cursor. A message that arrives late with an `at` below the cursor counts as read.

### Delivery state
Ticks are per device pair. The device that wrote a message shows `Pending` until the contact's
ack covers it (exactly as before). Another of our devices shows a message written elsewhere as
`Delivered` once it holds it, which means "held by your devices"; it has no counter for it and does
not know whether the contact has it. "You" items are always `Delivered`.

### Files and voice
The blob is ciphertext addressed by hash, so any holder can serve it. A device serves a blob to
a contact only for a conversation with that contact (unchanged); **to our own devices it serves
any blob that any of our conversations references** (`ref/{hash}/*`). A fetch tries, in order: the
contact's live session device, our own devices we are in sync with, the contact's other devices,
our remaining own devices; the first that works delivers. A message we wrote on another device
(`all_remote`) is fetched like an incoming one (auto-download under the limit). `Attachment.state`
of such a message reflects this device's bytes.

### "You" (saved items)
A conversation with ourselves: `peer_did` = our own DID, pair `BLAKE3(did, did)`, docs
`dm/{pair}/{day}` as for any chat. It is offered only over the own-device link: a contact's session
accepts only `dm/{its own pair}/...`, so a contact can neither request nor push these docs
(`doc_day_of`), and `Hello`/`chats()` for contacts never mention them. API: the existing calls
(`send_text`, `send_file`, `edit_message`, `delete_message`, `chat_day`, `mark_read`,
`download_attachment`, `save_attachment`) with `peer_did = own DID`. `chats()` always contains
this row with `peer_name = "You"`. Messages are `outgoing`, `Delivered`, and never create an
outbox marker; `fetch_older_history` is a no-op returning 0.

### Removing a contact
`remove_contact` drops the conversation (shards, blobs, rows) on this device and tombstones the
contact in the account doc; the other devices apply the tombstone and `chat_purge` the same
conversation. Relayed shards of a pair that is no longer a contact (and was blocked) are ignored,
so the sync cannot bring it back.

### Known limits
A batch relayed to a device that does not yet know the author's device (a contact's new device
learned by one of our devices only) fails the signature check and is dropped until the contact
list reaches it; it is retried at every session. Read cursors are per conversation, not per
message.
