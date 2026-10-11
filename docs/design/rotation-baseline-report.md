# Rotation A1 — core baseline and EndpointId dependency map

Status: proposal — not approved

Packet A1 (read-only audit). Nothing in source, tests, or other docs was changed. This report
records (1) the core-crate baseline at the stated commit and (2) every place in
`crates/core/src` (and the `proto` functions it calls) that stores, signs with, indexes by, or
derives from the per-install device transport key (`device_secret` / its public half, the iroh
`EndpointId`). It proposes no protocol and makes no policy decisions.

Sections 1–3 are **verified in source or by running the tests**. Section 4 ("Observations")
contains **inferences from reading the code**. They are not covered by tests and are marked as such.

## 1. Checkout state

| Item | Value |
|---|---|
| Worktree | `/home/abe/p2p_chat/.claude/worktrees/rot-a1`, branch `rot/a1` |
| `git rev-parse --short HEAD` | `1b6e632` |
| `git status --short` (before writing this file) | empty (clean) |
| `git merge-base --is-ancestor 35564bc HEAD` | true (rule 0.1 satisfied) |
| `git merge-base --is-ancestor 1b6e632 HEAD` | true |

### Merge markers (step 2)

| Command | Result |
|---|---|
| `grep -c '^<<<<<<<' crates/core/src/node.rs` | `0` |
| `grep -rc '^<<<<<<<\|^>>>>>>>\|^=======$' crates/core/src` (non-zero files only) | none |

The handoff (`rotation-agent-handoff.md` §2) and `endpoint-rotation-device-audit.md` both
mention committed conflict markers in `node.rs`. At `1b6e632` there are none: zero `<<<<<<<`,
zero `>>>>>>>`, and zero bare `=======` lines anywhere in `crates/core/src`. This agrees with
the packet note that dev `0f36a29` resolved them before the `35564bc` merge. Both docs are now
stale on this point. The coordinator owns them, and this packet did not edit them.

## 2. Core test baseline (step 3)

Command (run from the worktree; the shared target dir was set by the coordinator):

```
python3 scripts/buildlock.py --who rotation-A1 -- env CARGO_TARGET_DIR=/home/abe/p2p_chat/.claude/worktree/endpoint-rotation-blocking/target cargo test -p p2pcore > /tmp/claude-1000/rotation-A1-core.log 2>&1
```

Log: `/tmp/claude-1000/rotation-A1-core.log`.

Exit status: 0. Wall time was dominated by the build, which waited on buildlock behind `group-GA`. **121 passed, 0 failed, 0 ignored. No failures, so there are no pre-existing failures to classify.**

| Test target | Result line (verbatim) |
|---|---|
| unittests `src/lib.rs` | `test result: ok. 45 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 30.11s` |
| unittests `src/bin/uniffi-bindgen.rs` | `test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s` |
| `tests/accounts.rs` | `test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.04s` |
| `tests/calls.rs` | `test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 14.18s` |
| `tests/devices.rs` | `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.70s` |
| `tests/fanout.rs` | `test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 14.51s` |
| `tests/link.rs` | `test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 28.94s` |
| `tests/link_expiry.rs` | `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.32s` |
| `tests/restore.rs` | `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.38s` |
| `tests/tickets.rs` | `test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.78s` |
| `tests/vault.rs` | `test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.05s` |
| doc-tests `p2pcore` | `test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s` |

This is the first full `p2pcore` test baseline claimed on this branch, at `1b6e632`. It is
limited to this one run on this machine. The integration targets (`calls`, `fanout`, `link`,
…) bind real iroh endpoints, so they depend on the local network environment. Nothing here
was measured on Android.

Proto rotation baseline (rule 4), same target dir:

```
python3 scripts/buildlock.py --who rotation-A1 -- env CARGO_TARGET_DIR=/home/abe/p2p_chat/.claude/worktree/endpoint-rotation-blocking/target cargo test -p proto --test rotation_identity_boundary --test rotation_succession_model --test rotation_permit_model > /tmp/claude-1000/rotation-A1.log 2>&1
```

Exit 0. 33 passed (5 + 18 + 10):
- `rotation_identity_boundary`: `test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s`
- `rotation_permit_model`: `test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s`
- `rotation_succession_model`: `test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.30s`

## 3. EndpointId / device-key dependency table (step 4)

Terms used below:
- **device key** = `proto::device_public(&profile.device_secret)`. This is the iroh
  `EndpointId` (`node.rs:1781` binds the endpoint with `SecretKey::from_bytes(&me.profile.device_secret)`).
- **remote** = `conn.remote_id()`, the key authenticated by the QUIC handshake.
- "Rotates" means this install starts using a new `device_secret` under the same DID.
- Persistence locations: `account.json` = clear profile (`store.rs:23-31`). `account.redb` =
  sealed `State` / call log / label (`store.rs:216-218`). `chat.redb` = sealed chat store
  (`chat/store.rs:1-18`). `sync/account` = the account Loro doc snapshot, stored in
  `account.redb` (`sync/engine.rs:18,136`).
- "Migration needed": **yes** = existing data or code would act wrongly or drop something
  unless it is migrated or the code is extended. **no** = keyed by DID or recomputed per
  session. **unknown** = it depends on an open gate.

### 3.1 Local identity, vault, profile

| path:line | What it keys/signs | Persisted? (where) | Effect if key rotates | Migration |
|---|---|---|---|---|
| `store.rs:41-46` `Profile.device_secret` | The install's iroh secret key | Inside the vault only | This is the value that rotates | yes (re-seal vault) |
| `vault.rs:3,26-30` `Secrets{mnemonic, device_secret}` | AES-GCM-sealed under the DEK | `account.json` → `vault` | A new secret requires `vault::seal` again. The DEK can stay (sealing is per `Secrets`) | yes |
| `store.rs:23-31` `ProfileV2.device_public` | Clear copy of the device key (lock screen, switcher, status) | `account.json` | Stale until rewritten. `profile()` / `status()` show the old key while locked (`node.rs:660-670`, `node.rs:1754-1758`) | yes |
| `node.rs:1083-1087` `Me::load` | Derives `device` and makes a **fresh attestation (`iat = now`) on every unlock** | No (in memory) | A new key gets a new attestation automatically. There is no ordering against the old one (`proto/src/lib.rs:197-208`, no expiry) | no (code), unknown (G-AUTHORITY / G-REVOCATION) |
| `node.rs:1089-1096` `Me::info` → `ProfileInfo.device` | UniFFI-exposed device text | No | UI shows the new key | no |
| `node.rs:1738-1762` `NodeStatus.endpoint_id` | `ep.id()` or the derived device key | No | Changes. UI-visible | no |
| `node.rs:1768-1816` `start()` | Endpoint built from `device_secret`. One endpoint per unlock, never rebound | No | Rotation needs an endpoint rebind (close and rebuild), or a second endpoint. Not supported today | yes (code) |
| `node.rs:1205-1219` `finish_unlock` | Builds `Profile` from vault `Secrets` | — | Reads whichever secret is in the vault | no |
| `node.rs:1250` `set_identity` | `new_device_secret()` on create/restore | vault | — | no |
| `node.rs:1273-1293` restore-over-existing | Marks the registry entry of the **old** `device_public` `removed = true` | `account.redb` state | **The only key replacement path that exists today.** It treats the old key as a dead device (tombstone), not as a succession | unknown (G-RETIREMENT) |
| `node.rs:727-741` `set_passphrase` (legacy→V2) | Re-derives `device_public` from `p.device_secret` | `account.json` | Pattern for re-sealing | no |
| `rekey.rs:36` `key_from_phrase`, `store.rs:222-242`, `chat/engine.rs:162-167` | Store encryption keys derive from the **phrase**/DEK, not the device key | — | Unaffected | no |
| `accounts.rs:4-5` | Doc comment: `account.json` holds `device_public` | — | (see `ProfileV2`) | — |

### 3.2 Own-device registry, DeviceList, tombstones, self-sync

| path:line | What it keys/signs | Persisted? (where) | Effect if key rotates | Migration |
|---|---|---|---|---|
| `store.rs:141-154` `OwnDevice{device, attestation, removed, relay, …}` | Registry **keyed by device key** (no stable slot id) | `account.redb` state + account doc | The new key becomes a *new* entry. The old entry stays live unless someone tombstones it | yes (G-RETIREMENT, slot model) |
| `store.rs:177-180` `State.registry`, `State.own_list` | Registry and our signed `DeviceList` | `account.redb` | `own_list` lists device keys. It is re-signed only when contents change | yes |
| `node.rs:2316-2378` `my_device_list` | Finds self by `me.device` and **inserts self if missing**. Forces own `removed = false` locally. Signs `DeviceList` (seq = max(now_ms, prev+1)) | `account.redb` | After rotation: inserts new self entry at index 0. The old self entry stays and is **still published** (it is not removed). Contacts would dial both | yes |
| `sync/engine.rs:733-748` `ensure_self_registered` | Same insertion by `me.device` | account doc | Same as above | yes |
| `proto/src/lib.rs:713-731` `sign_device_list` / `:733-758` `verify_device_list` / `:760-775` `newer` | DID-signed device-key list (≤8), seq ordering | (contacts' copy, §3.4) | Format already allows the set to change. No predecessor/succession semantics | unknown (G-HISTORY) |
| `sync/accdoc.rs:4-11,185-201` `devices/<device_text>` | Account-doc map **keyed by device text**. `removed` is never taken back | account doc snapshot | New key = new map key. Old key's entry remains forever | yes |
| `sync/accdoc.rs:283-321` `read_into` registry | Accepts an entry only if its attestation is our DID **and attests that same device key**. ORs `removed`. Sets `self_removed` if `me.device` is tombstoned | `account.redb` | Any DID-attested key is accepted from a sibling. No succession check. A tombstoned **new** key would unlink this install (`sync/engine.rs:215`) | unknown (G-AUTHORITY) |
| `sync/accdoc.rs:137-151` `AccDoc::new/load` | Loro peer id = `peer_id(device, "account")` | account doc snapshot | New peer id after rotation. Old ops keep the old peer. Loro allows this; it only writes under the new id | no (data), see §4 O5 |
| `sync/engine.rs:69` `Ctx{me_device}` | Merge context | — | Recomputed | no |
| `sync/engine.rs:255-282` `note_own_device` | **Registers any DID-attested, non-tombstoned remote key** (TOFU by DID) | `account.redb` | Siblings learn a rotated key on first contact, with no proof of succession. The old entry stays live | unknown (G-AUTHORITY) |
| `sync/engine.rs:309-311` `tombstones()` | `HashSet<device>` of removed entries | derived | Tombstoning old keys requires per-key entries | yes |
| `sync/engine.rs:369-415` self session auth | `remote`, `accept_self_hello(att, did, remote, tombstones)` (`proto/src/lib.rs:780-800`) | — | Works for any DID-attested key that is not tombstoned. The new key is accepted, and so is the old one | unknown (G-REVOCATION) |
| `sync/engine.rs:314-327` `register_session` | Sessions keyed by remote device. Glare tiebreak on device key ordering | memory | Two sessions possible (old + new key of the same install) during overlap | no (runtime) |
| `sync/engine.rs:329-345,634-683` dial targets / backoff | Non-removed registry keys + relay hints. Backoff keyed by device | memory | Keeps dialing a retired key until it is tombstoned | yes |
| `sync/engine.rs:295-307`, `:686-703`, `:707-725` `self_dial`, `self_probe`, `send_unlinked` | `addr_for(device, relay)` | — | Dials by key | no |
| `sync/engine.rs:474-478,575-578,614-617` | `sign_batch(&me.profile.device_secret, …)` for account/chat batches | No (signature checked per frame, not stored) | New secret signs. Peers verify against `remote` | no |
| `sync/engine.rs:464-466,729-731` `chat_sig_ok(&remote, …)` | Verifies a self-sync batch against the transport key | — | Bound to the live connection key | no |
| `sync/engine.rs:623-632` `touch_device` | `last_seen` by device | `account.redb` | Old entry's `last_seen` freezes | no |
| `sync/link.rs:36,140-158,165-198,306-340` link run | QR `device` = `me.device`. Transcript/HKDF/HMAC bound to `e_dev`, `n_dev` (`proto/src/link.rs:146-201`) | No (QR ttl) | An in-flight link breaks if E rotates mid-run. QRs are short-lived | no |
| `sync/link.rs:370-404` E finish | Exports registry (attestations, removed). **Re-linking a tombstoned key sets `removed = false` locally** | `account.redb` | — | unknown (G-RETIREMENT) |
| `sync/link.rs:439-498` `create_linked_account` | N keeps the temp endpoint's `device_secret` as its account key. Rebuilds registry from attestations | vault, `account.redb` | — | no |
| `sync/api.rs:127-154` `linked_devices`, `:157-174` `rename_device`, `:179-220` `unlink_device` | UniFFI surface keyed by device text. Unlink = `removed = true` on that key | `account.redb` | Unlinking an install whose key has rotated targets one key only. Its other key(s) stay live | yes |
| `sync/api.rs:66,268-273` | Test helpers (`fresh_link_endpoint`, `self_probe` modes) | — | — | irrelevant (test-only) |
| `proto/src/link.rs:257-262` `RegistryEntry` | attestation, label, removed | in sealed link grant | Same key-keyed shape | yes |

### 3.3 Contacts, tickets, invitations, grants, blocks

| path:line | What it keys/signs | Persisted? (where) | Effect if key rotates | Migration |
|---|---|---|---|---|
| `store.rs:48-54` `ContactDevice{device, relay}` | Contact's device keys + relay hint | `account.redb`, account doc `contacts/<did>.rec` | Peer's rotation: a new key must enter `devices`. The old key stays dialed until replaced | yes |
| `store.rs:56-88` `StoredContact.devices`, `.device_list`, `.verified` | Dial set, newest signed list, verified flag | same | See `note_device` / `merge_device_list` | yes |
| `store.rs:90-139` `RawDevice`/`RawContact` | Legacy decode of bare-key devices | same | — | no |
| `node.rs:2252-2290` `note_device` | Adds an unknown key at the front (max 8) **only if no `device_list` is held**. **Any unknown key resets `verified`** | `account.redb` | Every routine rotation would reset `verified` and push a new key. With a list held, the new key is accepted for this connection but not dialed later | yes (verified-DID policy, G-BLOCK/G-HISTORY) |
| `node.rs:3115-3145` `merge_device_list` | Replaces `devices` by a newer signed list. A new key resets `verified` | `account.redb` | Same `verified` reset on every rotation. Old keys **drop out of `devices`**, which also removes them from history-signature keys (§3.5) | yes |
| `node.rs:2296-2311` `accept_device_list` | Applies `merge_device_list` for a hello/accept | — | — | yes |
| `sync/accdoc.rs:31-44,203-226,340-380` `CRec`, `Verified{v, seq}` | Synced contact record. `verified` counts only against a list seq ≥ ours (S4) | account doc | Rotation → new list seq → sibling `verified` voided | yes |
| `node.rs:1098-1116` `Contact::from` → `Contact.device` | UI shows `devices.first()` | No | Displayed key changes | no |
| `node.rs:1122-1136` `safety_number` | **DID public key only** (`identity::public_key_from_did`) | No | Unchanged by rotation. The `verified` resets above are therefore not required by the safety number itself | no |
| `proto/src/lib.rs:182-216` `Attestation`/`attest`/`verify_attestation` | DID signs `{did, device, iat}`. No expiry, no sequence | Inside registry, account doc, hellos | Old attestations stay valid forever. A new key is attested by any mnemonic holder | unknown (G-AUTHORITY/G-REVOCATION) |
| `proto/src/lib.rs:219-237,473-528` `InviteClaim`/`ContactTicket`/`issue_contact_ticket` | Ticket names `device` (+ relay), DID-signed | `State.ticket` in `account.redb` | Ticket points at the old key | yes |
| `node.rs:1906-1924` `ticket()` reuse | Reuses the stored ticket if `iss`, `name`, `exp`, `relay` match. **Does not compare `claim.device` with `me.device`** | `account.redb` | After rotation the old-key ticket is still handed out (see §4 O1) | yes |
| `proto/src/lib.rs:862-920` `accept_contact_hello` | Requires `claim.device == my_device` (`:889`), attested `remote` (`:896`), binding over both keys | — | Redeeming a ticket minted for the old key **fails** after rotation | yes |
| `proto/src/lib.rs:820-860` `contact_hello`, `:922-958` `accept_contact_welcome` | Binding message = nonce‖joiner device‖owner device. Welcome device must equal `pending.device` and `remote` | No (in flight) | Mid-handshake rotation fails the add | no |
| `node.rs:1936-1987` `add_contact` | Dials `claim.device` from the ticket. Saves `new.device` | `account.redb` | — | no |
| `node.rs:2016-2058` `accept_hello` | Blocked check → `apply_contact(…, new.device, false)` | `account.redb` | — | no |
| `node.rs:3077-3089,3092-3113` `stored_contact`, `apply_contact` | Puts `device` first. A new key resets `verified` | `account.redb` | Same reset pattern | yes |
| `proto/src/lib.rs:530-582` `Grant`, `issue_grant`, `verify_grant` | **DID → DID** (`iss`, `holder`). No device in the grant | `StoredContact.grant_from_them`, `State.revoked` | Grants survive rotation | no |
| `node.rs:2836-2858` `renew_grant`, `node.rs:3150-3157` `grant_outlives` | DID-keyed | `account.redb` | — | no |
| `store.rs:168-173` `State.revoked/blocked/redeemed` | `blocked` is a **DID** set. `redeemed` is invite nonces | `account.redb`, account doc `tomb/`, `redeemed/`, `revoked/` | Blocking is DID-based and independent of device keys | no |

### 3.4 Calls (routing, glare, fan-out)

| path:line | What it keys/signs | Persisted? (where) | Effect if key rotates | Migration |
|---|---|---|---|---|
| `node.rs:286-305` `Leg`, `addr_for(&device.device, relay)` | One dial leg per contact device | — | Dials whatever is in `contact.devices`, so retired keys get legs that fail | no (but see `devices`) |
| `node.rs:3183-3190` `addr_for` | `EndpointAddr` from device key + relay | — | Pure | no |
| `node.rs:2462-2483,2503-2537` `start_call`/`run_outgoing` | Sends `me.attestation`, our `DeviceList` | — | Uses the current key. The list still contains the old self key (see `my_device_list`) | yes |
| `proto/src/lib.rs:960-1014` `call_hello`/`accept_call_hello` | Requires attested device == `remote`. Grant holder = attested DID. `is_contact` closure | — | A new key of a contact is accepted (DID-attested). The old key stays acceptable too | unknown (G-REVOCATION) |
| `node.rs:2140-2232` `incoming_call` | `accept_call_hello` + `blocked` check, then `accept_device_list`, `note_device(remote)` | `account.redb` via `note_device` | See `note_device` | yes |
| `node.rs:2137-2139` `is_contact_device`, `node.rs:2070-2079` reserved-slot check | Admission pool by known contact key | — | A rotated contact key lands in the general pool until learned | no |
| `node.rs:2237-2248` `yield_on_glare` | Compares `me.device` vs `remote` | — | Tiebreak changes with rotation (harmless) | no |
| `node.rs:2750-2757` callee `Accept{renewed_grant, devices}` | Sends our `DeviceList` | — | — | yes (list content) |
| `node.rs:2600-2610` caller `Accepted` | `renew_grant`, `accept_device_list` | `account.redb` | — | yes |

### 3.5 Chat: sessions, batch signatures, message signatures, Loro peers, delivery state

| path:line | What it keys/signs | Persisted? (where) | Effect if key rotates | Migration |
|---|---|---|---|---|
| `chat/wire.rs:12,46-62` `sign_batch`/`verify_batch` | Ed25519 by `device_secret` over domain‖doc‖update | No (checked per frame) | Verified against `sess.device` = connection key | no |
| `chat/wire.rs:29-30` `ChatMsg::Auth{attestation, grant, relay}` | Same proof as a call | — | — | no |
| `chat/engine.rs:321-340` `auth_msg`/`verify_auth` | `accept_call_hello` + not-blocked closure | — | — | no |
| `chat/engine.rs:383-450` `chat_handshake` | Dialer finds the grant via `contacts.has_device(remote)` (`:392`). Session keyed by `remote` (`:420-443`). Glare on device order | memory | Dialing a rotated contact key that is not yet in `devices` fails with `NotFound` | yes |
| `chat/engine.rs:342-380` `chat_dial` | Dials every `contact.devices` key | — | — | no |
| `chat/engine.rs:453-482` `handle_chat_incoming` | Unknown keys proceed to the Auth check. Blob ALPN gate by contact / own / session key | — | — | no |
| `chat/engine.rs:52-87` `ChatCore.sessions: HashMap<[u8;32], Session>`, `Session.device/dialer` | Live sessions by device | memory | — | no |
| `chat/doc.rs:32-40` `peer_id(device, name)` | **Loro peer id = BLAKE3(domain‖device‖doc name)** | Implicit in every op of every shard (`chat.redb` `shard/…`) | The new key gives a new peer id per doc. The old key's ops remain under the old peer | yes (see §4 O3, O4) |
| `chat/doc.rs:331-353` `Shard::new/load` | `set_peer_id(peer_id(me.device, …))` | `chat.redb` | — | yes |
| `chat/doc.rs:449-463` `export_own_since` | Exports **only ops of `self.peer`** | — | Unacked ops written under the old peer are never sent to the contact again (§4 O3) | yes |
| `chat/doc.rs:470-563` `apply_remote` | Non-history batch must contain **only `signer_peer`** ops (`ForeignPeer`). Every new/changed message must verify under a key in `ctx.keys` for its author DID | `chat.redb` | Peer-id and key checks both bind to device keys | yes |
| `chat/doc.rs:118-180` `MsgRec.sig`, `sign_msg`, `verify_msg` | **Per-message signature by the author's device key, stored in the Loro doc** | `chat.redb` shards, history snapshot blobs | Durable: old messages stay signed by the old key forever. Verifiers must keep old keys to accept history containing them | yes (G-HISTORY) |
| `chat/doc.rs:393-445` `add_message`/`resign`/`edit`/`delete` | Sign with `me.profile.device_secret` (`chat/engine.rs:1035,1090,1092`) | `chat.redb` | An edit after rotation re-signs the whole record with the new key | no |
| `chat/engine.rs:814-821` live `chat_receive` `Ctx` | `signer_peer = peer_id(sess.device, doc)`, `keys = [(contact, sess.device), (me, me.device)]` | — | Only the current connection key and our current key are accepted for live batches | no |
| `chat/own.rs:54-62` `chat_keys` | Own DID → **all registry keys incl. removed**. Contact DID → `contact.devices` only (≤8, list-replaced) | — | Messages signed by a contact's retired key fail `BadSignature` in history / own-relay once that key leaves `devices` (§4 O2) | yes |
| `chat/engine.rs:1808-1836` `chat_apply_history`, `chat/own.rs:128-160` `chat_own_apply` | Use `chat_keys` | `chat.redb` | — | yes |
| `chat/store.rs:9-11` `ack/`, `mc/`, `mo/` | `ack` = contact's VV (peer-keyed). `mc` = our op counter **without** peer id. `mo` = (other-own-device peer, counter) | `chat.redb` | `mc` is interpreted with the *current* `peer_id(me.device)` (`chat/engine.rs:1143-1151`). After rotation, delivery ticks for pre-rotation messages are computed against the wrong peer (§4 O4) | yes |
| `chat/engine.rs:916-950` `chat_ack` | `my_peer = peer_id(&me.device, …)` decides `newly`/`out` | `chat.redb` | Old-peer `mc` entries never tick. `out` reflects the new peer only | yes |
| `chat/engine.rs:954-987` `chat_send_own` | `my_peer`, `sign_batch` | — | — | no |
| `chat/engine.rs:1299-1324` `did_of_device`, `chat_may_serve` | Blob gate: own (non-removed registry) / contact device / session key → DID, then blocked + contact check | — | A rotated contact key can fetch blobs only once it is in `devices` or a live session | no |
| `chat/engine.rs:1454-1500` `chat_blob_cands`/`chat_blob_dial` | Candidates by device + relay hints | — | — | no |
| `chat/blobs.rs:32-40,296-337` `Gate`, provider event peers map | Gate on the authenticated `endpoint_id` per connection | memory | — | no |
| `chat/own.rs:35-38` `is_own_device`, `:41-49` `own_blob_sources` | Registry keys, non-removed | — | Old self key would still be "own" for others until tombstoned | yes |
| `chat/own.rs:66-72` `pair_owner` | DID + not-blocked | — | — | no |
| `chat/presence.rs:1-98` | `EphemeralStore` per **session** (one key `online`, no Loro peer id set). Online = any session of DID live. Not persisted | memory | Session-scoped, so rotation only moves the session. Presence is unaffected beyond reconnect | no |
| `chat/crypt.rs:28-147` | Blob content keys (random) | `chat.redb` `blob/` | Not device-derived | no |

### 3.6 Call history, voice, misc

| path:line | What it keys/signs | Persisted? | Effect | Migration |
|---|---|---|---|---|
| `store.rs:186-205` `CallRecord` | `peer_did` only | `account.redb`, account doc `calls/` | — | no |
| `voice.rs` | No device-key references (grep empty) | — | — | no |
| `wire.rs` (core) | Framing only | — | — | no |

### 3.7 Acceptance cross-check: every grep hit

`grep -rn "device_secret\|EndpointId\|NodeId\|PublicKey" crates/core/src` → 44 hits. None
match `EndpointId` or `NodeId` literally: the code uses `iroh::PublicKey` and `[u8; 32]`.

| Hit(s) | Covered by row / classification |
|---|---|
| `account_tests.rs:82,113,114,116` | irrelevant: test helper `PublicKeyText` (text form of a test key). The test at `account_tests.rs:75-89` exercises `note_device` resetting `verified` (§3.3) |
| `store.rs:45` | §3.1 `Profile.device_secret` |
| `node.rs:15` | import (`PublicKey`, `SecretKey`, `EndpointAddr`), used by the rows below |
| `node.rs:667` | §3.1 `ProfileV2.device_public` (locked `profile()`) |
| `node.rs:727,728` | §3.1 `set_passphrase` |
| `node.rs:1085` | §3.1 `Me::load` |
| `node.rs:1094` | §3.1 `Me::info` |
| `node.rs:1107` | §3.3 `Contact::from` |
| `node.rs:1217` | §3.1 `finish_unlock` |
| `node.rs:1250` | §3.1 `set_identity` |
| `node.rs:1295` | §3.1 `set_identity` (vault seal) |
| `node.rs:1757` | §3.1 `NodeStatus.endpoint_id` |
| `node.rs:1781` | §3.1 `start()` endpoint secret |
| `node.rs:3184` | §3.4 `addr_for` |
| `node.rs:3276` | irrelevant: unit test fixture |
| `vault.rs:3,29` | §3.1 `Secrets` |
| `vault.rs:155,161` | irrelevant: vault unit test |
| `chat/wire.rs:55,56` | §3.5 `sign_batch` |
| `chat/wire.rs:118,124` | irrelevant: wire unit test |
| `chat/engine.rs:978` | §3.5 `chat_send_own` |
| `chat/engine.rs:1035,1090,1092` | §3.5 `add_message`/`edit`/`delete` |
| `sync/api.rs:66` | §3.2 `fresh_link_endpoint` (N's temporary key becomes its account key) |
| `sync/api.rs:268,272,273` | irrelevant: `self_probe` test modes |
| `sync/link.rs:439,440,441,457,491,493` | §3.2 `create_linked_account` |
| `sync/engine.rs:477,577,616` | §3.2 self-sync `sign_batch` |

The literal grep misses most dependencies, because the key mostly travels as `device`,
`remote`, `me.device`, or `[u8; 32]`. Sections 3.2–3.5 were found by reading `node.rs`,
`store.rs`, `sync/*.rs`, `chat/*.rs` and `proto/src/{lib,link}.rs`.

## 4. Observations (inferred from source, not covered by tests)

These are code-reading inferences relevant to rotation. They are not claims of tested
behavior, and the packet scope excludes fixing any of them.

- **O1 Stale ticket reuse.** `ticket()` (`node.rs:1910-1919`) does not check
  `claim.device == me.device`. `accept_contact_hello` requires it (`proto/src/lib.rs:889`).
  If the device key changes while `State.ticket` survives, the app keeps showing a ticket
  that can no longer be redeemed. The restore path (`node.rs:1279-1287` loads the old
  `state`) appears to be able to reach this today. The ticket would also name the old key as
  the dial target.
- **O2 Retired contact keys break history verification.** `chat_keys` (`chat/own.rs:54-62`)
  takes contact keys only from `contact.devices`. `merge_device_list` replaces that set, and
  `note_device` truncates it to 8. History snapshots and own-device relays containing
  messages signed by a contact's retired key would then be rejected with `BadSignature` as
  a whole batch (`chat/doc.rs:552-559`). Own keys are kept, including removed ones. This is
  an asymmetry.
- **O3 Unacked ops stranded under the old peer id.** `export_own_since` sends only
  `self.peer` ops. The contact's `apply_remote` rejects non-history batches containing any
  peer except `peer_id(sess.device, doc)`. Messages written under the old key and not yet
  acked by the contact could not be delivered through the live path after rotation. A
  history-snapshot pull by the contact would still carry them.
- **O4 Delivery ticks keyed by the current device.** `mc/…` stores only a counter.
  `chat_delivery` and `chat_ack` pair it with `peer_id(me.device, …)`. Pre-rotation
  messages would be compared against the new peer's acked counter. Depending on the counts,
  they would show the wrong state.
- **O5 Self entry is never retired.** `my_device_list` and `ensure_self_registered` insert
  the new key but do nothing to the old self entry. The old key stays in our signed
  `DeviceList`, in the registry, and in sibling dial targets until tombstoned. Separately,
  restore-over-existing (`node.rs:1288-1292`) does tombstone the previous self key. That is
  the one existing "replace own key" behavior, and it uses removal semantics.
- **O6 `verified` resets on every new key.** `note_device`, `apply_contact`,
  `merge_device_list` and accdoc S4 all void `verified` when a contact gains a key. The
  safety number (`node.rs:1124-1136`) depends only on DIDs. Under periodic rotation this
  makes "verified" decay on every rotation. Whether a DID-verified status should survive an
  authenticated rotation is a policy question (handoff: "enforce verified-DID policy").
- **O7 Local un-tombstoning.** `my_device_list` sets own `removed = false`
  (`node.rs:2336`). E's link finish sets `removed = false` for a re-linked key
  (`sync/link.rs:399`). The account doc never takes a tombstone back
  (`sync/accdoc.rs:194-200`), and `read_into` ORs it back in (`:308`). The local `State` and
  the doc can therefore disagree until the next merge. This matters if rotation reuses
  tombstones for retirement.
- **O8 Attestations have no order.** `attest` has no expiry or sequence. Any mnemonic holder
  can attest any key (`proto/src/lib.rs:197-208`). `accept_self_hello` and `note_own_device`
  accept any non-tombstoned DID-attested key. This repeats the known limitation from
  `rotation_identity_boundary.rs`, here confirmed in production call sites.

## 5. Contact removal ⇒ block, and every block check site (step 5)

### Removal implies block (verified in source)

| path:line | Behavior |
|---|---|
| `node.rs:981-1012` `Node::remove_contact` | `contacts.retain(did ≠)`, **`blocked.insert(did)`** (`:989`), `ticket = None`, call history `remove_peer`, `chat_purge(did)`, persist, `sync_local`, hang up live/waiting calls with that DID |
| `sync/accdoc.rs:203-238` `write_from` | Every blocked DID gets `tomb/<did> = now_ms` (`:227-231`). The tomb is deleted only when the DID is no longer blocked **and** is a live contact (`:232-237`) |
| `sync/accdoc.rs:382-399` `read_into` | A contact missing from the doc with a tomb is removed **and blocked** (`:391`). Every tomb DID that is not a contact gets blocked (`:395-398`). So a removal on one own device becomes a block on all own devices |
| `docs/protocol.md:314-330` | Documents "Removing a contact is a local block" |

### Block lift sites

| path:line | Behavior |
|---|---|
| `node.rs:1983-1984` → `node.rs:3093-3094` `apply_contact(lift_block = true)` | Only when *we* scanned their ticket (`add_contact`) |
| `sync/accdoc.rs:339` | A contact alive in the doc (`added_at*1000 > tomb`) unblocks locally |

### Block check sites (every read of `State.blocked`)

| path:line | Path guarded |
|---|---|
| `node.rs:2034-2035` `accept_hello` | Incoming `ContactHello` from a blocked DID → `Refused("blocked")` |
| `node.rs:3095-3096` `apply_contact(lift_block = false)` | Saving a contact (incoming add, or `save_contact` without lift) refuses a blocked DID |
| `node.rs:2160-2169` `incoming_call` (`is_contact` closure) | Incoming `CallHello` (calls) |
| `chat/engine.rs:331-339` `verify_auth` | Chat `Auth`, both directions (dialer and acceptor) |
| `chat/engine.rs:1310-1324` `chat_may_serve` | Blob serving to a contact device |
| `chat/own.rs:66-72` `pair_owner` | Own-device relay of a conversation pair whose contact is blocked |
| `sync/accdoc.rs:227-237,339,391,397` | Tomb write/delete and merge (block state propagation, not admission) |
| `sync/api.rs:236-240` `blocked_for_test` | Test accessor only |

Not found: the following have no `blocked` read.
- Outgoing paths (`start_call`, `chat_dial`, blob fetch). A blocked DID is not a contact, so these find no target.
- `proto::accept_call_hello`. The block is applied by the core closure.
- `presence` (session-scoped). Presence only exists over a chat session, which `verify_auth` gates.
- `DeviceList` publication. Our list goes to whoever we call or who calls us successfully, and there is no per-recipient exclusion.

Rotation-relevant point (verified): block state is **DID-keyed only**. No device key appears
in `blocked`, `tomb/`, or `revoked`. Excluding blocked contacts from future endpoint updates
therefore needs a new send-side filter, because none exists today. Updates currently flow only
inside `CallHello` / `Accept` (`devices`), which blocked DIDs cannot reach.

## 6. Not in scope / not done

- No fixes, no protocol proposals, no edits outside this file.
- Docs that are now stale about merge markers (`rotation-agent-handoff.md` §2,
  `endpoint-rotation-device-audit.md`) were not edited. The coordinator owns them.
