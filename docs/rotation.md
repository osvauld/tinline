# Endpoint rotation

What rotates today, when, and how it survives a crash. Decisions: D11 and D12 in
`docs/design/rotation-open-decisions.md`. Earlier option studies: `docs/archive/rotation/`.

**Off by default (decided 2026-10-19, for the alpha).** `RotationPolicy::DEFAULT` has
`enabled: false`: no endpoint is scheduled to rotate until a policy with `enabled: true` is
stored (`set_group_rotation_policy`). Why: the apps have no rotation setting and no lost-touch
screen yet, so a contact who missed the announce window would drop out with nothing shown. The
mechanism below is unchanged and tested; `private_rotate_now` still rotates on request. A stored
policy row from before the flag reads as off. Turning it on by default needs the settings and
lost-touch UI first.

## What rotates

- **The group endpoint of a hosting device.** One group endpoint per install and account
  (`ep/secret` in `group.redb`), used by every group this device belongs to. A device that hosts
  any group's chat or calls rotates it. Members do not rotate theirs.
- **The private (1:1) endpoint of every install** (its device key; packet F, below). Each linked
  device rotates its own, on its own timer, on the same policy.

## When (D11: periodic only)

`due = made + interval + jitter` (`group::rotation::rotation_due`, a pure function).

- `made`: when the current key became current (local clock, `ep/made`).
- `interval`, `jitter`, `announce`: this device's `RotationPolicy`, stored as `ep/policy`.
  `jitter` is drawn once per key in `0..=jitter_ms` and persisted (`ep/jitter`), so a restart
  never moves the deadline.
- Nothing else moves it: removals, flags, host revocations and personal blocking never trigger
  a rotation or reschedule one. Record times (`at`) are never read.
- Overdue on resume (the device was off past the deadline): it rotates once, and the next
  deadline counts a full interval from that rotation.
- Saving the setting never postpones the next rotation: the deadline counts from `made`, not
  from the save. An interval below `MIN_INTERVAL_MS` is refused.
- A `made` more than an interval in the future (the clock went back) is due now.
- A running group call on this endpoint defers a due rotation by at most 1 h
  (`CALL_ROTATION_DEFER_MS`); then the rotation cuts the call and members redial the backups.

### Values (decided 2026-10-16)

All in `RotationPolicy::DEFAULT` (`crates/group-core/src/rotation.rs`):

| Value | Default |
|---|---|
| enabled | **no** (2026-10-19) |
| interval | 7 days |
| jitter | up to 1 day |
| announce window | 7 days (= interval) |
| overlap of old and new endpoint | none: the old one closes right after the promote |
| `MIN_INTERVAL_MS` (setting floor) | 1 day (decided 2026-10-16; no maximum) |

API (UniFFI, no app calls them yet): `group_rotation_policy()`,
`set_group_rotation_policy(policy)` (`Rejected` when out of bounds),
`group_endpoint_rotation_due()`, `group_rotate_now()`. A Settings control would need: the
interval (and jitter) as a choice list at or above the floor, the next deadline from
`group_endpoint_rotation_due()` (only shown on a hosting device), and a "change it now" button
for `group_rotate_now()`.

## How (crash-safe)

`engine/rotation.rs` `group_rotate`, store rows in `store/endpoint.rs`:

1. Stage: a fresh key goes to `ep/pending`. The current key does not change.
2. Bind the new endpoint while the old one keeps serving.
3. Promote: `ep/secret`, `ep/made`, `ep/announce` and `ep/jitter` in one transaction; the staged
   row is deleted in the same transaction.
4. Swap: the new endpoint replaces the old one in the runtime, sessions on the old one close,
   and the old endpoint closes.
5. Announce (below).

Any failure before the swap leaves the old endpoint serving and drops the staged key; the next
tick (every 2 s) tries again. On start, a leftover `ep/pending` is discarded: a key that never
served never becomes current. A crash after the promote leaves the new key current. A malformed
`ep/*` row is an error, never a silently made new key. Keys are held in `Zeroizing` buffers.
One rotation runs at a time.

## Discovery after a rotation

- **Moved:** the rotated host dials every current member device in its directories that has no
  session with it and sends `Moved{auth, group, hosts}`, with backoff, until all are back or the
  announce window ends (`ep/announce`). The window is persisted, so the campaign resumes after a
  restart. A member that stays offline longer than the window needs a fresh invite or device
  sync.
- **Hosts:** other hosts and call hosts learn the new address from the host's next `Hello`, and
  members relay DID-signed host addresses (`Hosts`) to every session they have.
- Removed DIDs and non-members are never told.

## Removed members (D12)

Rotation no longer closes a removed member's upload window. A host records when it first
derives each removal (`rm_seen/{gid}/{did}`, local clock) and accepts the removed member's
pre-removal posts (U11) for `REMOVED_UPLOAD_WINDOW_MS` (2 days) from then. After that it
refuses them. The rule is by DID, so it holds even when the removed member still reaches this
host's group endpoint through another group they share.

## Open

- Two hosts of one group rotating within seconds of each other could each keep only the other's
  closed address until a member relays `Hosts`. Per-device jitter makes this rare; no extra
  stagger exists (accepted 2026-10-16).
- A rotation that cuts a running call (after the 1 h defer, or on rotate-now) sends no end
  reason: members notice by heartbeat loss and redial a backup host (accepted 2026-10-16).
- A dial in flight on the old endpoint at the swap can wait out the idle timeout (~20 s) before
  it retries on the new one.

## Private endpoint (packet F, decided 2026-10-16)

The private endpoint key is the install's device key (iroh `EndpointId`, attested by the DID).
Rotating it changes nothing identity-derived: the DID, the sealed stores (keyed by the phrase), the
vault's data key (a remembered `unlock_key` stays valid), the writer ID (install `device_id`,
U16), author proofs and per-DID tokens all stay. Own-device Loro docs get a new Loro peer id per
key (harmless). Code: `crates/core/src/node/rotation.rs`.

**When.** The same `RotationPolicy` as the group endpoint (`set_group_rotation_policy` stores the
one setting for both; no second store): `due = made + interval + jitter` with `made`/`jitter` in
the sealed state (`State::private_ep`), checked every 2 s while started. Every install, hosting or
not. Nothing else moves it: not blocking, unlinking, removals or anything else. A 1:1 call in
progress defers it by at most 1 h (`CALL_ROTATION_DEFER_MS`). `private_rotate_now()` for tests and
Settings; `private_endpoint_rotation_due()`.

**How (crash-safe), under the lifecycle lock:**

1. Stage: a fresh key in memory only; the sealed state gets `private_ep.pending` (old and new
   public keys, the succession link) and the overlap key (the current secret).
2. Bind the new endpoint while the old one serves.
3. Promote: the vault (resealed with the same data key) and `account.json` get the new key in one
   atomic file write; `Me` and the node's endpoint are swapped in memory at the same moment.
4. Complete: the old key becomes a `rotated` registry tombstone (also `removed`, so nothing dials
   it; `linked_devices` hides it), the install's entry under the new key carries the succession
   links, `made`/`jitter`/`announce_until` restart from the promote, the shown ticket is retired.
5. The old endpoint overlaps for `PRIVATE_OVERLAP_MS` (1 day; `node/rotation.rs`), then closes.
   The overlap survives a restart while it lasts (the old secret is in the sealed state until then).
6. Announce: the re-signed device list goes to every contact we neither block nor are blocked by
   (`devlist.rs` outbox, persisted, retried with backoff and after every start); our own installs
   learn the new key through the account doc on the next self-sync.

A crash before the promote leaves the old key (the staged one was never on disk; the next start
drops `pending`); after it, the new key, and the next start completes step 4 from `pending`.

**Overlap.** A connection to the old endpoint from a device of a contact we may tell gets our list
pushed from the new endpoint and is closed (`moved`); from one of our own installs, a self-sync
from the new endpoint. Anyone else is handled as on the current endpoint: a DID we block is
refused at its hello exactly as before (and never told the new key); a hello on a ticket naming
the old key is answered under that key (`HeldRequest::on`), and the Welcome for a held request is
sent from the endpoint of the key it was made under while that endpoint is open.

**Verified.** `verified` survives a rotation: every install keeps its last `KEPT_LINKS` (4)
succession links `{from, to, sig}` (signed by the old key, domain `DEVICE_SUCCESSION`), our
device list carries them (`DeviceList::links`, at most 16), and a contact keeps `verified` when
every unknown device in a new list has a chain of links back to a key it knew
(`proto::succeeds`). A new install (link, restore) has no such chain and still resets it.
Before a chat dial to a contact still owed our list, the list is delivered first
(`list_before`), so the contact meets the new key through the list.

**Tickets.** The next `my_ticket()` after a rotation names the new key. An older ticket keeps
working while the key it names answers (the overlap day): its hello reaches the old endpoint and
is answered under that key, and the new contact is then sent our list. After the overlap an old
ticket cannot reach us (the joiner sees "could not reach"); they need a fresh ticket or QR.

**Lost touch.** When a rotation's announce window (`announce_ms`, 7 days) ends, every contact the
list never reached (still in `list_outbox`: every attempt to all of their known devices failed
for the whole window) is marked `lost_touch` (sealed state, local) and background tries stop.
Any authenticated exchange with them (a session, a call, a delivered list) or a fresh ticket swap
clears it. `Contact.lost_touch` (UniFFI) carries it to the apps. The fix is a fresh ticket/QR swap;
there is no discovery service.

| Value | Default | Where |
|---|---|---|
| interval / jitter / announce | 7 d / ≤ 1 d / 7 d | `RotationPolicy::DEFAULT` (shared with groups) |
| overlap of the old private endpoint | 1 day | `PRIVATE_OVERLAP_MS`, `node/rotation.rs` |
| succession links kept per install | 4 | `KEPT_LINKS`, `node/rotation.rs` |
| links per device list | 16 | `proto::MAX_LIST_LINKS` |

### What the apps need (not built)

- Contact list / chat header: `Contact.lost_touch` → "Lost touch: swap codes again" with the
  QR/ticket share and scan actions; refresh on `on_contacts_changed`.
- Settings › Privacy: the interval choice (and jitter) at or above `MIN_INTERVAL_MS`, through
  `group_rotation_policy` / `set_group_rotation_policy` (one setting for both endpoints); the next
  change from `private_endpoint_rotation_due()`; a "change it now" button for
  `private_rotate_now()`.
- Re-render the shown QR after a rotation (`my_ticket()` returns a new one).

### Open (private)

- Both sides rotated while apart but share a group: no recovery through the group host yet (U1
  group path); they are marked `lost_touch` like any other pair.
- We did not rotate, they did, and their note never reached us within their window: we are not
  marked `lost_touch` (only the side whose note failed is). Their next dial to us still heals it.
- A contact offline for longer than about `KEPT_LINKS` intervals (a month) misses links and sees a
  new install: `verified` resets.
- A chat dial to a contact that is owed our list costs one extra dial attempt per device first.
- Rotating while unlocked only (a locked node does not run the schedule); Android resume hooks
  and clock jumps (§2 Q5–Q8, §3 Q2–Q3 of the archived digest) are unchanged.
