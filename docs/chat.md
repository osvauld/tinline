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
endpoint key, proven by the QUIC handshake). The device key authenticates the *session* (and
signs each batch on the wire); it signs no chat content. Content (P9c, section 7 "Signatures and
writer IDs"):

- Every message version is signed by its author's **DID key**, and a device writes every chat doc
  under its **writer ID** = hash(DID ‖ device ID) as the Loro peer id, where the device ID is
  random per install, not derived from any key. A receiver accepts an op only if it lies in a span
  whose DID signature proves it, under that author's writer ID. This is what makes "who wrote this
  op" checkable, on every device, without knowing any of the author's device keys.

## 2. Phase 1: 1:1 chat

### Docs
- `dm/{pair}/{day}`: the conversation, one Loro doc per UTC day (`YYYY-MM-DD`). `{pair}` =
  hex of `BLAKE3(sorted(did_a, did_b))`, so both sides name it identically.
- UTC days, not local days: both sides must agree on the shard boundary regardless of time zone.
  The UI groups by local day; that is presentation only.
- No permits needed: the existing contact relationship (grant) is the permission. A contact's
  action is Block (U19); chats are never deleted (U24), so a day's doc lives as long as the
  account's chat store (P10: see "Writer counters never repeat").

### Message record (from group-chat-sync §3, trimmed)
```
messages/<id> = { id, author, at, reply_to?, file?,
                  v:<writer ID, 16 hex>: { text, edited_at?, deleted, dev, did_sig }, ... }
```
- `id`: random 128-bit, chosen by the author. `author`, `at`, `id`, `reply_to`, `file`: immutable,
  written once. Each writer (one of the author's installs) files its latest version of the
  message under its own key `v:<writer ID>` as one value: its content, the writing install's
  device ID (`dev`) and the author's DID signature (`did_sig`; section 7).
- What a message shows is its highest-ranked version (P10): a delete beats any edit, then the
  later `edited_at`, then the higher writer ID. A writer's own versions never go back (older
  edit time, undone delete). A device's edit gets an `edited_at` just above what it shows, so it
  supersedes it even with a lagging clock; a deleted message cannot be edited.
- Insert: `author` = the signer's DID; `|at − receiver's now| < 24 h` on arrival for live
  pushes (looser than the node's 300 s: phones' clocks drift and messages arrive late), and
  `at` must fall inside the shard's day ± 1 h.
- `text`, `edited_at`, `deleted`: author only. A deleted message keeps its id; text is cleared.
- Reactions (later): `reactions/<did>/<emoji>`, each DID writes only its own slot.

### Wire
ALPN `tinline/chat/6` (signed receipts; `/1` to `/5` are refused, see "Wire" below) on the same iroh endpoint as calls. The connection opens with the
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
- Ticks: one = written on this device; two = the peer's `ChatAck` covers it (unsigned
  transport ack, unchanged). Recipient-signed delivered/read receipts are separate
  evidence (chat/6, section 9); the earlier "read cursor" idea was not built.
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

**Updated direction:** [group-chat.md](group-chat.md) specifies member-hosted group sync:
multiple opt-in desktop/member hosts cache and relay, every client validates original author
proofs, and fixed Rust schemas replace kunki's generic manifest/Lua layer. No authoritative
host, no rollback to host state, direct member sync fallback, no gossip in v1. The summary
below is subordinate to that design; causal revocation and proof-preserving history are
security gates, not solved merely by a meta frontier or message signatures.

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
Removal = a revocation in meta. Intended policy: posts genuinely concurrent with removal
remain valid; writes with removal in their causal past are rejected. A signer-selected stale
meta frontier cannot prove concurrency or prevent malicious backdating. The enforcement and
partition semantics must be resolved and tested before implementing this policy; see
[group-chat.md §6](group-chat.md#6-validation-and-causal-policy).

### Delivery
- v1: authenticated member-host sync; multiple nominated devices cache and forward original
  signed envelopes, with direct member sync as fallback. Hosts reconcile symmetrically using
  version vectors and original author proofs, not host-signed aggregate snapshots.
- QUIC protects each hop between verified peers. Member hosts can read group content;
  forwarding through a host is not participant-to-participant secrecy from that host.
- Later: optional iroh-gossip fan-out needs group payload encryption and a designed key/epoch
  rotation protocol. It is only an acceleration layer, never durable storage or authorization.
- Delivery distinguishes local writes, replica storage receipts and recipient receipts.
  Host storage does not mean all members received it; offline delivery needs replica overlap.

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
`doc.rs` Loro day shard + batch validation · `wire.rs` frames of `tinline/chat/6` ·
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

### Wire (`tinline/chat/4`)
One bidirectional stream, opened by the dialer; frames are u32 BE length + bincode (cap 16 MiB).
`Auth{attestation, grant, relay}` both ways first: the same proof as a call (a call hello with
id "chat" is built and checked by `proto::accept_call_hello`; `proto` is unchanged).
Then `Hello{shards:[(day, vv)]}` (last 7 days + days with unacknowledged ops; also acts as an
ack), `Sync{doc, vv, update, sig}`, `Push{doc, update, sig}`, `Ack{doc, vv}`,
`HistoryReq{before, limit}` / `HistoryResp{days:[{day, vv, hash, key, size}]}`.
`sig` = Ed25519 by the device key over `"tinline-chat-batch-v1" ‖ len(doc) ‖ doc ‖ update`.
For chat day shards, a device only ever sends **its own** ops (export from the peer's vv with other peers' counters
masked), so a receiver can require that every op of a live chat batch is the signer's own.
**Version 3 (P6 too).** `Auth` presents the peer's *message* token (a call token does not open
chat; a token taken back by a block is refused).
**Version 5 (photo sizes, 2026-10-14).** A file record of a picture adds `width`, `height` (the size it is
shown at, EXIF orientation applied) and `preview` (hex of a 4x3 RGB grid of its colours, 36 bytes), made by
the sender's core when it sends an `image/*` (`chat/photo.rs`; png, jpeg, webp; 0x0 and empty if it can't
read it). The DID signature input appends them (marker `2`, `u32 BE` width, `u32 BE` height, the preview
string) only when any is set, so every record written before keeps its input. Validation: width and height
at most 100 000, preview at most 72 hex characters. `Attachment` gains `width`, `height` and `preview`
(raw 36 bytes) so apps can draw a photo's box at its real shape, softly coloured, before it downloads.
`/4` peers are refused (426, update the app, no wipe); own-device sync moves to `tinline/self/4` with it,
because an own device relays these records and checks their signatures.
**Version 4 (account profiles).** `Hello.shards`, `Sync.doc`, and `Ack.doc` also accept the
reserved document name `profile`. It exchanges the authenticated sender DID's account profile
(all its linked-device edits), not a chat shard. Profile updates have field/bounds/permit
validation, not chat-message span signatures. Existing message-token and block checks apply;
no visible message is needed. Pictures use the existing encrypted BlobHub serving gate.
Replacement/removal automatically issues the owner-signed old-picture deletion request;
there is no opt-in boolean. See [profile-sync.md](design/profile-sync.md) for APIs and limits.
`/3` peers are refused with code 426 and an update-app reason; **this upgrade does not require a
wipe**. Own-device sync remains `/3`, adding `profile` and private `profile-cache` to `Hello.docs`.

**Earlier version check (P9c, U18; P10).** Version 3 was incompatible with apps before P10
(`tinline/chat/2`: one `v` per record) and before P9c (`tinline/chat/1`). The endpoint still lists
`/1` and `/2` only to refuse them: an old app's connection is accepted and closed at once with
application code 426 and a reason naming both versions ("chat tinline/chat/2 is no longer
supported, this app speaks tinline/chat/4: update the app (and wipe its data)"). Dialing, we offer
the old versions as well; if the peer picks one, it is an old app: we close with the same code and
the dial fails with that reason (logged as "chat with <did>: the other device runs an older app:
..."). Own-device sync moved to `tinline/self/3` the same way (`crates/core/src/version.rs`; node accept
loop and every dial site use it).
One session per contact: if both dial at once, the one dialled by the lower device key stays.
Blobs travel on the stock iroh-blobs ALPN of the same endpoint (iroh-blobs 0.103, iroh 1.x).

### Validation (one verdict on every device)
`Shard::apply_remote` imports into a scratch fork and rejects the whole batch (nothing stored,
session closed) if: ops are not self-contained; the doc has anything but `messages`; a message has
an unknown, missing or ill-typed field (each version's `dev` and `did_sig` included); a new message's
`author` is not the signer; `at` is outside the shard's day ±1 h (live pushes: also |at − now| <
24 h); an existing message changed `id`, `author`, `at`, `reply_to` or `file`; someone but the
author changed the version; a message disappeared; a deleted message kept text; any op is not
proven by its author's DID signature under the author's writer ID (section 7; live: only the
signer's own ops); a writer's version goes back (older `edited_at` or an undone delete, judged per
span against that writer's previous version); a touched message does not end as one of the
proven versions. The verdict
depends on nothing but the batch, the doc and the session's DID, so every device reaches the same
one. Unit tests: `chat::doc_tests`.

### On disk (app data dir)
`chat.redb`: sealed records (AES-256-GCM under `BLAKE3-derive-key("tinline chat store v1",
DEK)`, each bound to its path), layout in `store.rs` (conversations, shard snapshot + appended
updates + meta, acks, tick markers, outbox markers, blob key index, read-permission refs).
`blobs/`: the iroh-blobs store (ciphertext only; chat tags `tl/<hash>`; GC every 30 s).
Profile pictures use separate owner-scoped `profile/<DID digest>/<hash>` tags: an old-picture
permit drops only that claim, never chat's. Profile documents, keys and cache-retention markers
are sealed in the account store; see [profile-sync.md](design/profile-sync.md).

### Decisions where the text was silent
- Ticks track the *creation* op of a message; an edit does not take the second tick away.
- A message id -> day index (`mi/`) lets edit/delete take only the id.
- Conflicting concurrent edits of one message (same author, two devices) sit side by side under
  the two writers' version keys; every replica shows the highest-ranked one (delete, then
  `edited_at`, then writer ID), in any arrival order (P10). Before P10 a single `v` let Loro's
  order (Lamport, then peer id) pick the winner, and the rollback rule then refused the batch on a
  replica that already showed the newer edit: the replicas stayed apart (the flaky
  `concurrent_edits_whose_loro_winner_is_older_are_refused`).
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
- **Per-version author signatures.** Every version carries `did_sig`, the author's DID signature
  (section 7). Creating, editing and deleting all write a new signed version under the writer's
  version key. Every apply path (live
  batch, history snapshot, own-device relay) verifies every new op against the author's DID; an
  unsigned or mis-signed op rejects the whole batch. Because the doc id is signed a message cannot
  be replayed into another day or pair, and because the span is signed an old version cannot be
  written again as a new op; a writer's version may not move `edited_at` backwards or un-delete.
- **Vouched import.** History snapshots (older days, or a day whose batch depends on ops of ours
  that we lost, e.g. after a reinstall) may hold ops of either party, but every op must be proven
  by its author's DID signature. A contact therefore cannot put words in our mouth or occupy a
  writer ID of ours; the worst it can do is withhold messages.
- **Writer counters never repeat (P10).** Loro identifies an op by (peer id, counter) and drops
  an op whose id it already holds, so two different ops under one (writer ID, counter) make the
  second silently vanish on the contact's side. Hence: a conversation is never deleted or rebuilt
  (U24; `chat_purge` and `delete_conversation` are gone), so block -> unblock -> re-add continues
  the same docs, and pre-block messages are delivered by the normal sync after the re-add; a chat
  store that is not there when it opens (lost, deleted) gets a new device ID, hence a new writer
  ID, before it is created; a cached day whose ops differ from the store's (a write whose
  persist failed) is reloaded before it is used again, so unstored ops are never sent or built
  on. Tests: `crates/core/tests/readd.rs`, `a_day_rebuilt_under_the_same_writer_id_is_silently_lost`.
- `chat.redb` is opened with a bounded wait (~2 s, like `account.redb`): the node before a
  restart may still hold it.
- The original 1:1 chat added no `proto` change; account profiles separately add the scoped
  picture-deletion permit. `crates/audio` remains untouched (voice encode/decode left to the apps).
- Test knob: `P2P_CHAT_CLOCK_MS_OFFSET` shifts the chat clock (day-shard tests only).

### Signatures and writer IDs (P9c, strict; byte formats in docs/protocol.md)
U18: the app is not launched, so there is no compatibility with apps or data from before P9c
(the P8/P9/P9b transition machinery is gone). Installing P9c over a pre-P9c build meant wiping the app data on
every device of the account; an older app is refused with a clear version error (Wire, above).
The `/3` to `/4` profile upgrade keeps that data.

- **DID signature as a record field (U4 revised, U17 a).** Every version's `v` carries `did_sig`:
  the author's DID key over the canonical content (doc id, id, author, at, text, edited_at,
  deleted, reply_to, file), the device ID `dev`, and the **span** of ops that wrote it: Loro peer
  id, counter of its first op, op count (the span ends with the insert of the writer's own
  version key `v:<writer ID>`). It proves "this DID
  wrote this version as exactly these ops under this writer ID"; a copy on any other span does not
  verify. No device key signs content; there is no other signature and no commit message.
- **Writer ID (U16).** A device writes every chat doc under `writer_id(DID, device ID)` as the Loro
  peer id, always. The device ID is random per install (`device_id` in `account.json`: new on
  create, link and restore, and when the chat store is created afresh (P10); kept when only the
  device key changes). All replicas keep the
  sender's number.
- **Every op proven (U18).** `apply_remote` splits the new ops of a batch into spans (per peer id,
  consecutive ops on one message, ending with the writer's own version key); an op outside such
  a span (trailing ops, a span over two messages, a removal, another writer's version key, ops on
  anything else) refuses the batch (`Unsigned`). For each span, the
  version it wrote must have `writer_id(author, dev)` = the span's peer id
  (`ForeignPeer` otherwise) and a `did_sig` that verifies for exactly that span (`BadSignature`).
  Live, every span must be the signer's. Every touched message must end as one of the proven
  versions. There is no acceptance path based on device keys, device lists or remembered writer
  IDs, so the verdict is the same on every device; nothing is kept hidden (P1's per-message hiding
  existed for messages signed by device keys a device never learned, which no longer exist).
- **Consequences.** A contact cannot place ops under a writer ID of another DID, seen here before
  or not (closes P9b's "squat an unseen sibling writer ID"): its own messages under it fail the
  writer-ID hash, ours fail our DID signature. After a restore (new device ID, new writer ID),
  messages from before and after it verify on every device because they are DID-signed.
- **Limits.** (1) Same-DID copying by a holder of the recovery phrase (or a hostile linked device)
  is the accepted shared-mnemonic limit: such a holder can sign anything as us. (2) A span's ops are
  bound by count and by the version they produce, not one by one: an altered op that still yields
  the signed version is harmless. (3) The device key still signs batches on the wire (session
  authentication), not content.

## 8. Own devices (task 34g)

A person's linked devices hold the same conversations. Pair ids are `BLAKE3(sorted DIDs)`, so
`dm/{pair}/{day}` is the same doc on every device of the account. The devices exchange shards over
the own-device link (`tinline/self/3`, `docs/protocol.md` "Own-device sync"); a contact never
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
  contact in good standing, or "You"); authors are only we and that contact; and **every op must
  be proven by its author's DID signature under the author's writer ID** (section 7), whichever
  device wrote it. So a relayed batch can neither forge the contact's words nor ours, and a message
  from the contact verifies against the contact's DID, not against the relay. A rejected batch
  is dropped (nothing stored) and the session goes on; the usual per-field rules (immutable
  `id/author/at/reply_to/file`, no un-delete, no removal, day window) hold.
- A device still sends the **contact** only its own ops (`export_own_since`), and the contact
  accepts a live batch only with ops of the session's DID. So each of our devices
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

### Blocking
A block keeps the conversation on every device (U19). The blocker's devices refuse the blocked
DID's `Auth` with its signed notice (`ChatMsg::Notice`), so no session, sync, push or history
request happens. The blocked person's app, once it holds the notice, deletes its contact entry
for the blocker but keeps the conversation read-only (`chat_day`, `chats()` still show it; writes
and `fetch_older_history` fail with `Rejected("blocked")`), stops retrying, and reports its
unacknowledged messages with `not_delivered_reason = "blocked"`. Messages written before the notice
stay in the docs; if the block is lifted and they add us again, the normal sync delivers them.
Own devices keep syncing a blocked conversation. There is no "delete chat" action today (before
P5 only `remove_contact` deleted history; `chat_purge` was removed in P10). See `protocol.md`
"Blocking".

### Known limits
Read cursors are per conversation, not per message. (A batch relayed by an own device no longer
depends on which devices of the contact this device knows: content is verified by DIDs.)

## 9. Signed delivered/read receipts (`tinline/chat/6`, receipt-runtime branch)

Full design, limits and validation: [message receipt runtime slice](design/message-receipt-runtime-slice.md).

- A contact's device signs (account DID key) a `Delivered` statement for exactly the
  versions it durably accepted from that contact, and a `Read` statement for exactly the
  incoming, undeleted versions its own `mark_read` step covered. The statement names the
  conversation pair, the shard day, the signer DID and install, and sorted version
  digests (`dm_version`: doc, id, author, dev, author's verified `did_sig`). An edit is a
  new version; nothing carries over. Read holes stay holes.
- The obligation is written in the same transaction as the message rows / cursor move;
  signing, ledger append and intent completion are one later transaction. No
  confirmation exists before that commit.
- **Version 6 wire.** After `Hello`: `ReceiptHello{days:[(day, receipt-ledger vv)]}`,
  `Receipts{day, update}` (original native ledger ops the peer lacks; never re-signed),
  `ReceiptAck{day, vv}`. Accepted only from the conversation's two DIDs, kinds
  Delivered/Read only, for days we hold. A refused receipt frame is logged and dropped;
  the session continues. `/5` peers are refused with 426: **update the app, no wipe**.
- Projection: `chat_message_receipts` / `chat_day_receipts` (levels `Unconfirmed`,
  `Delivered`, `Read` with inspectable proofs). `Message.delivery` keeps its meaning and is
  never upgraded by receipts. Both apps draw the mark from the core's `ReceiptTick`
  (clock, check = signed delivered, double check = signed read) and show signer/device
  provenance in the message menu ([UI](design/message-receipt-ui-handoff.md)).
- Not yet: receipts across our own devices, group message receipts, a user setting to
  disable read receipts (open product question).

