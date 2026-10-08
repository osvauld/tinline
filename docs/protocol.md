# osvauld p2p call protocol (v0)

Implemented by `crates/proto`. Pure functions: callers supply `now` (unix secs),
redeemed-nonce / revoked-grant sets, and the contact list, and persist results.
ALPN: `osvauld/p2p/0`.

## Identities and devices

- **Identity**: osvauld `Identity` (BIP39). `did:key:z…` over its ed25519 signing key. The person.
- **Device key**: random per-install ed25519 key. Its public half is the iroh
  `EndpointId`. The QUIC/TLS handshake proves the peer holds it; the transport passes that
  as `remote_device` into every check below. Text form: base64url (no pad).
- **Attestation** `{v, did, device, iat}`, signed by the DID: "this device is mine".
  Several devices per DID are fine; each has its own attestation.

All signed things are `SignedBlob { payload: b64url(json), signature: b64url }`,
signature over `DOMAIN || payload`. Domains: `osvauld/p2p/{attest,invite,grant,bind}/v1\0` plus `invite/v2\0` for compact invites.
The verifying key is always recovered from the DID named inside the payload; callers then
compare that DID to the one they expected.

## Contact ticket (QR)

Text (v2, issued today): `OSVC2:` + RFC 4648 base32, uppercase, no padding, so the QR uses
alphanumeric mode (~260 chars vs ~800 for v1). Decoding is case-insensitive and trims
whitespace. Binary body:

```
signing_pk[32] | device[32] | nonce[6] | exp[3] | flags[1] | name[n] | relay_ext | sig[64]
```

- `exp`: minutes since 2026-01-01T00:00:00Z, u24 big endian (issuer rounds up).
  `iat` is not carried (decodes as 0). `nonce` is 6 random bytes (single-use id).
- `flags`: high 3 bits relay code (0 none, 1..4 = `https://{use1,usw1,euc1,aps1}-1.relay.n0.iroh.link./`,
  7 custom: `relay_ext` = u8 length + URL); low 5 bits = name length, so names are capped
  at 31 bytes (truncated at a char boundary when issuing).
- `sig`: Ed25519 by the DID key over `osvauld/p2p/invite/v2\0 || 0x02 || body-before-sig`.
- The DID is `did_from_public_key(signing_pk)`; nothing is displayed that is not signed.

Inside `ContactTicket`/`ContactHello.invite` the v2 invite is a `SignedBlob` whose payload is
`b64url(0x02 || body-before-sig)` and signature `b64url(sig)`; `open_invite` picks v1 (JSON,
payload starts with `{`) or v2 (first byte 0x02). The claim handed to the handshake has the
same shape either way (`InviteClaim.v` = 1 or 2).

Text (v1, still accepted): `osvc1.` + b64url(json). Fields: `{v:1, did, name, device, relay, invite}`.
`invite` is a signed `InviteClaim {v, iss, nonce(16B), iat, exp, device, name, relay}`.
A v1 ticket parsed from text re-serialises as v1 (it cannot be re-signed).

The visible fields are display-only; `verify` requires them to equal the signed ones
(mismatch is an error) and the claim to be unexpired.

## Add contact (joiner B scans owner A's ticket)

1. B: `contact_hello` verifies the ticket, then sends `ContactHello`:
   attestation(B, B.dev), B's name, A's invite blob, `binding`, and `grant_for_you`
   (B issues A a call grant).
   `binding = sign_B(bind || nonce || B.dev || A.dev)`.
2. A: `accept_contact_hello` checks, in order:
   invite signed by claimed issuer and `iss == A`; unexpired; `invite.device == A.dev`;
   nonce not in `redeemed_nonces`; attestation valid; `attestation.device == remote_device`;
   binding verifies under B's DID for this nonce and these two devices; `grant_for_you`
   is issued by B's DID to A and valid. A replies `ContactWelcome` (attestation, name,
   grant for B) and returns `NewContact` including `redeemed_nonce`, which the caller
   must persist before/atomically with saving the contact.
3. B: `accept_contact_welcome`: attestation DID == ticket DID; attested device ==
   `remote_device` == ticket device; grant issued by ticket DID to B. B stores A with
   the ticket's signed name.

Any failure: send `Reject { reason }` (reason is for logs, not trusted) and close.

## Call (B calls A)

1. B opens a connection to A's device and sends `CallHello { call_id, attestation, grant }`
   where `grant` is the one A issued B at add-contact time.
2. A: `accept_call_hello`: attestation valid and `device == remote_device`; grant issued
   by A to the attested DID, unexpired, id not revoked; DID is a contact.
3. A answers `Ringing`, then `Accept { renewed_grant }` / `Decline` / `Busy`.
   Either side ends with `Hangup`. `renewed_grant` lets A roll the caller's grant forward.

A different device of the same DID with its own valid attestation is accepted.

Framing: u32 big-endian length + JSON `Msg` (`{"t": "...", ...}`), max 64 KiB. The length
is rejected from the header alone. `decode_frame` returns `None` until a whole frame is
buffered and reports bytes consumed.

## Multi-device

Design: `docs/design/device-linking.md`. Implemented in `crates/proto` (`DeviceList`,
`Cancel`, `link.rs`); all additive, old peers still parse the messages they know.

### DeviceList

`DeviceList { v: 1, did, seq, devices: [{ device, relay? }] }`, a `SignedBlob` under domain
`osvauld/p2p/devices/v1\0`, signed by the DID. `seq` is unix ms chosen by the signer.
`verify_device_list(blob, expected_did)`: signature by the DID in the payload, DID equals the
expected one, 1..=8 devices, no duplicates, version 1. A relay hint failing the syntactic
check (`https://`, at most 200 chars, no whitespace or control characters; the core also parses
it as a relay URL) is dropped and the device kept, like a bad hint in a hello.
`newer(a, b)`: higher `seq` wins; on equal `seq` the lexicographically larger signature wins,
so every receiver converges on the same list. Carried in optional `devices` fields of
`CallHello` and `Accept` (omitted from the wire when absent).

### Cancel

`Cancel { reason }`, reason one of `answered_elsewhere | declined_elsewhere | caller_hangup`.
The caller sends it to the other devices of the callee that it dialled with the same
`call_id` (the first `Accept` wins; a `Decline` from any device ends the call).

### Calls to several devices (task 34c)

Every `CallHello` and `Accept` we send carries our current `DeviceList` (`devices`). The sealed
account registry (`registry`, one entry per install, seeded with this device; real linking comes
with task 34d) is the source; the list is re-signed with `seq` = unix ms (always higher than the
previous one) whenever its content changes, that is when the registry changes or our relay does.
Our own entry carries our current relay; others carry the last hint we know.

**Receiving a list** (in a `CallHello` or an `Accept`): `verify_device_list(blob, contact_did)`;
if it is `newer` than the stored one, the contact's devices are replaced (relay hints kept for
devices we already knew), `verified` is reset if the list holds a device not in the previous set,
the state is persisted (epoch fenced) and the UI hears `on_contacts_changed`. Older, equal,
forged or other-DID lists change nothing. Once a contact has a list, `note_device` no longer
adds devices: a device missing from the list that dials with a valid attestation is accepted for
that call (same DID), is not dialled later, and resets `verified`. A contact without a list
(peer from before device lists) behaves as before: the calling device is added, newest first.

**Outgoing call** (one `call_id`, we are the coordinator). All devices of the contact are dialled
concurrently, each connection does the usual `CallHello` handshake, and each device that fails
simply drops out (the 30 s per-device and 45 s overall dial limits and the 60 s ring limit apply
as before).

- The first `Accept` wins: that connection becomes the call and is the only one media is read
  from or sent on. Every other leg gets `Cancel{answered_elsewhere}` and is closed. A second
  `Accept` that arrives meanwhile (simultaneous answers) gets the same `Cancel`, and that device
  ends `answered_elsewhere` and stops its media, so exactly one device ends up active.
- `Decline` from any device: the call ends `declined` and the other legs get
  `Cancel{declined_elsewhere}`.
- `Busy` or `Reject` (or a dead connection) from a device only removes that device. When none is
  left the call ends `no_answer` if a device rang and gave up, `busy` if every device we reached
  said busy, else `unreachable`.
- Hanging up while dialling or ringing sends `Cancel{caller_hangup}` to every leg. When we hold
  no device list for the contact it may be a peer from before lists, which does not know `cancel`:
  it gets `Hangup` instead. (An old peer that does receive an unknown `cancel` frame fails to
  decode it, treats that as a dead connection and ends `cancelled`, the same outcome.)
- Glare: unchanged and decided per device pair. When a call from device X arrives while our
  outgoing call to the same DID is dialling or ringing, the call from the lower device key wins;
  if X is the lower key our outgoing call ends `superseded` (not logged) and every one of its
  legs, siblings of X included, gets `Cancel{caller_hangup}`.

**Incoming call**: while ringing (including the call-waiting banner) a `Cancel` ends the call
`answered_elsewhere` / `declined_elsewhere` (history `missed: false`), `caller_hangup` ends it
`cancelled`. If we had already sent `Accept` and then receive `Cancel`, the call ends at once with
the reason of the cancel (`answered_elsewhere` for the lost answer race; its history duration is 0)
and media stops. A `Cancel` is only honoured from the connection of that call, which is the
caller's.

### Link QR

`OSVL1:` + base32 (uppercase, no padding; decode is case-insensitive, trims whitespace) of
`v(1) | device[32] | link_secret[16] | exp[3] | flags[1] [| len | url]`. `exp` is minutes since
2026-01-01T00:00:00Z (u24 BE, rounded up, default ttl 5 min). `flags` high 3 bits are the
relay code of the contact ticket (0 none, 1..4 known relays, 7 custom with u8 length + URL);
the low 5 bits must be zero. Trailing bytes are refused. Not signed: a bearer secret.

### Link handshake (ALPN `tinline/link/1`)

E = existing device, N = new one; either may display. `T = "tinline/link/v1" || e_dev || n_dev ||
secret`, `K = HKDF-SHA256(ikm = secret, info = T)`.

- `LinkHello { role: scanner|displayer, proof }`, `proof = HMAC-SHA256(K, role || own_device)`
  (compared in constant time against the QUIC-authenticated remote device).
- Confirmation code: first 20 bits of `HMAC(K, "confirm")` mod 1,000,000, shown `482 913`.
- `LinkGrant { sealed }`: `b64url(nonce(12) || AES-256-GCM)` under `HKDF(K, "grant")`, AAD =
  `T`; plaintext JSON `LinkGrantBody { mnemonic, account_name, registry: [{attestation, label,
  removed}] }`.
- `LinkDone { attestation, label }`: `accept_link_done` requires a valid attestation, DID ==
  ours, device == `remote_device`; the label is sanitized (64 chars).
- `Unlinked`, `Reject { reason }`. `LinkMsg` is a separate type with the same framing; call
  peers never parse it.

### Own-device sync authorization

`accept_self_hello(attestation, my_did, remote_device, tombstoned)`: attestation verifies, DID
== ours, device == `remote_device`, device not tombstoned. No grant is involved: a call grant
is not an attestation and never opens self-sync, and a self attestation without a grant of ours
never gets a call accepted.

| Check | Defends against |
|---|---|
| Devices-domain signature, DID == expected | A list forged or lifted from another account or blob kind |
| At most 8 devices, no duplicates, relay hint check | Dial-amplification, junk hints stored and dialled |
| `newer`: seq then signature tie-break | Replay of an old list; receivers disagreeing on a tie |
| `Cancel` sent only by the caller on its own call | (informational; callee ignores it for unknown `call_id`) |
| QR secret in `K`, proof bound to role and own device | A scanner without the QR; a proof reflected back with the roles swapped |
| `T` includes both device keys | Proofs and sealed grants replayed onto another device pair |
| Constant-time proof compare | Timing oracle on the secret |
| 5 minute `exp`, single use (caller closes listener) | A photographed QR used later |
| Confirmation code on both screens | A third device racing a photographed QR |
| Sealed grant AEAD with AAD `T` | Tampering or splicing the mnemonic transfer; wrong-pair delivery |
| `LinkDone` attestation DID and device checks | A device attaching itself under another DID or another connection |
| Self-sync: DID, device, tombstone checks | A contact, a stranger, a removed device, or a relayed hello entering own-device sync |

## What each check defends against

| Check | Defends against |
|---|---|
| Domain-separated signatures | A blob of one kind replayed as another |
| Key from claimed DID + expected-issuer compare | Forged claims; blobs signed by the wrong party |
| `attestation.device == remote_device` | Relay/MITM or replayer forwarding someone's hello over its own connection |
| Ticket visible == signed fields | A tampered QR/text swapping name/device/relay |
| Invite `iss == me`, `device == my_device` | Redeeming another owner's (or another phone's) invite here |
| `exp` on invite and grants | Stale QRs; grants living forever |
| Redeemed nonce set | Replaying a captured hello to add a second contact from one ticket |
| Binding over nonce + both devices | Lifting a joiner's hello onto another invite or another device pair |
| Grant `holder == attested DID` | A stolen grant presented by someone else |
| Grant `iss == me` | Grants minted by third parties |
| Revoked set | Cutting off a contact or a leaked grant |
| Contact check | A valid grant surviving after the contact was removed |

## Removing a contact (there is no revocation yet)

Grants carry a random 16-byte id and `accept_call_hello` takes a revoked-id set, but the core
never writes to that set: no grant is revoked individually today. Removing a contact is a local
block instead: the contact is dropped, their DID goes on a blocked list, and the grant we gave
them is no longer honoured because calls from a blocked DID are refused with the same generic
"not accepted" as any other failure (so they learn nothing). The block is lifted only when *we*
add them again by scanning their ticket; a ticket of ours that they scan does not lift it.
Grants are long-lived (a year) and renewed on every answered call; `renewed_grant` in `Accept`
replaces the stored grant only if it is validly signed by the callee for us and expires later.
Tickets are single-use via the redeemed-nonce set and also expire.

## Tickets in the core

- Only the one ticket the node currently hands out can be redeemed (it remembers its nonce).
  Redeeming it, `set_name`, `remove_contact` and `reset_ticket` all drop it; the next `my_ticket`
  mints a fresh one. A leaked or old ticket is dead even if its nonce was never spent.
- The inviter makes the new contact, the spent nonce and the dropped ticket durable in one
  write (sealed account store) BEFORE it sends `ContactWelcome`; if that write fails it sends
  `Reject`, nothing changed, and the same ticket can be tried again. The UI hears of the contact
  after the commit.
- Tickets live 7 days; one with less than a day left is replaced when asked for.
- The `relay` in a ticket or hello is dialled and stored, so it must be an `https://` URL of at
  most 200 characters that parses as a relay URL. A ticket whose relay fails this is refused;
  a bad relay hint in a hello is dropped.
- Names from peers (hello, ticket) lose control and bidi-override characters and are cut to 64
  characters. Decline/reject reasons shown to the user are cut to 100.
- A contact keeps up to 8 devices (`proto::MAX_LIST_DEVICES`), each with its own relay hint
  (`ContactDevice { device, relay }`; the old single `relay` of a stored contact becomes the first
  device's hint when it loads). A call dials all of them at once, see "Calls to several devices".
- An unauthenticated connection holds one of 24 slots (plus 8 reserved for contacts' devices)
  from accept until it becomes a call or a contact, refusals and their short linger included,
  and has 15 s in total to deliver a hello.
- An active call that receives no media for 30 s ends with reason `connection_lost`. After
  1.5 s without media `CallStats.reconnecting` is true (the call is not over yet).
- Calls are logged to `calls.json` (last 500, newest first; `recent_calls`, `calls_with`).
  The log is written before `Ended` is delivered, so the UI re-reads it on `on_call_state`
  `Ended`. Removing a contact deletes their entries.
- Safety number: per party, SHA-512 over `"tinline-safety-v1" || signing_pk`, then 5200 rounds
  of `H = SHA-512(H || signing_pk)`; the first 30 bytes give 6 groups of 5 bytes, each read as a
  big-endian integer mod 100000 and zero-padded to 5 digits. The two 30-digit halves are
  concatenated in sorted order and shown as 12 groups of 5. Only the DID keys count, so it
  survives a change of device. `verified` is local and resets when the contact's device changes: whether the device arrives
  through a ticket exchange or through an authenticated call from a device we had not seen
  (`note_device`, which also tells the UI the contacts changed).
- Availability ("not now"): while unavailable an authenticated call is answered with the same
  `Reject` as any refusal, never surfaced to the UI, and logged as `unavailable`. The caller
  ends with `unreachable`, so it cannot tell "away" from "offline" or "blocked".

## End reasons

`CallState::Ended { reason }` and `CallRecord.reason` carry exactly one of these tokens
(detail goes to the log, not the reason):

| Reason | Meaning |
|---|---|
| `hangup_local` | Active call ended by us (also: we locked/removed the contact mid-call) |
| `hangup_remote` | Active call ended by them |
| `declined` | Caller's view: the callee (any of its devices) declined |
| `declined_local` | Callee's view: we declined (not a missed call) |
| `cancelled` | Callee's view: the caller gave up (`Hangup` or `Cancel{caller_hangup}`, or the link died) before we answered: missed. Caller's view: we hung up before they answered |
| `no_answer` | Rang for 60 s (30 s for a call waiting over another call), nobody answered (either side; missed on the callee) |
| `unreachable` | Caller's view: could not connect, or they refused (offline, blocked, or unavailable) |
| `connection_lost` | Active call lost the connection or media for 30 s |
| `busy` | Caller's view: every device we reached declined while in another call, or was already handling two calls. Callee's history: missed while busy |
| `unavailable` | History only (callee): turned away while unavailable; `missed` is false; never an `Ended` event |
| `superseded` | Our outgoing call yielded to their simultaneous call; not logged |
| `answered_elsewhere` | Callee's view: another of our devices took the call (also if we had answered at the same moment and lost); `missed` is false |
| `declined_elsewhere` | Callee's view: another of our devices declined the call; `missed` is false |

Old strings: `hung up` -> `hangup_local`/`hangup_remote`; `missed` -> `cancelled`/`no_answer`;
`declined: <text>` -> `declined`; callee's own `declined` -> `declined_local`;
`could not reach <name>: <err>`, `rejected: <text>` -> `unreachable`; `connection lost: <err>`,
`no audio` -> `connection_lost`; `no answer` -> `no_answer`; `ended` -> varies;
`they called at the same time` -> `superseded`.

## A second call during a call

There is no hold. When a call arrives while another is active and nothing else is waiting, the
callee parks it beside the active call (`Node::waiting_call`) and the UI shows a banner with a
quiet tone; the active call's audio is untouched. A third call is answered `Busy` at once.

- **Decline** (`decline`): the callee sends `Busy`, so the caller ends with `busy`; the callee
  logs `declined_local`.
- **End & answer** (`end_and_answer`): the active call ends `hangup_local`, then the waiting call
  takes the slot and is answered.
- **Ignored**: after 30 s (`P2P_WAITING_RING_SECS` in tests) the callee hangs up, the caller ends
  `no_answer` and the callee logs a missed call.
- If the active call ends first, the waiting call becomes an ordinary ringing call
  (`on_incoming_call` again).

`scripts/e2e_callwait.py` runs all three with headless peers.

## Known gaps

- Device secret storage (Keystore, Android Keystore) is the caller's job; proto only
  derives the public key. Losing it means re-adding contacts.
- No device revocation list and no per-grant revocation: a stolen unlocked phone is stopped
  only by the contact removing us (a block). Attestations never expire (`iat` is informational).
- The ticket is a bearer secret until redeemed: whoever scans first wins. Show it
  in person, keep `ttl` short.
- A ticket's visible `relay` hint is signed but not authenticated by anything else;
  a relay only sees ciphertext, but can deny service.
- No clock-skew tolerance; `now >= exp` is expired. Callers choose the clock.
- Contact names are self-reported and not unique; show the DID fingerprint on confirm.
- No forward secrecy or media encryption here: media rides the QUIC connection.
- The joiner's grant check on Welcome ignores revocation (fresh grants only).
