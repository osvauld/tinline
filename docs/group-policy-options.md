# Group causal policy options: D01–D05 (packet G-B)

Status: proposal — not approved. Gates D01–D05 (`docs/group-chat-tests.md:81-92`) stay open.
Nothing here picks an option. Each option is a candidate oracle with executable tests. A
`limitation` test passes by showing what an option cannot do. D06 (device succession, forks,
revoked predecessors) belongs to rotation; see `docs/design/rotation-fork-revocation-options.md`.
It is not repeated here.

Model: `crates/group-sim/src/policy_candidates.rs` (23 tests). It is wired in through one
`#[path]` `mod` line at `crates/group-sim/src/tests.rs:13-14`. It is test-only and is not a wire
format, a production API or a storage design.

## 1. What the model is (verified in source/tests)

- **Membership ops.** `MetaOp` (`policy_candidates.rs:29-103`) is signed with the real DID key.
  Ops form a causal DAG through these fields:
  - `parents`: the meta frontier the issuer had seen.
  - `seen_posts`: the content the issuer had accepted.
  - `at`: the issuer's *claimed* clock. Only the timestamp candidates read it.
- **Posts.** Posts are the existing real `Envelope`s: device-signed, attested, with Loro
  updates. The existing `Body` has no `meta_frontier` field, so the policy `Header`
  (frontier, claimed `at`, optional epoch) travels as JSON in the signed `text`
  (`:104-145`). Production would put it in `GroupUpdate.meta_frontier`
  (`docs/group-chat.md:140-150`). In both cases the signer chooses the value.
- **Policy node.** `Node` (`:272-783`) checks signatures and group, then admits ops and posts.
  An item whose references are missing stays pending, so I4 holds at this layer. The node then
  evaluates one `PostRule` (`:211-229`) with one `Membership` rule (`:187-207`).
- **Content layer.** `Node::materialize` (`:719`) feeds the policy-accepted envelopes, in
  causal order, into the **unchanged** existing `Replica`. Loro, proof and dependency checks
  are therefore real.
- **Cross-layer invariant.** `finalize` (`:737`) asserts this on every final state in every
  test: content never holds a post the policy rejected, and a policy-accepted post is held
  pending only when a dependency is missing.
- **All delivery orders.** `explore` (`:901`) delivers a trace to a fresh node in every
  permutation (24, 120 or 720 orders) and collects the distinct final outcomes: verdicts plus
  current membership. It also records `retroactive`: some order showed a post as accepted at an
  intermediate step and rejected at the end.
- **Partitions.** Partition checks use `side`/`heal` (`:945-958`). Each side gets a subset of the
  events, then each side gets everything.
- **Fixture simplifications** (open questions, not decisions):
  - The genesis creator is always an admin and cannot be removed (`:161-186`).
  - An authentic op from a non-admin is kept but has no effect. Under some candidates its
    effect depends on what arrives later, so it is not rejected when it arrives.
  - The policy layer has no pending bound.

## 2. Candidates

Post rules (`PostRule`):

| ID | Rule | Code |
|---|---|---|
| SF | **Signed frontier**: the author is a member at the post's own signed meta frontier | `:213`, `:681` |
| AV | **Arrival view**: decided once, against the receiver's whole current membership, when the post's references are present; never revisited | `:216`, `:407` |
| RW | **Removal wins over concurrent**: SF, *and* no authorized removal of the author is concurrent with the post. Concurrent means the removal is not in the post's frontier and the post is not in the remover's `seen_posts` closure | `:219`, `:605` |
| TS | **Author timestamp**: membership is linearized by the claimed `at`; valid iff a member at the post's claimed `at` | `:221`, `:568` |
| LC | **Local-clock expiry**: decided once at arrival; SF, and receiver clock < grant `expires_at` | `:223`, `:407` |
| TE | **Timestamp expiry**: SF, and claimed `at` < grant `expires_at` | `:225` |
| EC | **Epoch cut**: the post names an epoch on the winning chain (lowest op ID among competing children). The author is in that epoch's member list. Either the epoch is the tip, or the next winning epoch's signed `cut` names the post | `:228`, `:615-677` |

Membership resolution (`Membership`, D03):

| ID | Rule |
|---|---|
| RemoveWins | Each op is authorized at its own signed frontier. Among the maximal (concurrent) ops on a member, any Remove means the member is absent (`:444`). |
| AddWins | The same, except any maximal Add means the member is present. |
| Priority | Topological fold. Ready ops are taken junior-first (power: member 0 < admin 1 < creator 2; ties by op ID), so seniors act last. Each op is authorized against the *folded* state, not its own frontier (`:504`). |
| `cascade` | An orthogonal flag. Repeatedly drop Adds whose issuer is not an admin in the evaluated state, until nothing changes (`:538-564`). |

## 3. Results per gate (verified by tests)

"Order-indep." means one final outcome across all permutations. "Retro." means a post can be
shown as accepted and then dropped.

### D01 — revocation vs truly concurrent post

Trace (`:960-1000`):

1. A adds B.
2. B posts q, and A sees q.
3. A removes B. The removal's `seen_posts` contains q.
4. Concurrently, B posts p at the pre-removal frontier.
5. After learning of the removal, B posts r.

There are 120 orders.

| Option | Test | Order-indep. | Outcome | Partition |
|---|---|---|---|---|
| SF | `d01_signed_frontier_keeps_concurrent_post_in_every_order` (`:1004`) | yes, monotone | q ✓ p ✓ r ✗ | B's side shows p; after heal both keep it. Negative control: without `add_b`, everything stays pending. |
| AV | `d01_arrival_view_outcome_depends_on_delivery_order_and_never_reconverges` (`:1047`) | **no** (≥2 finals; even q flips) | depends on arrival | The sides disagree permanently after heal. This violates I5. |
| RW | `d01_removal_wins_drops_concurrent_post_in_every_order_retroactively` (`:1071`) | yes, retro | q ✓ (only because the remover signed it as seen), p ✗, r ✗ | B's side shows p until heal, then drops it. |

### D02 — removed member signs against a stale frontier with an old timestamp

- `d02_stale_frontier_post_is_byte_identical_to_honest_delayed_post` (`:1135`, invariant):
  - A B that already holds its own removal signs the stale frontier.
  - The result is **byte-identical** to the honest offline post p from D01: same bytes,
    signature and ID.
  - No receiver can tell the two apart. This is the gate's premise in executable form.
- Trace (`:1098-1131`):
  - B is removed at claimed time 10, and A had seen q.
  - Then B signs s at `{add_b}` with claimed time 3.
  - B also signs an honest r at `{rm_b}`.

| Option | Test | KIND | What the stale-frontier signer achieves |
|---|---|---|---|
| SF | `d02_signed_frontier_accepts_backdated_post_from_removed_member` (`:1164`) | limitation | s is **accepted in all 120 orders**. Citing an old frontier is enough. |
| RW | `d02_removal_wins_rejects_backdated_post_but_only_remover_seen_posts_survive` (`:1178`) | candidate | s is rejected in every order. s is accepted only on a receiver that lacks the removal (partition), and dropped after heal. The honest delayed post receives the same treatment. |
| TS | `d02_author_timestamp_accepts_backdated_claimed_time` (`:1206`) | limitation | s (claims 3 < 10) is accepted in every order. Backdating the claimed time is enough. |
| EC | `d02_epoch_cut_rejects_stale_epoch_post_not_named_in_cut` (`:1222`) | candidate | s is rejected unless the next epoch's cut names it (24 orders). It is accepted provisionally on receivers without the next epoch. |

### D03 — concurrent regrant/removal, competing admins, parent revocation

The resolution traces (`:1250-1320`) are:

- **Regrant.** A adds B (member) and C (admin). Concurrently, A removes B and C re-adds B.
  B posts `stale` (cites only the regrant) and `merged` (cites both). There are 720 orders.
- **Rivals.** Admins B and C remove each other concurrently. There are 24 orders.
- **Partition check (all options).** Each rival, isolated, believes the other was removed.
- **Stale posts (all options).** `stale` is accepted under every resolution rule.

| Option | Test | Regrant | Rivals |
|---|---|---|---|
| RemoveWins | `d03_remove_wins_resolution` (`:1324`) | B out; `merged` ✗ | **both** removed |
| AddWins | `d03_add_wins_resolution` (`:1336`) | B in; `merged` ✓ | both removed. Add-wins does not help, because there is no concurrent add. |
| Priority | `d03_priority_fold_resolution` (`:1348`) | B out. The creator's removal is applied after the junior admin's regrant. | Exactly one survives: the issuer of the lower op ID. The result is deterministic, but the choice is an arbitrary hash comparison. |

Parent-revocation trace (`:1362-1388`):

1. A makes B an admin.
2. B adds C.
3. A removes B, having seen B's add of C.
4. Concurrently with step 3, B adds D.
5. C posts `stale` at `{add_c}` and `after` at `{rm_b}`.

There are 720 orders.

| Option | Test | Outcome |
|---|---|---|
| no cascade | `d03_no_cascade_keeps_members_added_by_later_removed_admin` (`:1392`) | C and D stay; both of C's posts ✓ |
| cascade (RemoveWins, and Priority) | `d03_cascade_drops_members_added_by_removed_admin_but_not_stale_frontier_posts` (`:1408`) | C and D are removed. `after` ✗. `stale` ✓: a cascade-removed member citing the old frontier still posts. |

### D04 — grant expiry vs delayed delivery and receiver clocks

Trace (`:1437-1456`):

- B's grant has `expires_at = 10`.
- p1 claims 5.
- p2 honestly claims 15.
- p3, from B's second device, was written at 15 but claims 5.

There are 24 orders per receiver clock.

| Option | Test | KIND | Outcome |
|---|---|---|---|
| LC | `d04_receiver_clock_expiry_makes_history_depend_on_receiver_and_arrival_time` (`:1460`) | candidate | Clock 8 accepts all three; clock 12 rejects all three. A receiver whose clock advances mid-delivery gets a third history. History depends on the receiver, which is what `docs/group-chat.md:180-182` forbids. |
| TE | `d04_author_timestamp_expiry_is_deterministic_and_separate_from_current_access` (`:1502`) | candidate | Identical history at clocks 8 and 12: p1 ✓ p2 ✗. `current_access` (`:1494`) uses the local clock, and the receivers disagree on access but not on history. |
| TE | `d04_author_timestamp_expiry_accepts_backdated_post_after_expiry` (`:1526`) | limitation | p3 is accepted at every clock (8, 12, 1000), in every order. |
| explicit op (SF) | `d04_explicit_expiry_op_is_clock_free_but_unenforced_until_issued_and_cited` (`:1546`) | candidate | Expiry takes effect only through an admin's removal op; `expires_at` is advisory. The result is clock-free and monotone. p2 ✓ because its frontier lacks the op. A post citing the op is ✗. Enforcement needs an online admin. |

### D05 — member offline across removal/epoch change, later rejoins

Trace (`:1568-1589`):

1. A adds B.
2. B goes offline and writes o.
3. A removes B without having seen o, then re-adds B.
4. B returns and posts n1 on the same device chain (`previous = o`) and n2 on a new device.

There are 720 orders.

| Option | Test | Outcome |
|---|---|---|
| SF | `d05_signed_frontier_offline_posts_survive_removal_and_rejoin` (`:1593`) | o, n1, n2 are all accepted and monotone. This is the same mechanism that accepts D02's backdate. |
| RW | `d05_removal_wins_drops_offline_post_and_strands_same_device_chain` (`:1610`) | o ✗. n1 is valid by policy but **stranded**: it stays pending forever because its signed predecessor is rejected. n2 (fresh device chain) ✓. |
| (any) | `d05_rejected_predecessor_never_becomes_implicitly_accepted` (`:1634`, invariant) | Redelivery and repeated full sync never release n1. Negative control: under SF, n1 is accepted. |
| EC | `d05_epoch_cut_drops_uncut_offline_post_and_rejoin_needs_new_epoch` (`:1669`) | o ✗ unless the epoch issuer had o and named it in the cut, in which case o ✓. Rejoin is a new epoch that lists B; n2 ✓. |
| EC (partition) | `d05_competing_epochs_after_partition_tie_break_drops_losing_branch` (`:1695`) | Two admins issue competing epoch 2s while partitioned. Each side accepts its own post. After heal, the lower-ID epoch wins and the other side's honest post is dropped. Both sides end in the same final state. |

Shared admission invariant:
`forged_or_misscoped_meta_ops_and_headerless_posts_leave_policy_node_unchanged` (`:1734`). Each
of these is `Invalid` with a byte-identical node snapshot:

- a forged signature;
- an altered body;
- an op for another group;
- an issuer DID that does not parse;
- a validly signed post with no header.

An authentic self-add by a non-admin is stored and changes nothing. A positive control
releases the pending post.

## 4. Comparison table

| | SF | AV | RW | TS / TE | LC | EC |
|---|---|---|---|---|---|---|
| Order-independent | yes | **no** | yes | yes | per receiver only; differs across clocks | yes |
| Monotone (no retroactive drop) | yes | yes (but divergent) | **no** | yes | yes | **no** |
| Stale-frontier backdating by removed member | **succeeds** | succeeds wherever the post arrives first | blocked | **succeeds** (claimed time) | succeeds at slow-clock receivers | blocked unless named in cut |
| Honest delayed/offline post | kept | arrival-dependent | dropped unless the remover saw it | kept if claimed time < cutoff | arrival/clock-dependent | dropped unless cut names it |
| Partition | each side converges on heal | permanent divergence | provisional acceptance, withdrawn on heal | converges | divergent | provisional acceptance; competing epochs lose one side |
| Needs | signed frontier | — | `seen_posts` in removals | trusted clocks (not available) | — | online epoch issuer, cut size, tie-break |
| Same-device chain after a dropped post | n/a | n/a | stranded (needs new chain or skip rule) | n/a | n/a | stranded likewise |

D03 resolution is orthogonal to the post rule:

| | Regrant vs removal | Mutual admin removal | Requires |
|---|---|---|---|
| RemoveWins | removal wins | both removed | — |
| AddWins | regrant wins | both removed | — |
| Priority | senior wins | one survives, by op ID | a power order; equal powers are tie-broken arbitrarily |

Cascade drops a revoked admin's later grants. It does not stop that admin's grantees from
posting against stale frontiers.

## 5. What this does NOT prove

- **Small traces only.** Every order of 4–6 events is covered. Larger schedules, malicious
  hosts and resource bounds on the policy layer are not.
- **No retention rule.** No candidate rule was chosen for keeping or serving rejected
  envelopes, or for surfacing retroactive drops in the UI.
- **Provisional encodings.** The header-in-`text` encoding, the cut format (explicit post IDs
  rather than a VV) and the creator-immunity rule are fixtures.
- **No rotation.** Device succession, forks and revoked predecessors (D06) are not modeled
  here.
- **No real signing format.** No real wire format or signing format is defined.

## 6. Questions for the user (exact decisions needed)

1. **D01/D02:** Which matters more:
   - (SF) keep honest offline and concurrent posts, and accept that a removed member can keep
     posting by citing a stale frontier; or
   - (RW/EC) cut off a removed member's unseen writes, and accept losing honest delayed posts
     plus retroactive disappearance after a partition heals?

   Is a mixed rule acceptable? An example of a mixed rule: SF for history, plus "hide
   from notifications/UI after removal is known".
2. **RW only:** Must every removal carry the remover's content frontier (`seen_posts`)? That
   frontier is the only thing that keeps posts the remover already saw.
3. **Retroactive drops:** Are they acceptable at all? If so, how are they shown: post greyed
   out, removed, or marked "unverified after removal"? Should hosts keep serving dropped
   envelopes for convergence?
4. **Device chains:** When a policy rejects a post on a device chain, should later posts on
   that chain:
   - stay stranded (current replica behavior);
   - be allowed to skip a rejected predecessor; or
   - require a fresh device chain (new device ID) on rejoin?
5. **D03 resolution:** Should a regrant that is concurrent with a removal follow remove-wins,
   add-wins, or seniority? With mutual removal between equal admins, is "both removed"
   acceptable, or must one survive? If one must survive, what is the tie-break: op ID,
   admin age, or creator decides?
6. **D03 cascade:** Should removing an admin cascade to members that admin added? Before or
   after the removal (causally)? Should Priority's "authorize against the folded state"
   replace the "authorize at own frontier" rule?
7. **Creator:** Can the creator be removed or demoted? (The fixture says no.) What happens
   when all admins are removed?
8. **D04 expiry:**
   - Should `expires_at` exist at all, given that claimed time is backdatable (TE) and
     receiver time diverges (LC)?
   - Or should expiry be only an explicit admin-issued op?
   - Is "current access uses the local clock, history does not" the intended split?
9. **D05 epochs:** Are signed epochs with cuts wanted, with the availability cost: offline
   posts not named in a cut are lost, and competing epochs drop the losing side? If so:
   - Who may issue an epoch?
   - What is the tie-break between competing epochs?
   - What is the cut format (VV per device)?
   - What is the maximum epoch length?
10. **Unauthorized ops:** May they be retained as inert records, or must they be refused at
    receipt? Refusing at receipt conflicts with candidates whose authorization depends on
    knowledge that arrives later (Priority, cascade).
