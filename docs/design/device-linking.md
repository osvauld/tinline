# Linking a device to the same account — design

Status: design agreed (2026-10-09): mnemonic transfer chosen (§0). UI boards: canvas section
"Accounts & linked devices" (`*Accounts*`, `*Link*`, `*LinkedDevices*`, `AndroidMultiDevice` in this folder). Builds on `multi-device.md` (agreed model) and `accounts.rs`
(per-DID sealed storage).

**Implementation status (task 34d, 2026-10-09):** §1-§5 are implemented in `crates/core/src/sync/` (`link.rs`, `engine.rs`, `accdoc.rs`, `api.rs`); wire and merge rules in `docs/protocol.md` ("Link session in core", "Own-device sync"). Tests: `crates/core/tests/link*.rs`, `scripts/e2e_link.py`. Chats and "You" items sync since 34g (`docs/chat.md` section 8). Not done: the crash-before-`LinkDone` case is covered only by the generic "accept an unknown device with a valid attestation" rule (no dedicated test), cap on linked devices (the DeviceList already caps at 8), platform UI.

## 0. The decision everything rests on

Today every signed thing a device must produce is signed by the **DID key**, not the device key:
attestations, contact tickets, grants, and `renewed_grant` on every answered call. So a device that
answers calls and adds contacts must hold the identity secret.

**Decision (agreed 2026-10-09): linking transfers the identity secret (the mnemonic) to the new device over an
authenticated link.** Linking is "restore from phrase without typing 24 words", plus data sync.

- Consequence: every linked device is a full peer of the identity. Unlinking stops sync and drops
  the device from what contacts dial; it is **not** cryptographic revocation (already noted in
  `multi-device.md` decision 7). A stolen linked device is stopped only by identity rotation.
- Rejected for now: delegated device keys (only one primary holds the DID key; others get a
  primary-signed sub-key and sign grants/tickets with it). Real revocation, but every verifier in
  `proto` changes, contacts need chain verification, and the primary becomes a single point of
  failure for adding contacts. Revisit with identity rotation.

## 1. Roles and the QR

Two devices: **E** (existing, unlocked account) and **N** (new install or a device adding another
account). Either one shows the QR, whichever has no camera (desktop usually shows, phone scans):

```
OSVL1: base32( v | device[32] | link_secret[16] | exp[3] | relay_code/ext )
```

- `device`: the displayer's iroh endpoint key (for N, the fresh device key it will keep; for E, its
  own). `link_secret`: random, single-use, held only in memory. `exp`: 5 minutes.
- Not signed: it contains no identity and is a bearer secret for 5 minutes, shown on screen in person.
- The scanner dials `device` on a new ALPN `tinline/link/1`. QUIC proves both device keys.

## 2. Handshake (`tinline/link/1`)

Let `T = "tinline/link/v1" || E.device || N.device || link_secret` and
`K = HKDF(link_secret, T)`.

1. Scanner → displayer: `LinkHello { role, proof = HMAC(K, "scanner" || own device) }`.
2. Displayer → scanner: `LinkHello { role, proof = HMAC(K, "displayer" || own device) }`.
   Both check the proof against the QUIC-authenticated remote device key. A wrong secret gets the
   generic `Reject` and burns the QR (one attempt per QR; the displayer closes the listener).
3. Both screens show a **6-digit confirmation code** = first 20 bits of `HMAC(K, "confirm")`, and
   E shows N's chosen device label. The user confirms on E (the code catches a photographed QR
   being raced by a third device).
4. **E requires local authorization**: the passphrase, or device authentication on Android when no
   passphrase is set. Same bar as revealing the recovery phrase, because that is what happens.
5. E → N: `LinkGrant { mnemonic, account_name, registry_snapshot }`, additionally AEAD-sealed under
   `K` (defence in depth; the QUIC session is already encrypted end to end).
6. N: `Accounts::import(phrase, name, own_label, own_pass)`: same DID, its own device key, its own
   DEK and passphrase. N checks the DID from the phrase equals the DID in the registry snapshot.
7. N → E: `LinkDone { attestation(N), label }`. E verifies `attestation.did == mine` and
   `attestation.device == remote_device`, adds N to the registry, and both continue straight into
   own-device sync (§4) on the same connection.

Abort at any step before 6 leaves nothing on N. A crash after 6 leaves N a valid account that is
simply not in the registry yet: it re-announces itself on its first own-device sync (§4 accepts any
device attested by our DID, see §3 for removed ones).

Only the one unlocked account on E can be linked. N may already hold other accounts; linking adds
one and never replaces (the `create_new` reservation in `accounts.rs` already guarantees that).

## 3. Device registry

One Loro doc per account, `account/devices`, synced between own devices only:

```
devices/<device_text> = { attestation, label, added_at, added_by, removed_at? }
```

- An entry is valid only if its attestation verifies under our DID for that device key.
- Labels stay private to own devices. Contacts never see labels.
- Removal sets `removed_at` (a tombstone; it never comes back by sync merge). Re-adding a removed
  device means linking it again, which creates a new device key anyway.

What contacts see is a separate signed list, derived from the registry:

```
DeviceList { v: 1, did, seq, devices: [{ device, relay? }] }   signed by the DID, domain devices/v1
```

- `seq` = unix ms of the change, tie-broken by the signing device key; a contact keeps the highest.
  Sent in `CallHello` / `ChatHello` and answered in `Accept`/`ChatHello` when the peer's last seen
  `seq` is older. Replaces "a contact keeps its 4 newest devices" and the single contact-wide relay
  hint with per-device relay hints.
- A contact that receives a newer list drops devices not on it and stops dialing them. A device
  missing from the current list that later dials with a valid attestation is still the same person
  (the phrase can mint attestations), so the policy is: accept the call, ignore it for dialing, and
  reset `verified` (fixes security-review S4 consistently: verification is tied to the device set).

## 4. Own-device sync (`tinline/self/1`)

- Authorization: the remote attestation's DID equals ours, its device equals `remote_device`, and
  the device is not tombstoned in our registry. **No grant is involved**; a call grant is never
  accepted here and a self-sync proof is never accepted for calls (test both).
- Protocol: the same `ChatHello`/`ChatSync` round trip as `docs/chat.md` (version vectors, signed
  update batches per device Loro peer id), over a different doc set.
- Runs whenever two own devices are online at once: on link, on unlock, on network change, and on a
  backoff timer. Relays are fine; there is no server of ours.

| Synced (account-wide) | Device-local (never synced) |
|---|---|
| Device registry | Device secret, DEK, passphrase, platform key |
| Contacts doc: aliases, blocks, verification + device-set version, grants we hold | Availability ("not now"), audio devices, notification settings |
| Redeemed ticket nonces (so a ticket can't be redeemed once per device) | The currently issued ticket (each device issues its own) |
| Call history (one logical record per `call_id`) | In-flight call state |
| Chat shards, "You" saved items, file blobs (on demand) | Caches, thumbnails |

## 5. Unlink

- From any linked device: Settings → Linked devices → Unlink. Requires local authorization.
- Effect: tombstone in the registry, publish a new `DeviceList`, refuse its `tinline/self/1`.
  If reachable, send it `Unlinked`, and it locks and deletes that account's local data
  (best effort; a hostile device ignores it).
- UI must say plainly: *"This device stops syncing and stops ringing. If it was lost or stolen,
  anyone with its unlock could still use your identity. Move to a new identity to fully cut it off."*
- Unlinking yourself (on the device itself) = remove this account from this device.

## 6. Restore from phrase vs. link

- Restore (typing 24 words) gives the identity only, with a new device key and empty data. It
  appears in the registry once it meets another own device.
- The restored device has no addresses of the other devices. After restore, the screen offers
  "Link from another device to bring your contacts and chats", which runs §1–§2 with the restored
  device as N (the `LinkGrant` mnemonic must match the phrase already held).
- If no other device survives, there is no data to get back. That is the P2P cost; say it at
  onboarding.

## 7. Calls across own devices (summary; full detail is decision 3 in `multi-device.md`)

- The caller dials every device in the callee's `DeviceList` concurrently with one `call_id`.
- The caller is the coordinator: the first `Accept` it receives wins; it sends `Cancel{answered}`
  to every other device, which stop ringing and log "answered elsewhere". A `Decline` from any
  device ends the logical call (agreed: global decline) and cancels the rest.
- Callee devices also tell each other over `tinline/self/1` so history converges on one record.

## 8. Tests required before shipping

- Link happy path both QR directions; desktop↔Android on direct and relay paths.
- Wrong secret, expired QR, reused QR, second scanner racing, confirmation declined on E,
  E locked mid-link, crash on N between import and `LinkDone` (re-announce works).
- N's DID ≠ registry DID rejected; a contact of ours trying `tinline/self/1` rejected; call grant
  as sync token rejected; tombstoned device rejected.
- Registry/DeviceList: forged list, lower `seq`, removed device dialing, `verified` reset.
- Sync convergence: offline concurrent edits on 3 devices, tombstone wins, redeemed nonces union.
- Unlink with target offline / online; account switch during link.

## 9. Open questions for the owner

1. ~~Mnemonic transfer vs. delegated keys~~: mnemonic transfer, agreed 2026-10-09.
2. Cap on linked devices per account (proposed 5; contacts dial all of them)?
3. Should E show the confirmation code only, or require the code typed on E (stronger, slower)?
