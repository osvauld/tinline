# Rotation D1: transport-key use trace (author keys, Loro peer IDs, proofs)

Status: proposal — not approved. Read-only audit by packet D1. No code, tests, or builds were run.
Source state: branch `rot/d1` at `1b6e632` (descends from `35564bc`). Every `path:line` below was
read at that commit.

Scope: every place the per-install transport key ("device key", the iroh `EndpointId`) is used
beyond opening QUIC connections. For each use the trace lists who later verifies it and with which
key, what a rotation of that key would break, and which stored data embeds it.

Evidence labels:
- **[src]**: read directly in source at the cited line.
- **[test]**: an existing test covers it. The test was not re-run for this audit.
- **[infer]**: follows from reading the source, but no code or test has demonstrated it. Rotation
  consequences are always **[infer]**, because no rotation exists in production code.

---

## 1. One key, many roles (summary)

The device secret is a random per-install Ed25519 key. It is generated in
`crates/proto/src/lib.rs:98` and turned into a public key in `crates/proto/src/lib.rs:104`.
At rest it lives in `Profile.device_secret` (`crates/core/src/store.rs:45`) and in the vault's
`Secrets.device_secret` (`crates/core/src/vault.rs:29`). The same 32 bytes play all of these roles:

| # | Role | Signs/derives | Where | Verifier later needs | Persisted? |
|---|---|---|---|---|---|
| R1 | iroh endpoint identity (QUIC/TLS) | transport handshake | `crates/core/src/node.rs:1781` | the current public key (`remote_id`) | secret only in vault |
| R2 | Subject of DID-signed attestation | (DID signs it) | `crates/proto/src/lib.rs:197-213` | the DID, plus the device key equal to `remote_device` | yes (registry, link grant) |
| R3 | Chat batch signature (`tinline/chat/1`) | device secret | `crates/core/src/chat/wire.rs:55`, used at `crates/core/src/chat/engine.rs:978` | the **session's** device key (`engine.rs:802`) | no (signature not stored) |
| R4 | Per-message author signature | device secret | `crates/core/src/chat/doc.rs:172`, stored as the field `sig` at `doc.rs:401` and `doc.rs:429` | **some** key in a DID-indexed key set (`doc.rs:555`) | **yes, inside Loro docs**, everywhere the message lives |
| R5 | Own-device sync batch signature (`tinline/self/1`) | device secret | `crates/core/src/sync/engine.rs:477,577,616` | the session's device key (`sync/engine.rs:470`, `:730`) | no |
| R6 | Loro peer ID per (device, doc) | `BLAKE3("tinline-loro-peer-v1" ‖ device ‖ doc)[..8]` | `crates/core/src/chat/doc.rs:33-40`; account doc `crates/core/src/sync/accdoc.rs:137-149` | receivers recompute it from the session device (`engine.rs:816`) | **yes**: in every op ID, every VV, ack, `mo` row |
| R7 | Implicit "my peer" in bookkeeping | derived from the current device | `engine.rs:919,967,1145`; `doc.rs:359` | n/a (local) | `mc/` counters are stored **without** a peer (`chat/store.rs:242`) |
| R8 | Device-keyed authorization lookups | n/a | contacts, registry, tombstones, blob gate, sessions (§6) | n/a | yes |
| R9 | Device embedded in DID-signed objects | (DID signs) | invite/ticket, binding, DeviceList (§3) | the DID and the exact device bytes | yes (ticket, `own_list`, contact `device_list`) |
| R10 | Tie-break ordering | key comparison | glare `node.rs:2240-2250`; chat sessions `engine.rs:430-441`; self sessions `sync/engine.rs:315-327` | n/a | no |

Things that do **not** depend on the device key [src]:
- Grants. A grant goes from DID to DID with capability `call` (`crates/proto/src/lib.rs:542-553`).
  `verify_grant` checks the issuer and holder DIDs, never a device (`lib.rs:555-582`).
- Blocking (`State.blocked`, a set of DIDs, `store.rs:171`) and revocation by grant ID
  (`store.rs:169`).
- The account directory ID. It is the DID's multibase part (`crates/core/src/accounts.rs:23-28`).
- The keys of the sealed stores. They derive from the recovery phrase
  (`crates/core/src/rekey.rs:36-43`), or for the old format from the DEK (`rekey.rs:46-53`).
  Neither uses the device key.
- Blob encryption. Each blob gets a random AES key that the referencing message carries
  (`docs/chat.md` §2). The blob hash covers only the ciphertext.
- Conversation naming. `pair_id` hashes the two DIDs (`crates/core/src/chat/doc.rs:19-26`), and
  `doc_name` is `dm/{pair}/{day}` (`doc.rs:28-30`).
- Call history records. `CallRecord` has no device field (`store.rs:189-205`).
- Presence. It uses a per-session `EphemeralStore` (`crates/core/src/chat/presence.rs:39-45`)
  with no peer ID set in our code. Presence exists per session, and each session is keyed by
  device (R8).
- The safety number. It is computed only from DID keys (`docs/protocol.md`, "Tickets in the
  core"). Note that the `verified` flag *is* reset by device changes (§6.4).

---

## 2. R1/R2: transport identity and attestation

- [src] The endpoint is bound with `SecretKey::from_bytes(&me.profile.device_secret)`
  (`crates/core/src/node.rs:1781`). Every ALPN shares this one key: calls, chat, blobs, self-sync
  and link (`node.rs:1782-1788`).
- [src] `Me::load` derives `device` and signs a **fresh** attestation at every unlock, with
  `iat = now` (`node.rs:1083-1088`). Attestations never expire, and `iat` is informational
  (`crates/proto/src/lib.rs:197-213`). So one device key can carry many valid attestations, and
  an attestation for a retired key stays valid forever.
- [src] Each authorization check ties the attested device to the QUIC-authenticated remote:
  - call hello: `lib.rs:1000`
  - contact hello: `lib.rs:896`
  - contact welcome: `lib.rs:944-946`
  - self hello: `lib.rs:790-792`
  - link done: `crates/proto/src/link.rs:375`
- [src] The chat `Auth` is a call hello in another frame. It reuses `accept_call_hello`
  (`crates/core/src/chat/engine.rs:331-340`), whose contact predicate also excludes blocked DIDs
  (`engine.rs:336-338`).
- [src] The linking endpoint uses a **fresh** random key (`crates/core/src/sync/api.rs:65-76`).
  That key becomes the new install's device secret (`crates/core/src/sync/link.rs:439-441`,
  `:491`).
- [infer] Rotating R1 is mechanically easy: bind a new endpoint and attest the new key with the
  DID. Every other role below is coupled to R1 because the code reuses the same bytes.
  Separating them is a G-KEYS decision, not something the existing code provides.

## 3. R9: device bytes inside DID-signed objects

| Object | Device field | Checked against | Stored | Rotation effect [infer] |
|---|---|---|---|---|
| Contact ticket / invite | `device` (v1 JSON `lib.rs:220-229`; v2 body `lib.rs:323-326`) | `accept_contact_hello` requires `invite.device == my_device` (`lib.rs:889-891`) | `State.ticket` (`store.rs:175`) | Every outstanding ticket fails with `DeviceMismatch` once the issuer rotates. The ticket-reuse check (`node.rs:1910-1920`) compares issuer, name, expiry and relay but **not** `claim.device`, so after rotation the cached ticket would keep being handed out and could never be redeemed. |
| Contact binding | `BIND_DOMAIN ‖ nonce ‖ joiner_dev ‖ owner_dev` (`lib.rs:822-828`) | `lib.rs:901-905` | not persisted (one-shot) | Breaks only if rotation happens mid-handshake. |
| `DeviceList` | `devices[].device` (`lib.rs:682-702`) | `verify_device_list` (`lib.rs:733-755`) | our `own_list` (`store.rs:180`); contacts' `device_list` (`store.rs:66`); account doc `contacts/<did>.rec` (`accdoc.rs:5`) | This is the only existing DID-signed way to announce a new device key. It carries at most 8 entries (`lib.rs:679`, `:739`). On receipt the contact's device set is **replaced**, not extended (`node.rs:3142`). |
| Link QR / proofs / grant AAD | `T = prefix ‖ e_dev ‖ n_dev ‖ secret` (`link.rs:146-158`) | `link.rs:182-194`, `:238-256` | not persisted | Breaks only if rotation happens during a 5-minute link. |

## 4. R3/R4/R5: signatures made by the device secret

### 4.1 Chat batch signature (R3) [src]
- `sig = Ed25519(device_secret, "tinline-chat-batch-v1" ‖ len(doc) ‖ doc ‖ update)`
  (`crates/core/src/chat/wire.rs:45-62`).
- Signed in `chat_send_own` (`engine.rs:978`). Verified against `sess.device`, the
  QUIC-authenticated key (`engine.rs:802`).
- Not stored. `chat_persist` writes the update bytes and the meta, not `sig`
  (`engine.rs:713-735`).
- [infer] There is no historical verification burden. The only requirement is that the batch
  signer and the session key are the same live key.

### 4.2 Per-message author signature (R4) [src]
- Covers the doc ID, message ID, author DID, `at`, text, `edited_at`, `deleted`, `reply_to` and
  the file reference (`doc.rs:132-170`). Every create, edit and delete re-signs with the
  **current** device secret (`doc.rs:393-401`, `:420-431`).
- **The signature does not name the signing device.** The verifier tries every key the context
  lists for the author's DID: `ctx.keys.iter().any(|(did, key)| *did == new.author &&
  verify_msg(..))` (`doc.rs:555`).
- Only new or changed messages are verified (`doc.rs:551-553`). Stored messages are never
  re-verified on load (`Shard::load`, `doc.rs:343-352`, called from `engine.rs:698`).
- The key set depends on the path:
  - **Live contact session** (`engine.rs:814-821`): only the session device for the contact's
    DID, and only **our current** device (`me.device`) for our DID.
  - **Vouched history import** (`engine.rs:1808-1812`) and **own-device relay**
    (`crates/core/src/chat/own.rs:134`): `chat_keys` (`own.rs:54-62`). This is our current
    device, plus every registry entry **including removed ones** (`own.rs:57`), plus the
    contact's **current** `devices` (`own.rs:58-60`).
- Persisted inside the Loro doc. That covers shard snapshots and appended updates in `chat.redb`
  (`crates/core/src/chat/store.rs:3-17`), closed-day snapshot blobs, and the contact's copies of
  the same docs.

What verifiers need after a rotation [infer]:
- A message signed by our old key X is stored by the contact. It is never re-checked unless a
  path re-verifies it: a vouched snapshot, or an own-device relay on the **contact's** side.
  Those paths use the contact's `c.devices` for our DID, which is replaced wholesale by our
  newest `DeviceList` (`node.rs:3127-3143`). Once X drops out of our published list (at most 8
  entries), the contact's other devices can no longer verify old messages relayed to them. The
  batch is rejected with `BadSignature` and dropped. Under rotation, the "Known limit" in
  `docs/chat.md` §8 becomes permanent: it is no longer a transient lag.
- On our side, removed registry entries are kept forever: entries are never deleted, the only
  `registry.retain` is at link creation (`sync/link.rs:496`), and `removed` is sticky
  (`accdoc.rs:308`). So our own old keys keep verifying old messages, but the registry grows by
  one entry per rotation, without bound.
- `chat_keys` keeps removed own keys indefinitely and applies **no time bound**. A retired or
  removed key can still sign a new message with any `at` inside the day window, and vouched or
  relayed paths would accept it as ours. This is historical-validity versus current-authority
  (contract item 2). The code currently makes no such distinction.

### 4.3 Own-device sync batch signature (R5) [src]
- Same `sign_batch` primitive, used for the account doc and for chat shards
  (`sync/engine.rs:477`, `:577`, `:616`). Verified against `remote` (`sync/engine.rs:470-472`,
  `:730-732`).
- Not stored. Session-scoped. The account doc does not enforce peer-ID ownership, because own
  devices are fully trusted (`accdoc.rs:16-17`; `docs/protocol.md`, "Own-device sync").

## 5. R6/R7: Loro peer IDs and peer-relative bookkeeping

### 5.1 Derivation [src]
- `peer_id(device, name) = u64_le(BLAKE3("tinline-loro-peer-v1" ‖ device ‖ name)[..8])`
  (`crates/core/src/chat/doc.rs:33-40`).
- Chat shards: `Shard::new` / `Shard::load` set it from `me.device` (`doc.rs:333-352`). It is
  re-set after importing a snapshot (`doc.rs:350`).
- Account doc: `AccDoc::new/load(&me.device)` with doc name `"account"` (`accdoc.rs:137-149`),
  opened at `sync/engine.rs:97-101`.
- Fixture mirror (test-only): `design-models:crates/group-sim/src/tests.rs:49-55`, `:75`, `:97` uses the same
  construction with a non-production domain. Group envelopes are signed by the same fixture key
  (`design-models:crates/group-sim/src/tests.rs:138`).

### 5.2 How receivers use it [src]
- On a contact session, the receiver computes `signer_peer = peer_id(&sess.device, doc)`
  (`engine.rs:816`). `apply_remote` rejects a non-history batch that carries or grows any other
  peer (`doc.rs:480-498`, `Reject::ForeignPeer`). [test] `crates/core/src/chat/doc_tests.rs:111`
  (`forged_batches_are_rejected_and_not_stored`).
- A device sends a contact only its **own** peer's ops: `export_own_since` masks every other
  peer (`doc.rs:451-465`). Own devices get every peer's ops (`export_all_since`,
  `doc.rs:468-473`; `own.rs:113`).

### 5.3 Peer-relative state at rest [src]
`chat.redb` layout (`crates/core/src/chat/store.rs:3-17`):
- `shard/.../meta.vv`, `ack/{pair}/{day}`: version vectors keyed by peer ID.
- `mc/{pair}/{day}/{id}`: **only a counter** (`chat/store.rs:242`). The peer is implied to be the
  *current* device's peer (`engine.rs:1144-1146`).
- `mo/{pair}/{day}/{id}`: an explicit `(peer, counter)` (`chat/store.rs:263`), written for our
  messages created under another peer (`own.rs:155-167`).
- `out/{pair}/{day}`: set when `mine > acked[my_peer]` for the **current** peer only
  (`engine.rs:919-934`).

### 5.4 Consequences of rotating the key that drives R6 [infer]
1. **Unsent ops are stranded.** Take ops written under old peer P(X) that the contact had not
   acknowledged:
   - `chat_send_own` looks only at P(X′), and returns early when the contact holds all of
     P(X′)'s ops (`engine.rs:967-971`).
   - `export_own_since` would mask P(X) anyway (`doc.rs:459-462`).
   - The contact would reject P(X) ops from session X′ as `ForeignPeer` (`doc.rs:480-498`).
   - Our other own devices also export only their own peer to contacts.

   The only route left is indirect. A later P(X′) op that depends on P(X) ops (for example an
   edit of a P(X)-created message) makes the contact see `Pending` (`doc.rs:486-489`) and pull a
   vouched history snapshot (`engine.rs:824-829`). That snapshot verifies our messages against
   the contact's `c.devices` (§4.2), which may no longer contain X. A plain new message under
   P(X′) has no such dependency, so earlier P(X) messages could stay undelivered indefinitely.
2. **Ticks go wrong.** `mc` counters recorded under P(X) would be compared against
   `ack[P(X′)]` (`engine.rs:1144-1150`). A message pending before rotation would compare its old
   counter against the new peer's ack and could flip to `Delivered` without the contact holding
   it. `out/` markers likewise compare against P(X′) only (`engine.rs:932-934`), so the day could
   drop out of `pending_days` (`engine.rs:589`) and the outbox retry trigger could be lost.
3. **Version vectors grow.** Every rotation adds a new peer to each doc it writes. Day shards
   bound this per day. The `account` doc lives forever, so its VV gains one entry per rotation
   per device, and it is sent in every self `Hello` (`sync/engine.rs:441-443`).
4. **`creator_end` mislabels our own old messages.** For our own messages it records
   `peer != shard.peer` (`own.rs:161`), so messages created under our own old peer, when relayed
   back, would be recorded as written by "another own device" (`mo`). That is arguably correct
   behaviour, but it is accidental.
5. **Clone or restore of the same secret reuses the peer ID.** If a device secret were
   duplicated (vault copy, backup restore) and both copies wrote with possibly diverged state,
   Loro op IDs `(peer, counter)` could collide. Rotation to a fresh random key avoids this. Any
   scheme that re-derives or restores the old secret does not. This is a G-FORK input.

## 6. R8: device-keyed lookups and authorization [src]

1. **Session maps keyed by remote device**: chat `ChatCore.sessions` (`engine.rs:57-58`), self
   sessions (`sync/engine.rs:53`), blob gate connection→device (`chat/blobs.rs:296-316`).
2. **Contact recognition**: `is_contact_device` (`node.rs:2137-2139`) gates the reserved
   connection slots (`node.rs:2069`) and chat accept (`engine.rs:459-461`). On dial, the grant is
   looked up by `has_device(&remote)` (`engine.rs:390-394`). A device not yet known is accepted
   by attestation plus grant (`engine.rs:460`), then `note_device` (`node.rs:2256-2291`) adds it
   **only if the contact has no `device_list`** (`node.rs:2280-2284`). Otherwise it is accepted
   for that session but never dialled.
3. **Blob serving**: `chat_may_serve` maps device→DID through current contact devices or a live
   session (`engine.rs:1299-1324`). Own devices are served if the registry entry is not removed
   (`own.rs:35-38`).
4. **`verified` reset**: a new device in a contact's list, or a new calling device, clears
   `verified` (`node.rs:3139-3141`, `node.rs:2279`). [infer] Under the current rule, every
   routine rotation by a contact would clear the user's safety-number verification.
5. **Own registry**:
   - Keyed by device (`store.rs:143-155`; account doc `devices/<device_text>`, `accdoc.rs:5`,
     `:189-196`).
   - Tombstones are `removed == true` entries (`sync/engine.rs:309-311`).
   - A tombstoned dialer is sent `Unlinked` (`sync/engine.rs:385-395`). On receipt the device
     deletes its account (`sync/engine.rs:401-405`, `:226-247`).
   - [infer] If routine retirement of an old key reused `removed`, then a still-running install
     reconnecting with its old key would be told `Unlinked` and wipe itself. Tombstone
     (unlink) and retirement (rotation) therefore cannot share the existing flag without
     changing this behaviour.
6. **Our own `DeviceList`** includes all non-removed registry entries, truncated to 8
   (`node.rs:2350-2358`). [infer] Retired-but-not-removed keys would crowd out live devices.

## 7. Existing precedent: restore already changes the device key [src]

`set_identity` (restore) creates a **new** random device secret (`node.rs:1248-1250`).
For an existing account directory it:
- keeps the phrase-keyed `account.redb` and `chat.redb` (`node.rs:1272-1287`,
  `node.rs:1595-1606`);
- marks the old `device_public` registry entry `removed` (`node.rs:1288-1293`).

[test] `crates/core/tests/restore.rs:88-104` asserts that a restore yields a fresh device key.
`crates/core/tests/restore.rs:233-268` asserts that contacts, call history and a chat message
survive. That test checks only local presence of the message. It does **not** check delivery
ticks, delivery of unsent ops, edits of pre-restore messages, or verification by the contact's
other devices. So the hazards in §5.4 and §4.2 already apply to restore today, untested.

## 8. Data at rest that embeds the device key or depends on it

| Store | Item | Ref |
|---|---|---|
| `account.json` (clear) | `device_public` (shown on lock screen / status) | `store.rs:25-31`; `node.rs:667`, `:1757` |
| vault (sealed) | `device_secret` | `vault.rs:26-30`; `store.rs:45` |
| `account.redb` State | `registry[].device` + attestation, `own_list`, `ticket`, `contacts[].devices` / `device_list` | `store.rs:143-155`, `:175-180`, `:56-74` |
| `account.redb` account doc | `devices/<device>`, contact recs with devices/list; ops tagged with P(device, "account") | `accdoc.rs:4-10`, `:137-149` |
| `chat.redb` | Loro ops under P(device, doc); message `sig`; meta VV; `ack` VV; `mc` (implicit peer); `mo` (explicit peer); `out` (current peer) | `chat/store.rs:3-17`; `doc.rs:333-352`, `:401`, `:429` |
| `blobs/` | closed-day snapshots (encrypted Loro snapshots containing all of the above); file blobs do not depend on the key | `docs/chat.md` §2, §7 |
| Contacts' devices | our devices/list, our signed messages, our peer IDs in their shards and acks | symmetric to the above |

Note [src]: `identity::Identity` also derives a mnemonic-based `device_secret`
(`crates/identity/src/identity.rs:15`, `:46-53`, `:112-114`). No use of
`device_public_key`/`encryption_public_key`/`decrypt_sealed` appears in `crates/core` or
`crates/proto` (grep, no hits). It is **not** the transport key. Because it is shared across all
linked devices, `docs/design/multi-device.md` ("Current implementation gaps") rules it out as a
per-install key.

## 9. Verified vs not verified

Verified by reading [src]: every `path:line` claim above.
Covered by existing tests [test], not re-run: the foreign-peer rejection
(`doc_tests.rs:111`), forged messages in vouched snapshots (`doc_tests.rs:199`), cross-doc
replay (`doc_tests.rs:234`), and restore producing a fresh key with local data intact
(`restore.rs:88`, `:233`).
Not verified by any execution: every rotation consequence in §3–§7 marked [infer]. None of them
has a failing or passing test. They are candidates for D2 and for future model tests, not
established facts.

## 10. Migration questions for D2

1. **Author key vs transport key (G-KEYS):** should R3/R4/R6 keep using the transport key, or
   move to a separate long-lived per-install author key? If they move, how is the author key
   attested, and is *it* ever rotated or retired?
2. **Which key signed a message?** Message signatures name no key (`doc.rs:555`). Should a key
   ID or key generation be added to `msg_sig_input`? Or should verification keep the
   "try all keys of the DID" approach, with a retained historical key set? What bounds that set?
3. **Historical key retention for contacts:** contacts replace our device set from the newest
   `DeviceList` (`node.rs:3142`). What retained, audience-safe record lets a contact's other
   devices verify our old messages without revealing private endpoints (contract items 2, 4 and
   6)?
4. **Validity windows:** should a retired or removed key's signatures be accepted only for
   messages whose `at` precedes retirement? `at` is author-chosen, so is that check even
   meaningful? Or do we accept indefinitely, as `chat_keys` does now (`own.rs:57`)?
5. **Stranded ops on rotation:** must ops under the old peer be flushed (acknowledged) before the
   old key retires, or delivered later by another mechanism? That mechanism could be letting the
   new session vouch for an old peer, or a history push. Under G-RETIREMENT, what happens if the
   contact is offline for the whole overlap?
6. **Tick and outbox bookkeeping:** should `mc` become `(peer, counter)` like `mo`, and `out` be
   computed over all of our own historical peers? This needs a schema migration for existing
   `chat.redb`, and restore needs the same fix.
7. **Loro writer identity across rotation:** keep one peer per (author key, doc), or accept a new
   peer per rotation, with VV growth in the long-lived `account` doc? How do clone and restore
   avoid reusing a peer ID with diverged state (G-FORK)?
8. **Retirement vs tombstone:** the registry's `removed` triggers `Unlinked` and self-wipe
   (`sync/engine.rs:385-405`). Rotation needs a distinct "retired endpoint of a still-live
   device slot" state, which ties into the owner-replaceable device-ID/succession model. Which
   registry key (slot ID vs endpoint) should index it?
9. **Registry and `DeviceList` growth:** entries are never deleted, and the list caps at 8. What
   are the pruning rules, and what minimum history must stay to verify old signatures?
10. **`verified` semantics:** should routine rotation (verified succession) preserve a contact's
    `verified` flag, while an unexplained new device still resets it (`node.rs:3139-3141`)?
11. **Tickets:** should rotation invalidate or re-mint the cached ticket (`node.rs:1910-1920`
    lacks a device check)? Or should invites name a slot or DID instead of a transport key?
12. **Session-keyed state and tie-breaks:** chat and self sessions, glare and the blob gate are
    keyed or ordered by device key (§6, R10). Is a per-session transient effect during overlap
    acceptable? Do two live endpoints of one device during overlap count as one session or two?
13. **Group envelopes:** `group-sim` mirrors the chat coupling (author key = peer-ID source =
    envelope signer, `design-models:crates/group-sim/src/tests.rs:49-55`, `:75`, `:138`). The adapter must say which
    key signs group envelopes, and how old envelopes keep verifying after rotation, without
    changing group authority (contract item 4).
14. **Restore as a test case:** restore already rotates the device key (§7). Should the
    restore-path hazards be accepted as the first characterization tests for any author/peer
    migration?
