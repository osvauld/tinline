# Rotation forks and older-ancestor revocation: options (packet B3)

Status: proposal — not approved. G-FORK, G-REVOCATION and G-AUTHORITY are open; nothing here
selects an option.

Model: `design-models:crates/proto/tests/rotation_fork_revocation.rs` (15 tests; fake time, real Ed25519,
deterministic fixture owners/keys). This is not a production wire format, verifier API, storage
design or recovery protocol. Rotation side of group gate **D06** (`docs/group-chat-tests.md:92`:
"owner rotates assigned device slot/ID; conflicting succession or revoked predecessor ... define
fork/revocation/recovery policy").

## 1. Starting point (verified in source/tests)

- Existing models detect only a same-generation fork at the receiver's current head
  (`rotation_permit_model.rs:86-88`, test `:181`; `rotation_history_paging.rs:154-159`) and a
  same-slot clone fork between partitioned peers (`support/rotation_network.rs:189-212`).
  Detection is not convergence (`:210-211`).
- Older fork evidence after advancing and older-ancestor revocation were explicitly untested
  (`docs/design/endpoint-rotation-permit-model.md:66-70`).
- Revocation checks cover only the anchor, current head and incoming permit
  (`rotation_history_paging.rs:148-153`), so revoking an already-passed intermediate record has
  no effect on a receiver that accepted its descendants.
- B1 limitation: a checkpoint cannot detect a fork between the receiver's head and the
  checkpoint (`rotation_history_paging.rs:1092`).
- Trust model: linking transfers the mnemonic; every linked device can produce any owner-DID
  signature; unlink is not revocation (`docs/design/device-linking.md:15-22`, `:115-121`;
  `docs/group-rotation-integration-contract.md:28-30`).
- Group contract item 3: hosting needs the nominated device's verified path AND a still-authorized
  assignment; assignment revocation must not be undone by rotation
  (`docs/group-rotation-integration-contract.md:20-23`).

## 2. Model (verified in tests; candidate shape, not frozen)

Copied from B1 @390ab54: `Claims`, `Permit`, `View`, `verify_history_page`, `bind_live`,
checkpoint. Added (all in `rotation_fork_revocation.rs`):

| Element | Line | Meaning |
|---|---|---|
| Fork proof | `detect_conflict` `:576` | Two distinct, validly owner-signed successors of the SAME parent (`predecessor` and `previous_id` equal) where one is a retained record on the receiver's path. Anything else (different parent, sibling path, forged/tampered signature, other audience, already-revoked record) is an ordinary rejection with no flag. |
| `offer_page` | `:603` | Fork detection first (records `Conflict {generation, parent, kept, other}`, outcome `Flagged`, ONLY `conflicts` changes), then suspension/freeze gates, then B1 history verification. |
| `ForkPolicy` | `:359` | `FreezePath` (no route, no extension, no bind while any conflict exists) vs `KeepHead` (first-accepted head stays live; conflict retained for reporting). |
| `RevocationPolicy` | `:368` | (a) `Cascade`: revoked record + all descendants invalid; path suspended until an authority-signed `Recovery` naming a fresh successor of the revoked record's parent. Anchor revocation cannot be recovered by succession (`renomination_required`). (b) `EndpointOnly`: revocation blocks live binding to that record's endpoint; history and descendants stand; no recovery record. |
| `Authority` | `:379` | `OwnerDid` (current trust model = any mnemonic holder) vs `SeparateRecoveryKey` (HYPOTHETICAL, requires G-AUTHORITY change; key not held by linked devices; generation/custody/distribution unresolved). Applies to revocation and recovery records only; permits stay DID-signed. |
| `accept_revocation` / `accept_recovery` | `:656` / `:699` | Authority signature, scope (owner, audience, anchor), idempotent redelivery; recovery re-verifies the replacement against the reconstructed parent view. |
| `bind` / `route` | `:758` / `:791` | Refused while suspended or frozen; `EndpointOnly` blocks only a revoked head. |

Every rejection asserts the whole receiver (view, evidence, checkpoint, nonces, live binding,
conflicts, revocations, suspension) equals a clone taken before the call (`reject_*`,
`attempt_bind` helpers). Flags assert that only `conflicts` grew and that redelivery is a no-op.

## 3. Tests and KIND labels

| Test | KIND | Shows |
|---|---|---|
| `same_generation_fork_is_flagged_first_accepted_kept_and_freeze_path_stops_live_use` | candidate-policy G-FORK | gen-3 sibling flagged; view = first-accepted; route withdrawn; honest gen 4 `frozen`; either branch's bind `frozen` |
| `same_generation_fork_is_flagged_first_accepted_kept_and_keep_head_stays_live` | candidate-policy G-FORK | flagged; honest route kept; clone bind `not_head`; clone child `predecessor`; honest gen 4 accepted and bound; conflict still reported |
| `older_generation_fork_evidence_after_advancing_is_detected` | invariant | receiver at gen 9; gen-5 fork proof flagged under all 4 policy combinations; a clone tail without the divergence point is only `stale` |
| `older_fork_handling_freeze_path_versus_keep_head` | candidate-policy G-FORK | same older fork: freeze withdraws a gen-9 route and refuses gen 10; keep-head accepts gen 10 |
| `receiver_that_keeps_only_its_cursor_cannot_detect_an_older_fork` | limitation | without retained per-record `(generation, id, predecessor)` the gen-5 proof is just `stale` |
| `two_receivers_on_different_branches_both_flag_after_exchange_and_neither_switches` | invariant | A (honest) and B (clone) exchange heads: both flag, mirrored conflicts; C learns the fork from A's retained evidence; views unchanged and still different; freeze → no routes, keep-head → divergent routes |
| `last_arrival_wins_is_rejected_even_for_a_longer_later_signed_branch` | invariant | attacker branch issued later and 4 generations longer; strawman `last_arrival_wins` (`:1240`) would route to it; model flags, refuses its child and its possession proof under both policies |
| `forged_fork_and_revocation_evidence_is_rejected_with_state_unchanged` | invariant | tampered/foreign-DID/other-audience/sibling-path "forks" and foreign-DID/endpoint-key/wrong-scope revocations rejected; no flag, so non-mnemonic parties cannot freeze a path |
| `revocation_option_a_cascade_invalidates_descendants_until_owner_signed_recovery` | candidate-policy G-REVOCATION | intermediate gen 3 / head gen 6 / anchor: suspension, extension and bind `suspended`, receiver behind cannot page through (`revoked`), wrong-target/replacement recoveries rejected, valid recovery drops descendants 4..6 and needs a fresh bind; late revoked branch `revoked`, not a new fork; anchor → `renomination_required` |
| `revocation_option_b_blocks_only_that_records_endpoint_and_descendants_stand` | candidate-policy G-REVOCATION | intermediate: route and extension unaffected, receivers behind still page through; head: bind `revoked`, ordinary successor restores route; anchor: no effect; recovery `no_recovery_in_policy` |
| `holder_of_only_an_old_transport_key_cannot_bind_extend_or_revoke_under_either_option` | invariant | retired gen-4 key: `not_head`, `possession`, key-signed permit `owner_signature`, key-signed revocation `authority`; same after a further rotation |
| `holder_of_revoked_device_key_and_shared_mnemonic_is_not_excluded_by_either_option` | limitation | traces in §5; thief obtains the route under (a) and (b); owner's late successor/recovery is only flagged; thief can revoke owner records |
| `separate_recovery_authority_blocks_mnemonic_revocation_and_recovery_but_not_mnemonic_permits` | candidate-policy G-AUTHORITY | `OwnerDid`: thief revocation accepted. `SeparateRecoveryKey`: thief revocation/recovery `authority`; owner recovery accepted; thief sibling only flagged; BUT thief's DID-signed successor of the recovered head is accepted and bound |
| `revoked_host_assignment_is_not_restored_by_any_succession` | invariant | after assignment revocation: ordinary succession, cascade revocation + recovery, flagged fork, sibling path — none restores `can_host`; sibling never inherits nomination; assignment fixture unchanged |
| `strict_history_flags_a_fork_between_receiver_head_and_checkpoint_that_checkpoints_miss` | limitation | divergence at gen 5, receivers at gen 4. Strict: flagged. Checkpoint: honest cp@12 then clone cp@13 silently replaces it ("highest generation wins"); receivers on cp@12 vs cp@11 of different branches cannot produce a fork proof (`stale` / `predecessor`), not even by comparing checkpoints |

## 4. Option tables (proposed; no option selected)

### G-FORK — after a fork proof is flagged

| | FreezePath | KeepHead |
|---|---|---|
| Route | withdrawn for both branches | first-accepted head stays live |
| Extension of first-accepted branch | refused (`frozen`) | allowed |
| Who decided the survivor | nobody (waits for resolution) | arrival order at this receiver |
| Receivers on different branches | both unreachable | each routes to its own branch (no convergence) |
| Abuse by a mnemonic holder | can freeze any contact path at will (DoS) by signing a sibling | can win a race by delivering first; honest later branch only flagged |
| Abuse by a non-mnemonic party | none (forged proofs rejected) | none |
| Missing | resolution record and its authority (unmodeled) | user-facing conflict surfacing; when to stop trusting the kept head |

Both options require: no last-arrival-wins, retained per-record evidence to detect older forks,
and a divergence-point record in exchanged evidence (tails alone are plain rejections).

### G-REVOCATION — revoking anchor / head / older intermediate after descendants accepted

| Target | (a) Cascade | (b) EndpointOnly |
|---|---|---|
| Older intermediate | path suspended; descendants invalid; recovery from its parent drops them | no effect on route or extension (key already retired) |
| Head | suspended; recovery from its parent | bind to head refused; any ordinary successor restores routing |
| Anchor (nomination) | whole path suspended; succession cannot repair (`renomination_required`) | no effect (anchor has no endpoint) |
| Receivers still behind | cannot page through revoked record | page through it normally |
| State needed | retained evidence to locate target generation; revocation set; recovery parent | revocation set only |
| Interaction with group history | invalidates descendants that group proofs may already reference (needs G-HISTORY/group decision) | historical proofs unchanged |

### G-AUTHORITY — who signs revocation/recovery (and permits)

| | OwnerDid (current) | SeparateRecoveryKey (hypothetical) |
|---|---|---|
| Thief revokes owner records | yes | no |
| Thief "recovers" to own endpoint | yes (under a) | no |
| Thief extends a head with a new permit | yes | yes — permits are still DID-signed |
| Thief signs chat/grants/tickets as the DID | yes | yes (outside rotation) |
| Change required | none | new key class, custody, distribution to contacts at nomination time, its own loss/rotation; contradicts "every linked device is a full peer" |

Excluding a mnemonic holder from SUCCESSION would also need permits signed by non-shared keys
(the rejected "delegated device keys" design, `device-linking.md:21-23`) or identity rotation.
Neither is proposed here.

## 5. Attacker-capability matrix (verified traces where marked)

| Capability | (a) Cascade + OwnerDid | (b) EndpointOnly + OwnerDid | (a) + SeparateRecoveryKey |
|---|---|---|---|
| Old transport key only | nothing: `not_head`/`possession`/`owner_signature`/`authority` (tested) | same (tested) | same |
| Mnemonic only | sign revocations (DoS, tested), recoveries, permits, siblings (freeze DoS under FreezePath) | sign successors, revocations, siblings | sign permits/successors and siblings (tested); not revocations/recoveries (tested) |
| Both (stolen revoked head) | revoked key refused; own successor `suspended`; mnemonic-signed recovery accepted → thief route (tested) | revoked key refused (`revoked`); mnemonic-signed successor accepted → thief route (tested) | revocation/recovery refused; but DID-signed successor of recovered head accepted → thief route (tested) |

Statement: under the shared-mnemonic model no option here cryptographically excludes a hostile
linked device. Revocation in (a) and (b) only removes a specific record/key; the mnemonic restores
the attacker. First-arrival decides which branch a given receiver keeps; the losing branch is
flagged, never silently adopted.

## 6. B1 limitation extended (verified)

Strict history detects a fork whose divergence point lies between the receiver's head and the
new head, because it must verify the divergence record. A checkpoint skips it: anti-rollback by
generation lets a higher-generation checkpoint from another branch replace the head silently, and
two checkpoint receivers on different branches at different generations cannot produce a fork
proof. A checkpoint design would need, e.g., a link to the receiver's last known head or to the
previous checkpoint (a G-HISTORY question; not modeled).

## 7. What is NOT proven

- No resolution/unfreeze record, no user-facing conflict UI, no persistence, no session.
- Fork-evidence exchange is modeled as offering a page; gossip/forwarding policy is not modeled.
- `Recovery` and `Revocation` schemas, their expiry and audience fan-out are fixtures.
- The hypothetical recovery key is a fixed-seed fixture; no key scheme is proposed.
- Cascade invalidation of records already exported into group proofs is not modeled.
- Group `can_host` is a fixture of contract item 3, not the group-sim adapter.

## 8. Questions for the user / gates

1. G-FORK: after a proven fork, freeze the contact path (DoS-able by any mnemonic holder) or keep
   the first-accepted head (race-able)? What resolves a freeze, signed by whom?
2. G-FORK: must receivers retain per-record `(generation, id, predecessor)` for every audience so
   older forks remain detectable (cost: one record per rotation per contact)?
3. G-REVOCATION: (a) cascade with explicit recovery, or (b) endpoint-only? Is anchor revocation a
   rotation operation at all, or only a group/nomination operation?
4. G-REVOCATION × group: under (a), what happens to group proofs that already include revoked
   descendants (D06)?
5. G-AUTHORITY: keep the shared-mnemonic model and document that revocation cannot exclude a
   linked device (identity rotation is the only cut-off), or open a separate-authority design?
6. G-HISTORY × G-FORK: if checkpoints are adopted, must a checkpoint link to the receiver's prior
   head/checkpoint so cross-branch replacement is detectable?
