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
