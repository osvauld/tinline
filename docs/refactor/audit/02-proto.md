# Audit 02: crates/proto (read-only)

Total 18.6k lines: src ~4.9k (lib.rs 1390, tests.rs 1136, gcall 808, link 382, gcall_status 293, profile 94), tests ~13.2k.
Key finding: **production src is reasonably tidy; the duplication is almost all in tests/**, where ~10 files are
self-contained "test-only candidate semantic" models that each re-declare the same permit types and fixtures.
Only 4 test files touch the real `proto` API (capability_tokens, gcall_capacity_bounds, rotation_identity_boundary,
rotation_device_list_push partially). The other 8 `rotation_*` files test **models living in the test file itself**,
not production code (they say so in their headers: "Candidate ... NOT a production wire format").

## 1. File map (for docs)
src/
- lib.rs: wire core. enc/dec (b64url), device keys, SignedBlob + sign_blob/open_blob, Attestation, ContactTicket (v1 JSON `osvc1.`, v2 base32 `OSVC2:`), Grant/Cap/tokens (verify_grant, verify_token, TokenPolicy), Msg enum + length-prefixed JSON frames (MAX_FRAME 64K), DeviceList (+newer, seq bound), BlockNotice, accept_self_hello, add-contact handshake (contact_hello/accept_contact_hello/welcome), call hello (accept_hello/accept_call_hello).
- link.rs: device-link QR (`OSVL1:`), HMAC proofs, confirm code, AES-GCM sealed account grant, LinkMsg frames (ALPN tinline/link/1).
- gcall.rs: group-call signed claims (one `Claim` enum, domain `tinline/gcall/v1\0`), roster, control frames (MAX_CONTROL 4096), audio datagram codecs, MAX_PARTICIPANTS=12.
- gcall_status.rs: signed CallHostStatus (same domain/claim style) with shape bounds + freshness.
- profile.rs: owner-signed PictureDeletionPermit.
- tests.rs: 1:1 handshake/ticket/frame/devicelist/link/block-notice unit tests (uses `generate()` identities, not seeded).
tests/
- capability_tokens.rs (1234): test-only W2C model of per-capability tokens/renewal/block notice, plus a few checks against real `proto::issue_*`. Hand-rolled base64url encoder.
- gcall_capacity_bounds.rs: real gcall roster bounds; one `#[ignore]` red acceptance test.
- rotation_identity_boundary.rs: real `attest/accept_self_hello` over rotating endpoints (small, genuinely production).
- rotation_permit_model.rs (+support/rotation_permit_network.rs, rotation_permit_lifecycle.rs): base permit model; network composition and timed/crash lifecycle.
- rotation_succession_model.rs (+support/rotation_network.rs): older, simpler "Record/Signed/Receiver" chain model (different type shapes; superseded by the permit family?).
- rotation_challenge_contracts.rs: nonce vs session possession-challenge variants, quotas, durable-write counts.
- rotation_history_paging.rs: paging, live binding, strict vs checkpoint option, group ancestry contract.
- rotation_history_adversarial.rs: byte caps, frames, out-of-order pages, inbox queue drop policies.
- rotation_fork_revocation.rs: fork policies, revocation/recovery, authority options.
- rotation_storage_contracts.rs: crash-at-every-step sender/receiver recovery, ack ordering, overlap.
- rotation_time_contracts.rs: fake clocks, interval/overdue/reboot policies, jitter.
- rotation_visible_block.rs: block + rotation notes; "blocking never moves rotation" (maps to AGENTS.md policy).
- rotation_device_list_push.rs: device list push/forks; uses real `sign_device_list/verify_device_list/newer`.

## 2. Test duplication (ranked by payoff)

### A. Permit model copied 7 times (HIGHEST payoff, low risk)
`Claims` (10 fields), `Permit {claims,signature}` with `bytes()/issue()/id()`, and `View` (owner, audience, anchor, head, current_id, endpoint, generation, expires_at) with `View::initial` are byte-identical (only `type Id`/`[u8;32]` spelling and derive lists differ) in:
- rotation_permit_model.rs:17-125 (Claims 17, Permit 31, View 51, `next()` 116)
- rotation_challenge_contracts.rs:43-110 (+ `successor` 174)
- rotation_storage_contracts.rs:32-153 (+ custom `impl Serialize for Permit` 154, `successor` 184)
- rotation_time_contracts.rs:23-156
- rotation_fork_revocation.rs:42-181 (Claims adds Deserialize+deny_unknown_fields? see history_adversarial)
- rotation_history_paging.rs:36-181 (adds `wire_len`)
- rotation_history_adversarial.rs:38-177 (adds Deserialize/deny_unknown_fields, `bytes_parsed` in Work)
Diffs between paging/fork_revocation/adversarial blocks are 18-29 diff lines of comments/derives.
`View::page`/`verify_history_page` (atomic page verify, page limit, owner sig, scope, validity, revoked, fork, stale, gap, predecessor, unchanged_id) appears in permit_model.rs:~66 (as method), and as free fn `verify_history_page` in rotation_history_paging.rs:121, rotation_fork_revocation.rs:122, rotation_history_adversarial.rs:117 (near-identical, ~60 lines each). `Challenge`+`LiveBinding`+`bind_live` likewise x3 (paging:182-250, fork:183-250, adversarial:178-245). `Work` counter struct x3 (paging:114, fork:115, adversarial:109). `Checkpoint`/`verify_checkpoint` x2 (paging:251-340, fork:250-331).
Estimated saving: ~1,000-1,300 lines (7 x ~100 type/view + 3 x ~130 page/bind/checkpoint).
Proposed helper: `tests/support/permit_model.rs` (one `mod`), pub items `Claims, Permit, View, Work, Challenge, LiveBinding, verify_history_page(view,&[Permit],now,&Revoked,&mut Work)->Result<View,&'static str>, bind_live, Checkpoint, verify_checkpoint, PERMIT_DOMAIN, PAGE, INTERVAL, OVERLAP`. Include via `#[path="support/permit_model.rs"] mod permit_model;` (existing pattern) or, better, `tests/support/mod.rs` + `mod support;`. Caveat: each test crate compiles it separately so unused items warn: add `#![allow(dead_code)]` in the support module. Risk: storage_contracts has a custom Serialize for Permit and adversarial relies on Deserialize; make those superset (derive Deserialize, deny_unknown_fields) and keep a `Permit::wire_len()`. The "candidate" models are intentionally allowed to diverge as decisions evolve; a shared base would couple them, so give variants (history vs storage) separate small structs only where semantics really differ.

### B. Identity/key fixtures (high payoff, trivial risk)
- `owner(seed)`/`identity(seed)`/`ident(seed)`: `Mnemonic::from_entropy(&[seed;32])` -> `recover`: proto tests/capability_tokens.rs:56, rotation_device_list_push.rs:59, rotation_visible_block.rs:70, rotation_fork_revocation.rs:348, rotation_history_adversarial.rs:345, rotation_history_paging.rs:460, gcall_capacity_bounds.rs:6, src/gcall.rs:563, src/gcall_status.rs:182. **CROSS-FOLDER**: identical again in core/src/gcall/rank.rs:271, core/src/gcall/tests.rs:33, core/tests/device_list_*.rs, group-sim/src/{authority.rs:172,host_only.rs:64,flags.rs:67,multi_host.rs:73,removal_rotation.rs:74,tests.rs:102}. ~19 copies workspace-wide. Proposal: add `identity::testing::from_seed(seed: u8) -> Identity` (behind a `test-support` feature or `#[doc(hidden)]`) -- 1 function, ~4 lines each saved ~70 lines total, removes the "copied fixtures; coordinator consolidates later" comments (rotation_device_list_push.rs:43, rotation_visible_block.rs:55).
- `fixture_seed(label,seed,generation)` + `fixture_key(seed,generation)`: identical in rotation_device_list_push.rs:46/50, rotation_visible_block.rs:57/61, rotation_fork_revocation.rs:332/336, rotation_history_adversarial.rs:330/334, rotation_history_paging.rs:445/449; variant (own domain string `c1-` / `c2-fixture-endpoint`) in rotation_time_contracts.rs:163 and rotation_storage_contracts.rs:164 (the latter says "copied from time_contracts"). Also `fixture_device_id` x3 (paging:456, fork:343, adversarial:341). Also `challenge_contracts` `seed32`/`key`/`public` (160-172). Proposal: `tests/support/fixtures.rs` with `fixture_key(seed:u8, generation:u64)`, `fixture_device_id`, `owner`, `did`. Note the c1/c2 domain differences change key bytes but nothing asserts specific bytes (verify before merge).
- `did(seed)` = `owner(seed).did().to_string()`: capability_tokens.rs:63, device_list_push.rs:79, visible_block.rs:517. Also `ALICE/BOB/CAROL/DAVE/MALLORY` u8 consts (capability_tokens:46-49, device_list_push:83-87).
- `no_revocations()`: rotation_time_contracts.rs:157, rotation_storage_contracts.rs:180 (permit/paging use `&HashSet::new()` inline).
- `ser<T: Serialize>`: device_list_push.rs:66, visible_block.rs:92.
- `sha256` of concat patterns: `Sha256::digest([..].concat()).into()` appears ~20 times; add `fn h(parts:&[&[u8]])->[u8;32]`.
- base64: hand-rolled b64 encode/decode in rotation_device_list_push.rs:662-695 and capability_tokens.rs:521 (comment says "base64 not an allowed test dependency", but proto already depends on `base64` and tests can use it since it is a normal dependency of the crate; integration tests can `use base64`). Drop ~50 lines, simply `use base64::engine::general_purpose::URL_SAFE_NO_PAD`.
- Real-API test fixtures: tests.rs uses `identity::generate()` (random) and src unit tests (gcall.rs:560-600, gcall_status.rs:176-208) duplicate `ident/gid/host_ref` helpers: `gid()` = `"f0".repeat(32)` and `host_ref(id)` in both; `status()` in gcall_status builds the same HostRef inline. Move to a `#[cfg(test)] mod testutil` in lib.rs (~25 lines).

### C. Network/queue harness duplicated
`Peer`, `Packet`, `Network` {peers, queue} appear in support/rotation_network.rs:10-60 (for Signed records) and support/rotation_permit_network.rs:13-60 (for Permit pages); same VecDeque shape, offline/blocked/drain loops. `DurableRotation` (rotation_network.rs:64) vs `DurableDevice` (rotation_permit_lifecycle.rs:6) both implement initial/prepare/announce with `proto::new_device_secret()`. rotation_succession_model.rs (176 lines) plus rotation_network.rs (213) test the same invariants as rotation_permit_model + permit_network + lifecycle (ordered/idempotent, gap needs catch-up, forged/tampered/cross-slot/cross-audience rejected, fork detected, all delivery orders converge, crash boundaries, offline catch-up) on a simpler record type. See D.

### D. Overlapping scenarios (merge candidates)
| Scenario | Files | Verdict |
|---|---|---|
| ordered+duplicate idempotent; missing history needs catch-up; forged/tampered/cross-scope rejected; fork detected; all 3-record orders converge | succession_model.rs:91-175 vs permit_model.rs:128-237 + history_adversarial.rs:874 ("all_permutations_of_three_pages") | Redundant: succession_model is the older, weaker version of the permit model. Candidate to delete succession_model + support/rotation_network.rs (~390 lines) after confirming nothing unique (serialized restart test at :103; crash-boundary at rotation_network.rs:163-189 vs lifecycle.rs:91). Medium risk: confirm with owner whether it's historical documentation. |
| "newest key only is bound by live proof / retired key cannot bind" | challenge_contracts.rs:644, history_paging.rs:636, permit_model.rs:128, fork_revocation (bind_sender) | Overlap but different mechanisms (nonce/session/live binding); table-drive over `BindVariant` only for the shared assertions. |
| Expired history catches up but cannot authorize live use | permit_model.rs:158, permit_network.rs:184, lifecycle.rs:209, history_paging.rs:701 | Near-redundant (4x). Keep one model-level + one network-level. |
| Overdue resume creates one successor | time_contracts.rs:803-873 (already parameterized by OverduePolicy), lifecycle.rs:128 | Different layer; keep. |
| Block never triggers/shifts rotation | visible_block.rs:986, time_contracts.rs:526, lifecycle.rs:59/128 | Three separate asserts of same invariant; add one shared helper `assert_deadline_independent_of_blocks`. |
| Fork policies FreezePath/KeepHead | fork_revocation.rs POLICIES array (919), device_list_push.rs `Fork` enum (274) and fork tests 821-915 | Different domains (permit chain vs list); not mergeable. |
| Block-notice model | capability_tokens.rs:107-137, device_list_push.rs:109-140, visible_block.rs:139-160 (3 domains `w2c-`, `w2d-`, `r1b-`), plus the real `proto::BlockNotice` | Three test-only reimplementations of a signed block notice that proto already ships (lib.rs:1033). Replace with `proto::sign_block_notice` / `verify_block_notice` (supports tests tied to production). |
| Size/frames: roster fits limit | gcall.rs:747 (`roster_of_12_fits`) vs gcall_capacity_bounds.rs:36 `current_twelve_member_roster_roundtrips` | Duplicate; fold the integration test into the unit one. |
Table-driven candidates inside files: device_list_push.rs 4 restore/retain tests (695-767); time_contracts reboot tests (1112-1205, `reboot_with_wall(policy, delta)` already exists); capability_tokens `allow_calls_*`/`legacy_downgrade_*` per Shape.

## 3. Production duplication in src/
Already well factored by `sign_blob/open_blob/check_version`. Remaining:
1. `open_*` in gcall.rs (open_join/leave/end/start/host_request/host_refusal/note, lines 275-445): each repeats `let Claim::X(c) = open_claim(blob)? else { return Err(UnexpectedMessage) }; check_common(...)?`. A macro or generic `fn open_as<T>(blob, pick: fn(Claim)->Option<T>)` would save ~30 lines. Low risk, low payoff.
2. `sign_*` wrappers (sign_start/roster/end/host_request/host_refusal) are 1-liners `sign_claim(id,&Claim::X(v))`; could be `impl From<X> for Claim` + a generic `sign<T: Into<Claim>>`. ~15 lines.
3. gcall_status.rs duplicates gcall.rs: `GCALL_DOMAIN` (same literal at gcall.rs:22 and gcall_status.rs:18), `MAX_TEXT=128` (gcall.rs:41, status:28), `text_ok` (gcall.rs:251, status:119), size pre-check, version check, the `StatusClaim` single-variant enum (tagged "t") vs `Claim`. Easy: `use super::gcall::{GCALL_DOMAIN,text_ok,MAX_TEXT}` (make pub(crate)); or add `HostStatus(CallHostStatus)` to `Claim` (signer = host_ref.holder_did). The separate enum exists to keep the status out of the control `GroupCallMsg` (cross-kind refusal test, gcall_status.rs:224), so prefer sharing helpers not the enum. ~10 lines.
4. lib.rs: three "peek" functions parse the unverified payload: `peek_grant` (639), `peek_device_list` (1003), inline closure in `newer` (1012). One `fn peek<T: DeserializeOwned>(blob)->Option<T>`. ~8 lines.
5. `issue_token`, `issue_legacy_grant_for_test` (604-630) build the same Grant; the second is `#[doc(hidden)]` but `pub` in the library and used by core/src/store.rs, core/src/caps.rs. Make a private `mk_grant(id,v,cap,iat,ttl)`. ~8 lines. Likewise `forge_block_notice_for_test` vs `sign_block_notice` (1043 vs 1065): the latter can call a shared inner `block_notice(signer, from, ...)`.
6. Decode of 64-byte signature: `dec(x)?.try_into().map_err(|_| Error::Decode)?` at lib.rs:176 and ~1213 (accept_contact_hello) and in link.rs; add `dec64`. Tiny.
7. Verify-attestation-then-check-device pattern: accept_self_hello (1074), accept_contact_hello_with (1209), accept_contact_welcome (1270), accept_hello (1353) each call `verify_attestation` then compare `att.device_key()? != remote_device -> DeviceMismatch`. Helper `verify_attestation_for(att, remote_device)`. ~12 lines.
8. `NewContact` construction duplicated in accept_contact_hello_with (1233-1241) and accept_contact_welcome (1288-1296): fine, 2 sites.
9. lib.rs `contact_hello`/`accept_contact_hello` thin wrappers over `_with` variants (positional 7-8 arg functions, `#[allow(clippy::too_many_arguments)]` at 1191) -> a params struct would clean the API but touches core. CROSS-FOLDER (core callers).
10. lib.rs is 1390 lines; a split (ticket.rs, grant.rs, devlist.rs, handshake.rs, frame.rs) would help docs and review but is mechanical, zero logic risk. `Msg` frame encode: `encode_frame`->`encode_json_frame` and `decode_frame`->`decode_json_frame`->`_max` are 2 trivial indirection layers (872-905); link.rs presumably uses `decode_json_frame`. Also link.rs `LinkMsg` frames reuse them, good.
CROSS-FOLDER: link.rs HKDF/AES-GCM key derivation may overlap crates/cryptography (not checked); profile/gcall `random16()` vs identity RNG helpers; `base64` url-safe wrapper `enc/dec` likely re-declared in core (grep `URL_SAFE_NO_PAD` in core).

## 4. Dead code / unused / ignored
- `#[ignore]`: only tests/gcall_capacity_bounds.rs:80 `candidate_sixteen_member_roster_can_be_sent_and_verified` (deliberate red acceptance gate; MAX_PARTICIPANTS=12 while docs/design mention larger).
- Test-only public API: `issue_legacy_grant_for_test` and `forge_block_notice_for_test` are `pub #[doc(hidden)]` in the release lib (lib.rs:624, 1065). Used by core. Better a `test-support` feature.
- `call_hello(_me, ...)` has an unused `_me` parameter (lib.rs:1309, documented as reserved).
- `issue_grant` (lib.rs:618) "kept for the call paths and tests": used in core/src/node.rs and tests; fine.
- `TICKET_PREFIX`/v1 `ContactTicket` text: `ticket_v1` only in tests.rs:122 (issue path is v2 only); v1 parse path is accepted-legacy code.
- `Cargo.toml` has no `#[allow(dead_code)]`; tests/support modules are included with `#[path]` so each includes all helpers (no dead-code warnings only because items are pub-in-private-mod used somewhere; check with a build when allowed).
- Not checked (read-only, no builds): unused pub items across the workspace beyond the greps above.

## 5. Code vs docs/protocol.md mismatches
- protocol.md:482 says block-notice wire is "on the call ALPN (`osvauld/p2p/0`)" but ALPN is `osvauld/p2p/1` (lib.rs:24) and /0 is refused per line 5 of the same doc. Stale.
- protocol.md:18 lists domains `osvauld/p2p/{attest,invite,grant,bind}/v1\0` (+invite/v2) but code also has `devices/v1` and `block/v1` (lib.rs:35-36); the doc only mentions them later (125, 467). Update the summary line.
- protocol.md has no section for group-call wire (`tinline/gcall/1` ALPN, gcall.rs claims, gcall_status.rs, MAX_PARTICIPANTS=12, MAX_CONTROL 4096) other than via docs/design/group-call-spec.md; profile deletion is documented (312). `MAX_LIST_SEQ_AHEAD_MS` documented as PROVISIONAL, matches.
- Rotation: no production rotation code exists in proto; all of rotation_* tests are candidate models ("test-only/..." domains). Docs should not describe permits/succession/live-binding as implemented. The only production rotation-related test is rotation_identity_boundary.rs (attestation has no generation policy).
- Test file header comments reference deleted/renamed files (e.g. "rotation_silent_block.rs @0924939" in rotation_device_list_push.rs:43, "coordinator consolidates later").

## 6. Recommended order
1. (200-250 lines, trivial) shared `identity::testing::from_seed` + tests/support/fixtures.rs + drop hand-rolled base64 + use real BlockNotice in 3 tests.
2. (~1,000-1,300 lines, medium) tests/support/permit_model.rs shared by 7 files.
3. (~390 lines, needs owner call) retire rotation_succession_model + rotation_network if redundant.
4. (~60 lines, low) gcall/gcall_status helper sharing, `peek`, `dec64`, `verify_attestation_for`, grant/notice builders.
5. Doc fixes in section 5; optional lib.rs split.
