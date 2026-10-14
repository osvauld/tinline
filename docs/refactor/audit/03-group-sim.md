# Audit 03: crates/group-sim (read-only)

Scope: `crates/group-sim/` (23,819 lines in src, 278-line README; 12 model files + tests.rs/authority.rs). Test-only crate (`[dev-dependencies]` only, `lib.rs` is `#[cfg(test)] mod authority; mod tests;`). Everything else hangs off `tests.rs` through `#[path = "x.rs"] mod x;` (tests.rs:11-30). `group_call.rs` is nested in `multi_host.rs:36-37`, `scheduler.rs` in `reachability.rs:9-10`. All code is compiled only under `cargo test -p group-sim`. ~200 `#[test]`, 2 `#[ignore]` (both in group_call.rs).

Method: line-level LCS of non-blank, whitespace-trimmed lines (`diff --unchanged-line-format`), plus reading definitions and the "copied from" comments. Commit dates: every file last touched 2026-10-10 except `group_call.rs` (2026-10-11). Nothing in the crate changed after the real group core landed, so it is a frozen design record.

## 1. File map (purpose / status)

| File | Lines | Tests | Purpose | Status |
|---|---|---|---|---|
| lib.rs | 7 | 0 | Test-only crate root. Header comment stale ("Rotation, revocation, edits ... not modeled yet"). | active, stale doc comment |
| authority.rs | 334 | 4 | Signed genesis + delegation chain fixtures (Capability, Grant, Genesis). | legacy; `Genesis`/`Grant` still imported by tests.rs, persistence, policy_candidates, scheduler. Copied (with a device field) into multi_host/flags/removal_rotation as `MGrant` |
| tests.rs | 1123 | 18 | Legacy base: device-signed `Envelope`, `Author`, `Replica` (insert-only Loro), pending queue, causal repair, seeded loss schedules. Also hosts the `#[path]` mod list. | partially superseded (README "Superseded tests (R1b)"); kept as a "carrier" for persistence / policy_candidates / scheduler / reachability / block_in_group |
| host_only.rs | 2291 | 16 | W2-H: host-only group, DID-signed posts, random Loro writer ids, endpoint notes, sealed chat request, writer collision rules. | proposal; ancestor of multi_host. Partly superseded by multi_host and by `core/src/group` |
| multi_host.rs | 2801 | 18 | W2-M: multi-host with host permits, host reconnect, signed removal/block notices. Parent of group_call.rs. | proposal; ancestor of flags |
| flags.rs | 3103 | 16 | W2-F: removal by 3 admin flags (U9), group rotation on host revocation. Includes a `Cfg` space of candidate rules. | proposal; U9/U15 now implemented in `core/src/group/state.rs` (`derive_meta`, `REMOVAL_FLAGS = 3`) |
| removal_rotation.rs | 2602 | 10 | W2-U: announced, delayed host rotation after removal (U11, U12). | proposal; the newest of the 4-file lineage |
| chat_request.rs | 1934 | 17 | W2-Q: 1:1 chat request via host, sealed E2E, block-reply candidates. | proposal; real code in `core/src/group/requests.rs` + `mail.rs` |
| host_relay.rs | 824 | 6 | W2-R: rotation notes relayed by host (rewritten in R1b to host-star). | largely superseded by host_only/multi_host rotation tests (README table) |
| block_in_group.rs | 599 | 8 | W2-G: personal block inside a group, display candidates. | active (blocking policy not yet in core group tests), legacy Envelope carrier |
| policy_candidates.rs | 1775 | 23 | G-B: D01-D05 causal policy candidate oracles (arrival view, signed frontier, removal wins, epoch cut, clock expiry). | mostly superseded: user decided U11 = signed frontier (rotation-open-decisions.md:101). Only the losing candidates remain unique |
| persistence.rs | 1429 | 15 | G-C: durable/volatile replica, crash injection (S01-S07). | active as the only crash-model; real storage in `core/src/group/store.rs` + core tests |
| reachability.rs | 431 | 6 | Group consumers of a mock verified-succession adapter. `HostDirectory::install` is mirrored (read-only copy) in `crates/proto/tests/rotation_history_*.rs` | active/legacy |
| scheduler.rs | 794 | 5 | Five-node star scheduler (U7), seeded sleep/partition/loss. | active |
| group_call.rs | 3477 | 21 | G6 call-host model GC-S01..S17 plus G6-D1/D2. | superseded by `crates/core/src/gcall` (4.8k lines) whose `tests.rs` replays "GC-S cases from group_call.rs against the real engines" |

## 2. Duplicated scaffolding (the big one)

The four "W2" models form a copy-and-evolve lineage, declared in their own comments: `authority.rs@dcdf4a4 -> host_only.rs -> multi_host.rs@0924939 -> flags.rs@82d221f -> removal_rotation.rs`. 17 "Copied from ..." comments (flags.rs:58,94,122,221,327,492,1264,1859,1890; removal_rotation.rs:65,101,129,225,331,495,1119,1475,1539,1555,1847,1873; multi_host.rs:100,128,231,504).

Measured shared non-blank lines (LCS): flags/removal_rotation **1490** (of ~2600/3100 non-blank, i.e. >50 % of both), flags/multi_host 1118, multi_host/removal_rotation 1048, host_only/multi_host 644, host_only/flags 570, chat_request/host_only 288.

### 2a. Byte-identical items repeated in 4 files (host_only, multi_host, flags, removal_rotation)

| Item | Locations | Notes |
|---|---|---|
| `SimRng` (SplitMix64, `next_u64`, `bytes32`, `writer`) | host_only.rs:76-105, multi_host.rs:102-129, flags.rs:96-123, removal_rotation.rs:103-130 | diff shows only a comment differs (verified). 4x ~28 lines |
| helpers `hex32`, `has`, `ident`, `ep`, `signed`, `valid`, `digest`, `dir_key`, `pair` | host_only.rs:47-74; multi_host.rs:66-100; flags.rs:60-94; removal_rotation.rs:67-101 | identical bodies; only `POST_DOMAIN`... constants differ (domain-separation strings differing by name only) |
| Constants `OLGA,A1,A2,BOB,CAROL,H1..H3`, `MAX_CHAIN`, `MAX_PENDING(_POSTS)`, `FLAG_THRESHOLD`, `DOC` | multi_host.rs:44-62, flags.rs:37-56, removal_rotation.rs:41-63, host_only.rs:34-43 | actor ids differ by model (members differ, e.g. `HP` in multi_host, `DAVE/ERIN` in flags) |
| `Cap`, `MGrantBody`, `MGrant::{issue, digest, allows}` | multi_host.rs:131-232; flags.rs:125-223; removal_rotation.rs:132-227 | `allows` chain walk is the same code; copy of authority.rs grants |
| `PostBody`, `Post{sign,verify,wire,id,key,value}`, `Writer{new,observe,post}` | host_only.rs:108-200; multi_host.rs:234-315; flags.rs:224-308; removal_rotation.rs:228-312 | flags/rr identical except `ops_seen` |
| `Why`, `Verdict`, `Board{new,precheck,receive,park,check,apply,drain,snapshot,texts}` | multi_host.rs:317-500; flags.rs:309-494; removal_rotation.rs:313-497; host_only.rs:203-408 | Board is the Loro insert-only pending-queue replica. flags vs rr differ in 2 variants + 2 comments (verified diff) |
| `EpNote`, `NoteClaims`, `HostRecord` | host_only.rs:411-470; multi_host.rs:507-560; flags.rs:495-543; removal_rotation.rs:498-546 | |
| `Op`, `OpBody`, `SignedOp`, `Refuse`, `Acc` | multi_host.rs:562-636; flags.rs:544-648; removal_rotation.rs:547-700 | |
| `Device`, `RouteErr`, `Path`, `Frame`, `World` struct + ~30 identical-named methods | flags.rs:1266-1330, removal_rotation.rs:1129-1200, multi_host.rs:700-775 | diff of the struct block: 15 changed lines |
| `impl World` methods: `new, did, gep, hk, permit, post, send, xfer, dial, connect, connect_from_host, cut, heal, exchange, round, snap, assert_converged, accept_op, accept_host_record, accept_post, host_accept_note, host_sync, rotate_host, rotate_member, removal_notice, received, admits` | multi_host.rs:773-1706, flags.rs:1313-1953, removal_rotation.rs:1178-1966 | the 3 `World` impls are ~640-930 lines each, about 60 % common (flags vs rr commonly 28 method names) |
| `Replica`/`Ctx` (op closure, frontier derivation, flags) | flags.rs:735-1264 and removal_rotation.rs:740-1123 (`closure` flags.rs:754 / rr.rs:756) | |
| Test helpers `world()`, `with(|cfg|...)`, `set()`, `ids()` | flags.rs:1953-2040, multi_host.rs:1706-1722, rr.rs:1966-1999 | |

Dead copy risk: a Board bug fix (e.g. the `u64::MAX` writer redraw, which exists only in host_only.rs:92 and multi_host/flags/rr do carry it per diff) must be applied 4 times. The comments pin SHAs, confirming nobody expected them to be kept in sync.

### 2b. Duplicated crypto/sealing across chat models

host_only.rs:483-640 and chat_request.rs:57-253 each define: `did_sign`/`did_verify`/signing-bytes, `ReqKeyClaim`/`SignedReqKey` vs `RequestKeyClaim`/`SignedRequestKey`, `Header`, `Sealed`/`Wire`, `Inner`/`SignedInner{new,verify,digest,padded}`, `unpad`, `aead_key`, `aad`, `seal`, `open` (X25519 + AES-256-GCM, zero nonce). `seal`/`open`/`unpad`/`aead_key` are the same code with different domain constants and type names (host_only.rs:586-636 vs chat_request.rs:195-253). ~170 lines duplicated; chat_request.rs is the more general version (host_only's "chat request via host" is its own subset: test `chat_request_via_host_with_did_signed_content_then_direct`, host_only.rs).

### 2c. Other scaffolding

- Inline seeded LCG (`rng.wrapping_mul(6364136223846793005).wrapping_add(1)`): tests.rs:636, 665; scheduler.rs:400-409, 596+; plus 4 SplitMix copies. Two PRNG styles and 6 copies.
- `permutations(n)` + `explore(...)`: flags.rs:1984, 2009; policy_candidates.rs:886, 905. Same helper, differently typed. tests.rs:498 hand-rolls permutations for its test.
- `snapshot()/snap()`, `texts()`, `assert_converged/assert_same`, `assert_rejected_unchanged`: 8 `snapshot`, 6 `snap`, 5 `assert_converged`; each model defines its own byte snapshot to prove "refusal leaves state unchanged".
- Same-name type families repeated across files: `World` x8, `Frame` x5, `NoteClaims` x5, `Verdict` x5, `RouteErr` x5, `Device` x4, `Cfg` x4, `Board` x4, `Writer` x4, `Post` x4.
- Domain strings of the form `tinline/group-sim/<model>-<kind>/not-production/v1\0` (host_only.rs:30-33, multi_host.rs:39-43, flags.rs:33-36, removal_rotation.rs:35-40, group_call.rs:91-92, authority.rs:6-7, policy_candidates.rs:30): one per model per kind, same format.

### 2d. Proposed shared module(s)

Create `crates/group-sim/src/common/` (or a `sim_kit.rs` pulled in once from tests.rs):
1. `common::rng::SimRng` (SplitMix64 + `lcg(seed)` helper if the legacy LCG sequences must stay stable for seeds 1..20 tests: keep one `legacy_lcg`).
2. `common::keys` (`ident`, `ep`, `hex32`, `has`, `dir_key`, `pair`, `signed(domain,&T)`, `valid`, `digest`): take a `Domain` struct or generics so models pass their own constants.
3. `common::grant::{Cap, MGrant}` (one copy; make authority.rs `Grant` the base or retire it behind the host-permit variant).
4. `common::board::{Post, PostBody, Writer, Board, Why, Verdict}`: parametrize on a small `trait PostPolicy { const DOMAIN; fn extra_check(&self, ..) }` or a `BoardCfg { roster: Option<BTreeSet>, upload_cap }` (differences are one optional roster check, `ops_seen` field, and 1-2 `Why` variants). Keep one `Why` enum with all variants.
5. `common::seal::{Header, Sealed, SignedInner, seal, open, aead_key, aad, unpad}` for chat models (const generic over a `Context` struct holding KDF/AAD strings and PAD).
6. `common::net::{Device, Path, RouteErr, Frame, WorldBase}`: a `WorldBase` struct with `connect/dial/cut/heal/exchange/round/send/xfer/snap/assert_converged/rotate_host/rotate_member/host_sync/accept_*`, with models adding policy via a `trait Policy` (`Cfg` today). flags.rs and removal_rotation.rs could then be thin extensions (`Replica`+`Ctx`+their own `Op`/`Cfg`).
7. `common::explore::{permutations, explore}` generic over an event type.

Estimated savings: SimRng + helpers + grants + Post/Writer/Board + notes + ops + Device/World core across 4 files: about 1,800-2,300 lines (flags/rr overlap alone 1,490 lines; multi_host adds ~600 more unique to be merged into the shared core; host_only ~400). Chat sealing: ~170. RNG/explore/snapshot helpers: ~120. **Total about 2,100-2,600 lines (9-11 % of crate)**. Risk: medium. Not hard mechanically (identical text), but the models are decision records pinned to commits and docs cite line numbers (`docs/group-host-only-model.md:7-8`, `docs/group-persistence-contract.md:33-35`, `docs/design/rotation-author-group-adapter.md:38-50` cite `tests.rs:75..213`; `docs/group-policy-options.md:9-14`), so doc references would break. Do it with `cargo test -p group-sim` count parity (test list before/after) as the acceptance gate.

## 3. Overlapping tests

Within the 4-model lineage the same scenarios recur with each copy: genuine differences are the extra parameters (permit chains, flags, delay), but the baseline checks repeat.

| Scenario | Where | Verdict |
|---|---|---|
| Forged/tampered/wrong-group/replayed inputs leave state unchanged | host_only.rs:1953 `forged_tampered_wrong_group_and_replayed_posts_leave_state_unchanged`; flags.rs:2210 `forged_non_admin_host_only_...`; removal_rotation.rs:2364; host_relay.rs:563; tests.rs:547 (legacy) and tests.rs:1005 (`every_rejection_class_leaves_populated_signed_replica_byte_identical`); policy_candidates.rs:1738; block_in_group.rs:334 | Each exercises a different object (post / flag op / announcement / note / envelope / meta op), so not redundant, but the "snapshot, send bad input, assert equal snapshot" skeleton should be one table-driven helper: `fn assert_refused_unchanged(world, inputs: &[(label, bytes, expected_verdict)])`. Saves ~250 lines |
| Writer-id collision | host_only.rs:2025 `loro_same_peer_id_from_two_writers_silently_diverges...`, :2055 `cross_did_writer_collision_first_claim_at_host_wins...`, :2101 `same_did_writer_collision_...`; multi_host.rs:2391 `writer_collision_first_claim_at_each_host_diverges_across_hosts`, :2419 `writer_collision_lowest_claim_after_all_hosts_saw_converges` | The multi-host pair is genuinely different (cross-host); host_only trio is the single-host rule set. Merge candidates: host_only 2055 + 2101 into a table (rule x scenario); keep the others. Also notice this class is now settled; core `crates/core/tests/writer_id.rs` exists (CROSS-FOLDER) |
| Host / member rotation and recovery | host_only.rs:1650, 1733, 1786; multi_host.rs:2192, 2243, 2303, 2331; host_relay.rs (rewritten star) | host_only vs multi_host: same scenario with 1 host vs N hosts; README maps removed host_relay tests to these. Table-driven over `host_count in [1,2,3]` and `retention in [none, until_ack]` could replace the 3+4 tests: ~300 lines |
| Two-host partition then heal | tests.rs:588, multi_host.rs:2149, flags.rs `flags_commute_across_delivery_orders_and_two_partitioned_hosts` (flags.rs:2143), group_call.rs | legacy tests.rs:588 and multi_host.rs:2149 are the same invariant (no rollback); the tests.rs version uses the superseded carrier. Candidate for removal after confirming multi_host.rs:2149 covers the same assertions |
| Personal block leaves state unchanged | block_in_group.rs:334, host_relay.rs:643, group_call.rs:2968 (gc_s10), multi_host (notice test) | Different layers (grants / notes / calls); keep, but block_in_group.rs 8 tests use legacy Envelope carrier and could share a `World` from the new common kit |
| Removal behaviour | flags.rs:2051/2331/2390/2454/2535, removal_rotation.rs 10 tests, policy_candidates.rs D01-D05 (23 tests) | D01/D02/D05 in policy_candidates and the flags.rs `PostRule::{SignedFrontier,RemovalWins}` tests (flags.rs:2692, 2742) and rr.rs:2056/2183 encode the same signed-frontier vs removal-wins candidates over two different models |
| Permutation/delivery-order convergence | tests.rs:498, 827; flags.rs 2143; policy_candidates `d01_*` | same pattern, three hand-rolled permutation drivers (tests.rs:498, flags.rs:1984, policy_candidates.rs:886) |
| `Cfg`-variant tests ("X vs Y", e.g. `flag_expiry_never_vs_signed_window_vs_receiver_clock`, `count_cap_on_pre_removal_uploads_vs_rotation_only`) | flags.rs, removal_rotation.rs, multi_host.rs, chat_request.rs `candidate_*` | Already pseudo table-driven inside each test; keep. They document undecided options, so merging them reduces readability |

Estimated saving from test table-driven merges: ~600-900 lines; risk low if done after the shared kit exists.

## 4. Superseded / archive candidates (evidence)

1. **group_call.rs (3,477 lines)**: `crates/core/src/gcall/mod.rs:15` says "Spec gaps G1-G10 are settled as modelled in `crates/group-sim/src/group_call.rs`", and `crates/core/src/gcall/tests.rs:1` runs the GC-S cases against the real engines; `docs/design/group-call-spec.md:430,524,579` references it as the executable spec. It still carries unique value: the ASSUMPTIONS list and two `#[ignore]` tests (GC-S07 needs a real Opus encoder). Recommend: keep until the core tests cover every GC-S id (cross-check list), then move to `docs/archive` or mark `#[cfg(feature = "model")]`. Do not delete: core doc comments cite it.
2. **policy_candidates.rs (1,775 lines, 23 tests)**: gates D01-D05 are mostly decided (U11 option A, U9/U15 in `core/src/group/state.rs`); `docs/group-policy-options.md` is still "Status: proposal - not approved" (stale). The losing candidates are only informative. Archive candidate, medium confidence; first update `docs/group-policy-options.md`.
3. **tests.rs legacy Envelope + attestation path**: README admits "superseded by U4 revised + U16" (device-key attestation); kept only as carrier for persistence/policy/scheduler/reachability/block_in_group. Moving those to the DID-signed `common::board` would let tests.rs shrink (1,123 lines) and authority.rs (334) go. Evidence of need: docs cite tests.rs lines (see 2d).
4. **host_only.rs**: ancestor of multi_host (2,291 lines); the multi-host decision (U7) makes single-host scenarios a special case. README table shows host_relay.rs tests were already deleted because host_only/multi_host cover them; host_only is next. Keep the unique parts (sealed chat -> duplicate of chat_request.rs, writer collision single-host rules) and fold into multi_host with `HostCap` count 1.
5. **host_relay.rs (824, 6 tests)**: rewritten to star in R1b; mostly covered by host_only/multi_host per README. Candidate to merge `mnemonic_clone_with_higher_seq...` (host_relay.rs:804) and `same_seq_fork...` into host_only/multi_host.
6. **flags.rs vs removal_rotation.rs**: rr is the strict evolution of flags for removal; flags still owns the U9 flag rules and host-revocation rotation. Both active; consolidate rr onto flags' core.
7. **chat_request.rs**: active reference for block-reply candidates; real feature is `core/src/group/requests.rs`. Keep.

Not recommended for removal: persistence.rs (only crash model), scheduler.rs/reachability.rs (cited by proto tests: `crates/proto/tests/rotation_history_adversarial.rs:1778`, `rotation_history_paging.rs:1140`), block_in_group.rs (blocking display candidates still open).

## 5. CROSS-FOLDER duplication with crates/core

- **CROSS-FOLDER (core/gcall)**: `group_call.rs` constants and logic re-implemented in `crates/core/src/gcall/`: `phone_policy`, `Class`, `Cond`, `PhoneAction`, `Thermal` (group_call.rs:230-325 vs core `gcall/policy.rs:5,10,76`), `HEARTBEAT_MS`, `MAX_UP_PPS`, `DOM_SHARE_PCT` (group_call.rs:55,58,77 vs core `gcall/mod.rs:45,50`, `select.rs:17`), `rank`/`tier`/`tie_hash` (group_call.rs:354-405 vs `rank.rs:87,125,154`), forwarder `Fwd`/`select` (group_call.rs:406-600 vs `select.rs:32 Forwarder`), STATUS/CALL domains. Core is the source of truth now; group-sim is a second implementation that can drift.
- **CROSS-FOLDER (core/group)**: flags rule (3 distinct-or-same admin flags, owner protected), `FLAG_THRESHOLD = 3` in flags.rs:41 and rr.rs:44 vs `REMOVAL_FLAGS = 3` in `core/src/group/state.rs:191`; `ROTATION_DELAY = 2 days` (rr.rs:47, group_call.rs:83) vs core's own value; host permit + host accept (multi_host.rs `MGrant`) vs `HostInfo` and `HostPermit/HostAccept` records in `core/src/group/state.rs:57` and `record.rs`; call-host permit split also modelled in both. `MAX_PENDING = 16` / `MAX_CHAIN = 8` constants have no counterpart in core's shard/day docs (core uses per-day docs; group-sim README states it is "a standalone model with its own record types", `docs/group-shards.md:160`).
- **CROSS-FOLDER (cryptography/core)**: chat sealing here (X25519 + AES-GCM + blake3 KDF, zero nonce) is a sim-only construction; real code is `crates/cryptography/src/ecies.rs` and `core/src/group/mail.rs:100,112` (`seal`, `open_for`). Not a reuse opportunity (sim construction is labelled "SIMULATION construction") but the models test an invariant (host sees only routing metadata) that the real `mail.rs` tests should also assert.
- `proto/tests/rotation_history_adversarial.rs:1778` and `rotation_history_paging.rs:1140` carry "read-only copies" of `reachability.rs` `HostDirectory::install` (CROSS-FOLDER duplication, intentional but drift-prone).
- Core already has acceptance tests for many of the same scenarios (`crates/core/tests/group_core.rs`, `group_g2.rs`, `group_g3.rs`, `group_shards.rs`, `group_call.rs`, `gcall.rs`, `readd.rs`, `writer_id.rs`, `blocking_acceptance.rs`). A coverage map from group-sim test names to those would give the evidence needed before archiving anything.

## 6. Ranked recommendations (payoff / risk)

| # | Action | Lines saved | Risk |
|---|---|---|---|
| 1 | Extract `common` kit: SimRng, helpers, MGrant, Post/Writer/Board, EpNote/HostRecord, Op/SignedOp, Device/World base (collapse flags/rr/multi_host copies) | 1,800-2,300 | Medium: doc line refs; need test-count parity |
| 2 | One `seal/open` + Header/SignedInner module shared by host_only and chat_request | ~170 | Low |
| 3 | Table-driven "refused input leaves state unchanged" helper and shared `snapshot` trait | 250-400 | Low |
| 4 | Single `permutations/explore` and one PRNG (keep legacy LCG for seeds 1..20 tests only) | ~120 | Low (seed sequences must not change for tests.rs:621, scheduler.rs:355) |
| 5 | Table-drive rotation/recovery (host_only vs multi_host, host_count x retention) and writer collision | 300-500 | Medium |
| 6 | Port group_call.rs to a "model" archive once core gcall tests cover all GC-S ids; ASSUMPTIONS to the spec doc | up to 3,477 removed from default test build | Medium; core docs cite it |
| 7 | Mark policy_candidates.rs / host_relay.rs / tests.rs legacy path as archived after updating `docs/group-policy-options.md` and README "Superseded" table | up to ~3,700 | Medium-high: needs user/doc decision, since gates D01-D05 are officially "not approved" in docs |
| 8 | Fix stale lib.rs header comment; update README "Not covered" (flags, rotation, durable storage are now modelled) | 0 | Trivial |

Doc follow-ups: README.md (278 lines) says "Edits ... Group revocation ... Durable storage (S01-S07)" are not covered, which is out of date (flags.rs, removal_rotation.rs, persistence.rs exist). `docs/group-sim-coverage-audit.md` audits only 24 tests (pre-W2); it predates 175+ tests.

## 7. Limits

Read-only; nothing compiled or run. Line-share figures are LCS on trimmed non-blank lines (includes braces and comments), so they overstate semantic overlap by perhaps 15-20 %. Test counts come from `#[test]` grep. I did not verify that every GC-S case is ported into core gcall tests.
