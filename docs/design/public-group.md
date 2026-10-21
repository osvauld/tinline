# Public channel and lobby (alpha)

Status: spec, decided with the user 2026-10-21. Not built. Base: dev, rebased after
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
- `group_open_link(id)` (admins): writes `Open{active: true}` if not set and registers a
  short code with no invite and no expiry on each reachable host; the link format is
  unchanged (`tinline://g/...`). `Fetched` returns the token with `invite: None` and the group
  marked open.
- **Host admission:** a `Hello` from a non-member without an invite is welcomed **only while
  the group is open** (derived `open` now); the joiner may then send only its own records
  until its `OpenJoin` makes it a member (as invite joiners today). Closing
  (`group_set_open(id, false)`) refuses new joiners at once; the open code is dropped.
- An open code counts as one of the 256 live codes and is not swept by expiry.
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

### 4. Receipts off in public groups

When `open` is true: no read or delivered receipts are written or sent for that group
(`member_receipts`), `ReceiptHello` is not sent, and incoming receipt frames for it are
dropped. Own-post delivery still shows `Waiting` → `Delivered` from host `Ack`s (local, no
receipt traffic). Private groups are unchanged (`receipt-policy-decisions`).

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

1. `open_link_joins_without_invite`: a non-member's `Hello` without an invite is welcomed
   while open and its `OpenJoin` makes it a member (unit rule: `open_join_counts_only_while_open`, built).
2. `closing_refuses_new_open_joins_keeps_members`: after `Open{false}`, a new joiner gets
   `Refused`; an existing member still syncs.
3. `admins_only_hides_member_posts_in_every_order` (built): a member post written after
   `AdminsOnly{true}` is hidden on every device regardless of delivery order; one written
   before stays visible.
4. `admins_only_rejects_local_post`: `group_post` by a non-admin returns `Rejected`.
5. `presence_union_across_two_hosts`: members on host A and host B both appear online to a
   member of either host within 20 s; a disconnected member drops out within 75 s.
6. `hidden_member_not_listed`: a member with presence hidden never appears in `group_online`.
7. `presence_only_from_active_hosts`: a `Presence` signed by a non-host is ignored.
8. `public_group_sends_no_receipts`: a session of an open group carries no receipt frames;
   a private group still does.
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
