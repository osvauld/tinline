Status: proposal — not approved

# Multi-host group model with host permits and host-initiated reconnect (W2-M)

Packet W2-M. This is a simulation of the user decisions in
`docs/design/rotation-open-decisions.md`: U4 revised, U5 revised, U7 and U9 (section 0), plus
section 9 Q5 (multiple hosts and host details) and Q6 (the host reconnects members itself). No
production code was changed.

- Model and tests: `crates/group-sim/src/multi_host.rs`. It is wired in at
  `crates/group-sim/src/tests.rs:25-26`.
- Line numbers below refer to the W2-M commit.
- The model builds on `host_only.rs` (W2-H). `SimRng`, `Post`, `Writer`, `Board` and `EpNote`
  are copied from there with a provenance comment. The grant chain is copied from
  `authority.rs` (`multi_host.rs:1-30`).
- Neither source file was edited.

## 1. What the model is

- **Host permit.** A host permit is a real Ed25519 grant in the delegation chain
  (`MGrant`, `multi_host.rs:135-225`).
  - The chain runs creator → admin grant (`Admin` + `Delegate`, delegable) → leaf.
  - The leaf has `caps = {Host}`, `holder =` the host's DID, and `device = Some(route slot)`.
    The route slot is the one nominated hosting device.
  - The chain walk is the one from `authority.rs`, with one addition: a device-bound grant can
    never be a parent (`multi_host.rs:182`).
  - A host presents a `HostRecord` (`multi_host.rs:543`), which is its DID-signed group note
    plus the permit.
  - Every member and host checks the record (`check_record`, `multi_host.rs:1007`). The note
    signature must be valid and the audience must be this group. The permit must allow
    `Host` for the note's owner, its `device` must equal the note's slot, and the permit must
    not be revoked.
- **Device IDs.** Route slots are random draws from the seeded RNG. They are never derived
  from a key. They are not cryptographic, so see the limitation in section 2.
- **Topology (U7 + Q5).**
  - Every member holds every accepted host record (`View::hosts`, `multi_host.rs:631`).
  - Each host holds a directory of member device notes (`HostState`, `multi_host.rs:694`).
  - Members hold no other member's endpoint.
  - A member/host link (`connect`, `multi_host.rs:1250`) works like this: the member dials
    the host if it can; otherwise the host dials the member's directory endpoint. Then the
    member admits the host's current record, and the two exchange data (`exchange`,
    `multi_host.rs:1327`).
  - Hosts sync with each other (`host_sync`/`xfer`, `multi_host.rs:1409-1530`). They
    exchange host records, ops, directory notes and posts. Post selection is idealised "as if
    a VV exchange had completed", the same as in `host_only.rs`.
- **Group ops.**
  - `Admit`, `Flag` and `RemoveHost` (`multi_host.rs:556`) are signed by an admin's DID and
    carry the admin's grant.
  - Every device verifies them before any mutation (`accept_op`, `multi_host.rs:1085`).
  - U9 removal needs 3 flags from distinct admins. The threshold is a constant that encodes
    the section 8 lean; it is not decided.
  - An honest host that applies a removal drops the member's directory entries. It answers
    the removed member's next connection with the signed flags (`removal_notice`,
    `multi_host.rs:1312`).
- **Rotation.**
  - A member rotation note goes to every host the member can reach (`rotate_member`,
    `multi_host.rs:1532`). Hosts that missed it get it during host sync.
  - When a host rotates, its old endpoint is retired immediately; there is no overlap window.
    The host then dials every member at the member's current directory endpoint and hands
    over its new record (`rotate_host`, `multi_host.rs:1576`).
- **Visible block (U5 revised and 8b Q1).** The blocker sends a DID-signed `BlockNotice` on
  the private path, and answers any later direct attempt with the same bytes (`block`/`direct`,
  `multi_host.rs:1636-1690`).
- **Open policy is parameterised in `Cfg`** (`multi_host.rs:644-690`):
  - `ClaimRule` (G-FORK);
  - `Retention` (G-TIME);
  - `RemovalRule` and `HostCap` (G-AUTHORITY);
  - a `rogue` host set, which is a limitation fixture.
- **Idealisations.**
  - Join and invite bootstrap is a fixture.
  - Host records are distributed by `publish_record`, a fixture that stands in for relay
    through existing hosts.
  - Appointment order uses grant nonces as a stand-in for an ordered op log.
  - Time is fake `u64` and everything is in memory.

## 2. Verified in tests (`cargo test -p group-sim`: 147 passed = 129 baseline + 18 new)

| Test (`crates/group-sim/src/multi_host.rs`) | KIND | What it shows |
|---|---|---|
| `host_permit_must_chain_to_an_admin_and_bind_the_device_else_members_refuse` :1724 | invariant | Every member and host refuses each of eight single-cause bad records, and its full device state stays byte-identical. The eight cases are: a member grant used as a permit, a permit issued by a non-admin (under her member grant, or self-rooted), no permit, a tampered device, a permit for another device, a note signed by a non-holder, and a permit lacking `Host`. Positive control: the same host with a valid permit from alice is accepted everywhere and relays. After bob signs `RemoveHost`, the record is refused as `Revoked`, its links are refused, and state is unchanged. |
| `host_without_admin_capability_relays_but_cannot_grant_flag_remove_or_appoint` :1918 | invariant | Hugo's host binary has its own DID and a `{Host}` permit. Admit, flag and remove-host ops it signs are refused as `NotAdmin`. A host appointment it signs is refused as `PermitInvalid`. A grant it issues lets nobody post. State is unchanged at every member and at H1. It still relays to members cut off from H1. Positive control: the owner's admit of the same newcomer is accepted. |
| `host_permit_binds_one_device_and_a_sibling_of_the_same_did_is_not_a_host` :2013 | invariant | Hana's phone shares a DID with her desktop host H1. If it honestly presents H1's permit with its own device ID, the record is refused (`DeviceMismatch`). A permit hana issues to herself is refused (`PermitInvalid`). State is unchanged, and the phone stays an ordinary member whose posts converge. Device IDs are not any key chunk. |
| `phrase_sharing_sibling_can_claim_the_host_device_id` :2088 | limitation | The phone signs a higher-seq note that claims the desktop's device ID, with the phone's own endpoint and H1's real permit. Members accept it and dial the phone as the host. Device binding is not cryptographic under a shared mnemonic. |
| `post_via_one_host_reaches_member_served_only_by_another_host` :2116 | invariant | Alice can reach only H1 and carol only H2. Carol lacks alice's post until H1 and H2 sync, and then gets it. Every post frame carol received came from H2. Everyone converges. |
| `two_hosts_partitioned_then_healed_converge_without_rollback` :2143 | invariant | Two partitioned sides each converge internally and differ from each other. After healing, everyone has all 4 posts and no local write is lost. |
| `member_rotation_note_reaches_every_host_and_hosts_repair_each_other` :2186 | invariant | The note goes out as one frame per host. With H2 cut, only H1 gets it, and H2's dial to carol fails. A tampered relayed note or a stale note leaves H2 byte-identical. After host sync, H2 holds the new endpoint and dials carol. No member ever sees carol's new endpoint. |
| `rotated_host_reconnects_members_at_their_current_endpoints_without_overlap` :2237 | invariant | The old H1 endpoint stops listening at once. H1 pushes its new record to each online member at that member's current endpoint, including carol, who had rotated earlier. H2 learns it too. Bob was offline and cut from H2; he is recovered by H1 dialing him (`HostDialedMember`). Forged and stale H1 records leave state unchanged. |
| `host_and_member_both_rotated_while_apart_recover_via_another_host` :2297 | invariant | Carol and H1 are `Lost` to each other. Carol reaches H2, uploads her new note and learns H1's new record. After host sync, H1 dials carol, and carol dials H1. Everyone converges. |
| `single_host_both_rotated_retain_old_endpoint_until_ack_vs_no_retention` :2325 | candidate-policy G-TIME | With one host: under `None`, the result is `Lost` both ways, so a re-share is needed. Under `UntilHostAck`, the old endpoint keeps listening, H1 dials it, the ack releases it, and links resume on the new endpoints. |
| `writer_collision_first_claim_at_each_host_diverges_across_hosts` :2385 | limitation | Each partitioned host binds the shared writer ID to a different DID. After healing they refuse each other's post, so the hosts and their members diverge permanently. |
| `writer_collision_lowest_claim_after_all_hosts_saw_converges` :2413 | candidate-policy G-FORK | Hosts hold a new writer's posts until every host has seen every claim for it, and then the lowest claim wins. Nothing is delivered during the partition. The same DID wins in both placements. The loser is told, redraws its writer ID and re-posts, and everyone converges. |
| `removed_host_is_refused_by_members_and_hosts_afterwards` :2456 | invariant | After bob's `RemoveHost` for H2, members refuse H2 in both directions and refuse its record (`Revoked`), with state unchanged. H1 refuses host sync with it. Carol's next rotation note goes only to H1. H2 keeps carol's pre-removal endpoint until she rotates. |
| `host_removal_any_admin_vs_only_admins_appointed_after_host` :2505 | candidate-policy G-AUTHORITY | Alice was appointed before H1's permit; bob was appointed after it. Under `AnyAdmin`, alice's removal is accepted. Under `AdminAppointedAfterHost`, it is refused with state unchanged and bob's is accepted. |
| `member_removal_applied_by_honest_hosts_but_rogue_host_still_serves_removed_member` :2551 | limitation | Three admins flag carol. H1 and every honest member remove her, and H1 refuses her (`Removed`). The rogue H2 keeps her endpoint and keeps feeding her new posts. Her later posts are rejected by every honest replica with state unchanged. |
| `removed_member_and_blocked_contact_receive_verifiable_signed_notices` :2614 | invariant | Block: forged and misaddressed notices leave state unchanged. The genuine notice is recorded. Later direct attempts get the byte-identical notice. No roster changes, and group posts still flow both ways. Removal: forged flags (from a non-admin, or with a tampered target) leave state unchanged. On her next connection, carol receives the 3 signed flags, verifies them and records her removal. A repeat attempt returns the same notice with no change. |
| `host_binary_own_did_vs_phrase_holding_linked_device` :2723 | candidate-policy G-KEYS | An own-DID host's posts are `NotMember` everywhere. A phrase-holding host device can post as hana, and can sign her block notices and endpoint notes, and all of these are accepted. |
| `host_count_cap_candidates_and_endpoint_exposure_scales_with_hosts` :2766 | candidate-policy G-AUTHORITY | `Max(2)` refuses a third valid host with state unchanged; `Unbounded` accepts it. Afterwards, exactly {H1, H2, H3}, and no member, hold carol's new group endpoint. |

## 3. Superseded silent-block tests relabelled (comment-only, no logic change)

These are the tests listed in `docs/group-host-only-model.md` §4.D. Each now reads
`// KIND: candidate-policy G-BLOCK` followed by
`// Rejected option per U5 revised (blocking is visible).`:

- `crates/group-sim/src/chat_request.rs:1307` `host_drop_yields_no_answer_never_refused`.
  It was `invariant`.
- `crates/group-sim/src/chat_request.rs:1426`
  `silent_block_is_indistinguishable_from_offline_or_unanswered_target`. It was `invariant`.
- `crates/group-sim/src/chat_request.rs:1471`
  `candidate_silent_decline_is_indistinguishable_from_block`. It was already G-BLOCK, and the
  comment was added.
- `crates/group-sim/src/block_in_group.rs:618`
  `blocker_host_candidate_neutral_relay_converges_and_is_silent`. It was already G-BLOCK, and
  the comment was added.

The test count of those files is unchanged.

## 4. Proposed (not decided; the tests only exercise these)

- **Permit shape.** The permit is a grant leaf with a `device` field. Device-bound grants are
  never parents. "Host" is a capability that admins hold and can delegate to a non-admin
  device.
- **Host removal.** It is a signed `RemoveHost` op naming the permit digest, and every device
  applies it. "Which admins" is a candidate (section 5, Q3).
- **Cross-host writer-ID rule.** `LowestAfterAllHostsSaw` is one deterministic candidate.
  Every new writer stalls while any host is unreachable. It also has an uncovered race: a
  claim that reaches a host after the other host already settled. Closing that race needs an
  agreement round, which is not designed.
- **Member endpoint retention until host ack.** This is the coordinator proposal from
  section 9 Q6. It is needed only when every host the member knows rotated while it was away.
  With two or more hosts and one unchanged, the model recovers without it (test :2297).
- **Removal notice = the signed flags themselves.** The honest host returns them on every
  attempt. Whether the removed member is told immediately or only on its next attempt is
  8b Q2, and is still open.

## 5. Open questions for the user

1. **Maximum number of hosts (G-AUTHORITY).** Every host holds every member's group endpoint
   and sees delivery metadata, so exposure grows linearly (test :2766). Is there a cap, and
   who enforces it: members refusing extra records, or admins?
2. **Host binary identity (G-KEYS).** Should the host have its own DID plus a permit
   (coordinator lean), or be a phrase-holding linked device? A phrase-holding host can act as
   that user everywhere (test :2723).
3. **Who may remove a host?** "Newer admins can remove": does that mean any admin
   (`AnyAdmin`), or only admins appointed after the host (`AdminAppointedAfterHost`)? Can the
   owner always remove a host? The fixture's "appointed after" rule refuses the owner because
   the owner has no grant; this is unexercised and not intended as a decision. Appointment
   order also needs a real op-log order, which concurrent ops (D01–D05) do not yet define.
4. **Cross-host writer-ID tie-break (G-FORK).** The options are first-claim-per-host, which
   diverges (test :2385), or a deterministic rule such as lowest claim after all hosts saw
   it, which stalls new writers during a partition (test :2413). Another option is to avoid
   collisions by construction, but no scheme is proposed here.
5. **Member old-endpoint retention until host ack (G-TIME).** Should it be adopted, and is it
   needed only with one host? If so, is there a maximum retention time? The old endpoint stays
   reachable to anyone who knew it.
6. **Device binding under a shared mnemonic.** Any phrase-holding sibling can claim the host
   device ID (test :2088). Accept this as a known limitation, or require a separate host key?
   A separate host key would be a transport key, which U4 revised keeps out of content.
7. **Rogue host after U9 removal.** Without group-content encryption, a host that ignores a
   removal keeps serving reads to the removed member (test :2551). Accept for launch, or
   require removing (or not appointing) untrusted hosts? Group re-keying is out of scope here.
8. **Removed host's stale knowledge.** A removed host keeps the member endpoints it learned
   until those members rotate. Should members rotate their group endpoint early after a host
   removal? Today rotation is periodic only, and blocking does not trigger it.
