# Public channel and lobby (alpha)

Status: spec, decided with the user 2026-10-21. Built: §1 (settings records), §2 (open link,
host admission; `tinline/group/5`), §3 (presence) and §4 (receipts off); the rest not yet. Base: dev, rebased after
`integrate/crate-split` lands (image posts build on its `crates/transfer`).

## Purpose

A casual alpha hangout: say hi, talk tech, share memes, and give the project a place to
reach people. Two public spaces, both ordinary groups (`docs/group-core.md`) with settings:

| | Channel | Lobby |
|---|---|---|
| Who posts | Owner and admins | Everyone |
| Joining | Permanent open link | Permanent open link |
| Online list | Yes | Yes |
| Read receipts | Off | Off |
| Images | Yes | Yes |

The owner is the user's own account (desktop app on the laptop). No anonymity promise; the
trust model of `docs/group-chat.md` §10 is unchanged.

## Hosts

- **Worker:** a headless host under its own synthetic identity ("Tinline host"), running
  `p2p-peer --data DIR group-host`. It joins with an invite, the owner appoints it
  (`group_appoint_host`), and it accepts automatically (`group_accept_host(id, true)`) for
  groups listed in its config. It has no battery backup: it is one host of several, never the
  only copy (every member device holds full history).
- **Laptop:** the owner's desktop app is the second host while it is open.
- **A phone** may be appointed if needed; fan-out is slower. No change needed.
- Members still dial every active host they know (unchanged). Load balancing comes later.

## Changes

### 1. Settings records (group-core)

New owner/admin meta records, latest by `(at, id)` per kind, judged "admin as of the record's
frontier" like `HostPermit`:

- `Open { active }`: the group accepts joins with an open invite (below).
- `AdminsOnly { active }`: only the owner and admins may post (the channel).

`derive_meta` exposes `open: bool` and `admins_only: bool`. "Public" = `open`; public groups
turn receipts off (§4).

**Post visibility under `AdminsOnly`:** a post shows only if its author was owner/admin as of
the post's frontier **or** `AdminsOnly` was not active as of that frontier. This is a pure
function of the post and the meta records, so every delivery order agrees (same rule shape
as U11). Posting while `AdminsOnly` is active returns `Rejected("admins only")` on the device.

### 2. Permanent open link

Invites cannot be permanent: `proto::Issued` caps their lifetime (`MAX_INVITE_TTL_MS`, 30
days). An open group needs no invite instead:

- New member record `OpenJoin { name }` (group-core, built): a join that counts when the
  group was open as of the record's frontier (`Open{active: true}` by an admin, latest by
  `(at, id)` inside the frontier). Joins already made stay valid after closing.
- `group_open_link(id)` (admins): writes `Open{active: true}` if not set and registers one
  short code with no invite and no expiry at every chat host reachable now (ourselves if we
  host, then each host we have a session with); the link names us if we host, else the first
  host that took it. The link format is unchanged (`tinline://g/...`). Asking again returns the
  stored link (`GroupMeta.open_link`, local) while the host it names still hosts.
- Codes say what they admit: `CodeKind::{Invite, Device, Open}` on `CodeRow` and in
  `GroupMsg::Code` (an invite-less code is no longer ambiguous). An open code has
  `expires = NO_EXPIRY` (`u64::MAX`). A host takes an `Open` code only from an admin while the
  group is open (the admin's `Open` record goes first on the same session stream).
- `Fetched` returns the token with `invite: None` and `InviteToken.open = true`;
  `GroupInvitePeek.open` shows it. The joiner keeps `GroupMeta.open_join` and writes
  `OpenJoin { name }` once it holds the group **and** the `Open{active: true}` record (so its
  frontier names it), instead of `Join`.
- `group_set_open(id, bool)` and `group_set_admins_only(id, bool)` (owner/admins; others get
  `Rejected`) write the record only when the setting changes. `GroupSummary` and `GroupDetail`
  carry `open` and `admins_only` (UniFFI default `false`).
- **Host admission:** a `Hello` from a non-member without an invite is welcomed **only while
  the group is open** (derived `open` now); the joiner may then send only its own records
  until its `OpenJoin` makes it a member (as invite joiners today). Closing
  (`group_set_open(id, false)`) refuses new joiners at once; the open code is dropped here and
  `RevokeCode` goes to every host we reach. A host we miss refuses (and drops) the open code
  at its next fetch, as the group is closed. A joiner admitted while open whose `OpenJoin` has
  not arrived when the group closes is refused at that update ("not open").
- An open code counts as one of the 256 live codes and is not swept by expiry.
- Wire: `tinline/group/5` (`Code.kind`, `InviteToken.open`; the token's encoding also changes
  inside self/5 group lists and direct-invitation letters, pre-release, no compatibility).
- Guess limits (3 per connection, 30/min per host) and the 300 ms wrong-answer delay stay.

### 3. Online list (presence)

- **Source:** a host knows which member devices hold a live session. Every 15 s, and on a
  session start/end, a host sends members and other hosts
  `Presence { group, host, online: Vec<(did, last_seen_ms)>, seq }` (new `GroupMsg` frame).
  Signed by the host's DID; members accept it only from active hosts.
- **Merge:** a member keeps the latest `Presence` from each host and shows the union. A DID is
  online if any host saw it within 60 s.
- **Hide me:** `group_set_presence_visible(visible)` per account, default visible. It travels
  in `Hello` (`presence: bool`). A hidden member is never listed by a host. Local setting, not
  a record: hiding leaves no history.
- **API:** `group_online(id) -> Vec<OnlineMember{did, name, is_admin}>` and
  `GroupEvents.on_group_presence(id)`.
- **Bound:** at most 500 DIDs per frame; larger groups show "500+ online" plus those listed.
- Presence is not stored and not replicated into docs; it is best-effort and may lag by
  ~15 s.

**As built** (`crates/core/src/group/engine/presence.rs`, `group_core::record::Presence`):

- **Open groups only.** A private group sends no `Presence` and drops incoming ones;
  `group_online` is empty for it (conservative: private groups unchanged).
- **Frame:** `GroupMsg::Presence { presence: Signed<Presence{group, host, dev, online: [(did,
  last_seen_ms)], seq, at}> }`, domain `tinline/group/presence/v1`. `dev` names the host device
  (hosts are devices); `at` is the host's clock when signing, so a receiver ages an entry as
  `at - last_seen` from when the frame arrived and clocks need not agree. `seq` is per host
  device, at least its clock (it keeps growing across restarts).
- **Send:** each host every 15 s from the dial-loop tick (2 s), and after a session starts or
  ends (debounced: at most one frame a second per group), to its member sessions and to the
  other hosts (`relay::relay_targets`, the call-frame rule: current members only, never a
  joiner or removed member). A host lists itself (unless its own setting hides it), every
  member with a live session whose `Hello` said `presence: true` (and none of that DID's sessions
  said `false`), and members whose last visible session ended within the window (with that
  time, so a reconnect does not flicker).
- **Accept and relay:** a receiver keeps a frame only for an open group, signed by an active
  host device, with a higher `seq` than the one it holds from that device. A host relays an
  accepted frame unchanged to its other sessions; the `seq` check ends relay loops. A member on
  one host thus sees the members of every host. Frames of a host that is no longer active are
  ignored when the list is computed.
- **Hide me:** `group_set_presence_visible(false)` writes the local row `set/presence_hidden`
  and ends our sessions to hosts of open groups, so the next `Hello` carries it at once; the
  host forgets the member's "recently left" entry when a hiding session starts.
- **Events:** `on_group_presence(id)` when the computed online set changes (on a new frame, and
  checked every tick, so expiries are reported too).
- **500+:** a frame lists at most 500 DIDs (newest first); the union may hold more. Showing
  "500+ online" is left to the apps (§8).
- **Timing knobs:** `group_set_presence_timing_for_test((every_ms, window_ms))` (dev-tools
  builds) lets tests run in seconds.
- **Open:** the setting is per install, not synced: two devices of one account may disagree (a
  host never lists a DID one of whose sessions with it hides; another host may still list it via
  a visible device).

### 4. Receipts off in public groups

When `open` is true: no read or delivered receipts are written or sent for that group
(`member_receipts`), `ReceiptHello` is not sent, and incoming receipt frames for it are
dropped. Own-post delivery still shows `Waiting` → `Delivered` from host `Ack`s (local, no
receipt traffic). Private groups are unchanged (`receipt-policy-decisions`).

**As built** (`engine::member_receipts`): "off" = the derived `open` now, or we are joining by an
open link (`GroupMeta.open_join`, before the group's records arrive). No delivered/read intent
is recorded, the session `ReceiptHello` is skipped, `Receipts`/`ReceiptAck` are not sent, and
incoming receipt frames are dropped without an answer. Nothing is backdated: posts that arrived
while the group was open get no receipts if it closes later; statements signed before it opened
stay in the outbox and go out only if it closes again.

### 5. Images

Image posts for both spaces, on `crates/transfer` (BlobHub) once it lands:

- `Post` gains `image: Option<ImageRef{hash, size, w, h, mime}>` (BLAKE3 hash). The sender
  resizes to at most 1080 px on the long side, JPEG/WebP, under 300 KB.
- Hosts fetch and keep blobs of visible posts; members fetch from hosts (members dial only
  hosts, U7). Verified by hash while streaming.
- Auto-download setting per device: always / Wi-Fi only / never (tap to load). Default: Wi-Fi
  only.
- A host refuses blobs over 1 MB and keeps at most 2 GB per group (oldest unreferenced first).

### 6. Moderation

Existing removal stays (3 counted flags; one admin may flag 3 times). No new rule now.

### 7. Headless host (`crates/peer`)

`p2p-peer --data DIR group-host [--accept GROUP_ID]...` runs the node forever:
accepts host appointments for the listed groups, logs sessions/presence counts once a minute,
and exits cleanly on SIGTERM (systemd unit on the worker, `Restart=always`).

### 8. Apps

Desktop first (it is a host), Android next:
- Group header: "N online" → sheet with the online list.
- Settings (admins): Open link on/off + copy link; Admins only on/off.
- Composer hidden for non-admins in an admins-only group ("Only admins post here").
- Profile/privacy: "Show me as online in public groups" toggle.
- Images in posts; the auto-download setting.

## Acceptance tests

Deterministic, offline, in `crates/core/tests/` (names indicative):

1. `open_link_joins_without_invite` (built, `crates/core/tests/public_group.rs`): a
   non-member's `Hello` without an invite is welcomed while open and its `OpenJoin` makes it a
   member (unit rule: `open_join_counts_only_while_open`, built; code rows:
   `open_code_never_expires_and_counts`, built).
2. `closing_refuses_new_open_joins_keeps_members` (built): after `Open{false}`, the link is
   refused at fetch and a joiner holding a token fetched while open gets `Refused` at `Hello`;
   an existing member still syncs.
3. `admins_only_hides_member_posts_in_every_order` (built): a member post written after
   `AdminsOnly{true}` is hidden on every device regardless of delivery order; one written
   before stays visible.
4. `admins_only_rejects_local_post` (built): `group_post` by a non-admin returns
   `Rejected("admins only")`.
5. `presence_union_across_two_hosts` (built, `public_group.rs`; 1 s / 4 s timing): members on
   host A and host B both appear online to a member of either host within 20 s; a disconnected
   member drops out within 75 s; an own post still turns `Delivered`.
6. `hidden_member_not_listed` (built): a member with presence hidden never appears in
   `group_online`; showing again lists it, hiding again takes it off.
7. `presence_only_from_active_hosts` (built): a `Presence` signed by a non-host is ignored; the
   same frame signed by the host is taken (unit: `presence_frames_verify_and_are_bounded`).
8. `public_group_sends_no_receipts` (built, a session simulation in p2pcore's lib tests,
   `group_receipts_sim.rs`, where every frame is observable): a session of an open group
   carries no receipt frames and drops incoming ones; a private group still does.
9. `image_post_roundtrip_via_host`: a member's image reaches another member through a host,
   hash-verified; an oversized blob is refused.
10. Headless: `p2p-peer group-host` accepts an appointment and relays between two members
    (e2e script, not a unit test).

Manual: worker host + laptop host + Android emulator; turn the laptop off, the lobby keeps
working through the worker.

## Later (not in this build)

- Load balancing: each member uses one home host (DID hash over active hosts, worker weighted
  higher), failover to the next, hosts relay to each other; `Busy` refusal; session caps;
  join/backfill queue. Decide limits from a `group-bench` load test (50 → 2,000 synthetic
  members against the worker).
- App update channel through the group (signed manifest, notify only); signing key TBD.
- Admin "hide post" (needed for takedowns if the space gets busy).
- Read-only web preview of the channel.
- Own `iroh-relay` (separate project).

## Build order

1. Settings records + derive (§1) with tests 3–4.
2. Open link + host admission (§2) with tests 1–2.
3. Receipts off (§4) with test 8.
4. Presence (§3) with tests 5–7.
5. Headless host (§7) on the worker.
6. Desktop UI (§8), then Android.
7. Images (§5), after `crates/transfer` lands.
