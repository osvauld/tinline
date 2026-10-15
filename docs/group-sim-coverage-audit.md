# group-sim coverage audit (packet G-A)

Status: proposal — not approved

Audit date 2026-10-10, branch `rot/ga` at `1b6e632` (clean). Read-only audit of
`design-models:crates/group-sim/src/{lib,tests,authority,reachability,scheduler}.rs` against the claims in
`design-models:crates/group-sim/README.md`, `docs/STATUS-group-chat.md` "Executable coverage" and the case IDs in
`docs/group-chat-tests.md`. No source or test was changed.

Notation: `T:` = `design-models:crates/group-sim/src/tests.rs`, `A:` = `authority.rs`, `R:` = `reachability.rs`,
`S:` = `scheduler.rs`. A test "asserts" a claim only if removing the behavior would make an
assertion fail. Where a check is listed as not exercised, that conclusion comes from reading the
code. **No mutation runs were made.** The predicted surviving mutations in §5 are hypotheses for the
coordinator to run, not results.

## 1. Test run (verified)

```
python3 scripts/buildlock.py --who group-GA -- env CARGO_TARGET_DIR=/home/abe/p2p_chat/.claude/worktree/endpoint-rotation-blocking/target cargo test -p group-sim (design-models branch) 2>&1 | tee /tmp/claude-1000/group-GA.log
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 22.60s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s   (doc-tests)
```

This matches the baseline of 24 tests: 13 in `tests.rs`, 3 in `authority.rs`, 5 in `reachability.rs` and 3 in `scheduler.rs`.

## 2. Cross-cutting findings (verified in source)

1. **No KIND labels.** None of the 24 tests has `// KIND:` comments (rules-every-packet item 6),
   so nothing marks which tests are invariants and which are limitations. For example,
   `R:238 offline_mutual_retirement_does_not_gain_magical_discovery` is a limitation test.
2. **Rejections are almost always checked against an empty replica.** These tests check emptiness
   (`accepted/pending/fanout.is_empty()`):
   - `T:432` and `T:452`;
   - `T:601`, after every input;
   - `T:668`.
   Only two tests reject against a non-empty baseline:
   - `T:670-672` (`declared_dependency_cannot_substitute_for_actual_loro_causality`) checks
     counts only (`accepted.len()==1`, `fanout.len()==1`).
   - `R:160-168` (`every_group_proof_predecessor_is_checked…`) checks history length, that no
     secret was leaked, and the current endpoint.

   No test compares `Replica.doc` deep value or VV, the accepted IDs, or pending contents
   before and after a rejection on a populated replica. The "byte-for-byte unchanged" standard is
   not met anywhere.
3. **Several rejection cases fail for more than one reason**, so a single check is never isolated:
   - `T:425-426` (`wrong_did`) changes `body.author` without re-signing. Rejection happens at
     authorization first (`T:201`: X is not on the allowlist), and also at attestation-DID
     binding (`T:213`) and at the signature (`T:219`). The attestation↔author/device binding
     at `T:213` is never the sole cause of any rejection in the suite.
   - `T:433-434` (cross-group) changes both `group` and `doc` (`T:199-200`). That other replica's
     state is not checked after the rejection.
   - `A:246-262`, nondelegable case 1 `(Read,Post), delegable=false`, also lacks `Delegate`. So
     `!p.body.delegable` (`A:134`) is never tested alone: no parent has `Delegate` together with
     `delegable=false`. Case 3 isolates the missing-`Delegate` check; the `delegable` flag itself
     is untested.
4. **Missing negative controls / non-vacuity guards:**
   - `T:470 seeded_reordered_loss_and_retry_schedules_converge` ignores every `receive` result
     (`T:482`, `T:489`) and ends with `oracle.assert_same(&r)` (`T:493`). `assert_same` (`T:356`)
     compares doc, VV, accepted keys and empty pending. If every envelope became `Invalid`, both
     replicas would be empty and the test would still pass. There is no length/fanout check and
     no omitted-dependency control (both required by `group-chat-tests.md:51-53`).
   - `T:381 all_small_delivery_permutations…` is guarded only by `fanout.len()==3` (`T:403`). The
     oracle's results (`T:387`) and the duplicate deliveries (`T:400`) are not asserted, and
     `Outcome::Duplicate` is never asserted anywhere in the crate (grep: only `T:228` returns it).
5. **Complete-set reconciliation guarantees convergence by construction.** `Replica::sync_from`
   (`T:351-355`) feeds every accepted envelope, and `Fleet::healthy_reconcile` (`S:251-260`)
   sends every missing envelope over all healthy links. For the seeded tests, convergence is
   mainly evidence that the receive logic does not depend on the order of the earlier
   partial/lossy deliveries. It is not evidence that a gossip/anti-entropy protocol works. The
   README admits this (`README:58-59`).
6. **The pending queue drops a promoted envelope that turns out invalid.** `T:252` ignores
   `self.apply(next)`'s result. An envelope that sits in pending and then fails `apply` is
   silently discarded, with no outcome surfaced. No test covers this path: the invalid envelope
   in `T:667` fails immediately and is never queued.
7. **The bound checks at `T:202-205` (deps ≤256, text ≤20 000, update ≤1 MB, `sequence != 0`)
   and the peer-ID binding `b.peer != peer(device, doc)` (`T:206`) have no test.**
8. **No test models multiple devices per DID.** `Author::new` (`T:69-90`) uses one key per seed,
   with the same seed for the DID. A/A2 cases (P05, P17) cannot be expressed in the current fixtures.
9. **Stale doc text:** `docs/group-chat.md:311-312` says "No conceptual case is a passing
   executable test". `docs/STATUS-group-chat.md:5-7` still points at the stale `group-chat`
   worktree and says the work is "uncommitted", but it is committed on this branch (`a3b8829`).
   Coordinator-owned; not edited here.

## 3. README / STATUS claims → tests

Verdicts:
- **Asserted** means the claim is asserted with a negative control where one is relevant.
- **Partial** means only some of the claim is asserted, or the test is confounded.
- **Structural** means the claim is true by construction, so no assertion could fail.
- **Overclaim** means the wording exceeds what the tests show.

| Claim (source) | Test(s) | Verdict and reason |
|---|---|---|
| Original-author relaying / offline delivery (`README:13`, `STATUS:59`) | `T:368 original_author_survives_host_relay_and_offline_delivery` | **Asserted** for relaying: Accepted (`T:374`), author and signature preserved (`T:377-378`). "Offline" is structural: authors are never replicas in `tests.rs`, so the author is never online after posting. |
| All six permutations of small traces (`README:13`, `STATUS:57-58`) | `T:381`, `T:605 cross_author_causal_chain_converges_in_every_delivery_order` | **Asserted**. `T:605` asserts Accepted on the oracle (`T:616`) and fanout 3. `T:381` is guarded only by fanout (finding 4). |
| Missing-dependency negative controls (`README:14`) | `T:407`, `T:633-638`, `T:659` | **Asserted**: Pending, then Accepted, with empty accepted/fanout in between. Covers previous-chain and cross-author cases. |
| Tampering (`README:14`, `STATUS:56`) | `T:419` (`tampered`) | **Asserted** for one field (`text`), rejected by signature alone, on an empty replica only. |
| Wrong group / outsider / identity (`README:14`) | `T:419` | **Partial**. Outsider is asserted through the allowlist. "Identity" (`wrong_did`) is confounded (finding 3); the attestation↔device binding (`T:213`) is never isolated, and no tampered attestation is tested. Wrong group is confounded with doc, and the post-rejection state is not checked. |
| Extra Loro mutation injection (`README:15`, `STATUS:56`) | `T:437 malicious_author_cannot_hide_extra_loro_mutations…` | **Asserted** (Invalid, no accepted/fanout state). The rejection comes from the semantic comparison (`T:342`), not from the peer-range check (`T:277`), which stays unexercised because the extra op is from the author's own peer. |
| Two-host partition recovery (`README:15`, `STATUS:59`) | `T:455 two_hosts_partition…`, `T:641 independent_posts_and_cross_author_join…` | **Asserted**: same state, 2 or 3 accepted. The "without rolling back" stale step in `T:465-467` is **vacuous**: the stale replica is empty, so `sync_from` does nothing. No lower VV, refusal or replace request is modeled (cf. N07). |
| 20 seeded loss/reordering/retry schedules (`README:15`, `STATUS:58`) | `T:470` | **Weak / potentially vacuous** (finding 4): no non-vacuity guard and no negative control; convergence is forced by full `sync_from`. With 10 envelopes and `MAX_PENDING=16` the limit is never hit. |
| Bounded pending queues (`README:16`, `STATUS:59`) | `T:497 pending_dependencies_are_bounded` | **Partial**: `Limit` is returned and `pending.len()==16`. The test does not assert that the overflow envelope is absent from pending, and it never releases the withheld dependency to show drain behavior or what happens to the refused envelope. |
| Signed genesis and hash (`README:16-18`, `STATUS:53`) | `A:179 genesis_hash_signature_version_and_creator_are_checked`, `T:547` (`T:556-558`) | **Asserted**: nonce, creator and version (re-signed) changes are rejected, with a positive control. Through `Replica::from_genesis` only a signature flip is tested. |
| Holder binding (`README:19`, `STATUS:54`) | `A:227`, `T:572` (owner's grant presented by C) | **Asserted**, both with positive controls. |
| Scope (`README:19`) | `A:229` (other genesis), `T:585-594` (permit for group "other") | **Asserted**. |
| Capability narrowing (`README:19`) | `A:246-262` case 2, `T:575-584` (read-only permit) | **Asserted** for child ⊄ parent and for a leaf without Post. |
| Delegation authority (`README:19`) | `A:236-237` (forged root), `A:246-262` | **Partial**. Root issuer ≠ creator is asserted. Not tested: child issuer ≠ parent holder (`A:133`, i.e. a third party signing under someone else's grant), the `delegable=false` flag alone (finding 3), and an unparsable holder DID (`A:125`). |
| Chain-depth cap (`README:19`, `STATUS:54-55`) | `A:263-293` | **Asserted** at the boundary: 8 links accepted, 9 refused. |
| "Validate actual Loro prerequisites against declared closure … omitting a required dependency fails even when the receiver already has it" (`README:23-26`) | `T:659` (`T:669-670`) | **Asserted** on a populated replica (counts only). Transitive closure is exercised only through `previous` chains. No test has a cross-author dependency that is reachable only transitively. |
| Scratch import plus semantic comparison (`README:26-27`) | `T:437` | **Asserted** (see injection row). |
| Original envelopes survive forwarding (`README:27`, `STATUS:59`) | `T:378`, `T:555`, `R:211`, `S:357-362`, `S:418` | **Asserted** through signature equality. "No synthesized diff" is structural: the simulator has no other export path. |
| Reachability: historical vs active at retirement (`README:31`, `STATUS:60`) | `R:124 historical_expired_bindings_are_retained_but_never_used_as_fallback_routes` | **Partial / structural**. `active()` (`R:73-75`) never reads `history`, so "no fallback" cannot fail. There is no positive control that routing through `new` works before `t=20` in this test, and the old binding is already retired (`retire_at=10`) at the probe time. |
| Reject private/wrong-audience record anywhere in the path (`README:31-32`, `STATUS:60-61`) | `R:147 every_group_proof_predecessor_is_checked…` | **Asserted**, with unchanged state after each rejection (`R:161-168`). The final `wrong_group` rejection (`R:172`) has no unchanged-state check. Only position 0 is tested for a wrong-group audience, and a mid-path owner mismatch is not tested. |
| Nomination isolated from siblings / other owners (`README:32-33`, `STATUS:61`) | `R:216 sibling_or_other_owner_path…` | **Partial**: only single-element paths, on an empty directory. `install` (`R:49-72`) checks only `first.device_id`. A path `[10, 99]` (nominated device, then a sibling) would be accepted, because the mock contract delegates ordering and continuity to rotation. `MockRotation` ignores the `Nomination` argument (`R:39`), and no test uses an adapter that returns `None`. |
| Route/retry after host device-ID replacement (`README:33`) | `R:175 replaceable_device_ids_route_messages_without_new_group_delegation` | **Asserted** for routing: the queued frame is lost (`R:196`), the stale route is refused (`R:197`), and connect plus delivery succeed after the update (`R:204-208`). "Without new group delegation" is **structural**: `Replica` authorization never consults `HostDirectory` or device IDs, and the author never rotates. |
| Failed discovery after offline retirement (`README:33-34`, `STATUS:65-66`) | `R:238`, `S:380 mutual_retirement_drops_queued_message…` | **`R:238` is weak**: connect fails because `known-old` has retired (`now=11 > 10`), and it would fail at any time because `known-old` is not in `live`. Nothing ties the result to `unknown-new`, and only one side retires. `connect` returns `bool`, so "unreachable, not proof failure" (contract table, `group-rotation-integration-contract.md:63`) cannot be distinguished. **`S:380` is stronger**: one stale packet, the control push fails, the local write is retained, and nothing is accepted remotely. |
| Five-node, ten seeded 100-event schedules with sleep, partitions, duplicate/drop/reorder, **independent host ID replacement**, controls, retries (`README:40-45`, `STATUS:62-64`) | `S:264 five_nodes_seeded_sleep_partition_rotation_loss_and_retry_converge` | **Overclaim on rotation stress.** Time in the random phase is `now = 4 + step/10 ≤ 13` (`S:306`). The initial bindings retire at 30 (`S:7`, `S:67`), and `rotate` never shortens a predecessor's `retire_at` (`S:218-223`). **No binding ever retires during the seeded schedules**, so host rotation never causes a routing failure there. Failed routes and stale packets come only from sleep and partition. Convergence, fanout and signatures are asserted (`S:347-363`). |
| "Mandatory faults and coverage counters ensure each seed exercises intended failures" (`README:43-44`) | `S:286-303`, `S:364-375` | **Partial**. The `duplicates`/`drops` counters are incremented by the test itself for its manual queue edits (`S:290`, `S:292`), so they cannot fail. `queue_limits`, `delivered` and `sends` are never asserted; whether `QUEUE_LIMIT` (64) is ever reached is unknown (not instrumented here). `stale_packets > 0` and `failed_routes > 0` are real checks that depend on the seed. |
| Bounded queue in the scheduler (`README:42`, `STATUS:63`) | `S:137-140`, `S:337` | **Partial / likely unexercised**: the bound holds by construction, and the refusal path is not shown to occur. |
| Address updates over still-known routes before retries (`README:45-46`, `STATUS:65`) | `S:401 replacement_host_can_push_update…` | **Asserted**, with a negative step (`S:412`) and a positive step (`S:413-417`). In `S:264` the ordering exists, but the overlap is trivially satisfied (see the "overclaim" row). |
| Replica contents, proofs and fanout converge; a retired queued destination loses a frame, not the local write (`README:46-47`) | `S:347-363`, `S:390-397` | **Asserted**. |
| Address payloads: mock verifier, strict extensions only (`S:196-203`) | — | A shorter (rollback) path is silently ignored but still counted as a control (`S:204`). No test asserts a rollback outcome. |

## 4. Conceptual case IDs (`docs/group-chat-tests.md`) → coverage

The verdicts are:
- **exec**: an executable test asserts the oracle in-memory.
- **partial**: some aspect is asserted, or the case is confounded or structural.
- **none**: no test covers the case.

No case reaches persistence, real iroh, or UI level.

### Invariants I1–I10

| ID | Status | Evidence / gap |
|---|---|---|
| I1 | partial | `T:419`, `T:561`; device-binding check `T:213` not isolated |
| I2 | partial | `T:378`, `S:357-362` (signature preserved); no host-signature substitution test |
| I3 | partial | emptiness checks on empty replicas only (finding 2); persistence/indexes not modeled |
| I4 | exec | `T:407`, `T:633-638` |
| I5 | exec | `T:381`, `T:605`, `S:264` (with the caveats in findings 4–5) |
| I6 | partial | `T:465-467` is vacuous; no host VV/refusal model |
| I7, I8 | none | no acks modeled |
| I9 | none | no pull/subscribe/blob |
| I10 | partial | `T:497` pending bound; `S:137` queue bound likely unexercised; size bounds `T:202-205` untested |

### P01–P18 (pure)

| ID | Status | Evidence / gap |
|---|---|---|
| P01 | partial | genesis verify `A:183`, `T:549`; no invitation |
| P02 | exec | `A:184-194`; via replica only a signature flip `T:556-558` |
| P03 | none | no invitations |
| P04 | partial | valid attestations are used everywhere; wrong DID is confounded; no tampered attestation and no wrong-key attestation |
| P05 | none | no A2; peer binding `T:206` and peer range `T:277` never the cause of a rejection |
| P06 | partial | `T:433-434` (confounded), `T:585-594` (permit for another group, re-signed) |
| P07 | partial | delegation acceptance `A:226`; no meta/admin mutation exists |
| P08 | partial | widening and depth exec (`A:246-293`); nondelegable flag not isolated; parent-signature flip `A:233-235`; "no accepted changes" not applicable (pure function) |
| P09 | partial | forward-unchanged exec `T:368`; replacing the author proof with H1's signature is not tested |
| P10 | none | unknown fields/versions/containers are not tested; envelopes are never deserialized in the simulator |
| P11 | none | overwriting another author's key is guarded by `T:271` and the semantic comparison, but untested |
| P12 | none | edits/deletes not modeled |
| P13 | partial | one field (`text`) `T:424`; no attachments |
| P14 | exec | `T:407`, `T:633-638` (message dependencies only; no meta) |
| P15 | partial | `T:659` (omitted prerequisite), `T:497` (bounded); no forged-ID or impossible-frontier test |
| P16 | partial | duplicates in `T:381` are guarded by fanout only; `Duplicate` is never asserted; no events/receipts |
| P17 | none | no multi-device authors |
| P18 | exec | `T:381` (A,A,B), `T:605`, `T:641` |

### D01–D06 (policy gates) and S01–S07 (persistence)

| ID | Status | Evidence / gap |
|---|---|---|
| D01–D05 | none | no revocation, expiry, removal or epochs; G-B owns candidates |
| D06 | partial (limitation) | mock path only (`R:175`); no fork, revocation or capability preservation; rotation-owned |
| S01–S06 | none | no durable/volatile split (`README:56`); G-C |
| S07 | partial (structural) | only original envelopes are ever sent (`S:160-172`); no export after overlap is asserted |

### N01–N14 (direct sync and hosted delivery)

| ID | Status | Evidence / gap |
|---|---|---|
| N01 | partial | `S:264` partitions; `T:455` |
| N02 | partial | `T:368` (structural offline) |
| N03 | partial | `T:641`, `T:455`; no meta |
| N04 | none | no ack |
| N05 | partial (structural) | replicas are symmetric; there are no host-required paths |
| N06 | partial | `S:380`, `R:238` (weak) |
| N07 | partial (vacuous) | `T:465-467` |
| N08 | none | |
| N09 | partial | drop/dup/reorder in `S:264`; no session restart, no notifications |
| N10 | partial | bounds exist; no slow subscriber; queue limit unexercised |
| N11 | none | |
| N12 | none | |
| N13 | none | |
| N14 | partial | forged author envelope `T:419`; no raw snapshot; no rollback attempt |

### F01–F10, R01–R08 and contract shared cases

| ID | Status | Evidence / gap |
|---|---|---|
| F01–F10 | none | single fixed shard name `T:73`; no blobs |
| R01–R08 | none | no runtime or UI |
| More than eight rotations | none | max 3 rotations per node in `S:264`; no succession/delegation interaction |
| Sibling asks to host | partial | `R:216` (first element only) |
| Old proof after retirement | partial | `R:124` (structural) |
| Revoked predecessor/assignment | none | |
| Succession keeps hosting | none | no device-level capability |
| Audience-safe history | partial | `R:147` (mock path) |
| Transport/author transition | none | explicitly excluded `README:52-53` |
| Offline mutual retirement | partial | `S:380`; no distinct "unreachable" outcome |

## 5. Weak or vacuous tests (summary) and proposed hardening

Everything in this section is **proposed** and was not executed.

| Test | Problem | Proposed fix (owner: group-sim author / coordinator) |
|---|---|---|
| `T:470` seeded schedules | can pass with every envelope Invalid; no negative control | assert `oracle.accepted.len()==10` and that each oracle receive is Accepted; add a control that withholds one envelope from both the schedule and the oracle source, and assert `!pending.is_empty()` or no convergence |
| `T:455` stale step | the stale replica is empty | give the stale replica a strict subset, sync both ways, and assert that h1's accepted IDs and doc are unchanged |
| `T:419` `wrong_did` | triple-confounded | add the author to the allowlist and re-sign with X's key, using A's attestation; also test an attestation for a different device key, so that `T:213` is the sole cause |
| `T:419` cross-group | state not checked; group and doc confounded | assert `other` is unchanged; test a group mismatch with a matching doc name, and the reverse |
| `A:240` nondelegable | `delegable` flag never isolated | parent `(Post, Delegate)` with `delegable=false`; positive control with `true` |
| delegation authority | issuer ≠ parent holder untested | C issues a child under B's admin grant: expect refusal |
| `T:497` pending bound | drain and overflow fate unasserted | assert the overflow ID is not in pending; deliver the withheld envelope and assert that 17 are accepted, with the refused one absent until resent |
| pending drain `T:252` | invalid promoted envelope silently dropped | test: an envelope with insufficient Loro deps but a missing declared dep sits pending, then the dep arrives; assert it is not accepted, and record which outcome a policy should surface |
| rejection tests generally | empty-baseline emptiness only | add a helper that snapshots `(doc.get_deep_value(), oplog_vv, accepted keys, pending keys, fanout)` and compare it before and after each rejection on a populated replica |
| `R:124`, `R:238` | structural / undiscriminating | add positive controls (route works at `t<retire`) and make `live` contain the old endpoint at the probe time; return a typed reason (`NoActiveBinding`/`EndpointGone`) instead of `bool` |
| `R:216` | single-element paths only | add `[nominated, sibling]` as a **limitation** test showing the mock accepts it (rotation adapter responsibility) |
| `S:264` rotation stress | no binding retires in the random phase | extend time past `BINDING_LIFETIME`, or shorten the lifetime; make the `queue_limits` counter mandatory or remove the claim; count injected faults through `Fleet` methods, not manual counter increments |
| size/peer checks `T:202-206`, `T:277` | untested | one rejection test per bound, plus a peer-ID impersonation test (needs an A2 fixture) |

### Predicted surviving mutations (hypotheses only, not run)

Each of the following deletions is expected to leave all 24 tests passing:
- `!p.body.delegable` (`A:134`)
- `b.issuer != p.body.holder` (`A:133`)
- `public_key_from_did(&b.holder).is_none()` (`A:125`)
- `a.did != b.author || a.device_key()…` (`T:213`): the forged case is also caught at `T:201`/`T:219`
- `b.peer != peer(...)` (`T:206`)
- `meta.partial_end_vv` peer check (`T:277-279`)
- each size bound (`T:202-205`)
- `self.accepted.values().any(|a| a.key() == e.key())` (`T:271`): possibly, since the semantic comparison may catch it

Suggested verification: the coordinator runs these mutations one at a time on a scratch branch.

## 6. What this audit does not establish

- No mutation testing, coverage instrumentation, or check of whether `QUEUE_LIMIT` is ever reached.
- Nothing about production code; group-sim is test-only (`lib.rs:1-3`).
- No judgment on the policy gates (D01–D06, G-HISTORY, G-FORK); only that they are unexecuted.
