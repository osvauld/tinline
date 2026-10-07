# Chat — design

Status: design, 2026-10-07. Order decided: **1:1 chat first, then groups**, then group calls.
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
- Blobs are stored as plaintext inside app-private storage (Android file-based encryption
  covers it at rest): per-device encryption would change the hash and break fetching by hash
  across devices. Sealing the blob store under our own key is a later option.
- A closed shard that is edited later (an edit or delete of an old message) gets a delta in
  the vault; the next compaction makes a new snapshot blob and drops the old one.

## 3. Phase 2: files (iroh-blobs)

A message's `file = { hash (BLAKE3), name, size, mime }`. The recipient fetches it from the
author, or from any member who already has it (groups). A device serves a blob only to a peer
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
