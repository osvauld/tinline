# Group day shards — design

Status: draft for review (2026-10-12). Replaces "one Loro doc per group" (`group-core.md`
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

## 4. Sync (ALPN `tinline/group/2`)

- `Hello`/`Welcome` carry `meta_vv` and `days: [(day, vv)]` for the last `HELLO_DAYS` (7).
- `Update { doc: Meta | Day(day), update }`, `Ack { doc, vv }`. Meta updates are sent before
  shard updates in every session and every relay.
- History: `HistoryList { before, limit } -> [(day, vv, size)]`, `HistoryGet { day } ->
  snapshot`. Any host (all hosts hold every day) or member serves it; the receiver validates
  each record as on import. Members fetch older days only when the user scrolls (or search
  reconcile asks — no: search indexes only what is local).
- Relay: hosts relay per doc exactly as today.
- Removed: `Removed { records, … }` carries meta records only.

## 5. API and app impact

- `group_messages(gid, before, limit)` pages across day shards backwards (loading shards
  lazily, fetching history from a host when the local days run out), like `chat_day`.
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

## Open for the user

1. Day shards by UTC day like DMs (recommended) vs fixed-size chunks (N posts).
2. Members keep all days they ever received (recommended, like DMs) vs a retention window.
3. History for members only from hosts (simplest) or also from other members.
