# Rotation live-possession challenge: storage options (packet C3)

Status: proposal — not approved

Question: does live possession of a rotated endpoint key need durable nonces at all?
This compares two **candidate-policy G-STORAGE** designs. Neither is selected. G-STORAGE,
G-RETIREMENT and G-TIME stay open.

Executable model: `design-models:crates/proto/tests/rotation_challenge_contracts.rs` (14 tests, fake time,
real Ed25519, in-memory only, every rejection compares the whole receiver before/after).

## 1. Verified in source/tests

### 1.1 Existing variant (a) and its descendants

| Fact | Source |
|---|---|
| Per-receiver `next_nonce` described as "Persisted high-water mark; outstanding challenges are volatile and bounded" | `design-models:crates/proto/tests/support/rotation_permit_network.rs:23-25` |
| Issuance: prune expired, `>= 8` → `challenge_capacity`, `checked_add` → `nonce_exhausted`, deadline `now + 5` | `support/rotation_permit_network.rs:99-104` |
| Challenge bytes = domain, receiver handle, nonce, newest permit ID; no sender key, no session | `support/rotation_permit_network.rs:48-52` |
| Comment: "Counter allocation is assumed durable before issuance; real persistence is pending" | `support/rotation_permit_network.rs:97` |
| Accept verifies the signature, THEN removes the nonce (`session_replay` if absent), then checks expiry | `support/rotation_permit_network.rs:119-124` |
| Restart keeps high-water, clears outstanding | `support/rotation_permit_lifecycle.rs:51-56` |
| One shared pool of 8 per receiver; a different receiver is unaffected | `support/rotation_permit_lifecycle.rs:243-263` |
| B1 `Challenge {receiver, nonce, head}`; comment "C3 owns the real session/challenge design; this is not a replay-safe handshake" | `design-models:crates/proto/tests/rotation_history_paging.rs:182-200` |
| B1 `bind` returns `challenge_unknown` for foreign/consumed nonces and consumes only on success | `rotation_history_paging.rs:410-430`; `docs/design/rotation-history-policy-options.md:32` |
| B2 replayed challenge/proof → `challenge_unknown` | `design-models:crates/proto/tests/rotation_history_adversarial.rs:305`, `:1588` |
| `rotation_permit_model.rs` `live()` accepts arbitrary challenge bytes; comment says production must bind to the connection or use iroh's authenticated transport | `design-models:crates/proto/tests/rotation_permit_model.rs:107-113` |

### 1.2 What today's handshakes already bind to the QUIC/TLS session

| Fact | Source |
|---|---|
| Module doc: device key is the iroh endpoint key, "the QUIC handshake already proves the peer holds it"; checks tie the attested device to `remote_device` | `crates/proto/src/lib.rs:3-7` |
| `accept_self_hello` requires `att.device == remote_device` | `crates/proto/src/lib.rs:780-798` |
| `accept_contact_hello` requires `att.device == remote_device` and a DID signature over (invite nonce, both device keys) | `crates/proto/src/lib.rs:895-905`, `:820-828` |
| Core feeds `remote_device` from `conn.remote_id()` | `crates/core/src/sync/engine.rs:369,385`; `crates/core/src/chat/engine.rs:384,455`; `crates/core/src/node.rs:1969,2068` |
| Self-sync already uses `conn.stable_id()` to identify a session | `crates/core/src/sync/engine.rs:361` |
| Transport: keep-alive 5 s, idle timeout 20 s | `crates/core/src/node.rs:1775-1779` |
| No use of `export_keying_material` or 0-RTT in `crates/core/src` or `crates/proto/src` | `grep` (no matches) |

So today nothing application-level is a channel binding: hellos rely on the transport's
authenticated `remote_id()` plus DID-signed attestations, which are replayable on their own
(`attest` has no expiry, `crates/proto/src/lib.rs:197-208`) but useless without the device key
because `remote_id()` must match.

### 1.3 iroh / noq / rustls APIs (read in `~/.cargo/registry`, iroh 1.3.0 per `Cargo.lock`, rustls 0.23.45)

| API | Source read | What it says |
|---|---|---|
| `Connection<HandshakeCompleted>::remote_id() -> EndpointId` | `iroh-1.3.0/src/endpoint/connection.rs:1159-1169`; set via `static_info_from_conn` → `remote_id_from_noq_conn` (`:368-372`, `:383-411`) | the single peer certificate's SPKI decoded as the Ed25519 key (`peer_identity()`, exactly one cert required) |
| `Connection<T>::export_keying_material(output, label, context)` | `iroh-1.3.0/src/endpoint/connection.rs:1115-1122` (in `impl<T: ConnectionState> Connection<T>`, `:822`) | RFC 5705 exporter; both peers get the same bytes for the same label/context |
| `Connection<T>::stable_id()` | `iroh-1.3.0/src/endpoint/connection.rs:1096-1104` | fixed for the connection lifetime |
| delegation chain | `noq-1.3.0/src/connection.rs:883-895` → `noq-proto-1.3.0/src/crypto/rustls.rs:198-209` → `rustls-0.23.45/src/quic.rs:116-122` | rustls doc (`quic.rs:111-114`): TLS 1.3 exporter, never the early exporter; fails before the handshake completes |
| 0-RTT connection states exist | `iroh-1.3.0/src/endpoint/connection.rs:700,732,1239,1274` | `remote_id()` returns `Result` there |
| Server cert must equal the dialed endpoint ID; TLS 1.3 only; CertificateVerify signature checked with the raw Ed25519 key | `iroh-1.3.0/src/tls/verifier.rs:33-75,77-100` | |
| Client cert: no intermediates; CertificateVerify signature checked; key otherwise unchecked | `iroh-1.3.0/src/tls/verifier.rs:125-141,154-165` | |

Unverified: whether exporter calls succeed on `IncomingZeroRtt`/`OutgoingZeroRtt`
connections in practice (only the rustls doc comment was read); that `peer_identity()` is
populated only after the CertificateVerify check succeeded (noq/rustls internals not traced).

## 2. The two candidates as modelled

| | (a) `NonceReceiver` | (b) `SessionReceiver` |
|---|---|---|
| Durable | accepted views/live + `high_water` | accepted views/live only |
| Volatile | `outstanding: nonce → {requester, owner, binding, deadline}` | `sessions: id → {remote, owner, challenge}`, `next_session`, `boot_entropy`, `draws` |
| Signed bytes | domain ‖ receiver endpoint ‖ nonce ‖ newest permit ID | domain ‖ receiver endpoint ‖ sender endpoint ‖ random challenge ‖ newest permit ID |
| Issued to | any requester with a valid page (possession comes later) | only a session whose transport-authenticated `remote` equals the page's newest endpoint |
| Bound to session | no — answer may arrive on any connection | yes — proof accepted only in its issuing session |
| Lifetime | TTL 5 (fake ticks) | session lifetime (model: until `close`; no TTL) |
| Restart | outstanding lost, `high_water` = last persisted | everything volatile lost; session ids restart at 0 |

Common: page verification (copied from `rotation_permit_model.rs` @6b1aaef), newest record
must be current, failed accept does not consume the challenge (B1 choice), success consumes it
and writes the accepted state once. `Quota` parameter: `GlobalOnly` (existing), `PerRemote(n)`,
`PerOwner(n)`; a global cap of 8 always applies.

## 3. Results (from the executable model)

| Property | (a) | (b) | Test |
|---|---|---|---|
| Wrong key / wrong page rejected unchanged | yes | yes (wrong remote refused before a challenge exists) | `both_variants_bind_only_the_newest_endpoint_key_and_reject_unchanged` (invariant) |
| Replay across sessions | consumed nonce → `challenge_unknown`; but a first answer is accepted from ANY connection | other session → `possession`; consumed → `challenge_unknown`; closed → `session_unknown`; other receiver with identical random → `possession` | `nonce_replay_across_sessions_is_rejected_but_answer_is_not_session_bound`, `session_proof_is_rejected_in_any_other_session_or_receiver_and_after_use` |
| Replay after receiver restart | rejected **because** high-water was persisted before issuance | rejected because the challenge is fresh random; session ids repeat harmlessly | `nonce_replay_after_receiver_restart_is_rejected_by_durable_high_water`, `session_replay_after_receiver_restart_fails_although_session_ids_repeat` |
| If the freshness source fails | counter not written ahead → old nonce re-issued, captured proof accepted by a requester **without the key** | repeated boot entropy → captured proof accepted, but only inside a session authenticated as that key | `nonce_counter_without_write_ahead_reissues_old_nonce_and_accepts_captured_proof` (limitation), `session_replay_succeeds_if_challenge_randomness_repeats_after_restart` (limitation) |
| Capacity exhaustion | 8 outstanding; anyone holding a captured update can fill it; 1 disk write per issued challenge; recycles every TTL | 8 pending; only holders of a current endpoint key of a known contact can fill it; 0 writes; recycles on session close; 1 pending per session | `nonce_capacity_exhaustion_costs_disk_writes_and_recycles_only_after_ttl`, `session_capacity_exhaustion_costs_no_disk_writes_and_recycles_on_close` |
| Peer isolation | `GlobalOnly`: one key blocks everyone; `PerRemote`: Sybil keys (free) fill the pool; `PerOwner`: replaying a victim's captured page exhausts the victim's quota. `PerRemote` does stop a single key. | `PerOwner(2)`: strangers get `remote_mismatch` before using capacity; a contact forking its own DID into fresh keys is capped at 2; other contact unaffected | `nonce_variant_admission_cannot_isolate_peers_from_an_unauthenticated_requester` (limitation), `nonce_per_remote_quota_isolates_a_single_flooding_key`, `session_per_owner_quota_isolates_peers_and_strangers_cannot_consume_it` |
| Nonce overflow | never wraps; `nonce_exhausted` is durable → permanent issuance failure after restart | no counter (n/a) | `nonce_high_water_never_wraps_and_exhaustion_survives_restart` (invariant) |
| Durable writes per honest update | 2 (high-water + accepted state) | 1 (accepted state) | `durable_writes_per_honest_update_a_two_b_one` |
| Durable writes per unanswered / hostile attempt | 1 each (forced by any requester; at most 8 per TTL per receiver) | 0 | same test, and the capacity tests |

## 4. Failure modes

(a) durable nonce:
- Correctness depends on a write-ahead of every issued nonce (or of a reserved block). A
  lost/torn/reordered write re-issues nonces (limitation test). C2 owns torn-write semantics.
- Unauthenticated requesters force disk writes (write amplification, flash wear on phones)
  and can fill the shared pool. No quota tested isolates peers against an unauthenticated
  requester.
- Overflow is durable unless a reset policy exists (open; would need its own replay argument,
  e.g. binding a per-install epoch).
- Benefit only (a) has: a challenge can be answered asynchronously over a different
  connection/relay path. Live possession by definition needs the sender online, so whether
  this matters is a product question (below).

(b) session-bound:
- Freshness moves from disk to the RNG at boot. A broken RNG (repeated entropy) re-enables
  replay (limitation test), though in this model only inside a session authenticated as that
  key.
- Capacity is held for the session lifetime. The real transport sends keep-alives every 5 s
  (`node.rs:1777`), so an idle-but-alive peer can hold a pending challenge indefinitely; a
  production (b) needs a volatile per-session challenge deadline (not modelled here).
- A legitimate contact can still mint forks of its own DID to obtain more admitted keys;
  `PerOwner` bounds that per DID, but many malicious contacts can still fill the global cap.
- Requires request and answer in one live session, which matches the "live" definition but
  rules out mailbox/relay-queued possession proofs.

## 5. What a real QUIC/TLS channel binding would provide (proposed)

Verified above: iroh exposes `remote_id()` (key from the TLS 1.3 certificate, whose
CertificateVerify signature iroh checks with the raw key) and `export_keying_material`
(RFC 5705 exporter, post-handshake only per rustls doc). Proposed, not tested against iroh:

- A (b) session could use `remote_id() == newest permit endpoint` as the possession proof
  itself: the TLS handshake already signs a fresh transcript with that key. An explicit
  application signature then only binds the *permit* to the session.
- If an explicit signature is kept, signing an exporter value (label such as
  `"tinline/rotation-possession/v1"`, context = newest permit ID) instead of a receiver
  random would bind the proof to the TLS session without a random-challenge round trip and
  would remove the "repeated boot entropy" failure, because the exporter derives from both
  peers' handshake randomness. Requires the full handshake (never 0-RTT).
- Either way nothing is persisted for possession; the only durable write per update is the
  accepted view (shared with (a)).
- The model does NOT simulate TLS, the exporter, or connection migration; these are
  unverified behaviours.

## 6. What this does NOT prove

- No socket, TLS, relay, 0-RTT, migration or real RNG behaviour is modelled; the session's
  `remote` is a trusted input.
- Disk writes are counted as ideal atomic commits; ordering, fsync cost and torn writes are
  C2's scope.
- Capacity 8 / TTL 5 / quota 2 are fixture values, not product defaults (G-TIME, G-STORAGE).
- No claim about multi-device sharing of one receiver's nonce space, linked devices, or
  shared-mnemonic revocation.

## 7. Policy questions for the coordinator / user

1. G-STORAGE: is per-issued-challenge durable writing (a) acceptable on phones, or should
   live possession be strictly session-bound (b) with nothing persisted?
2. Must possession ever be provable asynchronously (queued/relayed), or only on a live
   connection? (Only (a) supports the former.)
3. If (b): explicit random challenge, exporter-bound signature, or `remote_id()` alone?
   Any of these needs a volatile per-session challenge deadline value (G-TIME).
4. If (a): reservation block size, nonce-exhaustion recovery policy, and an admission rule
   that does not let unauthenticated requesters force writes.
5. Admission keying: `PerOwner` (verified DID) vs `PerRemote` vs global, and the global cap.
