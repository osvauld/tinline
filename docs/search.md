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

- `src` = `dm.{pair}` for a 1:1 conversation, `g.{gid}` for a group. `day` is the DM shard day;
  groups use `-` (one doc per group).
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
- Groups: `GroupEngine::write` and `apply` (`group/engine.rs`) after `store.append`: diff
  `state.items` before/after by id and text (new, edited, deleted posts).
- Names: contact add/rename/block (account doc), group create/rename/leave, own name.
- Removal: contact purge (`chat_purge`) and group forget delete `d/{src}/`, its postings (from
  the rows' `toks`) and `w/{src}/`.
- Lifecycle: opened and closed with the chat/group stores (stop, lock, account switch); listed
  wherever `chat.redb` is (account deletion, rekey).

## Reconcile

On open (in the background, never blocking unlock), for every DM shard and group compare the
store's current version vector with `w/{src}/{day}`; re-index those that differ (open shards
from the vault, closed shards from their snapshot blob). Missing file or `v` mismatch = all
differ. Indexing a message is idempotent: delete its old row/postings, write the new ones.

## Apps

- Android: a search field at the top of Chats (and Groups); results in two sections, People &
  groups, then Messages (avatar, name, snippet with bold matches, date). Tapping a message opens
  the conversation at that day and scrolls to the message, briefly highlighted.
- Jumping to a hit: a DM hit opens with `chat_day(conv, Some(day))` (the message is in that
  page; `id` finds it). A group hit has `day = "-"`: `group_messages(conv, before =
  "{at}:{id}~", limit)` returns the page that ends with the post (the `~` sorts after any id, so
  the post itself is included); older pages follow from its `older` cursor as usual, newer items
  from `group_messages(conv, None, …)`.
- Desktop (iced): the same, a search box above the conversation list; Ctrl+K / Ctrl+F focuses it.
- Debounce 150 ms; minimum 2 characters for message search (names from 1).

## Implementation notes (S1, `crates/core/src/search/`)

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
- **Group hook**: `GroupCore::write` / `apply` diff the posts by id and text (`Changed::posts`:
  added or changed posts, ids no longer shown, vv before and after); `group_after` indexes them.
  The groups' state is in memory whole, so group reconcile diffs every group whose watermark
  differs against the index (`s/g.{gid}/`), run when the index or the group store opens.
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

## Benchmarks

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
