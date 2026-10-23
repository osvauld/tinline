# Group day shards — design

Status: implemented (2026-10-12, branch `group-shards`); decisions below were taken by the user. Replaces "one Loro doc per group" (`group-core.md`
"Doc"; listed as a known gap there). Follows the original plan in `group-chat.md` §2
(`group/{gid}/meta` + UTC-day message shards). Base design, no interim fix. U18: no backward
compatibility, data may be wiped; ALPN and store domain get new versions.

Why: `docs/scale-year.md` (one heavy year, desktop): group store open 9.4 s / +709 MB (every
group loaded, every record's signature re-verified), remote apply 2.75 s and local write 385 ms
in a 100k-post group, a page 36 ms. Phones are 4–6× slower.

## 1. Docs

| Doc | Holds | Loaded |
|---|---|---|
| `meta` (one per group) | every record except posts: Genesis, Join, Leave, Admin, Flag, HostPermit, HostAccept, EncKey, CallHostPermit, CallHostAccept | always (at group store open) |
| `day/{YYYY-MM-DD}` | `Post` records whose `at` falls on that UTC day (later: edits, deletes, reactions of those posts) | on demand; at most N cached (like chat's `MAX_SHARDS_CACHED`) |

- Both are the same kind of Loro doc as today: root map `r`, `record id → SignedRecord bytes`,
  grow-only, key = id of value, op peer = `writer_id(author, dev)`.
- Acceptance adds two syntactic rules, still a pure function of batch + doc: a `Post` only in a
  day shard and only if its `at` lies in that day ± 1 h (as 1:1 chat); any other body only in
  `meta`. Wrong doc → whole batch refused.
- Meta stays small: it grows with membership churn, not with chat volume.

## 2. The frontier (`seen`) across docs

Today a record's `seen` is the author's version vector of the one group doc. After the split:

- A meta record's `seen` = the author's **meta** vv (unchanged meaning).
- A post's `seen` = the author's **meta** vv when it wrote the post. Ordering among posts is by
  `(at, id)` as now; posts need no frontier over other posts.

Post visibility is the existing U11 rule, evaluated against meta only: a post shows iff its
author was a member as of `seen` and fewer than 3 counted flags on the author lie inside `seen`;
`before_removal` as now. This is exactly today's semantics (the author picks `seen` today too;
host admission refuses removed DIDs, as today) — the split neither strengthens nor weakens it.
The open causal-policy gate in `group-chat.md` §6 is unchanged and still open.

Out of order: a post whose `seen` names meta ops this device does not hold yet is **pending**:
stored (it is signature-valid), hidden, and re-evaluated when meta advances. Sessions send meta
first, so pending is rare. No batch is refused for it (no deadlock between docs).

Derive becomes two parts:
- `derive_meta(meta records) -> GroupState` (members, admins, flags, hosts, system lines). Only on
  meta changes; cost independent of chat volume.
- `post_visible(state, post) -> Visible | Hidden | Pending`: per post, from per-DID join/leave/flag
  positions checked against `seen` (`O(records about that DID)`). Applied per shard as it loads
  or changes — never over the whole history.

## 3. Store (`group.redb`, domain `tinline/group-store/v2`)

```text
grp/{gid}                          GroupMeta   + read cursor, newest day, counters
meta/{gid}/snap | meta/{gid}/u/{seq}            meta doc snapshot + updates (as today's doc/)
shard/{gid}/{day}/meta             ShardMeta   vv, n_updates, closed ref (as chat)
shard/{gid}/{day}/snap | …/u/{seq}              open shard
days/{gid}/{day}                   empty       day index for paging / history lists
```

- Closed days (older than `CLOSE_AFTER_DAYS`, like chat) are compacted into one sealed snapshot
  (blob or row; reuse chat's compaction where it fits). No vouching needed: every record inside
  is self-signed.
- Verification on load: records are verified when they enter the store (import); loading our own
  sealed rows does not re-verify (AES-GCM binds each row to its path). Snapshots verify once at
  import.

## 4. Sync (ALPN `tinline/group/5`)

- `Hello`/`Welcome` carry `meta_vv` and `days: [(day, vv)]` for the last `HELLO_DAYS` (7).
- `Update { doc: Meta | Day(day), update }`, `Ack { doc, vv }`. Meta updates are sent before
  shard updates in every session and every relay.
- No separate history protocol: every day is an ordinary Loro doc and every member holds a copy
  of all of them. A session exchanges an inventory, the meta vv plus `(day, vv)` for every day held
  (paged: `Days { after, days }`). Each side then sends `Update{doc}` for every doc the other lacks
  or is behind on: meta first, then the days newest first, rate-limited. "Backfill" is just this
  sync running until both inventories match. Scrolling only reorders the queue.
- Relay: hosts relay per doc exactly as today.
- Removed: `Removed { records, … }` carries meta records only.

## 5. API and app impact

- `group_messages(gid, before, limit)` pages across day shards backwards (loading shards
  lazily, showing a loading row while backfill is still filling older days), like `chat_day`.
  Return shape unchanged; cursor `"{at}:{id}~"` still works; `day` becomes real.
- Unread counts and the chat list row come from `GroupMeta` (last post, unread), updated on post
  apply, so the list never loads shards.
- Search: group hits carry the real day; reconcile per shard watermark like DMs.
- Calls (`calldir.rs`, `calls.rs`) read meta state only — unchanged.

## 6. Tests and acceptance

- Existing `group_core`, `group_g2`, `group_g3`, search tests pass (adapted to shards).
- New: post arrives before the meta it names (pending → shown); flags inside/outside `seen`
  (same verdicts as today's tests); members on different hosts converge per shard; history
  paging from a host past `HELLO_DAYS`; a post in the wrong doc / wrong day refused; restart
  with closed days; group-sim models updated if they assume one doc.
- Scale (re-run `year-bench` on `/home/abe/build-cache/search-year-data/`, regenerated for v2):
  group store open < 250 ms desktop for the year dataset; remote apply and local write < 10 ms
  in the 100k group; `group_messages` page < 10 ms; RSS after open < 100 MB.

## Decisions (user, 2026-10-12)

1. Shards by **UTC day**, like DMs.
2. **Verify once, on arrival**: loading our own sealed store does not re-verify signatures.
3. **One history, every member holds a full copy.** No special history: the days are ordinary
   Loro docs synced by version vectors like meta. Members connect only to hosts (U7); members never
   serve each other directly. Hosts and members exchange whatever days the other side lacks, in both
   directions on their session, so a new or wiped host is refilled from the members connected to it.
4. **Keep everything**: no retention window; closed days are compacted, never dropped.

## Implementation notes (2026-10-12)

Where the code chose a detail the design left open, or differs from it:

- **Doc ids.** `doc::DocId { Meta, Day(YYYY-MM-DD) }`; a day must be canonical and exist on the
  calendar (`Update`/`Ack` naming anything else are refused). Acceptance (`doc::fits`) is part of
  `ops_between`, so the verdict covers batches, snapshots and history alike. Both docs of a
  writer use the same Loro peer id (`writer_id`); counters are per doc, so a batch must always
  be applied to the doc it names (never guess the doc from the bytes).
- **Frontier.** `seen` of every record is the author's meta vv. `Positions` in `GroupState`
  keeps per-DID join/leave and flag positions plus the meta vv we hold; `post_visible` answers
  Pending if `seen` names an op we lack, otherwise Visible/Hidden by the old U11 rule. A hidden
  verdict is final (it depends only on ops inside `seen`), so only pending posts are re-judged;
  `GroupMeta.pending_days` lists the days that have some, so a meta change loads exactly those.
- **Device list.** Posts no longer carry a device into `GroupState.members[..].devices` (meta
  only), so each device writes one `EncKey` into meta when it first appears (previously only when
  its DID had none). "Appoint a host" lists devices from meta.
- **Chat-list state.** `GroupMeta` holds `last` (newest shown post), `unread`, `newest_day`,
  `pending_days` and `owed_days` (days with posts of ours no host acknowledged). `group_mark_read`
  sets `read_upto` to the newest line and `unread` to 0. `groups()` never loads a day doc.
- **Store.** Closed days are one sealed snapshot row (`shard/{gid}/{day}/snap`, `closed` in
  `ShardMeta`) rather than a blob; a late update reopens the day until the next compaction. The
  compaction runs once after open and then hourly, under the group lock one day at a time.
  `ShardMeta.vv` is kept current, so `Hello` day lists, the inventory and search watermarks never
  open a doc. Delivery state of a post uses acks per day (`ackd/...`); other records use the
  meta acks as before.
- **Cache.** At most 12 day docs per group in memory (LRU), loaded under the group lock.
  Reconcile and compaction read days without populating the cache.
- **Paging.** `group_messages` starts one hour after the cursor's day (a post may sit in the
  neighbouring day doc within the +- 1 h tolerance) and walks day docs backwards until it has
  `limit` posts, merging them with the system lines of the meta state by `(at, id)`. The return
  shape did not change: there is no explicit "loading" row; a page ends where the local days end
  (`older == None`) and days that arrive later show up on the next call.
- **Sync.** `push` sends the meta doc and then the named day docs per session, relaying every
  accepted update (old days too: only what the peer lacks by its known vv); a day the peer did not
  list is sent whole. A session start pushes the last 7 calendar days we hold. Removed members
  upload their pre-removal posts one `Update{Day}` per owed day.
- **Inventory sync.** After `Hello`/`Welcome` each side sends `Days{after, days}` pages (64 days,
  `(day, vv)` ascending, an empty page ends it) and records the peer's. Then, once the peer's
  inventory is in (30 s at most), it sends `Update{Day}` for every day the peer lacks or is behind
  on, newest first, one every 200 ms (`group_set_backfill_paused` holds it for a metered network
  or low battery). The pass repeats 3 s later and then every 5 minutes. It runs only towards a
  member we host (accepted, not removed, not a joiner) or a host we dialled, and waits until the
  group's genesis is held. An update for a day older than the 7 live days raises no new-message
  events. The receiver validates it like any update.
- **Search.** Group rows carry the real day; per-day watermarks are the day doc's vv; each
  `s/{src}/{doc}` index key stores a keyed tag of its day, so reconcile can drop the stale docs of
  one day without opening rows. Days with pending posts are always re-indexed.
- **group-sim** (`design-models:crates/group-sim`) is a standalone model with its own record types; it does not
  use the core docs and was not touched.
