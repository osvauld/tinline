# Scale: one heavy year of chat data

Status: measurement only (2026-10-12, branch `search-year`). Nothing in the product was changed
or optimised; the only product-code edits are `#[doc(hidden)]` test hooks (below).

## Dataset

One account, generated once through the real code paths, 365 days ending "now":

| Part | Size |
|---|---|
| Contacts | 40 (real hello/welcome exchange in process; the node is never started, no network) |
| 1:1 messages | 21,835 over 365 days (~60/day, Zipf over contacts: the top contact has ~11/day), 50% ours, 55 with attachment metadata; written with injected timestamps through `chat_persist` (so the search hook ran) |
| Closed days | 8,233 shard-days compacted to snapshot blobs (`CLOSE_AFTER_DAYS`); only the last 8 days stay open |
| Groups | 3 small (2,000 posts, 6/7/8 members), 1 busy (30,000 posts, 25 members), 1 huge (100,000 posts, 80 members). Every record is signed and goes through `GroupCore::apply` (validation, `rederive`, `store.append`, post diff, search hook) |
| Text | chat-sized (mean ~10 words), 72% English-like, 28% Hindi / Malayalam / CJK / mixed, Zipf vocabulary; planted rare words `zyxqrare`, `qwertyuniq` |
| Searchable messages | 157,835 (21,835 DM + 136,000 group posts) |

Generation took 957 s (DM compaction 668 s of it) with a 2.8 GB peak RSS. Caveats of the generator:
group posts were applied in batches (1,000 records; 5,000 for the huge group) instead of one update
per record, so the group update rows are fewer and bigger than in real use; every record carries
a `seen` frontier of all active writers (up to 80 x 12 bytes). Own DM messages are stored as
delivered (no outbox mark), so no day is "pending" and nothing blocks compaction.

## Machine

Desktop: Intel i7-1165G7 (4 cores / 8 threads, 2.8 GHz, boost 4.7), 40 GB RAM, NVMe ext4,
Linux 7.0, rustc 1.92, `--release`. **The machine is shared**: other agents build and run
benchmarks at the same time (load 2.6 to 14 during these runs), so timings are noisy. Tables give
the median of 3 full runs, with the range of the three in brackets. Page cache is warm
(the dataset copy is `sync`ed before each run); a cold-cache unlock is slower (see below). Phone
estimates use the 4 to 6x factor from the task.

## How to run

```sh
cd /home/abe/p2p_chat/.claude/worktree/search-year
python3 /home/abe/p2p_chat/scripts/buildlock.py --who me -- cargo build --release -p p2pcore --bin year-bench
BIN=$PWD/target/release/year-bench; D=/home/abe/build-cache/search-year-data/s1.0
python3 /home/abe/p2p_chat/scripts/buildlock.py --who me --run -- $BIN gen --scale 1.0 --dir $D   # ~16 min; --scale 0.02 for a quick one
python3 /home/abe/p2p_chat/scripts/buildlock.py --who me --run -- $BIN run --dir $D --scenario intact > out.json
```

`run` copies the dataset (`cp -a --reflink=auto`) to `<dir>-work-<scenario>`, measures on the
copy and deletes it, so the generated dataset is never modified. It prints one JSON object.
Scenarios: `intact` (everything), `rebuild` (search.redb deleted first: full rebuild), `bg`
(background search reconcile left on, as in the app), `dirty` (a child process writes and exits
without closing any store), `apply` (only remote applies in the huge group). The dataset used
here is `/home/abe/build-cache/search-year-data/s1.0` (2.4 GB; `s0.02` is a quick one).

Hooks added (all `#[doc(hidden)]`, none used by the product): `crates/core/src/synth.rs`
(synthetic peers, contacts, DM and group writers, remote-post writer, reconcile switch and
"reconcile now"), `chat/engine/synth.rs` (`Inner::synth_dm`), `group/engine/synth.rs`
(`GroupCore::apply_for_test`, group builder), `GroupDoc::import_for_test`, and in
`search/api.rs` a `NO_BG_RECONCILE` switch and `search_reconcile_now` (the group reconcile body is
now `search_reconcile_groups_run`, returning its handle; behaviour unchanged). The
per-function breakdowns below came from temporary timers that are not committed; they are in
`docs/scale-year-trace.patch` (`patch -p1 < docs/scale-year-trace.patch` from the repo root, they
print `TRACE ...` lines to stderr).

## Numbers (desktop)

Unless noted: p50 / p95 in ms, median of 3 runs [range of the 3 medians].

### Open, memory and disk

| What | Result |
|---|---|
| `Node::new` | 0.4 ms |
| unlock (`unlock_with_key`) -> `chats()` ready | **38 ms** [24 to 38]; once 2.2 s right after the 2.4 GB dataset copy (write-back, cold reads of `account.redb`) |
| of which chat store open (`chat.redb`, rekey check, `search.redb` open) | ~10 to 30 ms (account state load 4 to 9 ms, `Me::load` 1 to 4 ms) |
| `chats()` (41 conversations) | 0.29 / 0.31 ms |
| group store open (first `groups()` call; all 5 groups loaded) | **9.4 s** [7.4 to 16.3]; RSS **+709 MB** (17 MB -> 726 MB) |
| search reconcile, intact index | DMs 120 ms [66 to 3,211 (cold)], groups 54 ms [48 to 140] |
| search reconcile, **full rebuild** (search.redb absent) | DMs **109 s** + groups **54 s** = **163 s** (one run); RSS 726 MB -> 1.81 GB |
| open after an unclean exit (`dirty`) | unlock 631 ms (search.redb open 609 ms vs 3 to 10 ms clean); RSS jumps to 854 MB at unlock |
| RSS: after unlock / after group open / after reconcile | 17 / 726 / 780 MB. Peak: 1.81 GB (rebuild). The `intact` runs peak at 2.0 GB but that includes harness copies (`group_messages(u32::MAX)`, three remote member docs) |

Disk (`accounts/<id>/`): `chat.redb` 61.2 MB, `blobs/` 64.3 MB (8,233 snapshot blobs),
`group.redb` 192.8 MB, **`search.redb` 2,056 MB** (13 KB per searchable message; 6.5x everything
else together). `account.redb` 3.6 MB. After the group write measurements `group.redb` was
385 MB (it grew by a full copy, because every 64th write stores a snapshot of the whole group
doc and redb does not shrink).

### 1:1 chat

| Operation | p50 | p95 |
|---|---|---|
| `chat_day` newest day, first touch (30 contacts) | 0.88 [0.68 to 2.05] | 1.68 |
| `chat_day` newest day, cached | 0.80 | 1.50 |
| `chat_day` closed day ~200 days back, first touch (blob read + decrypt + Loro load) | 0.94 [0.93 to 2.41] | 2.32 |
| `chat_day` closed day, cached | 0.77 | 1.93 |
| `chat_day` closed day, heaviest contact (~10 msgs/day), cold, 25 days | 1.58 | 1.73 |
| DM send, first message to a contact today (new/open shard) | 5.6 [4.8 to 5.8] | 11.4 |
| DM send, subsequent (includes the index write: two redb commits) | 5.0 [4.2 to 7.8] | 8.9 |

### Groups (per group size)

| Posts in group | `group_messages` first page (50) | deep page (middle) | oldest page | local write (`group_post`) | remote apply (core) | remote after-hook |
|---|---|---|---|---|---|---|
| 2,000 (x3) | 0.4 / 0.5 | 0.4 / 0.4 | 0.4 / 0.4 | 11 / 24 (p95 up to 420 with fsync stalls) | 20 / 29 | 4.3 / 9 |
| 30,000 | 9.7 / 11.4 | 9.5 / 10.4 | 10.1 / 11.3 | 86 / 296 | 444 / 523 | 4.2 / 30 |
| 100,000 | **35.6 / 47.1** | 35.9 / 39.4 | 35.3 / 38.3 | **385 / 1,044** (max 1.7 s) | **2,752 / 3,479** [1,715 to 4,201] | 14.8 / 22 |

"remote apply (core)" is `GroupCore::apply` for one incoming post; "after-hook" is `group_after`
(search hook, relay, enforcement) with no sessions and no event listener. `groups()` (summaries of all
five) costs 3.0 ms warm.

### Search (`Node::search`, limit 20, first page then pages 2 to 6)

| Query | p50 / p95 | pages 2 to 6 p50 |
|---|---|---|
| `th` (common prefix) | 1.3 / 2.4 | 2.6 |
| `the` | 1.3 / 1.4 | 1.5 |
| `meeting` (common word) | 1.3 / 5.2 | 2.7 |
| `meeting tomorrow` (2 words) | 7.2 / 18.7 | 26.8 |
| `photo flight` (2 mid-frequency words) | 7.7 / 22.3 | 18.3 |
| `zyxqrare` (rare word, 20 hits) | 0.47 / 1.5 | 0.29 |
| `zyx` (rare prefix) | 0.42 / 0.56 | 0.17 |

## Ranked bottlenecks

Phone budgets: unlock < 1 s, page < 50 ms, send < 30 ms; phone = desktop x 4 to 6.

1. **Group store open: 9.4 s desktop, 38 to 56 s phone (budget 1 s), +709 MB RSS. FAIL.**
   `Inner::group_core` (`group/engine.rs` ~l.555) loads and verifies every group in full at open.
   Per-function (100k + 30k + 3x2k = 136k records, one run): `GroupDoc::entries` -> `ops_between`
   -> **`SignedRecord::open` (ed25519 verify + bincode of every record): 6.0 to 7.3 s, ~45 to
   60 us per record, ~75% of the open**; `GroupDoc::load` (Loro import of snapshot + updates) 0.4 to
   0.9 s; `store.doc_rows` (scan + AES-GCM unseal) 0.25 to 0.4 s; `state::derive` 0.15 to 0.4 s.
   Memory is ~5.2 KB per post: the Loro doc, `Live.entries` (a decoded `Record` per post, with
   its `seen` vector) and `state.items` (a second copy of each text). It is not part of the unlock
   call (the app opens it lazily/at `start`), but it is what stands between "unlocked" and "groups
   usable", and 700 MB is a real risk for an Android background process.
2. **Group remote apply: 100k posts 2.75 s p50 (1.7 to 4.2 s), 30k 0.44 s, 2k 20 ms. Phone: 10 to
   16 s for one incoming post in the huge group, 2 to 3 s in the busy one. FAIL for >= 30k posts.**
   `GroupDoc::apply_remote` (`group/doc.rs`): `self.doc.fork()` 0.8 to 1.1 s and
   `scratch.import(update)` 0.9 to 1.0 s (importing one record into the fork) = ~85 to 90% (the
   validation copy is O(whole doc) per message); then `rederive` -> `state::derive` 0.15 to 0.2 s
   (O(n) over all entries), `posts_of` + `post_diff` (clone of every post text, twice) 0.17 to
   0.25 s, `store.append` 12 ms (a snapshot every 64th). Cost grows with group size, not message
   size, and a burst of N incoming messages in a busy group pays it N times unless the host batches.
3. **Group local write: 100k posts 385 ms p50, 1.0 s p95, 1.7 s max; 30k 86 ms. Phone: 0.4 to
   2.3 s (huge), ~0.4 to 0.5 s (30k). FAIL (budget 30 ms) from ~5k posts up** (2k posts: 11 ms p50
   desktop, 45 to 65 ms phone, borderline). `GroupCore::write`, per call at 100k: `rederive` 147 ms,
   `post_diff` + new-items filter 97 ms, `posts_of` 30 ms, the id `HashSet` 14 ms,
   `store.append` 12 ms (commit; the 64th write exports and seals a full snapshot, ~1 s), `doc.write`
   0.2 ms (the actual signing and Loro insert), plus ~70 ms in the callers (`group_after`:
   `search_group` rows, `group_message`). Everything that scales is derived state rebuilt from scratch.
   Write amplification: every 64th write re-stores the whole doc (`group.redb` doubled in the runs).
4. **`group_messages` page: 100k 35.6 ms (47 p95), phone ~150 to 215 ms (budget 50). FAIL for the
   huge group; 30k is 9.7 ms (phone 40 to 60 ms, borderline); <= 2k fine.** `group_messages`
   (`group/api.rs` ~l.440) clones all `Item`s (`state.items.clone()`) under the group lock, then
   keeps 50. The same clone pattern is in the search reconcile (`search_reconcile_groups_run`).
5. **Search index size and rebuild: `search.redb` 2.06 GB; full rebuild 163 s desktop (DMs 109 s,
   groups 54 s) = 11 to 16 min on a phone, RSS up to 1.81 GB. FAIL on disk and memory (it runs in
   the background, so no latency budget applies).** 13 KB per message because each word is indexed
   as all its 2 to 16 char prefixes (`search::text` tokenizer + `SearchIndex::apply` posting keys);
   8x the size of all the chat data together. The DM part is 13 ms per shard-day (8,233 shards:
   closed-blob read + decrypt + Loro import in `search_reconcile_dms` + one commit per day);
   the group part is ~0.4 ms per post. (Rebuild attribution to those functions is inferred from the
   per-unit costs, not profiled.) An intact index reconciles in ~0.1 s.
6. **Unclean exit: opening the 2 GB `search.redb` after a kill costs +0.6 s and the process RSS
   jumps by ~850 MB at unlock (redb repair filling its page cache), then stays.** Phone: ~3 s
   and a memory spike on every restart after a kill. Cheap on desktop, risky on a low-memory
   phone. (Observed once; not isolated further.)
7. Search, DM and unlock are fine: `search` <= 3.5 ms p50 for single words (phone <= 20 ms),
   two-word queries 7 to 18 ms p50 and up to ~35 ms for deep pages (phone 40 to 200 ms for
   pages 2 to 6, background paging); DM send 4 to 8 ms p50, <= 17 ms p95 (phone 20 to 50 ms p50
   worst case, near the 30 ms budget at p95); `chat_day` <= 2.4 ms even for closed days;
   unlock -> chat list 38 ms (phone 0.2 s). One cold-cache unlock took 2.2 s on desktop
   (first read after a 2.4 GB copy), so the 1 s budget depends on flash being warm.

## What this says

Everything 1:1 and the search *queries* scale to a year. Everything in groups scales with the
size of the biggest group, because a group is one doc that is fully verified at open, fully
cloned per page, fully re-derived per write and fully forked per remote apply. The search
*index* is the other outlier on disk and rebuild cost. Nothing was fixed here.
