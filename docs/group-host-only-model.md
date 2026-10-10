Status: proposal — not approved

# Host-only group model with DID-signed content (W2-H)

Packet W2-H. It implements user decisions **U4 revised** and **U7**
(`docs/design/rotation-open-decisions.md`, section 0), in simulation only. No production code
was changed. Model and tests: `crates/group-sim/src/host_only.rs`, wired in with one `mod` line
at `crates/group-sim/src/tests.rs:23-24`. Line numbers below refer to the W2-H commit.

The model sits alongside the existing group-sim fixtures and does not edit them. Those
fixtures still encode the superseded design: device-key-signed envelopes with attestations, and
members that know each other's endpoints (section 4).

## 1. What the model is

- **Content (U4 revised).** A `Post` (`host_only.rs:108-160`) carries exactly these fields:
  `group`, `doc`, `author` (the DID), `writer`, `seq`, `text`, `update` (Loro). It is signed by
  the DID key through `Identity::sign`, with real Ed25519, and verified with
  `public_key_from_did`.
  - There is no device key, transport key, attestation or endpoint field.
  - `writer` is a random per-device ID. It is used as the Loro peer ID and as the per-device
    sequence space. It is drawn from a seeded SplitMix64 stream (`SimRng`, `host_only.rs:76-101`)
    and is never derived from a key.
- **Validation.** Every replica runs the same checks: the host and every member
  (`Board::receive`/`check`, `host_only.rs:253-325`). The checks are:
  - group/doc, roster membership, bounds, and the DID signature;
  - duplicate and pending handling;
  - writer → DID binding (first accept wins at that replica) and the frozen set;
  - same `(writer, seq)` fork;
  - Loro blob peers must equal `writer`;
  - a scratch import must match the expected insert-only map.

  Members re-verify everything the host relays, so the host cannot alter content.
- **Topology (U7).**
  - Only the host holds member group endpoints: `World::directory`, keyed by DID plus a random
    per-device route slot (`host_only.rs:752-760`).
  - A member's whole group address book is the host's DID-signed note
    (`Device::host_note`, `host_only.rs:664`). No field can hold another member's group
    endpoint.
  - All group traffic flows member → host → members.
  - Reconnect follows U1's rule that "the side whose address is still live retries": the member
    dials the host, otherwise the host dials the member (`World::reconnect`,
    `host_only.rs:1073`).
- **Endpoints.**
  - Endpoints are fresh random fixture keys: `grp-<hex>` for group, `prv-<hex>` for private.
    Each rotation draws a new one, and no future endpoint is derived from anything.
  - A member's group rotation note goes to the host only (`host_only.rs:1109`).
  - A host rotation is pushed by the host to every member device it can reach
    (`host_only.rs:1138`).
  - Private notes go directly to unblocked contacts (`host_only.rs:1169`).
- **Chat request (U3).**
  - The sealing construction is copied from W2-Q (`chat_request.rs` @e0a5ce3) and trimmed
    (`host_only.rs:597`). It is a simulation construction, not an approved scheme.
  - The request carries no endpoint. The DID-signed acceptance carries the target's private
    endpoint, sealed to the requester.
  - The requester's private endpoint reaches the target only in the DID-signed direct hello.
  - The host routes by header only (`host_only.rs:1265`).
- **Idealisations.**
  - Host push picks the missing posts "as if a VV exchange had completed", as `scheduler.rs`
    does.
  - Join and invite bootstrap is a fixture.
  - Time is fake `u64`. Everything is in memory.
  - One static X25519 request key per DID. W2-Q's request-key rotation is not re-modelled.

## 2. Verified in tests (`cargo test -p group-sim`, 16 new tests)

| Test (`crates/group-sim/src/host_only.rs`) | KIND | What it shows |
|---|---|---|
| `did_signed_posts_with_random_writer_ids_two_own_devices_converge` :1412 | invariant | Two devices of one DID write concurrently under distinct random writer IDs (each equal to its Loro peer ID). Every replica converges. The post JSON has exactly the 7 body fields plus `sig`, and verifies against the DID key alone. Writer IDs change with the RNG seed while the identities stay fixed. |
| `members_receive_no_device_key_transport_key_or_endpoint_of_any_other_member` :1509 | invariant | A scan of every byte each member received, plus its replica state. It finds no identity device key, and no group or private transport key of any other device, including sibling devices and rotated-out keys. Positive controls: each member holds the host's current endpoint, and the host's bytes contain every member's endpoint. |
| `blocked_member_learns_no_private_endpoint_from_did_signed_group_content` :1561 | invariant | Re-run of the W2-G leak scenario (`block_in_group.rs:753`). Bob blocks Carol and rotates his private endpoint twice. Carol learns none of Bob's private endpoints, nor his group endpoint or device key, and her `dial` to Bob returns `NoAddress`. Alice has Bob's latest private endpoint. |
| `member_to_member_delivery_has_no_address_by_construction` :1615 | invariant | Every member-to-member dial is `NoAddress`. Members reach the host and the host reaches each device. Every frame has the host at one end. |
| `member_rotation_note_goes_to_host_only_and_host_routes_to_new_endpoint` :1650 | invariant | The rotation note is one frame, sent to the host. The host delivers to the new endpoint, and the old one has no listener. Stale, replayed, forged, tampered, private-audience, wrong-group, non-member and malformed notes leave the host state byte-identical. No member sees either endpoint. |
| `host_rotation_reaches_online_members_directly_and_offline_member_by_host_retry` :1733 | invariant | Online members get the host note pushed to them. The offline member's dial then fails (`EndpointGone`), but the host's retry to her still-live endpoint recovers her (`HostDialedMember`). Members refuse forged, wrong-owner and stale host notes with state unchanged. |
| `host_and_member_both_rotated_while_apart_lose_each_other` :1786 | limitation | With no member forwarding (U7), a member and the host that both rotated while apart cannot reach each other in either direction. The result is `Lost`, and only a manual re-share fixes it. |
| `host_overlap_window_candidate_recovers_member_only_within_window` :1811 | candidate-policy G-TIME | The host keeps its old endpoint listening for N ticks. A member that returns within N recovers; one that returns after N is lost, the same as with immediate retirement. |
| `host_offline_stalls_group_writes_queue_and_converge_on_return` :1847 | limitation | While the host is offline, no group delivery happens at all and boards stay unchanged. Local writes queue, and everything converges when the host returns. |
| `offline_queue_bound_candidates_unbounded_vs_refuse_new` :1887 | candidate-policy G-STORAGE | Unbounded keeps every write. `Refuse(2)` refuses the third write locally, leaving the writer doc, outbox and board unchanged. |
| `forged_tampered_wrong_group_and_replayed_posts_leave_state_unchanged` :1953 | invariant | Tested on a populated host board, each as a single cause: forged DID signature, tampered text, wrong group, wrong doc, non-member, seq 0, ops under another writer, and text that does not match the Loro op. Each is rejected byte-identically. A replay returns Duplicate or Pending with state unchanged. A member rejects a post the host altered. Positive control: the withheld post drains. |
| `loro_same_peer_id_from_two_writers_silently_diverges_without_a_collision_rule` :2025 | limitation | Two Loro docs that share a peer ID import each other's updates with no error and diverge permanently. Loro alone does not detect a collision. |
| `cross_did_writer_collision_first_claim_at_host_wins_and_loser_redraws` :2055 | candidate-policy G-FORK | The writer ID is bound to the first DID the host accepts, and the later DID is refused with host state unchanged. The single host serialises arrival, so members converge to the host's choice in both orders. The loser draws a new writer ID and re-posts. |
| `same_did_writer_collision_reject_later_vs_freeze_writer` :2101 | candidate-policy G-FORK | `RejectLater` leaves the host byte-identical and only the clone redraws. `FreezeWriter` grows only the frozen set, refuses the original device's next post too, and makes both redraw. All converge in both cases. |
| `chat_request_via_host_with_did_signed_content_then_direct` :2164 | invariant | The request is sealed and DID-signed inside, with no endpoint. A tampered, header-rerouted or replayed request is dropped with state unchanged. The acceptance carries the target's private endpoint, and the direct hello uses no host frame. The host and a bystander never see either private endpoint. |
| `host_holds_all_member_endpoints_and_metadata_members_see_writer_ids` :2246 | limitation | The host sees every device endpoint, so it can count each DID's devices, and it sees chat request from/to. Members can count a DID's devices from the writer IDs in content. |

## 3. Proposed (not decided; the tests only exercise these)

- **Where host content comes from.** All group content goes through one host. The tests suggest
  that host serialisation gives the writer-ID collision rule a deterministic winner. That holds
  only while there is exactly one host.
- **Host routing key.** The host's directory is keyed per `(DID, random route slot)`, so it
  needs to know about each device. Keying per DID would collapse two honest devices of one DID,
  the same issue as W2-R Q20.
- **Collision recovery.** On a refused post, a device "redraws its writer ID, rebuilds from its
  validated board and re-posts" (`host_only.rs:1219`). The device learns of the refusal from
  the host's verdict, but the protocol for signalling that refusal is not designed.
- **Host overlap window.** A window that keeps the old host endpoint listening is a candidate
  only. It does not derive any future endpoint.

## 4. Existing group-sim tests that encode superseded assumptions (not edited)

Line numbers are at the W2-H commit. Every test in `tests.rs` is now 2 lines lower because of
the added `mod` line.

**A. Content signed by a device key and carrying an attestation.** The `Envelope` fixture
(`tests.rs:26-56`) has a `device` field, the `peer` is derived from the device key
(`tests.rs:61-67`), and every envelope carries an attestation. This contradicts U4 revised.

Some tests assert the superseded property itself:
- `tests.rs:883` `attestation_must_bind_author_did_and_device_key_each_as_sole_cause`
- `tests.rs:1008` `second_device_cannot_write_under_another_devices_loro_peer_id` (the peer ID
  is derived from the device key)
- `tests.rs:1027` `every_rejection_class_leaves_populated_signed_replica_byte_identical` (it
  includes attestation and device-key cases)
- `block_in_group.rs:706` `blocked_member_never_learns_blockers_private_endpoint_through_group`.
  Its positive controls assert that the blocked member *does* learn B's author key and group
  endpoint.
- `block_in_group.rs:753`
  `author_key_equal_to_private_transport_key_leaks_private_endpoint_to_blocked_member`. This is
  the old-model limitation; its counterpart is re-run here at `host_only.rs:1561`.

Other tests use the device-signed `Envelope` only as a carrier. The property each one tests is
probably transferable to DID-signed posts, but has not been re-verified:
- `tests.rs`: 464, 478, 514, 527, 549, 601, 683, 747, 762, 807, 863, 907, 936, 1088, 1114
- `persistence.rs`: all S01–S07 tests, 656–1378. Its wire codec serialises the attestation,
  `persistence.rs:66-73`.
- `policy_candidates.rs`: all D01–D05 tests, 1004–1734. Its header says "device-signed
  `Envelope`s" (`policy_candidates.rs:11`), and its second-device chains use device keys
  (`policy_candidates.rs:133`).
- `block_in_group.rs`: 477, 513, 530, 564, 592, 617, 642, 672, 772.
- `reachability.rs`: 174, 215, 260, 305, 364, 405. These post `Author` envelopes through the
  mock directory.

**B. Members hold other members' group endpoints, or sync member-to-member.** This contradicts
U7.
- `host_relay.rs`:
  - The `GroupView` of every node holds every member's group note (`host_relay.rs:121-195`,
    with the bootstrap at 231-286). Members forward to each other through `forward`
    (`host_relay.rs:439`).
  - 469 `member_rotation_is_relayed_to_online_members_and_offline_member_catches_up_from_host`
    asserts `known_endpoint(C, A)`.
  - 560 `two_members_rotated_while_apart_recover_each_other_via_host`.
  - 588 `host_rotation_reaches_offline_member_through_verified_member_forwarding`. Under U7,
    this recovery path becomes host retry (`host_only.rs:1733`) or overlap
    (`host_only.rs:1811`).
  - 625 `forged_stale_replayed_wrong_owner_private_and_foreign_notes_leave_host_and_member_unchanged`
    covers notes accepted by members.
  - 703 `personal_block_does_not_stop_group_notes_or_change_group_state` asserts
    `known_endpoint(C, A)` and `known_endpoint(A, C)`.
  - 740 `host_and_all_forwarders_unreachable_means_no_recovery_without_reshare` relies on
    member forwarders.
  - 777, 835 and 878 cover catch-up, fork and clone notes held by members.
- `block_in_group.rs:428-468` (`endpoint_scenario`) relays B's group note to every member,
  C included.
- `scheduler.rs`: every node keeps `known` routes to every other node (`scheduler.rs:9-15`,
  `:104`), and nodes re-push addresses to each other. Affected tests: 327, 450, 478, 503, 547.
- Multi-host or peer-to-peer replica reconciliation, in which members are not funnelled through
  a single host: `tests.rs:568` `two_hosts_partition_then_reconcile_without_rolling_back_local_writes`
  and `tests.rs:844` `independent_posts_and_cross_author_join_survive_host_partition`. These may
  stay relevant if host succession or multi-host is chosen (D01–D06).

**D. "Blocked looks like offline / silent" (superseded by U5 revised, coordinator commit
73406aa).** These are W2-Q and W2-G tests that encode silent blocking. Per U5 revised they
should become candidate-policy G-BLOCK. This packet does not edit them:
- `chat_request.rs:1306` `host_drop_yields_no_answer_never_refused`
- `chat_request.rs:1424` `silent_block_is_indistinguishable_from_offline_or_unanswered_target`
- `chat_request.rs:1468` `candidate_silent_decline_is_indistinguishable_from_block`
- `block_in_group.rs:617` `blocker_host_candidate_neutral_relay_converges_and_is_silent`, which
  asserts that the block is silent.

W2-H itself does not model blocking on the chat path; how the notice works is open (decisions
section 8b). Its only block-related test (`host_only.rs:1561`) relies on the part of U5 that is
unchanged: a blocked contact is excluded from later private rotation notes. It makes no claim
about silence.

**C. Not superseded, as far as this packet checked.**
- `authority.rs` (genesis and grants; content-signing independent).
- `chat_request.rs` is DID-signed and hosted as a mailbox. Its request-key directory is
  compatible with U7.

## 5. Open questions for the user

1. **Writer-ID collision, cross-DID (G-FORK).** Should the host's first claim win, with the
   loser redrawing? Should the host tell the loser explicitly, or should the loser infer it
   because the post is never echoed back?
2. **Writer-ID collision, same DID (G-FORK).** The candidates are `RejectLater` and
   `FreezeWriter`. Freeze lets any phrase holder stall a sibling's writer, the same as W2-R Q19.
3. **Writer IDs reveal device count to members.** Members can count each DID's devices and see
   per-device activity. Accept this, or hide it? One option is a fresh writer ID per epoch or
   doc, which needs a rule, and this packet invents none. U7 says members never learn device
   IDs, and a writer ID is a per-device pseudonymous ID.
4. **Host routing key.** Should the directory be per device (the random slot used here) or per
   DID? This is the same as W2-R Q20. Per device means the host sees each device's rotations.
5. **Host succession or backup host** (D01–D06). With one host, a host outage stalls the group
   (`host_only.rs:1847`). Both sides rotating while apart loses the member (`host_only.rs:1786`).
6. **Host endpoint overlap window (G-TIME).** Should there be one, and how long? Without it,
   recovery depends on the member's endpoint still being live.
7. **Offline queue bound (G-STORAGE).** The options are unbounded, refuse new, or drop oldest,
   and the value is open. What does the UI show on a refusal?
8. **Request/encryption key for chat requests.** Is it a per-DID key published DID-signed
   through the host, as here, or W2-Q's rotating request keys? It is neither a device key nor a
   transport key, so U4 revised allows it. Confirm.
9. **Linked-device chat state.** An acceptance reaches every device of the requester's DID, but
   only the requesting device can use it. Should siblings sync outgoing requests?
10. **Host-visible metadata.** The host sees every device endpoint, rotations, and who
    requests a chat with whom. Is this acceptable for launch? (This extends W2-Q Q16.)
