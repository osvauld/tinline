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
session closed) if: the blob holds ops of a peer id other than the signer's device peer id for
that doc; ops are not self-contained; the doc has anything but `messages`; a message has an
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
