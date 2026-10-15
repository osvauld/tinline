# Group chat in core (G1 first slice, G2 second slice, G3 invitations)

Code: `crates/core/src/group/` (`record.rs`, `doc.rs`, `state.rs`, `store.rs`, `wire.rs`,
`engine.rs`, `api.rs`; G2: `own.rs`, `mail.rs`, `requests.rs`; G3: `invitations.rs`, `code.rs`).
Acceptance tests on real nodes: `crates/core/tests/group_core.rs` (G1), `crates/core/tests/group_g2.rs`
(G2) and `crates/core/tests/group_g3.rs` (G3).
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
  active}` (owner/admins), `HostAccept{active}` (the host device itself; hosting is opt-in),
  `EncKey{key}` (G2: the author's encryption public key, derived from the recovery phrase, not a
  network key; written once by each member device that finds its DID's key missing, so others
  can seal chat requests to it).
- **Signed frontier (U11).** Every record carries `seen`: the author's version vector when it
  wrote the record. "Was X before Y" is decided by whether Y's `seen` includes X's oplog position,
  never by clocks or arrival order.
- **Docs (day shards, `group-shards.md`).** Per group one `meta` Loro doc (every record except
  posts) and one doc per UTC day holding that day's `Post` records. Each has root map `r`:
  `record id → SignedRecord bytes` (binary value; id = first 128 bits of BLAKE3 of the bytes, so
  the key binds content and signature) and is a grow-only set; Loro supplies version vectors,
  incremental export and snapshots. A record's `seen` is always the author's **meta** vv, so
  posts need no frontier over other posts.
- **Acceptance (U10, U22 style).** Every device (hosts too) imports a batch only if every new op
  is an insert into `r` whose key is the id of its value, whose value is a canonical record of
  this group verifying under its author's DID, written under that author's writer ID, and
  belonging in this doc: a `Post` only in the day doc of its UTC day (`at` within the day +- 1 h,
  as in 1:1 chat), any other body only in `meta`. Deletes, other containers, nested containers,
  wrong key, peer or doc: the whole batch is refused. The verdict depends only on the batch and
  the doc (`doc::GroupDoc::apply_remote`). Signatures are verified when records enter a device;
  our own sealed store rows are not verified again when loaded.
- **Meaning is derived (`state::derive_meta`, `state::post_visible`)**, pure functions of the
  meta record set and positions, so every device holding the same records shows the same group in
  every delivery order. `derive_meta` runs on meta changes only; each post is judged on its own
  (Visible, Hidden, or Pending when its `seen` names meta records we do not hold yet; pending
  posts are judged again when the meta doc advances):
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
  `tinline/group/3` (`wire::GROUP_ALPN`; /3 = postcard frames). It is bound when the node starts and the account has a
  group, or on create/join, and closed with the private endpoint (stop, lock, account switch).
- **Only hosts know member endpoints (U7).** Members dial only hosts. Hosts accept, keep a
  directory `did/dev → member group endpoint`, and dial other hosts like members do. Host
  addresses (`SignedHostAddr`, DID-signed, newest `at` wins) are the only endpoints that travel to
  members, and only for devices that are active hosts in the receiver's own derived state (a
  joiner with no state yet takes the ones in its invite token). Records never contain endpoints.
- **Endpoint proof.** Each side of a connection first sends `EndpointAuth` (the gcall
  `Signed<GroupNote{did, dev, endpoint, issued_ms}>`, one statement for both); the receiver checks `endpoint` equals the QUIC remote key (QUIC proves possession).
- **Session** (`wire.rs`): member `Hello{auth, group, vv, days, invite?, hosts, relay}` → host
  `Welcome{auth, vv, days, hosts}` | `Removed{records, vv}` | `Refused{reason}` (`vv`: the meta
  doc's version vector, `days`: those of the day docs of the last 7 days we hold); then both sides
  exchange `Update{doc, update}` (Loro export of what the other lacks of one doc: meta first,
  then day docs), `Ack{doc, vv}`, `HistoryList` / `HistoryGet` (older days, below), `Hosts{hosts}`, and (G2)
  `Mail{mail}` / `MailAck{id}` (sealed chat requests). A host that rotated opens its own
  connection to a member and sends `Moved{auth, group, hosts}` (G2, see Rotation). Hosts relay every accepted
  update to every other session at once (live), and catch up a reconnecting member from its vv.
- **History (decision 3 of `group-shards.md`).** Every member device holds the full history of its
  groups. On every session, both sides (a host asks a member just as a member asks a host) page
  the peer's day list (`HistoryList{before, limit}` → `HistoryDays`, newest first) and fetch each
  day they lack (`HistoryGet{day}` → `HistoryDay{day, snapshot}`) in the background, one every
  200 ms, again 3 s later and then every 5 minutes; `group_set_backfill_paused` holds it while
  the platform reports a metered network or a low battery. A snapshot is imported like an
  `Update` (every record validated, whole batch refused if one is wrong). A host serves history
  to members in good standing only, a member to the hosts it dialled. A new or wiped host is
  refilled from members; members still dial only hosts (U7).
- **Host admission** at `Hello`: refused if this device is not an active host of the group;
  `Removed` if the DID is removed; `Welcome` if it is a member, or if it shows an unexpired invite
  issued by a current admin (a joiner may then send only its own records until its `Join` makes
  it a member). The host stores the member device's group endpoint on `Welcome`.
- **Multi-host.** Any number of hosts; each holds the full doc; hosts dial each other; members
  dial every active host they know; everything a host or member receives it relays, verified, so
  two members on different hosts converge (`members_on_different_hosts_converge`).
- **No per-recipient encryption (U14).** QUIC hop by hop; at rest the group store
  (`group.redb`) seals every row (AES-GCM, `storage::Sealed`) under
  `BLAKE3-derive-key("tinline/group-store/v2", recovery phrase)`, like the chat store.

## Removal (U9, U11, U12 "refusal is immediate")

1. Admins flag (`group_flag`). When a host derives that a member reached 3 counted flags, it
   first relays the records (so the member's own replica holds the flags), then sends that
   member's sessions `Removed{proof, vv}` and closes them. `proof` = genesis + the owner's admin
   grants of the flaggers + the counted flags, all signed; the member's app checks them
   (`engine::proof_removes`) and shows "removed" with who flagged and when (`GroupDetail.removal`).
2. From then on the host answers that DID's every `Hello` with `Removed`, carrying also what the
   member lacks of the causal past of the removing flags (so even a member that was offline
   derives its removal from verified records in its own replica), and sends it nothing newer. It accepts one upload per connection of that DID's own **posts whose signed frontier
   predates the removal** (U11 option A), refusing the batch otherwise
   (`the_host_refuses_a_removed_member_that_ignores_its_removal`,
   `an_offline_post_written_before_the_removal_is_delivered`).
3. The removed member's app disables posting (`group_post` → `Rejected("removed")`), stops
   dialing once a host acknowledged its last own op, and keeps its history (read-only).
4. Hosts rotate their group endpoint 2 days after the removal (U12, G2; see Rotation). The
   host also drops the removed DID's directory entries at once (U9).

## Devices (stage 3)

Membership is per DID. A linked device (same DID, its own device ID, writer ID and group endpoint)
joins with `group_device_token` from a sibling: the token names the group and its hosts and has
no invite; the host admits it because the DID is already a member. Every device writes under its
own writer ID; receivers recompute it from `(author, dev)`.

## Writer IDs after a lost group store (G2 stage 1, P10 for groups)

`group.redb` holds its own device ID (`dev/id`, U16's device value for groups). A store made from
nothing (new install, or a lost / deleted `group.redb`) gets a fresh random one, so the device
writes as a new writer of the same DID. Without that, a device whose post reached only a host
that is offline, and which then lost its store and rejoined through another host, would write
its next post at the same (writer ID, counter) as the one that host holds, and one of the two
would be dropped silently when the hosts meet
(`a_lost_group_store_never_reuses_a_counter_held_by_an_offline_host`). A post that never left
the device is lost with the store (nothing else holds it). A G1 store without the row keeps the
profile's device ID. Consequences: the group endpoint key (also in the store) is new too; a
device that hosted under its old device ID no longer hosts (an admin appoints the new one);
`group_device_id()` is this group device ID (it is no longer the chat device ID).

## Own devices (G2 stage 2)

Our linked devices tell each other the groups the account is in over the existing own-device
session (`tinline/self/3`, new frame `SelfMsg::Groups{groups}`, no protocol bump: self/3 is
unreleased). Each entry is a postcard (`codec`) `InviteToken` without invite (group, name, owner, the
active hosts' DID-signed addresses), i.e. what `group_device_token` carries. A device that lacks
a group adds it (`GroupMeta::from_own`) and dials its hosts; hosts admit it because the DID is a
member; contents come from the hosts and are verified like any other (U7, U10). Sent at session
start and whenever a group changes (deduplicated per session). For groups the device already has,
newer addresses of active hosts are merged. A group learned this way that a host refuses with
"not a member" before the device ever held it is forgotten. `group_device_token` keeps working.

## Chat requests (G2 stage 3, U3)

`mail.rs`: a `Mail{group, from, to, id, at, sealed}` envelope, DID-signed by the sender; `sealed`
is ECIES (`identity::encrypt_for`) to the recipient's `EncKey`, and inside a `Letter` repeats
group, sender and recipient (a host cannot re-wrap a body under another envelope). Letters:
`Request{req, name}`, `Accept{req, ticket}`, `Decline{req, blocked}`.

- Member → host: `Mail`; the host checks the signature, that a member sends only its own mail,
  that sender and recipient are members and the sender is not removed, keeps it
  (`mail/{gid}/{to}/{id}`, 7 days, at most 64 per recipient) and acknowledges (`MailAck`); the
  sender keeps it in its outbox and resends on every host session until acknowledged. The host
  passes it to the recipient's sessions and to other hosts, and on every new session of the
  recipient (and to every new host session) sends what it keeps. Recipients dedupe by id.
- Accept: the recipient mints a one-time contact ticket of its PRIVATE endpoint for that DID only
  (`State::req_tickets`, 7 days) and seals it to the requester: the private endpoint is revealed
  only then and only to the requester; the host never sees it
  (`group_relayed_mail_contains_for_test`). The requester's app adds the recipient from it with
  `ContactOrigin::GroupRequest`; the recipient's node takes that hello with the same origin.
  Both get message tokens only (P6/U13: calls need "allow calls"). Then they chat directly.
- Decline: the requester sees `Declined` (PROVISIONAL, from the design).
- Blocked: a request from a DID the recipient blocks is answered automatically with
  `Decline{blocked: true}` and never listed; the requester sees `Blocked` (P5: refusal with a
  notice). The shipped app also refuses locally to ask someone it blocks, someone who blocks it,
  or an existing contact.

## Rotation (G2 stage 4, U12, §10 Q8)

A host's rotation triggers are derived from signed records: each removal (the `at` of its 3rd
counted flag) and each counted host revocation (`HostPermit{active: false}`). When a trigger in a
group this device hosts is newer than its group endpoint key (`ep/made`) and the delay (2 days,
`ROTATION_DELAY_MS`; `group_set_rotation_delay_for_test`) has passed, the device makes a new key,
closes the old endpoint (every session ends; whoever knew only the old one, the removed member,
cannot reach it) and binds the new one. It then opens a connection to every current member
device in its directory (each member's `Hello` now carries its home relay) and sends `Moved{auth,
group, hosts}`: `auth` proves the new endpoint, `hosts` are the DID-signed host addresses. The
member keeps them only from an active host whose address names the connection's endpoint, and
dials. Members offline at the time are retried with backoff (2 s doubling to 30 s) for the delay
period. Removed DIDs (and non-members) are never told; their directory entries are dropped at the
removal. Other hosts learn the new address from the host's next `Hello`. Every member derives the
deadline from the same signed records ("announced" by the removal itself);
`group_endpoint_rotation_due()` shows it on a host.

## Inviting contacts directly (G3, `invitations.rs`)

An admin picks contacts (`group_invite_contacts(group, dids)`); each gets a fresh DID-signed 7-day
invite (the same bearer grant a link carries) plus the group's name, owner and hosts' signed
addresses, over the existing 1:1 private channel: a new `proto::Msg::GroupLetter{attestation,
grant, letter}` on the calls ALPN (no chat/3 or self/3 bump; both unreleased), authenticated like a
`CallHello` (attested device = the connection's, a token the receiver issued to the sender) and
admitted only with a MESSAGE token (P6). Answer `GroupLetterTaken`; a DID the receiver blocks gets
its block notice (P5). Letters: `Invite{id, token, inviter_name}`, `Answer{id, joined}`.

- The recipient checks the invite's signature, that its issuer IS the sending contact, the group and
  expiry, and lists "X invited you to G" (`group_invitations_incoming`, `GroupInvitationEvents`).
  `group_invitation_accept(id)` runs the normal join with the delivered token (the same checks as a
  link: hosts verify the issuer's role at `Hello`, every device from the records);
  `group_invitation_decline(id)` sends `Answer{joined: false}`.
- The inviter sees per invitee (`group_invitations_outgoing(group)`) Pending (with `delivered`),
  Joined (the invitee is a member in the group's records), Declined, Expired (7 days) or Blocked
  (the invitee answered with its block notice).
- Outbox: rows `inv/{id}` in the group store with `owe`; one delivery task per DID tries each of
  the contact's devices, retries with backoff (2..60 s) while the endpoint is up and again after
  every start, like the device-list push (P4).
- P5: refused locally (whole call, `Rejected`) for a DID we block, a DID that blocks us, a
  non-contact, an existing member. Incoming invitations from a DID we block are never listed.
- Own devices: rows go over self/3 (`SelfMsg::Invitations`, JSON rows, deduplicated per session);
  a device adds rows it lacks (an incoming one only if its invite verifies for its inviter) and
  takes an answer over Pending. Only the device that made a row delivers its letter.

## Short invite links (G3, `code.rs`)

`group_invite` and `group_device_token` return `tinline://g/<base58>` (the bare base58 code works
too): `0x01 ‖ host group endpoint (32) ‖ code (12 random bytes, 96 bits) ‖ relay hint` where the
hint is an index into the four n0 relays (2 bytes), a short n0 relay name, or a full URL. With an
n0 relay a link is 47 bytes = **76–77 characters** (was several hundred with `tlgroup1:`). The old
`tlgroup1:` text form is gone (U18); `InviteToken` stays as the internal/postcard form (own-device
sync, direct invitations).

- A host keeps `code/{code}` → `{group, signed invite or none, expires, by}`. If the issuer hosts
  the group, it keeps the code itself; otherwise it registers it over its member session with a
  host (`Code{code, invite, expires}` → `CodeAck{code, ok, reason}`; members only; an invite only
  from its signer, who must be an admin; device codes admit only the member's own DID), and the link
  names that host. At most 256 live codes per group per host (PROVISIONAL).
- Joining/peeking dials that host on the group endpoint with `Fetch{code}` as the first frame
  (instead of `Hello`) and gets `Fetched{token}` (full signed invite, group, name, owner, hosts'
  signed addresses) or `Refused{reason}`. The fetcher requires the dialled endpoint among the
  group's signed host addresses, the invite's DID signature, group and expiry; then joins exactly
  as before. `group_peek_invite` now does this fetch (network, blocks up to ~20 s; apps call it off
  the UI thread).
- Hosts forget codes when they expire (checked on lookup, pruned every 5 min), refuse unknown or
  expired ones with "unknown or expired invite code", wait 300 ms before each wrong answer, close a
  connection after 3 wrong codes, and refuse all fetches for a minute after 30 wrong codes from
  anyone (`group_guess_codes_for_test`).
- `group_revoke_invite(link)`: the issuer (or an admin) makes the host forget the code
  (`RevokeCode{code}`), locally if we are that host. An invite someone already fetched stays valid
  until it expires: it is a bearer grant (D17): `Issued<GroupJoin>` with `sub = Anyone`, redeemed
  through `redeem_invite` (`proto::issued`; expiries in unix ms).
- As before, a link reveals ONE host's group endpoint to whoever holds it (and anyone who fetches
  with it learns all active hosts' endpoints, as the old token carried them). A link names one
  host: if that host is offline (or rotated its endpoint, U12), the link does not work; ask for a
  new one (PROVISIONAL).

## Group calls (GA-I1, U21)

Code: `calldir.rs`, `calls.rs`; spec `docs/design/group-call-spec.md` §11 (GA-I1). Test:
`crates/core/tests/group_call.rs`.

- **Call hosts** are a separate capability: `CallHostPermit{host, host_dev, active}`
  (owner/admins) + `CallHostAccept{active}` (the device) + a member DID, derived like hosts
  (`GroupState::call_hosts`, `call_host_requests`). Chat hosting implies nothing about calls.
- **Calls run on the group endpoint** (ALPN `tinline/gcall/1`, U2). Members dial only the call
  host; call hosts' DID-signed addresses travel like chat hosts' (U7).
- **Control through the chat hosts:** `GroupMsg::CallHostStatus` (every 30 s and on change),
  `CallHostRequest` / `CallHostRefusal` (the starter's pick, §2.3), `CallStart{start, hosts}`,
  `CallEnd`. Hosts verify and relay; removed and joining members never get them.
- Call hosts rotate after a removal too (half a delay after chat hosts when they host no
  chat); rotation waits for a running call (≤ 1 h).
- API: `group_call_start/join/leave/set_muted`, `group_calls`, `group_call_hosting_enable`,
  `set_group_call_events`, `group_appoint_call_host` / `group_revoke_call_host` /
  `group_accept_call_host`, `GroupDetail.{am_call_host, call_hosts, call_host_requests,
  call_host_requested_here}`.

## API (UniFFI, `api.rs`)

`set_group_events(GroupEvents{on_group_message, on_group_changed})`, `groups() ->
[GroupSummary]` (the row and unread count come from the group's local state: no day doc is
loaded), `group_detail(id) -> GroupDetail` (members with roles/flags/devices/invited-by,
hosts with online state, host offers, my role, `am_host`, my removal), `group_messages(id, before,
limit) -> GroupPage` (oldest first, cursor `older`; pages back across day docs, loading only the
days a page needs, and shows what the device holds: older days still arriving in the background
appear on a later call), `group_mark_read`, `group_set_backfill_paused(paused)`, `group_create(name)`,
`group_invite(id)` (admins; short 7-day link, G3), `group_device_token(id)` (short link admitting only
our DID; apps no longer show it: own-device sync covers linked devices),
`group_peek_invite(link)` (fetches from the host), `group_join(link or code)` (blocks until joined, `Rejected(reason)` on
refusal), `group_post(id, text, reply_to)`, `group_flag(id, did)`, `group_set_admin(id, did,
admin)` (owner), `group_leave(id)`, `group_appoint_host(id, did, device_id)` /
`group_revoke_host(...)` (admins), `group_accept_host(id, accept)` (this device),
`group_device_id()`, `group_endpoint_id()`, (G2) `group_endpoint_rotation_due() -> Option<u64>`.
`GroupDetail` (G2) also has `host_requests` (devices an admin appointed that have not answered,
with `appointed_by`) and `host_requested_here` (an admin appointed THIS device: show "host this
group?" and answer with `group_accept_host(id, true|false)`; a later `HostAccept{false}` ends the
request). The permit arrives as a group change, so `on_group_changed` fires.
Chat requests (G2, `requests.rs`): `set_group_request_events(GroupRequestEvents{on_chat_request})`,
`group_chat_request(group_id, did) -> GroupChatRequest`, `group_chat_requests_incoming()`,
`group_chat_requests_outgoing()`, `group_chat_accept(request_id)`, `group_chat_decline(request_id)`;
`GroupChatRequest{id, group_id, group_name, peer_did, peer_name, outgoing, state:
ChatRequestState{Pending, Accepted, Declined, Blocked}, at, contact_added}`.
Test hooks: `group_known_endpoints_for_test`, `group_directory_for_test`,
`group_dial_only_for_test`, `group_ignore_removal_for_test`, (G2)
`group_set_rotation_delay_for_test(secs)`, `group_skip_request_checks_for_test`,
`group_relayed_mail_count_for_test`, `group_relayed_mail_contains_for_test`.

Delivery of our own posts: `Waiting` until some host acknowledged holding it ("Waiting for a
host"), then `Delivered`; on a host device our posts are `Delivered` at once.

G3: `set_group_invitation_events(GroupInvitationEvents{on_group_invitation})`,
`group_invite_contacts(group_id, dids) -> [GroupInvitation]`, `group_invitations_incoming()`,
`group_invitations_outgoing(group_id)`, `group_invitation_accept(id)` (joins, blocks),
`group_invitation_decline(id)`, `group_revoke_invite(link)`; `GroupInvitation{id, group_id,
group_name, inviter_did, inviter_name, invitee_did, invitee_name, outgoing, state:
GroupInvitationState{Pending, Joined, Declined, Expired, Blocked}, at (ms), expires (ms),
delivered}`. Test hook: `group_guess_codes_for_test(link, n)`.

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
9. Membership is per DID; a member's other device learns the group by own-device sync (G2) or
   joins with a device token.
10. A removed member's upload of pre-removal posts is accepted without a count cap until hosts
    rotate (U15); the rotation (G2) closes that window after 2 days.
11. (G2) The group store has its own device ID; a fresh store gets a fresh one (P10 for groups).
12. (G2) Chat requests: mail kept 7 days at hosts (max 64 per recipient); the acceptance ticket
    is good 7 days; request state is per device (not synced to own devices; another device of the
    recipient may still list a request answered elsewhere); declining shows "declined"; a blocked
    sender is told `Blocked`; existing contacts cannot be requested; asking again while pending
    returns the pending request. A DID with a pending request ticket that scans our normal QR
    also gets message-only tokens.
13. (G2) Rotation deadline = trigger record's `at` (the flagger's / revoker's clock) + 2 days;
    a host revocation rotates the remaining hosts only (members' own group endpoints, which the
    revoked host knew, do not rotate: U1 member notes are a later slice). Members offline longer
    than the delay after a rotation need a fresh token (device sync or invite).

14. (G3) A direct invitation is the same bearer invite as a link (not bound to the invitee); the
    invitee could pass it on. Rows are forgotten 7 days after expiry. A refused delivery (`Reject`:
    no longer a contact, token taken back) stops retrying and stays Pending until it expires.
15. (G3) Invitation rows ARE synced to own devices (self/3 was cheap to extend).
16. (G3) A link names one host; revoking a code does not revoke an invite already fetched with it;
    256 live codes per group per host; guess limits 3 per connection / 30 per minute per host.

## Left (later slices)

- Member group endpoint rotation and notes to hosts (U1); periodic host rotation (only removal
  and revocation trigger one today).
- The group endpoint is one per install for all groups: a removed member that shares another
  group hosted by the same device learns the rotated endpoint through that group.
- Group-call UI and platform condition feeders (GA-I1 wired the core), files/voice in groups, edits/deletes of posts, mute, group rename/photo,
  "who can invite" setting, readmission after removal, host-side limits (max hosts, upload caps).
- `blocking_acceptance.rs::personal_block_does_not_change_group_membership` (P5's file) still
  uses stub group helpers whose shape (`add_group_member(host, group, did)`) does not fit the
  invite model; it stays ignored. Its intent is covered by design: blocks never touch group state.
- The Android/desktop apps are not wired (G1 only provides the API).
