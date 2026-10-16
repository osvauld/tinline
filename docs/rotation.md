# Endpoint rotation

What rotates today, when, and how it survives a crash. Decisions: D11 and D12 in
`docs/design/rotation-open-decisions.md`. Earlier option studies: `docs/archive/rotation/`.

## What rotates

- **The group endpoint of a hosting device.** One group endpoint per install and account
  (`ep/secret` in `group.redb`), used by every group this device belongs to. A device that hosts
  any group's chat or calls rotates it. Members do not rotate theirs.
- **Not yet: the private (1:1) endpoint** (the device key). That is the next packet; see
  "Next" below.

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

All in `RotationPolicy::DEFAULT` (`crates/core/src/group/rotation.rs`):

| Value | Default |
|---|---|
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

## Next

- Private (1:1) endpoint rotation on the same `RotationPolicy` shape: rotation notes to
  unblocked contacts through the device-list push, the verified flag bound to the DID, ticket
  refresh, and "lost contact" detection. Backlog: `docs/design/rotation-acceptance-checklist.md`.
