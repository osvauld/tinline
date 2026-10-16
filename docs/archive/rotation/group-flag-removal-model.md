Status: proposal — not approved

# Group removal by admin flags and group-level rotation on host revocation (W2-F)

Packet W2-F. This doc covers a simulation of two user decisions in
`docs/design/rotation-open-decisions.md`:

- **U9** (section 0): admins flag a member, and at 3 flags the member is removed and its
  grant revoked.
- **Section 10 Q8** (decided): when a host is revoked, the whole group rotates.

It also uses U4 revised, U5 revised, U7 and section 9 Q5. The open U9 details in section 8
and D01/D02 (section 6) are modelled as candidates; none is selected. No production code was
changed.

- **Model and tests:** `design-models:crates/group-sim/src/flags.rs`. It is wired in at
  `design-models:crates/group-sim/src/tests.rs:27-28`. Line numbers below refer to the W2-F commit.
- **Provenance:** the grant chain, posts, writer, board, endpoint notes, host records, signed
  ops and the link/host-sync/rotation machinery are copied from `multi_host.rs` @0924939
  (`flags.rs:1-30` lists the changes). No other file was edited.
- **`Cfg::default()`** (`flags.rs:715`) gives every open parameter a value only so the other
  tests can run. These defaults are not selections.

## 1. What the model is (verified in source)

### Ops

- **Group ops** are `Flag`, `Withdraw`, `Admit { lifts }` and `RemoveHost` (`flags.rs:544`).
  Each is signed by the issuer's DID key.
- **Fields every op carries** (`flags.rs:564`):
  - `issuer_grant`, which must chain to the genesis creator with `Admin`;
  - `seen`, the issuer's op frontier.
- **Fields a flag also carries:**
  - `at`, a claimed time;
  - `seen_posts`, the posts the issuer had accepted.

### Verification on every device

Every device (member or host) verifies each op itself in `Replica::accept_op`
(`flags.rs:984`), in this order:

1. Signature and group.
2. `Ctx::static_check` (`flags.rs:876`): is the issuer an admin, is the target flaggable, is
   withdrawal or rejoin allowed at all.
3. An op that cites unknown ops waits pending (bounded).
4. `frontier_check` (`flags.rs:1017`): the op is authorized **at its own signed frontier**.
   The issuer must not be removed there; the target must be a member there; a withdrawal
   must come before the threshold *there*.

A host has no extra authority: it verifies the same way.

### Membership is derived from the op set

- Membership is a pure function of the op set: `Ctx::derive` (`flags.rs:800`).
- Effective flags exclude flags that were withdrawn, lifted by a readmission, or expired.
- Removal is decided by `threshold_met`/`removal_met` (`flags.rs:768`, `:784`):
  - with ≥3 fixture admins, 3 flags are needed;
  - with fewer admins, the `SmallGroup` candidate decides.
- **Grant revocation.** A removed DID's bootstrap grant digest, and any Admit grant for it,
  go into `revoked_grants`. A grant that has been revoked cannot be presented again.

### Posts

- A post carries `ops_seen`, the author's signed op frontier, like the D01/D02 `Header` in
  `policy_candidates.rs`.
- `Replica::post_ok` (`flags.rs:1132`) applies the `PostRule` candidate.
- Accepted posts go into a store. The board is a real Loro materialisation of that store.
- When a rule drops posts retroactively, `rebuild_board` (`flags.rs:1197`) rebuilds the
  board.
- `Board` here has no roster; the policy layer decides membership.

### Hosts after a removal (U7)

- `recompute` (`flags.rs:1093`) drops the removed DID's directory entries.
- `accept_note` (`flags.rs:1237`) refuses the removed DID's rotation notes.
- `connect_inner` (`flags.rs:1597`) answers the removed member's dial with the signed flags
  and their causal frontier (`removal_notice`, `flags.rs:1660`), and opens no session. This
  is the visible notice required by U5 revised.

### Group rotation on host revocation (section 10 Q8)

1. A device that newly applies `RemoveHost` sets `rotate_due` (`flags.rs:1066`).
2. `run_rotations` (`flags.rs:1842`) then rotates it once:
   - a member runs `rotate_member` (`:1860`) and sends its new note only to hosts still
     permitted in its view;
   - a remaining host runs `rotate_host` (`:1892`), dials every member at the member's
     current directory endpoint and runs a full exchange, which delivers the revocation op.
3. The revoked host never rotates.
4. `revoke_host` (`:1931`) is the fixture for the wave: the owner signs and hands the op to
   one legitimate host.

### Exploring delivery orders

- `explore` (`flags.rs:2009`) delivers a trace to a fresh replica in every order.
- It records the distinct final outcomes.
- It also records whether some order showed a post, or a removal, that later disappeared.

### Idealisations

- The admin set used for counting is a fixture (`Ctx::admins`, `flags.rs:735`). It is fixed
  at bootstrap and is not updated when an admin is removed.
- The owner counts as an admin.
- Hosts are separate DIDs with owner-issued `{Host}` permits, so host-permit cascade does not
  arise.
- Join/invite is a fixture.
- Posts are uploaded only by their author (members do not relay their store).
- Time is fake `u64`, and everything is in memory.

## 2. Verified in tests (`cargo test -p group-sim (design-models branch)`: 163 passed = 147 baseline + 16 new)

| Test (`design-models:crates/group-sim/src/flags.rs`) | KIND | What it shows |
|---|---|---|
| `three_flags_from_distinct_admins_remove_member_and_revoke_grant_everywhere` :2051 | invariant | After 2 flags no roster changes. After the 3rd, every device derives the identical state, including both hosts and carol once she is told. Carol is removed, her grant is revoked, and both hosts drop her endpoint. An Admit re-presenting her old grant is refused (`GrantRevoked`) with every device byte-identical. |
| `same_admin_flagging_three_times_distinct_admins_vs_any_flags` :2105 | candidate-policy G-AUTHORITY | Alice flags 3 times from 2 devices. Under `DistinctAdmins`, nobody removes carol until bob and dave flag. Under `AnyFlags`, one admin removes her alone. |
| `flags_commute_across_delivery_orders_and_two_partitioned_hosts` :2143 | invariant | 3 flags + 2 posts in all 120 orders give one outcome, with no retroactive change, and a duplicate flag leaves state unchanged. With H1 and H2 partitioned, the sides hold 1 and 2 flags and nobody removes carol. After healing, every device holds the identical derived state with carol removed. |
| `forged_non_admin_host_only_wrong_group_and_non_member_flags_are_refused_unchanged` :2210 | invariant | Nine bad flags are refused at all 9 devices, each with byte-identical state. The cases are: malformed bytes; a forged issuer; a target swapped after signing; a member; a self-issued admin grant; a host permit used as the issuer grant; a host with no grant; another group; a non-member target. Positive control: alice's genuine flag is applied everywhere. |
| `fewer_than_three_admins_owner_alone_vs_admin_count_vs_no_removal` :2331 | candidate-policy G-AUTHORITY | The admins are only olga and alice, so bob's flag is `NotAdmin` (unchanged). Under `OwnerAlone`, olga's single flag removes. Under `ThresholdIsAdminCount`, both flags are needed. Under `NoRemoval`, carol stays even with both. |
| `owner_protected_vs_flaggable` :2390 | candidate-policy G-AUTHORITY | Under `Protected`, flags on the owner are `NotFlaggable` (unchanged). Under `Flaggable`, the owner is removed: her later ops are `IssuerRemoved` and her posts `NotMember`, both unchanged. **But** the grants and host permits she issued still verify, so admins keep acting and hosts keep relaying. |
| `flag_withdrawal_final_vs_before_threshold` :2454 | candidate-policy G-AUTHORITY | Under `Final`, a withdrawal is `NotAllowed` (unchanged). Under `BeforeThreshold`, a withdrawal leaves 2 effective flags. Withdrawing someone else's flag is `NotFlagger`, and withdrawing after the removal is `AfterThreshold`, both unchanged. A withdrawal concurrent with the 3rd flag gives one final outcome (not removed) over 24 orders, but with a **retroactive un-removal**. |
| `flag_expiry_never_vs_signed_window_vs_receiver_clock` :2535 | candidate-policy G-TIME | Under `SignedWindow(100)`, flags at 0, 50 and 300 do not combine, but a flag re-signed claiming `at=10` counts (the signer chooses the time). Under `ReceiverClock(100)`, two replicas with the same op set and different arrival times **diverge permanently**: one removes carol and the other never does. |
| `removed_member_refused_by_every_honest_host_endpoint_dropped_and_told_with_signed_flags` :2586 | invariant | The hosts drop carol's endpoint; it appears nowhere in host state. They refuse her post (citing her current frontier) and her new note, and cannot dial her (`NoAddress`), all with state unchanged. Her dial gets the 3 signed flags from H1, which she verifies. A repeat attempt gives the same notice and no change. |
| `posts_around_third_flag_signed_frontier_keeps_concurrent_and_backdated_posts` :2692 | candidate-policy D01 | Over 720 orders there is one outcome: q1, q2 and q3 are kept, never retroactively. Backdated q3 has the same signed shape as honest q2 (the D02 limitation). q4, which cites the removal, is refused unchanged. In the world, honest hosts refuse the removed member's link, so her honest offline post **never uploads**, although H1 accepts it when handed it directly. |
| `posts_around_third_flag_removal_wins_drops_unseen_posts_retroactively` :2742 | candidate-policy D01 | Over 720 orders there is one outcome: only q1 (the post a flag saw) is kept. Honest q2 is dropped too, and some orders show q2 and then drop it. In a replayed order the board is rebuilt on real Loro. q3 is `RemovedConcurrent` and q4 `NotMember`, both unchanged. |
| `rejoin_after_removal_allowed_vs_forbidden` :2785 | candidate-policy G-AUTHORITY | Under `Allowed`, a readmission with a new grant that lifts the 3 flags gives one outcome over 120 orders: carol is a member again, her post is kept, the old grant stays revoked and the new grant is live. Re-presenting the old grant is `GrantRevoked`. Under `Forbidden`, the readmission is `NotAllowed` (unchanged), and her post citing it stays pending forever. Under both, a plain Admit of a removed DID is `Removed` (unchanged). |
| `admins_flagging_admins_immune_vs_flaggable` :2892 | candidate-policy G-AUTHORITY | Under `Immune`, flags are `NotFlaggable` (unchanged). Under `Flaggable`, alice is removed on both devices. Her later flag is `IssuerRemoved` and her post `NotMember`, both unchanged. Her earlier flag on erin still counts toward removing erin. A flag she signs with a stale frontier is still applied. |
| `host_revocation_rotates_the_group_and_revoked_host_reaches_nobody` :2965 | invariant | See section 3. |
| `host_revocation_held_content_and_unreached_member_keeps_using_revoked_host` :3047 | limitation | H3 keeps pre-revocation posts. Erin, unreached, still dials H3, and her new post goes only to H3: it never reaches another device and is not re-sent later. Once H1 reaches her, she rotates and H3's address for her is dead. |
| `undetected_rogue_host_still_learns_every_new_endpoint_after_group_rotation` :3088 | limitation | After the rotation, permitted host H2 holds every member's new endpoint and the new posts. Rotation excludes only revoked hosts. |

The run recorded in the report used:

```
CARGO_TARGET_DIR=/home/abe/p2p_chat/.claude/worktree/endpoint-rotation-blocking/target python3 scripts/buildlock.py --who rotation-W2-F -- cargo test -p group-sim (design-models branch)
```

## 3. Section 10 Q8 in detail (verified, test :2965)

1. There are three hosts. The owner signs `RemoveHost(H3)` and gives it to H1.
2. H1 rotates and pushes the op to every member at the member's current endpoint.
3. H2 learns of the op by host sync and also rotates.
4. Every member and the two remaining hosts end with note seq 2: each rotated exactly once.
5. After revocation, every `ep-note` frame goes to H1 or H2.

Then all of these hold:

- All 9 addresses H3 knew are `EndpointGone`: 7 member devices plus 2 hosts.
- H3 receives **no frame at all** after the revocation.
- H3's received bytes contain no current endpoint.
- Members refuse H3's record (`Revoked`), and links in both directions fail. Member state is
  unchanged.
- Host sync with H3 is refused.
- A new post converges on all honest devices and never reaches H3.

The three limits named in Q8 are shown by tests :2965 (held content), :3047 (an unreached
member) and :3088 (an undetected rogue).

## 4. Findings

1. **"Before threshold" withdrawal is retroactive** (test :2454). Withdrawal can only be
   judged at the withdrawal's own signed frontier. So a withdrawal concurrent with the 3rd
   flag reverses a removal that some replicas, and hosts, already applied. Those hosts have
   already dropped the member's endpoint and must relearn it.
2. **Receiver-clock expiry diverges permanently** (test :2535), the same failure as D04.
   Signed-window expiry is deterministic, but the time is signer-chosen.
3. **Removing the owner does not invalidate what the owner issued** (test :2390). Grant
   chains root at the genesis creator key. "Owner removed" therefore needs an explicit rule:
   cascade, ownership transfer, or "owner cannot be removed".
4. **Under U7, signed frontier alone does not keep honest offline posts** (test :2692).
   Honest hosts refuse a removed member's link, so the member's queued pre-removal posts
   never upload. Keeping them would need a host path that accepts uploads from removed
   members when the post's frontier predates the removal. That path is not designed.
5. **Posts sent to a revoked host before the sender learns of the revocation are stranded**
   (test :3047). Members do not re-send their own posts to other hosts.
6. **Stale-frontier ops** (test :2892). A removed admin can still sign ops citing an old
   frontier, and they are accepted. This is the D02 backdating limitation, applied to ops.
7. **Forbidden rejoin** (test :2785). A post citing a refused op waits pending forever and
   occupies a bounded pending slot. This is the same question as G-H Q2.
8. **Removal-wins sub-option** (test :2742). "Kept if *some* flag on the author saw the post"
   is one choice; "every counted flag saw it" is another. Neither is designed.
9. **Posts the policy rejects are not retained.** Under `RemovalWins`, combining it with an
   un-removal (withdrawal or rejoin) would make the outcome depend on arrival order. This
   combination is not tested.

## 5. Proposed (not decided; the tests only exercise these)

- **Op shape.** The flag/withdraw/admit/remove-host ops carry a signed op frontier, and each
  op is authorized at that frontier.
- **Derived membership.** Membership, removal and grant revocation are derived as a pure
  function of the verified op set.
- **Removal notice.** The notice is the signed flags plus their causal frontier, returned on
  every dial of an honest host.
- **Rejoin.** A rejoin is an `Admit` that names the flags it lifts and must cite them.
- **Rotation trigger.** Group-level rotation is triggered by newly applying a `RemoveHost`.
  The remaining hosts deliver the revocation through their own post-rotation reconnect.

## 6. Open questions for the user (section 8, plus new ones)

1. **Distinct admins (Q1, G-AUTHORITY):** must the flags come from 3 distinct admin DIDs, or
   do any 3 flags count? Note that a DID's two devices count as one admin.
2. **Small groups (Q2):** with fewer than 3 admins, which rule applies: owner alone,
   threshold = admin count, or no removal? Does the owner count as one of the admins? Is the
   admin count fixed at the time of the flags, or does it change as admins are added and
   removed (a D03-style concurrency question)?
3. **The owner (Q3):** if the owner can be removed, what happens to the grants and host
   permits the owner issued (finding 3)?
4. **Withdrawal (Q4a):** if flags can be withdrawn, is a concurrent withdrawal allowed to
   reverse a removal already shown (finding 1)? Or must withdrawals be refused once any
   device could have reached the threshold, which needs an agreement round?
5. **Expiry (Q4b):** should flags expire at all? If so, signed claimed time (backdatable) or
   receiver clock (divergent)?
6. **Threshold (Q5):** fixed at 3, or a per-group setting? (The model uses a constant.)
7. **Rejoin (Q7):** should rejoin be allowed? If yes, who may readmit (any admin, or only
   flaggers)? Must the readmission lift every effective flag?
8. **Posts around the 3rd flag (Q8 = D01/D02):** signed frontier (keeps backdated posts) or
   removal wins (drops honest concurrent posts, retroactively)? Under U7, should a host
   accept a removed member's upload of pre-removal posts (finding 4)?
9. **Admins flagging admins (Q9):** may admins flag other admins? If an admin is removed,
   does that admin's earlier flag still count? Are stale-frontier ops by that admin accepted
   (finding 6)?
10. **Posts stranded at a revoked host (new):** should a member re-send its recent posts to
    the remaining hosts after learning of a host revocation (finding 5)?
11. **Rotation trigger scope (new):** is "rotate once on applying any `RemoveHost`" the
    intended trigger? Should a U9 member removal also trigger a group rotation? (A removed
    member never held member endpoints, so the model does not rotate for it.)
