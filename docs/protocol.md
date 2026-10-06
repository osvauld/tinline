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
signature over `DOMAIN || payload`. Domains: `osvauld/p2p/{attest,invite,grant,bind}/v1\0`.
The verifying key is always recovered from the DID named inside the payload; callers then
compare that DID to the one they expected.

## Contact ticket (QR)

Text: `osvc1.` + b64url(json). Fields: `{v:1, did, name, device, relay, invite}`.
`invite` is a signed `InviteClaim {v, iss, nonce(16B), iat, exp, device, name, relay}`.
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

## Revocation

Grants carry a random 16-byte id. The callee keeps a revoked-id set and passes it to
`accept_call_hello`. Removing a contact = drop it from the contact list (and optionally
revoke its grant id). Grants are short-lived by choice of `ttl`; use `renewed_grant` in
`Accept` to refresh. Tickets are single-use via the redeemed-nonce set and also expire.

## Known gaps

- Device secret storage (Keystore, Android Keystore) is the caller's job; proto only
  derives the public key. Losing it means re-adding contacts.
- No device revocation list: a stolen unlocked phone is stopped only by revoking its
  grants. Attestations never expire (`iat` is informational).
- The ticket is a bearer secret until redeemed: whoever scans first wins. Show it
  in person, keep `ttl` short.
- A ticket's visible `relay` hint is signed but not authenticated by anything else;
  a relay only sees ciphertext, but can deny service.
- No clock-skew tolerance; `now >= exp` is expired. Callers choose the clock.
- Contact names are self-reported and not unique; show the DID fingerprint on confirm.
- No forward secrecy or media encryption here: media rides the QUIC connection.
- The joiner's grant check on Welcome ignores revocation (fresh grants only).
