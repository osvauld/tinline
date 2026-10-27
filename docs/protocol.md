# osvauld p2p call protocol (v1)

Implemented by `crates/proto`. Pure functions: callers supply `now` (unix ms for issued
statements, seconds for tickets), an `IssuanceStore` (what the issuer remembers: spent nonces,
cut-offs) and the contact list, and persist results.
ALPN: `osvauld/p2p/1` (P6: separate message and call tokens; `osvauld/p2p/0` is refused with
close code 426 like the other superseded versions).

## Identities and devices

- **Identity**: osvauld `Identity` (BIP39). `did:key:z…` over its ed25519 signing key. The person.
- **Device key**: random per-install ed25519 key. Its public half is the iroh
  `EndpointId` (the private endpoint). The QUIC/TLS handshake proves the peer holds it; the
  transport passes that as `remote_device` into every check below. Text form: base64url (no pad).
  It is not permanent: an install can rotate it periodically (`docs/rotation.md` "Private
  endpoint"; off by default since 2026-10-19); the DID, the install's device ID and writer ID stay.
- **Attestation** `{did, device, iat}`, signed by the DID: "this device is mine".
  Several devices per DID are fine; each has its own attestation.

All signed things are `proto::Signed<T>`: a canonical postcard body plus an Ed25519 signature over
`cryptography::sign::input(T::DOMAIN, [body])` (u64 BE length-framed domain and parts; every domain,
one per statement type, is in `cryptography::domains`). Bytes are `body || sig[64]`; in JSON frames
one base64url string. Opening refuses non-canonical or oversized bodies and verifies strictly
against the signer the body names (a DID, or a raw key for tickets); callers then compare that
signer to the one they expected (`open_from`).

## Contact ticket (QR)

Text: `OSVC2:` + RFC 4648 base32 (uppercase, no padding, so the QR uses alphanumeric mode, about
260 chars, about 670 with a code wake token) of a `Signed<Ticket>`. Decoding is case-insensitive and trims whitespace.
`Ticket { pk[32], device[32], nonce[6], exp_min: u32, name, relay: None | Known(u8) | Custom(url), wake: Option<bytes ≤ 400> }`
under domain `osvauld/p2p/invite/v2\0`, signed by the DID key `pk` it carries.

- `wake` (2026-10-26): the owner device's **code wake token** (`docs/push.md` "Code wake
  tokens"), the relay's sealed bytes: `msg` wakes only, a tag of its own, expiring with the
  ticket. Present only when the owner's device has push registered; signed with the rest. With
  an FCM token (163 chars, sealed to 250 bytes) a typical code grows from 262 to 666 characters:
  QR version 9 → 17 at level M, the apps' level (53 → 85 modules a side; alphanumeric mode). Codes from before this field fail to decode (pre-release; they expire within 7 days).

- `exp_min`: minutes since 2026-01-01T00:00:00Z (issuer rounds up). `nonce` is the ticket's id;
  good for anyone, any number of times, until it expires or is reset (see "Tickets in the core").
- `Known(i)`: `https://{use1,usw1,euc1,aps1}-1.relay.n0.iroh.link./`. Names are capped at 31 bytes
  (truncated at a char boundary when issuing).
- The DID is `did_from_public_key(pk)`; nothing is displayed that is not signed. There is no other
  ticket version (`osvc1.` is refused).

The visible fields are display-only; `verify` requires them to equal the signed ones
(mismatch is an error) and the claim to be unexpired.

## Add contact (joiner B scans owner A's ticket)

1. B: `contact_hello` verifies the ticket, then sends `ContactHello`:
   attestation(B, B.dev), B's name, A's invite blob, `binding`, `grant_for_you` (B's
   message token for A) and `call_grant_for_you` (B's call token for A, absent when B gives
   A message only; see "Tokens"), and with push on `wake` (B's wake token for A, `{token,
   exp_ms, kinds}`, unsigned like `WakeTokens`; see "Contact requests").
   `binding = sign_B(bind || nonce || B.dev || A.dev)`.
2. A: `accept_contact_hello` checks, in order:
   invite signed by claimed issuer and `iss == A`; unexpired; `invite.device == A.dev`;
   attestation valid; `attestation.device == remote_device`; neither the nonce nor
   `redeemed_key(joiner DID, nonce)` (`"<nonce>/<did>"`) in `redeemed_nonces`;
   binding verifies under B's DID for this nonce and these two devices; `grant_for_you`
   is a message token issued by B's DID to A and valid (and the call token, if any, a call
   token). A replies `ContactWelcome` (attestation, name, message token and optional call token
   for B) and returns `NewContact` including `redeemed_nonce`; the caller persists
   `redeemed_key(B, nonce)` before/atomically with saving the contact. The core does not send
   that Welcome at once for a ticket hello: it holds the hello as a request (below).
3. B: `accept_contact_welcome`: attestation DID == ticket DID; attested device ==
   `remote_device` == ticket device; grant issued by ticket DID to B. B stores A with
   the ticket's signed name.

Any failure: send `Reject { reason }` (reason is for logs, not trusted) and close.

### Contact requests (2026-10-13; `crates/core/src/contact_req.rs`)

Codes are reusable for a week, so nobody becomes a contact without the owner's yes.

- **Held.** A good ticket hello (all checks of step 2, a live ticket, not blocked) is stored in
  `State::contact_requests[joiner DID]` (the hello, the joiner's device and relay hint, the
  ticket's nonce and `exp`, first arrival time) in one durable write, then answered
  `ContactPending`. The app is told once per joiner (`ContactRequestEvents::on_contact_request`);
  a repeated or replayed hello replaces the held one and stays one request. At most 64 wait
  (PROVISIONAL); more are refused. A request lasts until the ticket it used expires; a later
  reset of the ticket does not drop it (the user can still answer it).
- **Welcomed at once** (as before): a group chat request ticket (`req_tickets`, already accepted,
  message only), a DID that is already a contact (a re-add), and the test switch
  `auto_accept_contacts_for_test`.
- **Accept** (`accept_contact_request(did)`): step 2 runs again on the held hello at the current
  time (so an expired ticket or a joiner redeemed since fails); the contact (message + call
  tokens, `policy_for`), `redeemed_key`, the request's removal and the Welcome to deliver
  (`State::welcome_outbox`) are one durable write. The accepting device then dials the joiner's
  attested device (hint: the hello's relay) and sends `ContactWelcome` as the first message;
  the joiner answers `WelcomeTaken`, or `Reject` (no matching request: delivery stops), or its
  block notice. Untaken Welcomes are retried after 2, 5, 15, 30, 60, then every 300 s while
  online, and at start, for 7 days.
- **Tokens at setup** (2026-10-26, `docs/push.md` "Tokens at setup"): with push on, the hello
  carries the joiner's wake token for the owner and the Welcome the owner's for the joiner
  (optional `wake` field on both, filled in when sent). Each side keeps the other's token as if
  `WakeTokens` had arrived, once the other is its contact (the owner at Accept, for the
  joiner's attested device; the joiner when it takes the Welcome). The normal `WakeTokens` round
  still runs afterwards (idempotent). A Welcome whose dial fails wakes the joiner (`msg`) with
  that token and redials on the fast schedule.
- **Not now** sends nothing. There is no decline message, so the joiner never learns the
  request was seen.
- **Joiner.** Before the hello leaves, `State::sent_requests[owner DID]` keeps the
  `PendingContact` (serialisable), the ticket's relay and `exp`, and whether our hello gave
  them a call token. `ContactPending` marks it delivered (`add_contact_by_code` returns
  `Requested`); a Welcome on the same connection adds them at once (`Added`).
- **Queued request** (2026-10-26, `crates/node-runtime/src/request_out.rs`): a code request
  whose owner device cannot be reached (no connection, or no answer to the hello) is not
  dropped. `SentRequest` keeps the code text (`ticket`), `add_contact_by_code` returns
  `Requested` and the request is listed in `sent_contact_requests`. A background outbox makes a
  fresh hello from the code and redials it until the owner answers (`ContactPending`, a Welcome,
  `Reject` or its block notice) or the code expires; it survives restarts (sealed state) and runs
  again at start. When the code carries a wake token, the first dial sends a `msg` wake if it has
  not connected after 3 s (`MSG_EARLY_WAKE_MS`), every failed dial wakes again (at most once per
  30 s, `MSG_WAKE_EVERY_MS`; a relay 410 stops it) and the redials follow `MSG_REDIAL_AT_MS`;
  otherwise they back off 2, 5, 15, 30, 60, 300, then every 900 s (PROVISIONAL). A group chat
  request ticket is not queued: a dial that never connected restores what was held before, and
  a reply timeout keeps it unlisted so a late Accept still lands. A dialled-back `ContactWelcome` is taken only if a held request names the
  connection's device and `accept_contact_welcome` passes against it (and we don't block them);
  then the contact is stored, the request dropped, and `on_request_accepted` raised. Sent
  requests are listed (`sent_contact_requests`) while delivered or queued, and unexpired, and kept 7 days
  past `exp` for a late Welcome.
- Requests, sent requests and the Welcome outbox are local to the device (not in the account
  doc): the hello names this device. PROVISIONAL.
- Wire: `ContactPending` and `WelcomeTaken` are new `Msg` kinds on `osvauld/p2p/1` (no ALPN
  bump). An older joiner fails to decode `ContactPending` (its add fails; the owner still holds
  the request, and an Accept's Welcome is refused by the old app, so only the owner keeps the
  contact). An older owner welcomes at once as before.
- Typed results for the apps: `peek_code` (`NotACode` / `Own` / `Expired{name, did}` /
  `Found{card}`), `add_contact_by_code` (`AddOutcome::Added|Requested`, `AddError::NotACode |
  OwnCode | Expired | Offline | Unreachable | Refused | Blocked | Failed`), `my_code`
  (code + `expires_at`). `add_contact` (returns a contact) remains for tests and tools; a held
  request is an error there.

## Call (B calls A)

1. B opens a connection to A's device and sends `CallHello { call_id, attestation, grant }`
   where `grant` is the call token A issued B (or, if B holds none, its message token).
2. A: `accept_hello`: attestation valid and `device == remote_device`; token issued by A to
   the attested DID, redeemed (`proto::issued`); DID is a contact. Then: blocked -> `Notice`;
   token taken back by a block (cut-off) -> `Reject { "not accepted" }`; not a call token,
   calls not allowed for B, or a call token at/below B's call cut-off ->
   `Reject { "calls not allowed" }` (never rings; B's call ends `calls not allowed` and B drops
   its call token).
3. A answers `Ringing`, then `Accept { renewed_grant }` / `Decline` / `Busy`.
   Either side ends with `Hangup`. `renewed_grant` is a fresh call token for B.

Video is offered in the same messages: `CallHello.video` / `Accept.video` =
`Option<VideoOffer { max_pixels, max_fps, decode, encode, hw_decode, hw_encode, hint_flags, codec_switch }>`
(codec lists of `"av1"` / `"vp8"`, best first; `decode` always has `av1`; `hint_flags` optional,
absent = false: the device's video sender reads the receiver's hint flags, video control message
5; `codec_switch` optional, absent = false, not written when false: the device's receiver follows
`VideoCodec` below). Each direction uses `send_codec(sender, receiver)`: score each common codec by sender
hardware encode + receiver hardware decode, with sender-order ties. Both ends
compute it from the two offers, with no further message. Both offers must be
present and `proto::video_codecs` must validate **both** resulting directions
against the actual lists before streams start: no common codec = audio-only,
not an unoffered AV1 fallback. Details: `docs/video.md` ("Call protocol") and
`docs/design/video-hw-vp8.md`. `CallHello.camera` (absent = false) says the
caller pressed Video and calls with its camera on: the callee rings it as a video call
(`CallInfo.video`). Either camera can still go on in any call; the call log's `CallRecord.video` is
true if the call was placed as video or a camera was on while it was active.

Developer tuning can filter `encode`/`hw_encode` to a single locally supported
send codec, without changing `decode` or `hint_flags`. Auto offers still contain AV1. Matching
updated builds are required for forced-codec experiments; older peers assume AV1
is always offered. See `docs/design/video-tuning.md`.

**Mid-call codec switch** (2026-10-18, developer builds' in-call sheet): `VideoCodec { codec }`
(JSON `{"t":"video_codec","codec":"vp8"}`) on the call's control stream, sender to receiver, once
the call is active with video: "my video is `codec` from my next keyframe". The sender sends it
only to a peer whose offer has `codec_switch: true` (an older device's parser refuses the unknown
tag, which would end its call) and only for a codec in that peer's `decode`; it then starts the
new encoder with a keyframe (a new group). The receiver ignores a codec it doesn't decode, keeps
its decoder until a keyframe of the new codec arrives, then swaps; a keyframe that arrived before
the message (another QUIC stream) is held and decoded once the message is in. VP8 hardware vs
software is the same bitstream and sends nothing; nor do size, frame rate, bitrate or decoder
choices (the receiver's decoder swap asks for a keyframe with the existing video control
message 2). Details: `docs/video.md` ("Switching codecs mid-call").

## Tokens (P6; decisions U13, U15)

Every statement a DID hands out (tokens, group invites, picture deletion permits) is an
`Issued<C> {iss, sub: Did(holder) | Anyone, iat_ms, exp_ms, nonce[16], claim}` signed in the one
envelope (`proto::issued`). `Signed<Issued<C>>::redeem` is the one admission check: signature,
issuer, subject, `iat_ms <= now + ISSUE_SKEW_MS` (5 min), `now < exp_ms`, `iat_ms` above the
store's cut-off, nonce not spent; the store (`IssuanceStore`) stays where its data lives.
A `Grant` is `Issued<Cap>` with `Cap::Message | Cap::Call` and `sub = Did(holder)` (domain
`grant/v1`). Chat (`tinline/chat/4`) `Auth` presents the message token; calls present the call
token. Only the issuer verifies its own tokens, so taking one back is local and per DID:
`State.cut_offs[did] = {message, call}` (unix ms); a token with `iat_ms <= cut-off` is refused
(`core::store::TokenCutOffs`). Cut-offs only rise and sync between own devices (max per field).

- Ticket/QR contacts get message + call; contacts from a group chat request message only
  (`ContactOrigin::GroupRequest`; the group code passes it to `Inner::add_contact_from`). A
  contact that already exists keeps its call permission when it re-adds.
- `allow_calls(did, true)` sends a fresh call token (`Tokens { grants }`, answered
  `TokensTaken`, retried from the device that queued it). `allow_calls(did, false)` raises the
  call cut-off: every call token of that DID stops ringing; chat goes on.
  `calls_allowed(did)`: we let them call; `can_call(did)`: we hold an unexpired call token.
- A block raises both cut-offs. Unblocking does not lower them: after the unblock the other side
  re-adds us (their entry was deleted, U20) and the re-add carries tokens issued above the
  cut-off.
- Renewal: the call token on every answered call; the message token when a chat session finds
  it within 30 days of expiry (PROVISIONAL), delivered as `Tokens`. A received token replaces the
  held one in its slot only if it is valid, of that kind, and expires later.
- Push wake tokens travel as `WakeTokens { device, token, exp_ms, kinds }`, answered
  `WakeTokensTaken`, on the calls ALPN. Not DID-signed (a capability for the relay only);
  `token` is at most 2048 chars. See `docs/push.md` "Handing tokens to contacts".
- A linked sibling that has not synced a downgrade or block yet still honours the old token
  (best effort, section 14 Q4 open).

A different device of the same DID with its own valid attestation is accepted.

Framing: u32 big-endian length + JSON `Msg` (`{"t": "...", ...}`), max 64 KiB. The length
is rejected from the header alone. `decode_frame` returns `None` until a whole frame is
buffered and reports bytes consumed.

## Multi-device

Design: `docs/design/device-linking.md`. Implemented in `crates/proto` (`DeviceList`,
`Cancel`, `link.rs`); all additive, old peers still parse the messages they know.

### DeviceList

`DeviceList { did, seq, devices: [{ device, relay? }], links: [{ from, to, sig }] }`, a
`Signed<DeviceList>` under domain `osvauld/p2p/devices/v1\0`, signed by the DID. `seq` is unix ms
chosen by the signer. `links` (at most 16, `MAX_LIST_LINKS`) are succession links of the listed
installs' recent key rotations: `sig` is Ed25519 by the old key `from` over `[did, to]` under
`tinline/device-succession/v1` (`sign_succession` / `verify_succession`). `succeeds(list, device,
known)` says whether a chain of good links leads from `device` back to a key the receiver knew.
`verify_device_list(blob, expected_did)`: signature by the DID in the payload, DID equals the
expected one, 1..=8 devices, no duplicates, well-formed links. A relay hint failing the syntactic
check (`https://`, at most 200 chars, no whitespace or control characters; the core also parses
it as a relay URL) is dropped and the device kept, like a bad hint in a hello.
`newer(a, b)`: higher `seq` wins; on equal `seq` the lexicographically larger signature wins,
so every receiver converges on the same list. Carried in optional `devices` fields of
`CallHello` and `Accept` (omitted from the wire when absent).

Pushed (P4, decision U8): whenever the install's set of device keys changes (a device linked,
unlinked, seen via own-device sync, this install restored with a new key, or a private endpoint
rotation, `docs/rotation.md`) it re-signs the list
and sends `Devices { list }` as the first message of a connection on the calls ALPN to every
contact it neither blocks nor is blocked by; answered `DevicesTaken` (whether or not the list was
kept) or, by a receiver that blocks the sender, with its `Notice`. Queued per contact
(`list_outbox`, local), retried with backoff (2..60 s) and after every start; each attempt sends
the current list. A new contact is sent the list too when the install has more than one device.
A relay-only change is not pushed (PROVISIONAL (coordinator)). The receiver keeps a list only if
it verifies for the DID it names, is `newer` than the one held, and its `seq` is at most one day
(`MAX_LIST_SEQ_AHEAD_MS`, PROVISIONAL (coordinator)) ahead of the receiver's clock; the same
bound applies to lists in calls and in own-device sync.

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
devices we already knew), `verified` is reset if the list holds a device not in the previous set
that is not a rotated key of one in it (`succeeds` over the list's links),
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

E = existing device, N = new one; either may display. `T = sign::input(LINK_TRANSCRIPT, [e_dev,
n_dev, secret])`, `K = kdf::derive("tinline/link/v1 key", T)` (BLAKE3; `crates/proto/src/link.rs`).

- `LinkHello { role: scanner|displayer, proof }`, `proof = hash::keyed(K, LINK_PROOF, [role, own_device])`
  (compared in constant time against the QUIC-authenticated remote device).
- Confirmation code: first 20 bits of `hash::keyed(K, LINK_CONFIRM, [])` mod 1,000,000, shown `482 913`.
- `LinkGrant { sealed }`: `b64url(aead::seal)` (nonce(12) || AES-256-GCM) under `kdf::derive("tinline/link/v1 grant", K)`, AAD =
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

### Link session in core (task 34d)

`LinkHello` also carries the sender's device `label` (sanitized, not covered by the proof), shown on
the other screen next to the code. Flow: scanner dials the QR's device (core checks the QUIC remote
equals it), scanner sends its hello, displayer verifies (proof, role, remote device, QR not expired
or spent), answers, both emit `on_link_code`. E waits for `link_approve` (5 min), then sends
`LinkGrant`; N validates (mnemonic DID equals every attestation DID in the registry, E is in it, no
account dir for that DID yet), creates the account in memory with its own device key and data key
(the key of the endpoint that carried the link), sends `LinkDone`. E registers N (label, attestation,
relay if known), re-signs its `DeviceList` lazily and kicks own-device sync.

One link at a time (`reserve_link`). The first connection to a displayed QR consumes it: a wrong
proof, a racing second scanner and a late scanner all get a plain close (`rejected`), never a
reason. Failure tokens surfaced to the UI: `expired cancelled rejected bad_proof mismatch timeout
net locked busy bad_grant account_exists`. Locking E cancels its link. Env knob for tests:
`P2P_LINK_TTL_SECS`.

### Own-device sync (ALPN `tinline/self/5`, task 34d; `/1` refused since P9c, `/2` since P10, `/3` since photo sizes, `/4` since postcard, see "Protocol versions")

Frames: u32 BE length + postcard (`cryptography::codec`) of `SelfMsg` (16 MiB cap).

- `Auth { attestation, relay, unlink }`: dialer first, acceptor answers. The acceptor runs
  `accept_self_hello` against its registry's tombstones. An unknown device with a valid attestation
  of our DID is accepted and added to the registry (a linked device that never announced itself).
  A tombstoned dialer is answered with `Auth` then `Unlinked` (it had to prove it is ours first).
- `Hello { docs: [(name, version vector)] }` both ways, then `Sync { doc, vv, update, sig }` with
  the ops the other side lacks (also the live push after any local change). `sig` is the sender's
  device key over `chat::wire::sign_batch(doc, update)`; the receiver verifies it with the
  QUIC-authenticated device. All own devices hold the identity secret, so peer-id ownership of
  ops is not enforced (unlike chats).
- `Unlinked`: "you were removed". Sent after `Auth`, either by the unlinker (dialer with
  `unlink: true`, skipping session setup) or by an acceptor that tombstoned the dialer. The receiver
  locks and deletes the account and emits `on_unlinked`. Ignored while a call is running (the next
  contact with a device that knows retries).
- One session per device; if both dialled at once the connection dialled by the lower device key
  survives on both ends. Sessions are not held open (decision 2026-10-17, same rule as chat): a
  session with no stream data either way for the idle time (60 s, `wire::IDLE_CLOSE`) finishes
  its stream and closes; the other side sees a clean end. Each device remembers, per other device,
  the local change generation that device is known to hold (`SelfSync::synced`, set when a
  session ends idle, or ends cleanly from the other side, with no newer local change unseen). Any
  local change (account doc, calls, profile, chats, read cursors, groups) bumps the generation,
  so the devices behind it are owed a sync. Dial loop: every 15 s (`P2P_SELF_SYNC_SECS`) and at
  once on a change, it dials only owed devices without a session; start, a link,
  `network_changed` and endpoint rotation (`sync_kick`) owe every device. Per device exponential
  backoff 20 s..300 s, cleared by a kick. A device with a change dials the others, so devices in
  sync stay unconnected; `last_seen` in the registry therefore means "last synced".

What syncs: one Loro doc `account` (peer id = `BLAKE3("tinline-loro-peer-v1" || device || "account")`),
sealed in `account.redb` under `sync/account`, with maps:

| map | entry | merge |
|---|---|---|
| `devices/<device>` | `{att, label, removed, relay, links, rotated}` | field LWW; `removed` and `rotated` are sticky (OR); `links`: the longer copy, never our own entry's |
| `contacts/<did>` | `{rec, alias, verified, calls}`; `rec` = name, devices, device_list, grant, call_grant, added_at | alias/verified/calls LWW; `rec`: name LWW, newer signed device list wins (S4 reset applies), each token slot: the one that outlives (a later re-add without a call token clears it), max added_at |
| `cutoffs/<did>` | our token cut-offs for `<did>` (JSON) | max per field |
| `sent/<did>` | our latest signed block/unblock notice to `<did>` (`Signed<BlockNotice>`, base64url) | verified on read; highest `at` wins; `<did>` is blocked iff that notice is a block (see "Blocking") |
| `notices/<did>` | `{notice, name, blocked, at}`: the latest notice `<did>` signed to us | verified on read (`to` = our DID, `from` = `<did>`); highest `at` wins; a block deletes the contact entry for `<did>` if `added_at*1000 <= at` (deletion request) |
| `redeemed/<nonce>/<joiner>` | `"1"` (a record) | union, never shrink |
| `calls/<call_id>` | CallRecord JSON | written by builds before the calls doc; no longer read |

The call log is a second doc, `calls` (peer id as above with `"calls"`), sealed under `sync/calls`,
in the `Hello` list and synced like `account` (signed `Sync`, an empty batch acks). It only grows
and gets trimmed, so it stays out of the doc that carries contacts and devices. Older builds ignore
the name. On the first open it is seeded from the local call log.

| map | entry | merge |
|---|---|---|
| `calls/<call_id>` | CallRecord JSON | one record per call; better rank wins (answered beats `*_elsewhere`, then duration, then content), newest 500 kept; calls with blocked contacts are kept |

`verified` is stored with the contact's device-list `seq`; it counts only against a device set at
least as new as the receiver's (S4). Registry entries also carry `links` (the install's succession
links, JSON) and `rotated` (`"1"`: a key the install rotated away from; a tombstone like
`removed`, never taken back, not shown as an unlinked device). Never synced: device secret, DEK, passphrase,
current ticket, `last_seen`, in-flight calls. Availability now syncs in the separate account
profile document, not the `account` map (below). `State` stays the working copy: every persist flushes
into the doc (`write_from`) and every received batch is imported and merged back (`read_into`) under
the `shared` lock, so a missing entry always means a local deletion. Chats and "You" items (34g)
add more docs to the `Hello` list (done in 34g, below).

#### Account profiles over existing links

Own-device `Hello.docs` also advertises `profile` and private `profile-cache`, sealed at
`sync/profile` and `sync/profile-cache`. The former carries extensible JSON fields including
name, availability and the current picture descriptor; the latter forwards cached contact
profiles only to our own devices, merging inner Loro histories to avoid rollback. Existing signed
`Sync`/empty-ack dispatch is reused. A fresh linked/restored install learns existing profile
fields rather than authoring competing defaults. Presence remains ephemeral.

Contacts exchange only our own `profile` via the reserved name in `tinline/chat/4`
`Hello.shards`, `Sync.doc`, and `Ack.doc`, under existing verified-DID/message-token/block
admission. This is not a new channel or a chat-message signature-history scheme. Profile pictures
reuse encrypted BlobHub transfer with contact/current-picture and own-device/cache serving checks.

Replacement/removal automatically signs a `Signed<PictureDeletionPermit>` previous-picture deletion request by the owner DID over domain
`"tinline-profile-picture-deletion-v1\\0"` and `{owner, picture_hash,
scope:"cached-profile-picture", consent:true}`. Verification binds owner, hash, purpose and consent;
only that owner's old cached-picture claim is dropped, never the current picture or chat media.
This requests cooperative cache deletion, not remote erasure. Full APIs/bounds/test status:
[profile-sync.md](design/profile-sync.md).

#### Chats and "You" over the own-device link (task 34g)

- `Hello.docs` also lists `dm/{pair}/{day}` for every shard of every conversation of the account
  (contacts and "You"), newest day first, at most 4096; more are refused (`too many docs`). A doc the
  peer does not list is sent whole. `Sync{doc, vv, update, sig}` for such a doc carries all ops beyond
  the peer's vector (any peer ids); `sig` is the sending device's `sign_batch(doc, update)`, checked
  against the authenticated device (a bad signature closes the session, as for the account doc).
- The receiver validates a chat batch like a vouched history snapshot (docs/chat.md section 8): doc
  of a current conversation, authors only us and that contact, every op proven by its author's DID
  signature under the author's writer ID (below, "Chat message signatures and writer IDs"), plus the
  field rules. A rejected batch is dropped, the session continues.
- `Read { cursors: [(pair, unix ms)] }` (appended `SelfMsg` variant, unsigned): read cursors, merged
  by max; sent after `Hello` and whenever one moves, forwarded to other sessions.
- `Acks { acks: [(doc, vv)] }` (appended `SelfMsg` variant, unsigned): what a contact acknowledged
  holding of each day, merged into our stored ack like the contact's own `Ack`; sent after `Hello` and
  whenever an ack grows, forwarded onward. So a device that is not talking to the contact still shows
  two ticks on our messages only once the contact holds them (each message of ours is ticked against
  the writer ID it was created under, recorded when written or when it arrives: `mo/` in
  docs/chat.md).
- Live updates: a write to any shard marks it dirty for every session, which pushes the ops the peer
  lacks. Blobs use the stock iroh-blobs ALPN: an own (non-removed) device is served any blob that any of our
  conversations references.
- Chat shards still use one session per contact *device*, each device sends its own chat ops
  only, and chat document names are restricted to `dm/{pair with that contact}/{day}`.
  `tinline/chat/4` additionally permits the sender's account `profile`, as described above.

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

## Protocol versions (P9c, U18; P10; P6; account profiles; photo sizes; receipts; postcard)

**Postcard (2026-10-15, pre-release, no back-compat).** Every binary frame, signed body and stored row
moved from bincode 1 to postcard 1 (`cryptography::codec`): chat `tinline/chat/7`, own-device sync
`tinline/self/5`, group `tinline/group/3`. `chat/6` and `self/4` are refused with code 426: **update the
app and wipe its data**.

**Open links (2026-10-21, `feature/public-group`, not merged).** Group ALPN `tinline/group/5` =
`/4` with `GroupMsg::Code` carrying a `CodeKind` (Invite / Device / Open) and `InviteToken` an
`open` flag (also inside self/5 group lists and direct invitation letters, whose encoding
changes with it), `Hello.presence` and `GroupMsg::Presence{presence}` (a host's DID-signed
`Presence{group, host, dev, online: [(did, last seen)], seq, at}`, domain
`tinline/group/presence/v1`, open groups only). In an open group no
`ReceiptHello`/`Receipts`/`ReceiptAck` is sent and incoming ones are dropped. A `/4` peer fails
at connect. Details: `docs/design/public-group.md` §2-§4. Image posts (§5) change records, not
frames: `Body::Post` gains `image: Option<ImageRef{hash, key, size, w, h, mime, preview}>` (the
picture sealed with `transfer::crypt` under a per-image key carried in the post; `hash` is the
ciphertext's BLAKE3), and the group endpoint also accepts the stock iroh-blobs ALPN, gated per
requester and hash. No ALPN bump (`/5` is unreleased).

**Group member receipts (2026-10-16, `feature/group-receipts`, not merged).** Group ALPN
`tinline/group/4` = `/3` plus `GroupMsg::{ReceiptHello, Receipts, ReceiptAck}` (member <->
host only; hosts relay the original native ledger ops). No compatibility with `/3` (user,
2026-10-16: no backward compatibility): a `/3` peer fails at connect. No data wipe (new rows
only). Details: `docs/design/receipt-runtime-roadmap.md`, "Group message receipts".

**Signed receipts (2026-10-15, `feature/receipt-runtime`, not merged).** Chat ALPN
`tinline/chat/6` adds `ChatMsg::{ReceiptHello, Receipts, ReceiptAck}` (`docs/chat.md` section
9). A `/5` app cannot decode them, so `/5` is refused with code 426: **update the app, no data
wipe** (receipt ledgers are new rows; message formats are unchanged). Own-device sync is unchanged
(`tinline/self/4`).

**Photo sizes (2026-10-14).** Current chat ALPN is `tinline/chat/5` and own-device sync is
`tinline/self/4`. A picture's file record carries `width`, `height` (shown size, EXIF orientation
applied) and `preview` (a 4x3 RGB colour grid, 36 bytes hex), and the author's DID signature covers
them (`docs/chat.md` "Version 5"). A `/4` or `self/3` app would reject such a record, so both are
refused with code 426: **update the app, no data wipe**. Records without a picture sign exactly as
before.

**Account profiles.** Chat ALPN `tinline/chat/4` added profiles; `/3` is also refused with code 426
because its day-only parser cannot understand `profile`. This changes no chat content/disk format:
upgrading `/3` clients requires an app update, **not a data wipe**. Own-device sync stays
`tinline/self/3`; older implementations do not advertise the new profile documents. Historical
pre-P10/P9c wipe requirements below still apply to those older data formats.

**P6.** Separate message and call tokens changed what the contact exchange, calls and the chat
`Auth` carry, and the account doc gained call tokens, call permissions and cut-offs. Chat and
own-device sync share P10's bump (`tinline/chat/3`, `tinline/self/3`, same release); calls and
contacts moved to `osvauld/p2p/1`, and `osvauld/p2p/0` is refused the same way
(`crates/core/src/version.rs`: `CALL`). No compatibility (U18).

The chat content format changed incompatibly (below; P9c: DID-signed record fields, P10: one
version per writer), so P10's chat wire was `tinline/chat/3` and own-device sync, which relays chat
docs, is `tinline/self/3`. There is no compatibility with apps or data from before P10/P9c:
upgrading those older formats means wiping the app data on every device.

- Accepting: the endpoint lists `tinline/chat/1`, `tinline/chat/2`, `tinline/self/1` and
  `tinline/self/2` only to refuse them. Such a
  connection is accepted and closed at once with application close code **426** and the reason
  `"<chat|own-device sync> <old alpn> is no longer supported, this app speaks <new alpn>: update the
  app (and wipe its data)"` (`crates/core/src/version.rs` `refuse`, called from the accept loop in
  `node.rs`). The node logs "refused <protocol> from an older app (<alpn>)".
- Dialing: `version::connect` offers the old ALPN as well; if the peer picks it, it is an old app:
  the connection is closed with 426 and the same reason, and the dial fails with
  `Error::Protocol("the other device runs an older app: <reason>")` (chat logs it per contact).
- Tests: `version::tests` (old app dialing us, us dialing an old app, two current apps).

## Chat message signatures and writer IDs (P9c, strict)

Formats inside the chat docs (`dm/{pair}/{day}`); the wire batch `sig` (device key over
`"tinline-chat-batch-v1" ‖ len(doc) ‖ doc ‖ update`) authenticates the sending device only.
Design and rules: docs/chat.md section 7 "Signatures and writer IDs".

- **Record**: `{ id, author, at, reply_to?, file?, v:<writer>... }`; `id/author/at/reply_to/file`
  written once at creation; `v:<writer ID as 16 lowercase hex digits>` = that writer's latest
  version, one map value `{ text, edited_at?, deleted, dev: hex(device ID), did_sig: hex(64 bytes) }`.
  Only that writer writes its key. The message shows its highest-ranked version: `deleted`, then
  `edited_at` (absent lowest), then writer ID (P10).
- **DID signature** (`chat::doc::msg_sig_input`): the author's DID key (Ed25519, `did:key`) over
  `"tinline-chat-msg-v3"` ‖ len-prefixed doc ‖ id ‖ author ‖ `at` (i64 BE) ‖ text ‖ `edited_at`
  (0, or 1 + i64 BE) ‖ `deleted` (1 byte) ‖ `reply_to` (0, or 1 + string) ‖ file (0, or 1 + hash,
  key, name, size u64, mime, kind, duration u32, waveform len u32 + bytes) ‖ device ID (32 bytes) ‖
  `WrittenAt` = Loro peer id (u64 BE) ‖ counter of the span's first op (i32 BE) ‖ op count of the
  span (u32 BE). Strings are u32 BE length-prefixed. The span is the ops that write the version,
  the last being the insert of the writer's own version key: for a new message the `messages`
  key, `id`, `author`, `at`, `reply_to?`, `file?`, `v:<writer>`; for an edit or delete only
  `v:<writer>`. (P8/P9/P9b used
  `"tinline-chat-msg-did-v2"` in a commit message, plus a device-key `sig`; both are gone.)
- **Writer ID** (U16): the Loro peer id a device writes every chat doc with = first 8 bytes (LE) of
  `BLAKE3("tinline-writer-id-v1" ‖ u32 BE len(DID) ‖ DID ‖ device ID)` (`u64::MAX` maps to
  `u64::MAX - 1`). The device ID is 32 random bytes per install of the account (`account.json`
  `device_id`): new on create, link and restore over the directory, and before a chat store is
  created afresh (P10: a lost store must not reuse (writer ID, counter) pairs contacts hold); kept
  when only the device key changes (endpoint rotation). It is never derived from a key.
- **Accepting a batch** (`Shard::apply_remote`, every path): the new ops are split per peer id
  into spans (consecutive ops on one message, ending with the insert of the span's own version
  key; only immutable fields besides, no removals); any op outside one refuses the batch. Each
  span must be proven by the version it wrote: `writer_id(author, dev)` = the span's peer id and
  `did_sig` verifies for the span; and it must not go back from that writer's previous version
  (older `edited_at`, or an undone delete). Live,
  every span's author is the session's DID. Every touched message must end as one of the proven
  versions. Nothing else is consulted (no device keys, device lists or remembered writer IDs).

## Session lifetime (decision 2026-10-17: consent-based links)

Tinline does not hold peer connections open, and links are consent-based: a link to a contact
opens or stays open only because of this side's own action or a real need. A contact can never
hold our link open or learn we are online without our action. Reachability comes from the relay
registration: a peer that needs us redials.

A chat session closes after 60 s with no `ChatMsg` either way and none of this side's reasons
below (`wire::IDLE_CLOSE`; presence datagrams do not count), the same way as a local
`Cmd::Close`: the stream is finished, then the connection closed, so the other side sees a normal
end and does not retry because of it. Each side decides only for itself; it stays open while
this side has:

- recent local activity with that contact: the user typed in the conversation
  (`Node::chat_typing(did)`) or wrote to it (send, edit, delete, file, voice) within the idle
  time; a live 1:1 call with that DID and sending a read receipt for their messages
  (`mark_read`) count as activity too, an automatic delivered receipt does not (user decision
  2026-10-17);
- something pending for that contact: unsent outbox days, wanted blob downloads, owed
  receipts, a pending profile push (the `chat_kick_pending` conditions);
- a live 1:1 call with that DID.

ChatMsg traffic from the contact is real activity too (the idle timer counts it), but nothing a
contact sends can ask us to keep a session: the `tk1` "keep" datagram tried earlier the same day
was removed; one from an older build is ignored.

**Typing is the wake point.** The apps call `Node::chat_typing(did)` on each non-empty composer
edit. It is local only (no typing indicator goes to the contact), cheap and non-blocking, and
throttled in the core to one activity mark per contact every 3 s (`wire::typing_throttle`). It
dials the contact ahead of the send if no session is up, or sends our presence at once on the
one that is. **Opening a conversation connects nothing**; there is no "watch". Sending a message
dials as before.

At start a device dials only contacts with something pending (no longer every contact); the 30 s
tick dials only contacts with something pending. Delivery stays sender-driven: a contact who
wrote while we were away dials us when we are back (its retry backs off up to 60 s). Sessions a
contact opens to deliver are accepted as usual and close on idle. Own devices follow the same
idle rule (see "Own-device sync"). Group call hosting and active calls keep their own connections
and are not affected.

## Presence (on `tinline/chat/4`; consent-based since 2026-10-17)

Whether a contact is active with us right now. Each device keeps a Loro `EphemeralStore` with one
key (`online`), refreshed every 8 s, and sends it as a QUIC datagram on a chat session **only
while the user here is active with that contact** (typed, wrote to it, sent a read receipt for it, or has a live 1:1 call with it, within the idle
time; see "Session lifetime"): `b"tp1"` + `EphemeralStore::encode`, at most 512 bytes. Otherwise it
sends none, so a contact who dials to deliver a message learns nothing beyond the delivery
itself. The receiver applies it into a store belonging to that session only, so a contact speaks
only for its own device. An entry not refreshed within 30 s expires (the store compares the
sender's timestamp with our clock, so this also bounds the clock skew presence tolerates); a
closed session drops its store at once. A contact is online while any of its devices has a live
entry: `ChatEvents::on_presence_changed(did, online)`, `Node::contact_online(did)`. Nothing is
stored or retried.

So "online" means "they are active with me now"; its absence means unknown, never "offline".
The conversation header shows "Online" or nothing, there is no "they're offline" strip, and chat
and contact lists show no online state.

Datagrams rather than a `ChatMsg` variant: an older app never reads them and keeps working (it
just shows the contact as not connected), where an unknown `ChatMsg` variant would fail to decode
and close the session.

## What each check defends against

| Check | Defends against |
|---|---|
| Domain-separated signatures | A blob of one kind replayed as another |
| Key from claimed DID + expected-issuer compare | Forged claims; blobs signed by the wrong party |
| `attestation.device == remote_device` | Relay/MITM or replayer forwarding someone's hello over its own connection |
| Ticket visible == signed fields | A tampered QR/text swapping name/device/relay |
| Invite `iss == me`, `device == my_device` | Redeeming another owner's (or another phone's) invite here |
| `exp` on invite and grants | Stale QRs; grants living forever |
| Redeemed set per (joiner DID, nonce) | Replaying a captured hello (the same joiner redeeming the same ticket twice) |
| Held request + user Accept | A leaked reusable code adding someone without the owner's yes |
| Binding over nonce + both devices | Lifting a joiner's hello onto another invite or another device pair |
| Grant `holder == attested DID` | A stolen grant presented by someone else |
| Grant `iss == me` | Grants minted by third parties |
| Per-DID cut-offs (`IssuanceStore::cut_off_ms`) | Cutting off a contact or a leaked grant |
| Contact check | A valid grant of someone who is no longer a contact |
| Block check (signed notice) | A blocked contact's valid grant; a forged, misaddressed or replayed notice (refused, nothing changes) |

## Blocking (P5; U5 revised, U19, U20, U23)

There is no Remove for a contact: the contact action is Block (`Node::block`, `unblock`;
`remove_contact` is an alias of `block` until the apps switch). A block keeps the contact, the
chat history and the call history, and is visible to the blocked person.

**Notice.** `proto::sign_block_notice` signs, with the DID key and the domain
`osvauld/p2p/block/v1\0`, the JSON claim

```text
BlockNotice { v: 1, from: <blocker DID>, to: <blocked DID>, blocked: bool, at: <unix ms> }
```

as a `Signed<BlockNotice>` (like grants). `blocked: false` is an unblock.
`at` is strictly increasing per (from, to). `verify_block_notice(blob, me)` checks the signature
under the DID named in `from` and `to == me`; the receiver then keeps it only if its `at` is above
the notice it holds from that DID (a replayed or older notice changes nothing; an unblock from
someone whose block we never held is ignored). Every block notice is also a **deletion request**
(U20, always sent): the receiving app deletes its contact entry for `from` (address, grant, device
list) if it was added before `at`, and keeps the conversation read-only
(`block::apply_block_deletion_request`). Not enforceable against modified builds.

**Wire.** On the call ALPN (`osvauld/p2p/0`): `Msg::Notice { notice }` and `Msg::NoticeTaken`.

- Delivery: the blocking device dials every device of the contact, sends `Notice` as the first
  message and waits for `NoticeTaken` (also sent for a valid notice already held, so it stops).
  Untaken notices sit in a local outbox, retried after 2, 5, 15, 30, then every 60 s while online,
  and again at start. Exactly one notice is signed per block; own devices never sign their own.
- Refusal (blocker side, every own device, from the synced notice): a `CallHello` or
  `ContactHello` that verifies under a blocked DID is answered with `Notice` instead of `Ringing`
  / `Welcome` (never rings, no call record); during a call, blocking sends `Notice` and ends the
  call. On `tinline/chat/4` the listener answers a blocked DID's `Auth` with
  `ChatMsg::Notice { notice }` and closes; no session, so no sync, push or history request.
  Blobs are not served to a blocked DID.
- Reading a refusal: a `Notice` that verifies as a block from the peer we dialled ends the call
  with `blocked`, fails `add_contact` / chat dials with `Rejected("blocked")`, and is taken like a
  delivered one.

**Blocked side (our app enforces, U20 layer 1).** While it holds a block from a DID: `call` ends at
once with `blocked` without dialling; `send_text` / `send_file` / `edit_message` /
`delete_message` / `fetch_older_history` fail with `Rejected("blocked")`; no chat retries; our
unacknowledged messages report `not_delivered_reason = "blocked"`. `block_state(did).blocked_me`
carries the verified notice for "<name> blocked you"; `chats()` keeps listing the conversation.
**Blocker side (U20 layer 2):** the refusals above hold for a build that ignores the notice.

**Own devices.** `sent/` and `notices/` sync in the account doc (both verified on read), so a
block made on one device is refused by all, and a notice received by one device is honoured by
all (including the deletion request).

**Unblock.** Signs a newer notice with `blocked: false`, delivered the same way. The other app
takes it even though it no longer has us as a contact (it holds our block) and clears "blocked
you"; messaging resumes only after they add us again (ticket/QR), since their entry was deleted.
Scanning a blocked contact's ticket ourselves unblocks them first.

Tokens are taken back per DID with cut-offs (see "Tokens"), never by id; a block takes back
both of the blocked contact's tokens.
Shared tickets are good until they expire or are reset; one-time request tickets are dropped when used.

Not in P5: blocking does not rotate any endpoint (U5) and changes no group membership; there is
no attempt counter yet ("tried to message you N times"); the call ALPN was not bumped for the new
messages (an older app fails to decode them, U18).

## Tickets in the core

- **Reusable codes (2026-10-13; 2026-10-14: no per-joiner limit).** A ticket adds any number of
  joiners, and the same joiner any number of times (their other devices; a re-add lifts a block),
  until it expires or is reset. The (joiner, nonce) entries (`redeemed_key`) are kept as a record
  only. One-time tickets (accepting a group chat request) redeem once, for their DID only, and
  are dropped when used. Which nonces still add a joiner is `core::store::Tickets`
  (`IssuanceStore::spent`), checked inside `accept_contact_hello_with`. Every addition is a request the user accepts (see "Contact requests").
- Live tickets (`State::live_tickets`, nonce -> `exp`, local): the one handed out now plus older
  ones replaced by a rename (`set_name` or a synced profile name: the old one keeps the old
  signed name), a relay change or the last day of life; they stay good until they expire.
  `reset_ticket` ("New code") and `block` clear them all at once, so every code handed out so
  far stops working; the next `my_ticket` mints a fresh one. Redeeming no longer drops it.
  A ticket that stops adding contacts this way has its code wake token revoked at the relay
  (`docs/push.md` "Code wake tokens"); an expired one is only forgotten (the relay refuses it
  by itself). With push on, a ticket without a token is replaced once the relay seals one, and
  one with a token is replaced when push goes off or the push token changes.
- The inviter makes the new contact and the redeemed (joiner, nonce) durable in one write
  (sealed account store) BEFORE it sends `ContactWelcome` (at once or after Accept); if that
  write fails it sends `Reject` (or Accept fails), nothing changed, and the joiner can try again.
  The UI hears of the contact after the commit.
- Tickets live 7 days; one with less than a day left is replaced when asked for (`my_code`
  gives the date).
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
  `Ended`. A block keeps their entries.
- Safety number (core only since 2026-10-18: the apps no longer show it or let you mark a contact
  verified; `safety_number`/`set_verified` stay in the API): per party, SHA-512 over `"tinline-safety-v1" || signing_pk`, then 5200 rounds
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
| `hangup_local` | Active call ended by us (also: we locked mid-call) |
| `hangup_remote` | Active call ended by them |
| `declined` | Caller's view: the callee (any of its devices) declined |
| `declined_local` | Callee's view: we declined (not a missed call) |
| `cancelled` | Callee's view: the caller gave up (`Hangup` or `Cancel{caller_hangup}`, or the link died) before we answered: missed. Caller's view: we hung up before they answered |
| `no_answer` | Rang for 60 s (30 s for a call waiting over another call), nobody answered (either side; missed on the callee) |
| `unreachable` | Caller's view: could not connect, or they refused (offline, unavailable, or not a contact there) |
| `calls not allowed` | Caller's view: they let us message them but not call (no call token, or taken back); ends at once, never rings |
| `blocked` | They block us (their signed notice, held or just received), or we blocked them mid-call; ends at once, never rings |
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
- No device revocation list and no per-grant revocation (tokens are taken back per DID): a
  stolen unlocked phone is stopped only by the contact blocking us. Attestations never expire (`iat` is informational).
- The ticket is a bearer secret for its week: anyone holding it can ask to be added. Each
  request needs the owner's Accept; New code (`reset_ticket`) cuts an old one off.
- A ticket's visible `relay` hint is signed but not authenticated by anything else;
  a relay only sees ciphertext, but can deny service.
- Issued statements tolerate a 5 minute issuer clock lead (`ISSUE_SKEW_MS`); `now >= exp` is
  expired. Callers choose the clock.
- Contact names are self-reported and not unique; show the DID fingerprint on confirm.
- No forward secrecy or media encryption here: media rides the QUIC connection.
- The joiner's grant check on Welcome ignores revocation (fresh grants only).
