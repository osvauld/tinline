# Group chat in core (G1, first vertical slice)

Code: `crates/core/src/group/` (`record.rs`, `doc.rs`, `state.rs`, `store.rs`, `wire.rs`,
`engine.rs`, `api.rs`). Acceptance tests on real nodes: `crates/core/tests/group_core.rs`.
Decisions: `endpoint-rotation-blocking:docs/design/rotation-open-decisions.md` §0 (U2, U4 revised,
U7, U9/U15, U10, U11, U12, U14, U16, multiple hosts §9 Q5). Choices marked **PROVISIONAL** were
made by the coordinator's rule ("most conservative option that keeps the feature working") while
the user was away; each is listed at the end.

## Model

- **Identity in content = DID only (U4 revised).** Every group op is one `Record` signed by the
  author's DID key. It carries the author's per-install random device ID `dev` (U16), never a
  device or transport key. The record's Loro peer id must be `writer_id(author, dev)` (the same
  function 1:1 chat uses, `chat::doc::writer_id`).
- **Record kinds** (`record::Body`): `Genesis{name, nonce, owner_name}` (owner; the group id is
  `BLAKE3(domain ‖ owner DID ‖ nonce)`), `Join{invite, name}`, `Leave`, `Post{text, reply_to}`,
  `Admin{member, admin}` (owner only), `Flag{member}` (admins), `HostPermit{host, host_dev,
  active}` (owner/admins), `HostAccept{active}` (the host device itself; hosting is opt-in).
- **Signed frontier (U11).** Every record carries `seen`: the author's version vector when it
  wrote the record. "Was X before Y" is decided by whether Y's `seen` includes X's oplog position,
  never by clocks or arrival order.
- **Doc.** One Loro doc per group, root map `r`: `record id → SignedRecord bytes` (binary value;
  id = first 128 bits of BLAKE3 of the bytes, so the key binds content and signature). It is a
  grow-only set; Loro supplies version vectors, incremental export and snapshots.
- **Acceptance (U10, U22 style).** Every device (hosts too) imports a batch only if every new op
  is an insert into `r` whose key is the id of its value, whose value is a canonical record of
  this group verifying under its author's DID, written under that author's writer ID. Deletes,
  other containers, nested containers, wrong key or peer: the whole batch is refused. The verdict
  depends only on the batch and the doc (`doc::GroupDoc::apply_remote`).
- **Meaning is derived (`state::derive`)**, a pure function of the record set and positions, so
  every device holding the same records shows the same group in every delivery order:
  - admins = owner + members the owner made admin (latest `Admin` per member by `(at, id)`);
    "admin as of a frontier" counts only the owner's `Admin` records inside it;
  - members = owner + DIDs whose latest join/leave is a `Join` with an invite that verifies,
    names this group and whose issuer was owner/admin as of the join's frontier;
  - flags count if the flagger was admin as of the flag's frontier; any 3 counted flags remove
    the member (U9/U15; the same admin may flag 3 times);
  - a post shows if its author was a member as of its frontier and fewer than 3 counted flags on
    the author lie inside its frontier (U11 option A). A shown post by a since-removed member
    carries `before_removal`;
  - a device hosts when its latest admin `HostPermit` is active, its own latest `HostAccept` is
    active and its DID is a member.
  - System lines (joined, left, removed, admin granted/revoked, host added/removed) are derived
    from the same records; "removed" is placed at the 3rd counted flag.

## Transport

- **Two endpoints (U2).** Each device runs a second iroh endpoint for groups (key in the group
  store, one per account per install, independent of the private endpoint), ALPN
  `tinline/group/1` (`wire::GROUP_ALPN`). It is bound when the node starts and the account has a
  group, or on create/join, and closed with the private endpoint (stop, lock, account switch).
- **Only hosts know member endpoints (U7).** Members dial only hosts. Hosts accept, keep a
  directory `did/dev → member group endpoint`, and dial other hosts like members do. Host
  addresses (`SignedHostAddr`, DID-signed, newest `at` wins) are the only endpoints that travel to
  members, and only for devices that are active hosts in the receiver's own derived state (a
  joiner with no state yet takes the ones in its invite token). Records never contain endpoints.
- **Endpoint proof.** Each side of a connection first sends `EndpointAuth{did, dev, endpoint,
  sig}`; the receiver checks `endpoint` equals the QUIC remote key (QUIC proves possession).
- **Session** (`wire.rs`): member `Hello{auth, group, vv, invite?, hosts}` → host `Welcome{auth,
  vv, hosts}` | `Removed{records, vv}` | `Refused{reason}`; then both sides exchange `Update`
  (Loro export of what the other lacks), `Ack{vv}` and `Hosts{hosts}`. Hosts relay every accepted
  update to every other session at once (live), and catch up a reconnecting member from its vv.
- **Host admission** at `Hello`: refused if this device is not an active host of the group;
  `Removed` if the DID is removed; `Welcome` if it is a member, or if it shows an unexpired invite
  issued by a current admin (a joiner may then send only its own records until its `Join` makes
  it a member). The host stores the member device's group endpoint on `Welcome`.
- **Multi-host.** Any number of hosts; each holds the full doc; hosts dial each other; members
  dial every active host they know; everything a host or member receives it relays, verified, so
  two members on different hosts converge (`members_on_different_hosts_converge`).
- **No per-recipient encryption (U14).** QUIC hop by hop; at rest the group store
  (`group.redb`) seals every row (AES-GCM, `storage::Sealed`) under
  `BLAKE3-derive-key("tinline/group-store/v1", recovery phrase)`, like the chat store.

## Removal (U9, U11, U12 "refusal is immediate")

1. Admins flag (`group_flag`). When a host derives that a member reached 3 counted flags, it
   first relays the records (so the member's own replica holds the flags), then sends that
   member's sessions `Removed{proof, vv}` and closes them. `proof` = genesis + the owner's admin
   grants of the flaggers + the counted flags, all signed; the member's app checks them
   (`engine::proof_removes`) and shows "removed" with who flagged and when (`GroupDetail.removal`).
2. From then on the host answers that DID's every `Hello` with `Removed` and sends it nothing
   new. It accepts one upload per connection of that DID's own **posts whose signed frontier
   predates the removal** (U11 option A), refusing the batch otherwise
   (`the_host_refuses_a_removed_member_that_ignores_its_removal`,
   `an_offline_post_written_before_the_removal_is_delivered`).
3. The removed member's app disables posting (`group_post` → `Rejected("removed")`), stops
   dialing once a host acknowledged its last own op, and keeps its history (read-only).
4. Group-level endpoint rotation 2 days after a removal (U12) is **not built** (see "Left").

## Devices (stage 3)

Membership is per DID. A linked device (same DID, its own device ID, writer ID and group endpoint)
joins with `group_device_token` from a sibling: the token names the group and its hosts and has
no invite; the host admits it because the DID is already a member. Every device writes under its
own writer ID; receivers recompute it from `(author, dev)`.

## API (UniFFI, `api.rs`)

`set_group_events(GroupEvents{on_group_message, on_group_changed})`, `groups() ->
[GroupSummary]`, `group_detail(id) -> GroupDetail` (members with roles/flags/devices/invited-by,
hosts with online state, host offers, my role, `am_host`, my removal), `group_messages(id, before,
limit) -> GroupPage` (oldest first, cursor `older`), `group_mark_read`, `group_create(name)`,
`group_invite(id)` (admins; 7-day signed invite + hosts), `group_device_token(id)`,
`group_peek_invite(token)`, `group_join(token)` (blocks until joined, `Rejected(reason)` on
refusal), `group_post(id, text, reply_to)`, `group_flag(id, did)`, `group_set_admin(id, did,
admin)` (owner), `group_leave(id)`, `group_appoint_host(id, did, device_id)` /
`group_revoke_host(...)` (admins), `group_accept_host(id, accept)` (this device),
`group_device_id()`, `group_endpoint_id()`. Test hooks: `group_known_endpoints_for_test`,
`group_directory_for_test`, `group_dial_only_for_test`, `group_ignore_removal_for_test`.

Delivery of our own posts: `Waiting` until some host acknowledged holding it ("Waiting for a
host"), then `Delivered`; on a host device our posts are `Delivered` at once.

## PROVISIONAL (coordinator) choices

1. The creator's device is the group's first host by default (`group_create` writes a permit
   and acceptance for itself). Apps on phones should appoint a desktop and revoke the phone.
2. Only the owner and admins issue invites; invites are bearer tokens valid 7 days, not bound to
   an invitee; the join's validity is judged against the issuer's role as of the join's frontier.
3. The owner cannot be flagged out (flags on the owner never count, §11 Q3) and cannot leave.
4. A removed admin's earlier flags still count; mutual removal removes both (§11 Q9).
5. Removal is final in this slice (no readmission op yet, §11 Q7).
6. Concurrent admin grant/revoke or host permit/revoke resolve by latest `(author clock, id)`.
7. A removed member's proof check accepts flags by the owner or by DIDs the owner made admin
   (any time); the full "admin as of the flag's frontier" rule applies once the flags are in the
   member's own doc (they are relayed before the removal notice).
8. Inert records (a non-admin's flag, a stranger's post) are kept, not refused (§6 Q10).
9. Membership is per DID; a member's other device joins with a device token (no own-device sync of
   group lists yet).
10. A removed member's upload of pre-removal posts is accepted without a count cap until hosts
    rotate (U15); with no rotation built yet, that window is open-ended.

## Left (later slices)

- Group endpoint rotation: hosts announce and rotate 2 days after a removal (U12) and the group
  rotates after a host revocation (§10 Q8); member rotation notes to hosts (U1). The store keeps
  the endpoint key per account, so rotation = new key + signed `HostAddr`/note; hosts already keep
  the member directory needed to reconnect members (§9 Q6).
- Chat requests through the host (U3): an E2E-encrypted request relayed by the host; nothing of
  it exists yet.
- Group calls (U21), files/voice in groups, edits/deletes of posts, mute, group rename/photo,
  "who can invite" setting, readmission after removal, host-side limits (max hosts, upload caps).
- Own-device sync of the group list (today a sibling joins with `group_device_token`).
- The doc is one Loro doc per group (not day-sharded like 1:1 chat); a large group's full sync is
  one batch. Sharding/compaction like chat is needed before big groups.
- Writer counters after a lost `group.redb` (P10's concern): a device never writes into a group
  before it holds the host's copy (joins write `Join` only after the first sync; posting needs a
  derived membership), and Loro continues our writer's counter from what the host holds, so
  counters are not reused. A post written offline right after losing the store, before any sync,
  is impossible today because the group is gone with the store. If chat's P10 rotation gives the
  install a new device ID, groups simply see a new writer of the same DID.
- `blocking_acceptance.rs::personal_block_does_not_change_group_membership` (P5's file) still
  uses stub group helpers whose shape (`add_group_member(host, group, did)`) does not fit the
  invite model; it stays ignored. Its intent is covered by design: blocks never touch group state.
- The Android/desktop apps are not wired (G1 only provides the API).
