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
- Desktop (iced): the same, a search box above the conversation list; Ctrl+K / Ctrl+F focuses it.
- Debounce 150 ms; minimum 2 characters for message search (names from 1).
