# group-sim

Test-only first group simulation slice. No production API and no transport/UI wiring.

```sh
python3 scripts/buildlock.py --who group-sim -- cargo test -p group-sim
```

Uses real DID-signed content (`host_only.rs` onward), real Ed25519 signatures and Loro updates.
The older modules (`tests.rs`, `persistence.rs`, `policy_candidates.rs`, `reachability.rs`,
`scheduler.rs`, `block_in_group.rs`) still use the legacy device-key-signed `Envelope` with a
DID/device attestation, superseded by U4 revised + U16; there it is only a carrier for
properties that do not depend on how content is signed (see "Superseded tests" below). Identities and
schedule seeds are deterministic test fixtures (never production secrets). Message keys and
insert-only schemas are provisional; the signing domain explicitly marks simulation data.

Each test carries a `// KIND:` label: `invariant`, `limitation` or `candidate-policy <gate>`.
A limitation test passes by demonstrating a missing guarantee; a candidate-policy test encodes
one option of an open question and selects nothing.

## Replica and authority tests (`src/tests.rs`, `src/authority.rs`)

- **Relaying and order independence.**
  - Original author envelopes survive host relay with their signatures intact.
  - All six permutations of two small traces converge. A duplicate redelivery must return
    `Duplicate`, or `Pending` while the envelope is still waiting.
  - Twenty seeded loss/reordering schedules converge after a complete-set repair. Every
    envelope must be accepted exactly once on both sides, and no valid envelope may be refused
    as `Invalid` or `Limit`.
  - Each seed also has a negative control. One envelope is withheld from both the lossy
    schedule and the repair source. That replica must not converge, and the same author's later
    envelopes must stay pending until the withheld one arrives.
- **Missing dependencies.** An envelope whose previous-chain or cross-author dependency is
  missing stays pending, with nothing accepted, then drains when the dependency arrives.
- **Rejections leave state byte-for-byte unchanged.** These tests use *populated* replicas:
  accepted history plus one pending envelope. Each rejection compares a snapshot taken before
  and after. The snapshot holds the Loro export bytes, the deep value and VV, every accepted
  and pending envelope's signed bytes, signature and attestation, and the fan-out list.
  - **Single-cause cases**, each with a positive control on the same replica:
    - group-only mismatch
    - document-only mismatch
    - text-size bound
    - dependency-count bound
  - The attestation cases and the second-device peer-ID case were removed by R1b (they
    asserted the superseded device-key model); the DID-signed equivalents are in `host_only.rs`.
  - **Rejected, but not as the only cause**, because `apply()` also refuses them: sequence 0
    and an oversized update.
  - **Rejected on a populated signed replica:**
    - tampered text
    - missing, wrong-holder, read-only and other-group permits
    - extra Loro mutation injection
  - **Still checked only on empty replicas** by the older tests: outsider, wrong DID and
    cross-group.
- **Pending bound.** When an envelope is refused with `Limit`, state is unchanged and the
  envelope is not kept. When the withheld dependency arrives, all 16 pending envelopes drain.
  The refused envelope stays absent until it is sent again.
- **Causality.**
  - Cross-author dependencies are explicit signed envelope IDs, with transitive closure.
  - The real Loro prerequisites are checked against that declared closure, not against
    whatever the receiver happens to hold. Omitting a required dependency therefore fails even
    when the receiver already has it. This is asserted on a populated replica, by count.
  - Receivers import the update into a scratch copy and compare the result with the accepted
    insert-only records.
  - Transitive closure is only exercised through `previous` chains.
  - General causal meta/frontier authorization is not implemented.
- **Two-host partition.** Two hosts partition and then recover. A stale peer holding a strict
  subset cannot change the up-to-date host (its snapshot is equal before and after), and that
  host catches the stale peer up. There is no model of refusing a lower VV or requesting a
  replace.
- **Signed genesis and grants.**
  - Checked: the creator-signed genesis hash, version and creator; holder binding; scope;
    capability narrowing; root issuer = creator; and the chain-depth cap (8 links accepted,
    9 refused).
  - Single-cause cases with positive controls: `delegable=false`, issuer ≠ parent holder, and
    a holder DID that does not parse.
  - The grant model is test-only and is not the courier token format. Expiry and revocation
    are deliberately absent, not solved.
- **Limitations shown by tests.**
  - An envelope promoted from pending that then fails `apply()` is dropped without any outcome.
    The caller only sees the outcome of the envelope that released it.
  - Repair in envelope-ID order overflows `MAX_PENDING` on a chain 60 envelopes deep. Before
    G-H, `Replica::sync_from` and `Fleet::reconcile` used that order. They now send in causal
    order, as a VV/frontier exchange would.
  - The receiver never keeps envelopes refused with `Limit`, so retrying is the sender's job.

## Reachability (`src/reachability.rs`): MOCK verified-succession adapter

These tests use a mock adapter, not a rotation verifier.

- **Whole-path install check.** `HostDirectory::install` checks the whole disclosed path:
  - The root must be the nominated device and must have no predecessor.
  - Each later record must name the previous record's device. This is a mock `predecessor`
    field, not a cryptographic link.
  - Every record must have the right owner and the group audience.
- **Paths that are refused.** These leave the directory unchanged:
  - `[nominated, sibling]`
  - a spliced predecessor
  - a private or wrong-group record at any position
  - an owner mismatch in the middle of the path

  An adapter that returns no path installs nothing.
- **Typed connect failures.** `Network::connect` returns one of two reasons:
  - `NoActiveBinding`: no binding is installed, or the current one has retired.
  - `EndpointGone`: the binding is live, but that endpoint/device is not listening.

  This separates "unreachable" from a proof failure, which is `install` returning false.
- **Expired predecessors.** They are kept as history but never used as a fallback, even while
  their endpoint still listens. A positive control routes before retirement.
- **Host device-ID replacement.** Routing and retry keep working across a replacement with no
  new group delegation. "No delegation" is structural: authorization never reads the directory.
- **Offline retirement (limitation).** Even with the old endpoint still listening, the failure
  is `NoActiveBinding`, and nothing discovers the new endpoint.

Mock paths assume that rotation has already verified signatures, ordering, forks and rollback.
Cryptographic succession, omitted-path detection, fork/rollback and proof-size bounds stay in
the rotation contract. Simulated host ID changes do not change author keys or prove writer
migration. Retirement here means routing retirement, not access-grant expiry.

## Five-node scheduler (`src/scheduler.rs`)

The scheduler has five tests. Since R1b the fleet follows U7: nodes 0–2 are members, 3–4 are
hosts; a member holds routes to the hosts only, and any member-to-member send, reconcile or
address push fails with `NoAddress`. All member traffic goes through a host. The first test
asserts this topology at the end of every seed.

1. **Ten seeded 100-event schedules.**
   - Setup: three members and two hosts. Cross-author causal writes, duplication, loss and
     reordering, sleep/wake, partitions, two forced host rotations and control frames.
   - Time stays at or below 13, under `BINDING_LIFETIME` (30), so **no binding retires in this
     test**.
   - Fault counters only count operations that actually happened, and the test asserts that a
     receiver saw a duplicate.
   - The event kind now comes from high LCG bits. The low bits used before G-H cycle with
     period 8, so every seed got the same sequence of event kinds.
2. **Ten seeded schedules to t=120 (four binding lifetimes).**
   - Every node rotates every 10 ticks, 12 times in all.
   - Random sleep, partitions, duplicate/drop/reorder and control frames run throughout.
   - At the end of every 7th tick, nodes 0–3 heal and every node re-pushes its address over
     routes it still knows. Outages are therefore shorter than the 20-tick overlap.
   - Asserted:
     - some sends are refused because a binding retired;
     - nodes 0–3 converge, with no duplicate fan-out and original signatures;
     - host 4, offline for 40 ticks (longer than one lifetime), is **mutually retired**:
       neither side holds a live binding of the other, both directions fail with
       `NoActiveBinding`, and host 4 misses every post made after it went offline.
   - An explicit out-of-band re-bootstrap fixture, not discovery, then restores convergence.
   - Without a real bound on outages, G-H saw online nodes fragment permanently in some seeds.
3. **Queue limit.** The transport queue limit (64) is reached. Further sends are refused and not
   kept, and a later reconcile delivers the rest.
4. **Address update before retry.** A replacement host pushes its address over a route the
   other side still knows, and the message retry then succeeds.
5. **Mutual retirement (limitation).** A queued frame is lost and both directions report
   `NoActiveBinding`. The local write is kept.

The scheduler uses idealized authenticated links. It selects missing envelopes as if a VV
exchange had completed; there is no real handshake or session code. Bootstrap paths are explicit
fixtures, and there is no automatic global directory refresh. Sleep keeps in-memory state, so it
is not a process-crash test.

## Superseded tests (R1b)

R1b brought the tests in line with U4 revised (no device or transport key in content), U5
revised (visible blocking), U7 (only hosts hold members' group endpoints) and U18 (no backward
compatibility). Removed, with where the current-design property is covered:

| Removed test | Covered by |
|---|---|
| `tests.rs` `attestation_must_bind_author_did_and_device_key_each_as_sole_cause` | `host_only.rs` `did_signed_posts_with_random_writer_ids_two_own_devices_converge`, `forged_tampered_wrong_group_and_replayed_posts_leave_state_unchanged` |
| `tests.rs` `second_device_cannot_write_under_another_devices_loro_peer_id` | `host_only.rs` `forged_tampered_wrong_group_and_replayed_posts_leave_state_unchanged` (`LoroMismatch`), `cross_did_writer_collision_first_claim_at_host_wins_and_loser_redraws` |
| `block_in_group.rs` `blocked_member_never_learns_blockers_private_endpoint_through_group` | `host_only.rs` `blocked_member_learns_no_private_endpoint_from_did_signed_group_content`, `members_receive_no_device_key_transport_key_or_endpoint_of_any_other_member` |
| `block_in_group.rs` `author_key_equal_to_private_transport_key_leaks_private_endpoint_to_blocked_member` | `host_only.rs` `blocked_member_learns_no_private_endpoint_from_did_signed_group_content` |
| `block_in_group.rs` `blocker_host_candidate_neutral_relay_converges_and_is_silent` | `block_in_group.rs` `personal_block_leaves_every_replica_grant_and_relayed_byte_unchanged` (host-is-blocker case); `multi_host.rs` `removed_member_and_blocked_contact_receive_verifiable_signed_notices` |
| `chat_request.rs` `silent_block_is_indistinguishable_from_offline_or_unanswered_target`, `candidate_silent_decline_is_indistinguishable_from_block` | `chat_request.rs` `candidate_blocked_requester_gets_sealed_signed_answer_vs_drop` (section 8b Q4 candidates) |
| `host_relay.rs` `member_rotation_is_relayed_to_online_members_and_offline_member_catches_up_from_host`, `two_members_rotated_while_apart_recover_each_other_via_host` | `host_only.rs` `member_rotation_note_goes_to_host_only_and_host_routes_to_new_endpoint`; `multi_host.rs` `member_rotation_note_reaches_every_host_and_hosts_repair_each_other`, `host_and_member_both_rotated_while_apart_recover_via_another_host` |
| `host_relay.rs` `host_rotation_reaches_offline_member_through_verified_member_forwarding` | `host_only.rs` `host_rotation_reaches_online_members_directly_and_offline_member_by_host_retry`, `host_overlap_window_candidate_recovers_member_only_within_window`; `multi_host.rs` `rotated_host_reconnects_members_at_their_current_endpoints_without_overlap` |
| `host_relay.rs` `host_cannot_substitute_endpoint_in_relayed_note` | `multi_host.rs` `member_rotation_note_reaches_every_host_and_hosts_repair_each_other`; forged host notes in `host_only.rs` `host_rotation_reaches_online_members_directly_and_offline_member_by_host_retry` |
| `host_relay.rs` `host_and_all_forwarders_unreachable_means_no_recovery_without_reshare` | `host_only.rs` `host_and_member_both_rotated_while_apart_lose_each_other` |

Rewritten to the current design: `host_relay.rs` (member views accept only the host's notes;
the host relays only its own note; recovery is host retry) for
`private_endpoints_never_appear_in_host_state_or_any_group_relayed_bytes`,
`forged_stale_replayed_wrong_owner_private_and_foreign_notes_leave_host_and_member_unchanged`,
`personal_block_does_not_stop_group_notes_or_change_group_state`,
`catch_up_latest_only_vs_full_history_under_gap_policies` (now: what a member's retry sends the
host), `same_seq_fork_keep_first_diverges_by_arrival_order_freeze_converges_but_blocks_routing`
(now: two hosts) and `mnemonic_clone_with_higher_seq_replaces_honest_group_endpoint_at_the_host`;
all five `scheduler.rs` tests (U7 star topology). Kept unchanged apart from comments: the
legacy-envelope carrier tests, the two-host partition tests in `tests.rs`, and
`chat_request.rs` `host_drop_yields_no_answer_never_refused` (it asserts that a host cannot
forge a refusal, not that blocking is silent).

The proto model of private-audience blocking is now `crates/proto/tests/rotation_visible_block.rs`
(was `rotation_silent_block.rs`).

## Not covered

- Edits and deletes.
- Author-key succession.
- Device forks and collisions.
- Group revocation and ban semantics (D01–D06).
- Durable storage (S01–S07).
- Endpoint discovery.
- File transfers.
- Real host capability proofs.
- OS lifecycle.
- Acks (I7, I8).
- Pull/subscribe.

In-memory host receipts do not establish persistence. Both harnesses repair loss with a
complete-set reconciliation pass, so convergence after that pass is largely by construction. It
is not evidence of an efficient production sync or gossip protocol. See
`docs/group-chat-tests.md` for the larger conceptual plan and `docs/group-sim-coverage-audit.md`
for the claim-by-claim audit.
