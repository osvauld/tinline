# Search — design

Status: design (2026-10-11), decided with the user. Built on `integrate/all` (branch `search`).

One local search over 1:1 chats, group chats and names (contacts, groups, "You"). Nothing leaves
the device: every device indexes what it holds, as Loro updates are accepted.

## Decisions

- **Separate file**: `search.redb` in the account directory, next to `chat.redb` and
  `group.redb`. It is derived data: deleting it is safe, the next open rebuilds it.
- **Encrypted at rest**: every value is sealed (`storage::Sealed`, AES-256-GCM, bound to its
  key path) under a key derived from the recovery phrase like the other stores
  (`blake3::derive_key("tinline/search-store/v1", …)`). Keys cannot be sealed (redb sorts them),
  so a key never holds a word: only a keyed hash of one (see Postings).
- **Per account**: one `search.redb` per account directory (`accounts/<id>/`), sealed with
  that account's phrase-derived key. Search covers only the selected (online) account; the
  index opens and closes with that account's chat/group stores, so an account switch switches
  the index and results never mix accounts. Removing an account removes its directory and the
  index with it.
- **Updated on accept**: the index is written right after a chat/group change is committed (not
  in the same transaction: a different file). Watermarks make that safe (see Reconcile).

## Layout

```text
d/{src}/{id}                         DocRow    sealed: kind, conv, day, at, author, text, toks
t/{tok}/{rev_at}/{src}/{id}          empty     a posting: message {src}/{id} contains prefix tok
n/{kind}/{id}                        NameRow   sealed: kind (contact|group|self), id, name
w/{src}/{day}                        bytes     watermark: the shard/group version vector indexed
v                                    u32 BE    index format version (mismatch -> rebuild)
```

- `src` = `dm.{pair}` for a 1:1 conversation, `g.{gid}` for a group. `day` is the shard day (a
  group's day doc is the UTC day of its posts, `group-shards.md`). Each `s/{src}/{doc}` key holds
  a keyed tag of its day, so one day's docs can be listed without opening rows.
- `tok` = first 16 bytes, hex, of `blake3::keyed_hash(index_key, normalized prefix)`.
  `index_key` is derived separately from the sealing key. Without it a token cannot be
  reversed or guessed offline.
- `rev_at` = `u64::MAX - at` as 16 hex digits, so a posting scan returns newest first.
- `DocRow.toks` is the exact token list written, so an edit or delete removes the old postings
  without re-deriving them.

What the file reveals without the key: per posting, its time and that some token recurs across
messages. Not words, not names, not authors. (The chat store already reveals day-level timing.)

## Tokenizing

- NFKC, then lowercase. No diacritic stripping in general: Indic vowel signs are combining
  marks and must stay. Latin letters may fold diacritics (é→e) by NFD + dropping `Mn` only when
  the base char is Latin.
- Words: Unicode word boundaries (`unicode-segmentation`), letters/digits only.
  CJK, Thai, Lao, Khmer, Myanmar runs (no spaces): overlapping character bigrams.
- Each word is indexed as all its prefixes from 2 to 16 chars (chars, not bytes). A 1-char word
  is indexed as itself.
- Queries use the same normalizer. A query word longer than 16 chars is looked up by its 16-char
  prefix and checked against the row text.

## Query

`search(query, limit, cursor) -> SearchResults { names, messages, next }`

1. Names: case-folded substring match over the `n/` rows (few hundred; decrypted and cached
   in memory per open).
2. Messages: normalize; pick the longest query term; scan `t/{tok}/` from the cursor (newest
   first); for each candidate open its `DocRow` and keep it if every query term is a prefix of
   some word in it (bigram scripts: substring). Stop at `limit`; `next` = the last
   `rev_at/src/id`.
3. Each hit: `kind` (dm|group), `conv` (contact DID or gid), `day`, `id`, `at`, `author`,
   `snippet` (around the first match, ~80 chars) and highlight ranges (UTF-16 offsets for
   Android, char offsets for desktop: return char offsets, the apps convert).

Deleted messages are removed from the index; edits re-index. Only posts are indexed in groups
(not system lines: joined/left/flags).

## Hooks

- 1:1: `chat_conv_ops` (`chat/engine.rs`) sees every accepted change (live push, own-device sync,
  local send/edit/delete, history import). After the transaction commits, index the touched
  records and store the shard's vv as the watermark.
- Groups: `GroupCore::write` and `apply` (`group/engine.rs`) after the store append: the posts
  that show now, per day doc (new posts, and posts that were pending on meta records).
- Names: contact add/rename/block (account doc), group create/rename/leave, own name.
- Removal: contact purge (`chat_purge`) and group forget delete `d/{src}/`, its postings (from
  the rows' `toks`) and `w/{src}/`.
- Lifecycle: opened and closed with the chat/group stores (stop, lock, account switch); listed
  wherever `chat.redb` is (account deletion, rekey).

## Reconcile

On open (in the background, never blocking unlock), for every DM shard and group day doc compare
the store's current version vector with `w/{src}/{day}`; re-index those that differ (open shards
from the vault, closed shards from their snapshot blob). Missing file or `v` mismatch = all
differ. Indexing a message is idempotent: delete its old row/postings, write the new ones.

## Apps

- Android: a search field at the top of Chats (and Groups); results in two sections, People &
  groups, then Messages (avatar, name, snippet with bold matches, date). Tapping a message opens
  the conversation at that day and scrolls to the message, briefly highlighted.
- Jumping to a hit: a DM hit opens with `chat_day(conv, Some(day))` (the message is in that
  page; `id` finds it). A group hit has the post's UTC `day`; the app jumps with `group_messages(conv, before =
  "{at}:{id}~", limit)` returns the page that ends with the post (the `~` sorts after any id, so
  the post itself is included); older pages follow from its `older` cursor as usual, newer items
  from `group_messages(conv, None, …)`.
- Desktop (iced): the same, a search box above the conversation list; Ctrl+K focuses it (Ctrl+F
  inside a chat is find in chat, below).
- Debounce 150 ms; minimum 2 characters for message search (names from 1).

## Find in chat (decided 2026-10-12)

Search inside one open conversation (a find bar in the chat screen), on the same index.

`search_in(kind: SearchConvKind, conv: String, query: String, limit: u32, cursor: Option<String>)
-> SearchResults` (`names` always empty; `messages`, `next` as in `search`; same cursor shape, same
`Locked` error, same snippet/highlights). Newest first. A conversation the chat list does not show
(or a group this device does not hold) returns nothing.

- **Segments**: the manifest already lists each segment's `(src, day)`; segments without the
  conversation are never opened. Inside a segment, a per-segment doc table `z/{seg}` (sealed, one
  u16 per doc: the index into `days`, hence its src; written with the segment, cached per open
  segment) lets the scan drop other conversations' postings without reading their rows.
- **Hot tier**: the in-memory postings are filtered by src the same way.
- **Budget**: other conversations' postings dropped by the doc table are cheap, so they count
  against a separate, larger budget (200 000); postings of this conversation count against the
  normal 20 000. A partial page sets `next` as today.
- **No exact match count** (it would mean scanning every posting of a common prefix): the bar
  shows the hits loaded so far and steps through them (up = older, down = newer), loading the
  next page when it reaches the end ("17+" while `next` is set).
- **Apps**: Android: the chat screen's overflow menu / search icon opens a find bar over the
  message list; desktop: Ctrl+F in a chat opens it (Ctrl+K stays the global search). Each step
  jumps exactly as a global hit does (DM: `chat_day(conv, day)`; group: `group_messages(conv,
  before = "{at}:{id}~", limit)`), scrolls to the message and highlights the matched words.
  150 ms debounce, from 2 characters.
- Format: the `z/` table is new, so the index format version goes to 3 (rebuilt on open).

## Implementation notes (S1, v1 layout; replaced by v2 below, `crates/core/src/search/`)

Where the code differs from the plan above, and why.

- **API** (`search/api.rs`): `Node::search(query, limit, cursor) -> SearchResults { names:
  Vec<NameHit { kind: SearchNameKind (Contact|Group|Me), id, name }>, messages: Vec<MessageHit {
  kind: SearchConvKind (Dm|Group), conv, conv_name, day, id, at, author, author_name, snippet,
  highlights: Vec<HitRange { start, end }> }>, next }`. `HitRange` rather than `Range` (Swift has
  a `Range` type). `conv_name` / `author_name` added so the apps need no second lookup. Names
  come with the first page only (`cursor = None`). `limit` is clamped to 1..=100; one call looks
  at most 20 000 postings (a very common prefix returns a partial page with `next` set).
- **Names are not indexed** (no `n/` rows): contacts, kept histories (people whose block deleted
  their contact entry), groups and "You" are all in memory already, so the query matches them
  there (folded like messages, substring). Nothing to hook on add/rename/block/leave, and never
  stale. "You" also matches our own profile name.
- **Layout**: message keys use a doc hash instead of `{src}/{id}`, to keep postings small (a
  message has ~60 postings): `d/{h}` (DocRow), `s/{src}/{h}` (empty, the docs of a
  conversation), `t/{tok}/{rev_at}/{h}` (empty), `w/{src}/{day}` (sealed vv), `v` (sealed
  u32). `h` = 16 hex of `blake3::keyed_hash(index_key, "doc\0" src "/" id)`; `tok` = 32 hex of
  `keyed_hash(index_key, "term\0" term)`. The cursor is `{rev_at}/{h}`.
- **No `toks` in DocRow**: a doc's postings are re-derived from its stored text, which is
  deterministic for one format version (a tokenizer change bumps `v`, and the index is rebuilt).
- **Indexed text**: the message text, plus the file name of a non-voice attachment. Deleted
  messages and texts without words have no row.
- **CJK single characters**: a 1-char query term in a bigram script only finds 1-char runs
  (there is no unigram index); the apps search messages from 2 characters anyway.
- **1:1 hook**: in `chat_persist` (it gained a `recs` argument: the messages the change touched)
  rather than around `chat_conv_ops`: every `chat_conv_ops` path ends there, except history
  import (`chat_apply_history`), which indexes the whole day itself. The watermark moves only from
  the shard's previous version (`Mark::prev`), so a hook never marks a shard whose older changes
  are missing from the index.
- **Group hook**: `GroupCore::write` / `apply` return the posts that show now per day doc
  (`Changed::posts`: the posts, the doc's vv before and after); `group_after` indexes them with
  the real day. Group reconcile works per day doc like DMs: each whose watermark differs (and
  each with pending posts) is read once, its shown posts written and the docs of that day
  (`SearchIndex::day_keys`) that no longer show dropped; run when the index or the group store
  opens. A pending post (it names meta records not held yet) is indexed when it starts to show.
- **Removal**: there is no `chat_purge` in the code: blocking keeps the history (U19), and a
  block's deletion request removes the contact entry but keeps the history read-only (U20). So no
  1:1 removal hook exists; instead DM hits are filtered at query time to the conversations the
  chat list shows (contacts, kept histories, "You"), and group hits to groups this device holds.
  `Inner::search_forget(src)` is there for a future purge; group forget (a refused or timed-out
  join) calls it.
- **Lifecycle**: opened in `chat_open` (same key material), closed in `chat_close` /
  `chat_close_now` (lock, account switch, drop). Like `chat.redb` it stays open across
  `stop`. Reconcile runs in the background after every open. A closed day is read from its
  snapshot blob; if the blob is not local the day is skipped (its watermark stays behind, so the
  next open tries again).
- **Rekey**: `search.redb` was always under a phrase-derived key, so it is never migrated or
  quarantined; any file whose `v` row is missing, unreadable or of another version is emptied
  and rebuilt. Account removal deletes it with the account directory.
- **Writes** are synchronous, one redb transaction per change, after the chat/group commit; a
  failure is logged and left to reconcile. A reconcile racing a live edit can leave the older
  text indexed until the next open (its watermark then still differs, so it heals).

## Index v2: hot tier + time segments (decided 2026-10-12)

Replaces the per-message posting B-tree for messages (the v1 layout above), after the benchmarks
below: ~87 random postings per message cost 15 KB of copy-on-write pages per commit, a 6 s worst
write on the send path, 1.5-4 GB at 100k messages. Chats are already time-sharded and old days
almost never change, so the index is sharded by time too. Names, the API, the cursor shape,
tokenizing (except the prefix cap) and the sealing stay as they are.

### Tiers

- **Hot tier, memory only.** Every *hot* day: a DM shard day or group day within `HOT_DAYS` (8,
  the DM compaction age) of today (UTC), plus any *dirty* cold day (below). Built in the
  background on open from the stores (a few days of messages), updated in memory on every change
  (send, push, sync, edit, delete, group apply). **No redb write on the send path.** Dropped and
  zeroized on lock / close / account switch. Size cap `HOT_MAX_DOCS` (e.g. 20 000): over it, the
  oldest dirty cold months are flushed into segments early.
- **Cold segments, written once.** When days fall out of the hot window (checked on open and
  once a day while running), all newly cold days across all conversations are written as one new
  segment in one transaction. Segments of the same UTC month are merged (one rebuild of that
  month's days) when a month has more than 4 or the month is over. A segment is capped at
  `SEG_MAX_DOCS` (e.g. 20 000) docs; a busy month becomes several. Busy and quiet conversations
  share segments, so a quiet chat costs almost nothing.
- **Dirty cold days.** A change to a day already in a segment (a late or backfilled group post, a
  pending post turning visible, an edit/delete of an old message) does not touch the segment: that
  (src, day) is loaded into the hot tier and the segment's docs of that day are skipped at query
  time. The owning month is rebuilt in the background (debounced, e.g. 10 min, or at the next
  open). Backfill of many old days goes straight to segment builds by month instead of the hot
  tier.

### Layout (format version 2; a v1 file is emptied and rebuilt)

```text
m                       sealed manifest: segments [{seg, month, max_at, min_at, docs,
                        days: [(src, day, vv)]}]; replaced atomically with each segment write
x/{seg}/{tok}           sealed postings of one term: local doc numbers, newest first, varint-delta
y/{seg}/{chunk}         sealed doc rows, 64 a chunk: (h, src, kind, conv, day, id, at, author, text)
v                       sealed u32 format version
```

- `tok` as in v1 (keyed BLAKE3), prefixes now **2..=8 chars** (a longer query term is looked up
  by its 8-char prefix and checked against the row text, which the query already does).
- Docs in a segment are numbered newest first, so a posting list is already in result order.
- Without the key the file shows segment sizes and months and which tokens recur within a
  segment; no per-message times (v1 had one in every posting key).
- Watermarks are the manifest's `(src, day, vv)`; a day whose store vv differs is dirty.

### Query

Unchanged API and result order (newest first, cursor `{rev_at}/{h}`). Sources: the hot tier and
the segments whose `max_at` is older than the cursor. A lazy k-way merge by (at, h) over the
longest term's posting list from each source; segments are opened only once the merge reaches
their `max_at`, so a page usually reads the hot tier and one or two segments. Docs of dirty days
are skipped in segments. Same AND check against the row text, same snippet/highlights, same scan
budget (20 000 postings) and `next`.

### Crash safety

The hot tier is derived from the stores; segments and the manifest are written in one
transaction. A crash loses at most the in-memory tier, rebuilt on open. Reconcile on open: hot
days -> memory; cold days covered by the manifest with the same vv -> nothing; otherwise dirty.

### Targets (desktop, 100k messages; emulator figures recorded too)

Send-path index cost < 0.5 ms p99; `search.redb` <= 1.5 KB/message; first full build of 100k
< 60 s; hot-tier build on open of 8 days measured (and in `scale-year.md` for the year
dataset); queries <= 10 ms p95.

### Implementation notes (v2, `crates/core/src/search/{index,hot,seg}.rs`)

Where the code settles what the design above leaves open.

- **Records.** `x/{seg:08x}/{tok:016x}/{part:03x}`: a posting list is cut into parts of 2 048
  docs (doc numbers as varint deltas from the part base), and `y/{seg}/{chunk}` holds 8 rows, not
  64. Reason, measured: redb grows its file by doubling regions when records of several KB sit
  between many tiny ones; one 100 KB posting list or 10 KB chunk per record made `search.redb`
  3-20x the size of its content. With records well under a 4 KiB page, and put in key order, the
  file is within 2-4x of the content (redb's doubling steps). Terms are 16 hex of the 8-byte
  keyed hash (not 32), doc hashes 8 bytes.
- **Watermarks** are an 8-byte digest of the version vector (`digest_of`, order independent),
  kept per `(src hash, day number)` in the manifest and in memory. A day belongs to the newest
  segment that lists it; older copies are ignored at query time and dropped at the next merge.
- **Hot window.** A day is hot while `today - day < HOT_DAYS` (UTC; `set_today` replaces the clock
  in tests and benchmarks). Writes never touch the file. A change to a segmented day loads that
  (src, day) from the segment into memory first, only if the change alters something (an
  identical re-index of a cold day does nothing).
- **When segments are written (`maintain`).** `Mode::Force` on open (after the background
  reconcile) and in tests: every cold day. `Mode::Tick`, once a minute from the reconcile task:
  days that fell out of the window or waited the 10 minute debounce. `Mode::Cap`, from
  `apply_bulk` (reconcile and history import): only the oldest cold days, until the memory tier
  is under half of `HOT_MAX_DOCS`; no merges in a bulk, they happen at the final `Force`. If a
  day changed while its segment was being written it stays in memory.
- **Merge.** A month is rewritten whole (its still valid days plus the new ones) when it would
  have more than 4 segments, or it is over and has more than the minimum for `SEG_MAX_DOCS`.
  Segment builds run on all cores. A month that was merged and has not changed is not merged
  again.
- **Query.** `Scan` is the lazy merge. For several terms the segments' posting lists of all
  terms are intersected part by part before any row is read (the first term walks, the others
  only narrow; the AND check on the row is unchanged), the hot tier checks the other terms'
  posting sets. The 20 000-posting budget counts candidates after that narrowing. The cursor
  is `{rev_at:016x}/{doc hash:016x}` (the doc hash is 16 hex again, so the cursor shape is as
  before); inside a segment the first posting after a cursor is found by bisection over the
  rows. The cursor orders by `(rev_at, hash)`, so a tier change between pages cannot repeat or
  skip an unchanged doc.
- **Forget** (`forget(src)`) drops the hot days and rewrites the manifest without the
  conversation's days at once (its rows stay in the segments until their month is merged, and
  are never returned: only days a segment owns count).
- **Lifecycle.** `close()` wipes (zeroizes) the hot tier and the manifest view. Lock, account
  switch and drop already call `search_close`. The reconcile task also runs the once-a-minute
  maintenance (one task per index).
- **Reconcile.** Compares the store's version vector with `watermark_is` (hot, then manifest);
  a day the manifest knows with the same vector is skipped. A crash between a hot change and its
  segment rewrite leaves the old digest in the manifest, so the day differs on the next open and
  is re-read from the store; the stale rows of the segment are overridden by the hot copy until
  the rewrite. Tests that plant stale rows (`search_plant_group_post_for_test`) now fix them
  with a reconcile instead of a reopen (a reopen drops memory).
- **Names** unchanged (in memory, not indexed). Query terms longer than 8 chars: the lookup is
  their 8-char prefix, the row check does the rest (tested).

## Benchmarks v2 (2026-10-12, release, same machine; load from other jobs varied)

`search-bench` now replays the corpus in time order through `apply` (memory only) with a
background thread writing the cold days into segments on every simulated day change (a day passes
every ~0.1-1 ms of the run, much harder than reality), then a full build in the reconcile shape
(per conversation-day batches into `apply_bulk`, then the final write), then the open of a hot
tier, then the same queries as v1.

| | v1 10k | v2 10k | v1 100k | v2 100k |
|---|---|---|---|---|
| send-path index cost p50 / p95 / p99 | 5.6 / 29.8 / 150 ms | **0.105 / 0.37 / 0.72 ms** | 9.8 / 50.1 / 198 ms | **0.104 / 0.50 / 1.29 ms** |
| send-path max | 670 ms | 18 ms | 6.1 s | 29 ms |
| `search.redb` size (per message) | 129 MiB (13.5 KB) | 64.8 MiB (6.9 KB on disk; content 2.1 KB) | 1542 MiB (16.2 KB) | 514 MiB (5.4 KB on disk; content 1.3 KB) |
| full build (reconcile shape) | 91 msgs/s (110 s) | 3 149 msgs/s (3.2 s) | 69 msgs/s (24 min) | **42 s** (2 385 msgs/s; 62-69 s on a loaded run) |
| hot tier of the last 8 days on open | n/a | 9.5 ms (100 msgs) | n/a | 53 ms (976 msgs) |
| query "th" p50 / p95 | 0.72 / 0.76 ms | 2.0 / 2.1 ms | 1.05 / 1.16 ms | 0.57 / 0.76 ms |
| query rare word | 0.07 / 0.09 ms | 0.40 / 0.45 ms | 0.36 / 0.38 ms | 0.56 / 0.75 ms |
| query "meeting tomorrow" | 5.0 / 5.2 ms | 2.0 / 2.0 ms | 7.5 / 8.9 ms | 6.5 / 7.7 ms |
| query "the call" | 0.95 / 0.98 ms | 7.1 / 7.4 ms | 1.25 / 1.32 ms | 5.2 / 6.1 ms |
| "th" page 5 alone | 0.94 / 1.4 ms | 2.8 / 3.9 ms | 1.2 / 2.0 ms | 0.76 / 1.0 ms |

(The 10k column has 24 segments of ~400 docs each because 10k spread over 730 days leaves a few
messages a month; its queries open all of them, so it is slower than the 100k case. A real chat
history is dense in time.) Targets: send-path p99 < 0.5 ms **missed at 100k (1.3 ms; p50 0.1
ms)**: the tail is the bench's simulated clock, which makes the segment writer compete for the
same cores and the same memory lock every few hundred messages; the cost of one `apply` itself
is about 0.05-0.1 ms. File size <= 1.5 KB/msg: the **content** is 1.3 KB/msg at 100k, but redb's
doubling file steps (and free pages left by merges) put the file at 5.4 KB/msg; compacting or
a smaller step is open. Full build < 60 s met (42 s; 62-69 s when the machine was loaded).
Queries <= 10 ms p95 met. The emulator was not run.

## Benchmarks (v1, kept for comparison)

`search-bench` (crates/core/src/search/bench.rs, binary `crates/core/src/bin/search-bench.rs`)
drives `SearchIndex` directly: no node, no network, no names or conversation lookups. Corpus:
synthetic, seeded, 5-40 words a message, Zipf word frequencies; 72% English (8k pseudo-words plus
common ones), 10% Hindi, 6% Malayalam, 8% CJK runs, 4% mixed Hindi/English; 50 conversations (10
groups), spread over 730 days. About 87 postings a message. Query times are the loop of
`Inner::search` (page, row read, normalise, match, snippet) at 20 hits a page, warm cache, 30 runs.

```
python3 scripts/buildlock.py --who me -- cargo build --release -p p2pcore --bin search-bench
python3 scripts/buildlock.py --who me --run -- $PWD/target/release/search-bench 100000 /path/to/scratch/dir
# Android x86_64 emulator (libaom install as in android/app/build.gradle.kts):
ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/28.2.13676358 AOM_DIR_x86_64_linux_android=$HOME/build-cache/p2p_chat/libaom/install-v3.14.1-x86_64-linux-android \
  python3 scripts/buildlock.py --who me -- cargo ndk -t x86_64 -P 28 build --release -p p2pcore --bin search-bench
adb -s emulator-5554 push target/x86_64-linux-android/release/search-bench /data/local/tmp/
python3 scripts/buildlock.py --who me --device emulator-5554 -- adb -s emulator-5554 shell /data/local/tmp/search-bench 10000 /data/local/tmp/sb
```

Results (2026-10-12, release build, `/home/abe/build-cache` NVMe ext4; desktop = i7-1165G7, 8
threads, 38 GB, other agents' jobs running at times; emulator = osvauld-api35 x86_64, 4 vCPU on the
same host, so its fsync is cheap and it is not a phone figure):

| | desktop 10k | desktop 100k | emulator 10k |
|---|---|---|---|
| live write per message p50 / p95 / p99 | 5.6 / 29.8 / 150 ms | 9.8 / 50.1 / 198 ms | 4.6 / 8.5 / 20 ms |
| live write max | 670 ms | 6.1 s | 2.4 s |
| rebuild, per day-shard batches (avg 1.5 msgs at 10k, 4.2 at 100k) | 91 msgs/s | 69 msgs/s | 209 msgs/s |
| rebuild, one batch per conversation (~200-2000 msgs) | 339 msgs/s | 312 msgs/s | 485 msgs/s |
| query "th" (2-char common prefix) p50 / p95 | 0.72 / 0.76 ms | 1.05 / 1.16 ms | 2.1 / 2.6 ms |
| query rare word p50 / p95 | 0.07 / 0.09 ms | 0.36 / 0.38 ms | 0.20 / 0.22 ms |
| query 2-word AND "meeting tomorrow" p50 / p95 | 5.0 / 5.2 ms | 7.5 / 8.9 ms | 8.2 / 10.5 ms |
| query 2-word AND "the call" p50 / p95 | 0.95 / 0.98 ms | 1.25 / 1.32 ms | 1.6 / 1.7 ms |
| query no hit p50 / p95 | 0.003 / 0.003 ms | 0.003 / 0.004 ms | 0.005 / 0.005 ms |
| "th" page 5 alone p50 / p95 (pages 1-5 total) | 0.94 / 1.4 ms (4.8 / 5.6) | 1.2 / 2.0 ms (6.3 / 9.7) | 1.4 / 1.8 ms (7.3 / 8.6) |
| `search.redb` after live writes | 129 MiB (13.5 KB/msg) | 1542 MiB (16.2 KB/msg) | 129 MiB |
| `search.redb` after rebuild (day-shard / per-conversation) | 257 MiB | 2056 / 4096 MiB | 257 MiB |

The 100k desktop run was not repeated on the emulator (it would write 1.5-4 GB there). File sizes
step in powers of two (redb grows the file in doubling regions), so they are upper steps, not
exact; the live-write 100k index fits in 1542 MiB.

### Where the time goes

- Queries are fine: every case is under 10 ms at 100k, "th" 1.1 ms. The 2-word AND is the slowest
  (7-9 ms) because it scans the postings of the longer word and reads and matches a row for each
  until 20 contain both.
- **Write latency is over budget** (desktop p95 30-50 ms, p99 150-200 ms, p50 6-10 ms; emulator
  p95 8.5 ms). The work in `apply` is not it: building the ~87 postings (term set plus keyed
  blake3) costs 0.05 ms a message, and a one-key commit costs 1.2 ms (the fsync floor). The rest is
  the redb transaction: a posting key is `t/<hashed term>/<rev time>/<doc>`, so the 87 keys of one
  message land in 87 unrelated leaf pages of the B-tree. Every commit copy-on-writes those pages
  (about 4 KB each, plus branch pages) and flushes them, hence the roughly 15 KB written per message
  that also shows in the file size, and the cost growing with the index (more pages, less cached).
  Tail stalls (hundreds of ms to seconds) are redb's page allocation and file growth plus the
  disk's flushes under load.
- **Rebuild** is bound by the same per-commit cost: 70-90 msgs/s with the real reconcile shape
  (one transaction per day shard, small shards in this synthetic spread), 310-340 msgs/s with
  conversation-sized batches. A 100k-message history would take about 20 minutes to reconcile
  per-shard on this desktop, 5 minutes in big batches. The rebuild file is larger than the live
  one (free pages from copy-on-write are not returned to the file).
- **Size**: 13-16 KB a message for about 87 postings of ~75-byte keys (~6.5 KB of raw keys) plus the
  sealed row; 1.5 GB at 100k messages on disk is the number to look at before launch.
