# Audit 04: crates/core chat/, sync/, search/, profile/

Scope read: all production files in `crates/core/src/{chat,sync,search,profile}` (about 14.2k lines), `chat/doc_tests.rs`, and the in-file tests of `search/index.rs`. Compared against `group/{engine,doc,store,wire}.rs`, `node.rs`, `rekey.rs`, `store.rs`, `version.rs`. Read-only. Line numbers are as of HEAD `214a664` plus the dirty tree. "CROSS-FOLDER" marks overlap with code outside this scope.

## 0. Ranked summary (payoff / risk)

| # | Finding | Lines saved (est.) | Risk | Payoff |
|---|---------|-----|------|--------|
| 1 | CROSS-FOLDER. `impl From<StorageError> for Error` replaces five copies of `fn io` and about 127 `.map_err(io)` calls. Add `Sealed` helpers to cut the `.map_err(|e| Error::Io(e.to_string()))` calls (36 in scope). | 150-200 | very low | high |
| 2 | CROSS-FOLDER. Three copies of the length-prefixed bincode frame reader and writer: `chat/wire.rs`, `sync/frames.rs`, `group/wire.rs`. Make them one generic `FrameReader<T>` / `FrameWriter<T>`. | about 120 | low | high |
| 3 | `sync/engine.rs::self_loop` repeats the "whole-synced doc" logic four times (account, calls, profile, profile-cache). A table of `WholeDoc { name, doc, sent }` entries removes it. | 90-120 | medium | high |
| 4 | `chat/engine.rs` is 2.3k lines with natural seams. The "apply a batch, commit, announce" tail is copied across `chat_receive`, `chat_own_apply`, `chat_send_message`, `chat_edit` and `chat_apply_history`. | 100-150 | medium | high |
| 5 | `search/api.rs::search` and `search_in` are near-copies (cursor check, lookup build, scan loop). | 40 | low | medium |
| 6 | CROSS-FOLDER. The "scratch-fork import, pending check, did it grow" prefix of `apply_remote` exists in `chat/doc.rs` and `group/doc.rs` (twice). Two `Reject` enums overlap. | 40 | low-medium | medium |
| 7 | Small helper triplets: `decode_vv` (4 copies), `norm_phrase` / `key_from_phrase` (3), `day_valid` (2), `json()` (2), `ShardMeta` (2 types, same name), `UPDATES_BEFORE_SNAPSHOT` (2). | 60 | low | medium |
| 8 | Stale protocol-version text in comments and docs (chat/3 vs chat/4, self/2 vs self/3). | 0 | none | cheap and correct |
| 9 | `search/index.rs` (1.7k lines) has about 580 lines of tests and a 265-line `Scan` that can move to `scan.rs`. `search/bench.rs` (421 lines) ships in the production lib. | 0 (moves) | low | medium |
| 10 | Newtype boilerplate for the four Loro doc wrappers (`Deref`, `new`, `load`). | 30 | low | low |

---

## 1. Production-code duplication

### 1.1 Storage error mapping (CROSS-FOLDER) - highest payoff, lowest risk

Five identical functions exist:
- `rekey.rs:56`
- `store.rs:385`
- `chat/store.rs:24`
- `group/store.rs:67`
- `search/index.rs:76`

All are `fn io(e: StorageError) -> Error { Error::Io(e.to_string()) }`. `chat/blobs.rs:74` has `io_err(impl Display)`, and `sync/doc.rs:14` has `io<E: Display>`.

Call-site counts for `.map_err(io)` / `map_err(super::store::io)` / `map_err(crate::chat::store::io)` across chat, sync, search, profile and group are 127. Hand-written `.map_err(|e| Error::Io(e.to_string()))` has 36 hits in scope. The worst is `profile/mod.rs` with 17, each wrapped over 3 lines by rustfmt, e.g. `profile/mod.rs:101-104`.

Proposal: in `error.rs` add `impl From<storage::StorageError> for Error { Io(e.to_string()) }`. `error.rs` already has `From<std::io::Error>` and `From<serde_json::Error>`, so this is the same pattern. Every `.map_err(io)` becomes `?`. Optionally add a `trait IoCtx<T> { fn io(self) -> Result<T, Error> }` for `Result<T, impl Display>` for the loro, join and base64 sites. This is a mechanical change and the compiler checks it.

### 1.2 Frame reader and writer, three copies (CROSS-FOLDER)

| File | Lines | Differences |
|------|-------|-------------|
| `chat/wire.rs` | `ChatReader::recv` 51-85, `ChatWriter::send` 87-98 | typed to `ChatMsg`; error text "chat frame too large" |
| `sync/frames.rs` | `FrameReader::recv<T>` 18-42, `FrameWriter::send<T>` 44-60 | already generic over `T` |
| `group/wire.rs` | `Reader` / `Writer`, about 110-147 | typed to `GroupMsg` |

Each also redefines `MAX_FRAME = 16 MiB`. The bodies are byte-for-byte the same loop (u32 BE length, `buf.drain`, `chunk = vec![0u8; 64*1024]`). `sync/frames.rs` is already the generic version.

Proposal: promote `sync/frames.rs` to `crate::frames` with `FrameReader::recv<T: DeserializeOwned>` and `FrameWriter::send<T: Serialize>`. Keep `type ChatWriter = FrameWriter` (or just rename at the use sites). Add one `finish()` (already in `FrameWriter`) to replace the `let _ = writer.0.finish();` sites (`chat/engine.rs:599, 705`, `group/engine.rs` writer task). Keep the per-protocol `ChatMsg` / `GroupMsg` enums.

Note: `chat/wire.rs:18` `SIG_DOMAIN` and `sign_batch` / `verify_batch` are used by `sync/engine.rs` as `crate::chat::wire::sign_batch` (7 call sites) plus the wrapper `chat_sig_ok` at `sync/engine.rs:858`. They are not chat-specific, so they should move next to the frames module.

### 1.3 `sync/engine.rs::self_loop` (lines 480-662): four copies of "whole-synced doc"

The same shape is repeated for `acc`, `calls`, `profile` and `cache`:

- State variables at 495-498: `sent`, `sent_calls`, `sent_profile`, `sent_cache`.
- Hello handler at 513-531: an if/else chain per doc name.
- Sync handler at 541-580: three separate branches. Each is "if update non-empty, apply, sign an empty batch, send our vv as the ack; then merge the peer vv into `sent_*`".
- Push at 623-626 (in the reader branch) and again at 640-643 (in the `changes` branch): the same 4 `push_changes` lines twice.
- Initial hello docs at 489-494.

Proposed abstraction (home `sync/engine.rs` or a new `sync/whole.rs`):

```rust
struct WholeDoc { name: &'static str, doc: Arc<Doc>, sent: Option<VersionVector>,
                  apply: fn(&Arc<Inner>, u64, &[u8]) -> Result<(), Error>, acked: Option<Acked> }
```

Then `docs: Vec<WholeDoc>` replaces the four `sent_*` variables. A `for d in &mut docs { self.push_changes(me, d, writer).await? }` helper replaces the 4-line blocks, and the handlers become `docs.iter_mut().find(|d| d.name == doc)`. The account doc special-cases are `acked` merge and `touch_device`. The profile docs need no special case beyond `apply`.

Note on apply functions: `apply_account_update` (`sync/engine.rs:177`), `apply_calls_update` (`:226`), `apply_profile_update` and `apply_profile_cache_update` (`profile/mod.rs:393, 464`) all follow "lock shared, check epoch, import into a candidate, project, persist, `sync_bump`, notify". A shared skeleton `fn apply_whole(&self, epoch, f: impl FnOnce(&mut Shared) -> Result<Notes,_>)` would let new docs be added in about 10 lines. Risk is medium because it holds locks across steps. Do this after the table refactor.

The `push_*` helpers at 665-750 (`push_groups`, `push_invitations`, `push_acks`, `push_read`) are the same "send if changed since `peer.X`" pattern. `push_groups` and `push_invitations` are character-for-character identical except for names (`sync/engine.rs:676-693`). Fold them into `fn push_if_changed<T: PartialEq + Clone>(slot: &mut Option<T>, now: T, make: impl FnOnce(T)->SelfMsg, writer)`. About 15 lines saved.

### 1.4 `chat/engine.rs`: repeated chains

All lines below are in `chat/engine.rs` unless stated.

1. **Close all sessions loop**, 5 copies of `for s in core.sessions.lock().values() { let _ = s.tx.send(Cmd::Close); }`: `:254-255`, `:269-270` (`chat_close`, `chat_close_now`), `:289-291` (`chat_sessions_close`), `chat_close_sessions` at `:2253`, and the replace in `chat_handshake` (`:585`). Add `ChatCore::close_sessions(&self, only: Option<&str>)`. About 12 lines.

2. **"All DIDs we have a DM with"**, 6 copies of `contacts.iter().map(|c| c.did.clone()).collect()` plus pushing our own DID:
   - `:352` (`chat_started`)
   - `:355`
   - `:365`
   - `chat_list` at `:1488-1496`
   - `chat_compact` at `:2196`
   - `chat_compact_for_test` at `:2270`
   - `search/api.rs:338-342` (`search_reconcile_dms`) and `pair_owner` at `chat/own.rs:56-69`
   - `search/api.rs` has two more `contacts.iter().map(|c| c.did.clone())`, and `sync/engine.rs:~196` has one too.

   Propose `Inner::chat_peers(&self, with_notices: bool, with_self: bool) -> Vec<String>`, which would also be the single place that defines "contacts plus kept histories plus You". About 25 lines saved, and it removes a source of drift (compact skips notice-only chats; list and reconcile include them. Is that intended? Needs a decision).

3. **"Is there something pending for this DID?"**, 4 copies of `profile_pending(&did) || core.store.pending_days(&pair).map(|d| !d.is_empty()).unwrap_or(false) || chat_wants(...)`: `:360`, `:411`, `:716` (end of `chat_run`, without `chat_wants`). Make `Inner::chat_has_pending(&core, did) -> bool` with a `with_wants` flag.

4. **Day list for hello**: `chat_hello` (`:748-775`) and the `Hello` arm of `chat_handle` (`:801-826`) each build "recent_days plus pending_days plus (theirs)" with the same `if !days.contains` loops. Extract `fn hello_days(&self, core, pair, extra: impl Iterator<Item=String>) -> Result<Vec<String>>`.

5. **Announce events after a batch** (message added/changed plus chat row) appears in 4 places:
   - `chat_receive` `:1089-1103`
   - `chat_own_apply` (`chat/own.rs:174-183`)
   - `chat_send_message` `:1209-1214`
   - `chat_edit` `:1327-1333`
   - plus partial in `chat_apply_history`.

   Each does `let m = chat_api_message(...); ev.on_message_added/changed(m); if let Some(c) = chat_api_chat(...) { ev.on_chat_changed(c) }`. Propose `fn chat_announce(&self, core, me, did, pair, day, recs: &[(MsgRec,bool)], conv: &ConvMeta)`. About 30 lines.

6. **Batch ingest** is done three times with the same structure "get shard from cache, `apply_remote`, if the vv grew then `chat_commit`" (`chat_receive` `:1016-1100`, `chat_own_apply` `own.rs:~108-170`, `chat_apply_history` `:2104-2145`). They differ in `Ctx`, in the `Pending` handling and in the extra `mo`/`out` ops (own-device) or the full re-registration (history). A common `fn chat_ingest(&self, shards, core, me, did, pair, day, update, ctx, all_remote) -> Result<Ingest{Unchanged|NeedHistory|Committed{recs,conv,files}}, Error>` would remove the `'blk:` labelled break and the `if shard.vv()==before {...} else { ... }` with the mis-indented `else` block (`:1062-1068`, unformatted).

7. **Send path pair**: `chat_send_message` (`:1175-1214`) and `chat_write_in_day_for_test` (`:1240-1262`) both build `MsgRec`, call `add_message`, assemble `[put_mo_op, mi_op, out_op, register_file, conv_ops]` and call `chat_persist`. Factor `fn chat_local_write(...)`. Also `chat_edit` (`:1274-1310`). About 30 lines.

8. **Snapshot-blob creation** is duplicated in `chat_snapshot_blob` (`:1998-2028`) and `chat_compact` (`:2210-2242`). Both do `key = new_key(); hash = hub.add_bytes(key, snap); ref_ops(...); db.apply; release old hash`. Extract `async fn chat_make_snapshot_blob(&self, core, pair, day) -> (hex_hash, key_hex, size, ops)`.

9. **Blob decrypt**: `spawn_blocking(move || crypt::decrypt_bytes(&k, &ct)).await.map_err(...).map_err(...)` in `chat_load_shard` (`:918-922`) and `chat_import_day` (`:2178-2183`). Extract `async fn BlobHub::read_plain(&self, hash, key)` (home: `chat/blobs.rs`). Also removes the `parse_hash` / `parse_key` / `read_cipher` triplet repeated.

10. **Blob lookup by message id**: `chat_blob_of` (`:1870-1890`) and `chat_save` (`:1953-1968`) both do `conv_blobs(&pair)?.into_iter().find(|h| blob(h).is_some_and(|b| b.msg == id))`. `chat_save` ignores the shard-cache fast path. Make it one `chat_blob_of`.

11. **`chat_fetch_blob` / `chat_blob_conn` / `chat_import_day`**: `chat_blob_conn` (`:1780-1790`) is "first reachable candidate", while `chat_fetch_blob` re-implements the same candidate loop (`:1808-1850`) to be able to fall through on fetch error. Fine, but the `last = e` loops in `chat_dial` (`:476-498`) and these two are one pattern: `try_each(cands, |c| async {..})`. Small.

12. **Heartbeat/`select!` run loop** in `chat_run` (`:640-715`) is 107 lines with 6 near-identical `if let Err(e) = writer.send(..).await { break 'run e.to_string() }` blocks. Convert `Cmd` handling to a `fn chat_cmd(&self, ..., cmd) -> Result<(), Error>` and let `?` break. Shrinks to about 70 lines and makes the `'run: { ... }` label unnecessary.

### 1.5 Session handshake pattern (CROSS-FOLDER: `sync/engine.rs`, `chat/engine.rs`, `group/engine.rs`)

Common skeleton:
- `open_bi` / `accept_bi` chosen by `dialer`.
- Send Auth if dialer.
- `timeout(AUTH_TIMEOUT, reader.recv())`.
- Verify attestation.
- Send Auth if not dialer.
- Register with "lower dialer key wins" duplicate resolution: `chat/engine.rs:575-588`, `sync/engine.rs:366-381`. The tie-break expressions differ: chat has an extra `old.dialer != sess.dialer` guard and sync has none. Both should behave the same for the legitimate case (two sessions, different dialer), but a single `fn keep_new(old_dialer, new_dialer) -> bool` would document the rule once.
- `AUTH_TIMEOUT` / `DIAL_TIMEOUT` constants are redefined in 9 files (`caps.rs:44`, `block.rs:34`, `devlist.rs:36`, `node.rs:34`, `chat/engine.rs:31-32`, `sync/engine.rs:24-25`, `group/engine.rs:39`, `group/invitations.rs:42`, `gcall/net.rs:66`). The values differ (10/15/20/30 s) with no stated reason. Needs a decision before unifying: either one `net::timeouts` module or leave and document why.
- `version::connect(&ep, addr, &VERSION)` plus `tokio::time::timeout(DIAL_TIMEOUT, ...)` appears 9 times. Offer `async fn dial(ep, addr, ver: &Versioned, timeout) -> Result<Connection>`.

### 1.6 Small shared helpers (CROSS-FOLDER where noted)

| Helper | Copies | Fix |
|---|---|---|
| `decode_vv` | `chat/engine.rs:123` (rejects empty bytes as error), `group/engine.rs:213` (accepts empty as default), `sync/engine.rs:477 peer_vv_of`, `search/index.rs:108 vv`, `chat/own.rs:104` inline, plus `group/api.rs:241, 829` inline `VersionVector::decode(vv).ok()...`. Semantics differ on empty input (unintentionally?). | one `crate::vv::decode(b) -> Result<VersionVector, Error>` with an explicit `decode_or_empty`. Needs a decision on empty. |
| `merge_vv` | `chat/engine.rs:2278` is a free function; `loro::VersionVector::merge` is used elsewhere (`sync/engine.rs:520`, `chat/engine.rs:857` uses `merge`) | check whether `VersionVector::merge` is semantically identical and drop `merge_vv` (`chat/engine.rs:2278-2285`). |
| phrase normalization (`split_whitespace().map(to_lowercase).join(" ")`) | `rekey.rs:38`, `group/store.rs:72-75`, `search/index.rs:80-82` | `crate::rekey::norm_phrase`; `GroupStore::key_from_phrase` can call `rekey::key_from_phrase`-style with a new `Which::Group`. Note `search/index.rs` also derives with its own domain strings, fine, but the normalize should be shared (a divergence would silently change keys and lock users out). |
| `day_valid` | `chat/engine.rs:119` (`day_start(day).is_some()`), `group/doc.rs:56` (`day_start == Some && day_of(ms) == day`, stricter) | the chat version accepts non-canonical strings that parse; use the stricter one everywhere. Check `chat/own.rs:parse_doc` uses `day_start` directly. Behavior change: decide. |
| `json()` serialize helper | `chat/store.rs:125`, `group/store.rs:126` | `crate::store::json`. |
| `ShardMeta` | `chat/store.rs:37` (vv, next_seq, n_updates:u32, n_messages, closed:Option<ClosedRef>, own) vs `group/store.rs:51` (vv, next_seq, n_updates:u64, bytes, closed:bool). Different types, same name; `UPDATES_BEFORE_SNAPSHOT` 64 as `u32` (`chat/engine.rs:39`) and `u64` (`group/store.rs:48`). | Share a `DocRows` type and a generic "append update, snapshot every N" function. Both implement it separately: chat `chat_persist` (`:960-985`), group `append_shard`/`append_meta` (`group/store.rs:182-241`). Candidate: `storage::RowLog { prefix }` with `append(update, snapshot_fn)`. Medium risk (different meta fields). |
| `hexbytes` serde module | `chat/store.rs:100-108` | move to `crate::store`, other stores serialize vv as raw `Vec<u8>` (`group/store.rs:56` has no hex). Different on-disk formats; leave unless migrating. |
| `open` retry loop for redb | `store::open_waiting` (`store.rs:355`, 20 tries x 100 ms) vs `GroupStore::open` (`group/store.rs:131-148`, 50 tries x 100 ms) | the same function with different retry counts. Make `GroupStore::open` call `open_waiting(path, tries)`. |
| `now_ms` / random id | `chat/engine.rs:157 now_ms()->i64` (with env offset), `node.rs:3658 now_ms()->u64`, `node.rs:3666 random_id` (8 bytes), `chat/engine.rs:149 random_msg_id` (16 bytes), `chat/doc.rs:91 new_device_value` (32), `chat/blobs.rs:118` (tmp name), `chat/crypt.rs:28 new_key` | `crate::rand::hex_id(n)`. Tiny. |
| Day math | `chat/doc.rs:100-145` hand-rolled `days_from_civil` / `civil_from_days` / `day_of` / `day_start`, also `bin/year-bench.rs:57` has another `day_of` | the `group/` module imports `chat::doc::{DAY_MS, HOUR_MS, day_of, day_start, writer_id}` (`group/doc.rs:28`, `group/api.rs:18`). Move to `crate::time` / `crate::daydoc` (rename `chat::doc::writer_id` too) so group does not depend on chat. (Dependency inversion, no logic change.) |
| `put_str` | `chat/doc.rs:199`, `search/seg.rs:191` (plus `get_str`, `put_var`/`get_var` in `seg.rs`) | tiny; a `crate::codec` if more binary codecs appear. |
| `GroupMsg`/`ChatMsg` `Hello`/`Peer` parsing | `group/engine.rs:223 Peer::new` (limit, valid day) and `chat/engine.rs:801-815` Hello arm: same validate-limit-decode loop. | optional shared `parse_hello(days, max, valid)`. |

### 1.7 Doc wrapper boilerplate (`sync/*doc*.rs`)

`CallsDoc`, `ProfileDoc`, `ProfileCache` (and `AccDoc`) each repeat: newtype over `Doc`, `impl Deref`, `new(device)`, `load(device, snapshot)` (`callsdoc.rs:21-45`, `profiledoc.rs:17-37`, `profilecache.rs:15-30`). A small macro `doc_newtype!(CallsDoc, CALLS_DOC)` or giving `Doc` a `const NAME` (generic `Doc<N: DocName>`) saves about 30 lines. Low priority.

`sync/doc.rs::export_since` (line 62) and `chat/doc.rs::export_all_since` (line 770) and `group/doc.rs::export_since` (line 176) are three copies of "return None if the peer is not behind, else export updates from `theirs`". And `chat::Shard::export_own_since` (line 737), `group::GroupDoc::export_peer_since` (line 198) compute the same "from = theirs, then for every peer not in the writer set, from[p] = mine[p]" vector. CROSS-FOLDER: put these on a shared extension trait for `LoroDoc` (`LoroDocExt::export_for(theirs, writers)`), about 35 lines saved.

### 1.8 Document validation prefix (CROSS-FOLDER, `chat/doc.rs` and `group/doc.rs`)

`Shard::apply_remote` (`chat/doc.rs:780-790`), `GroupDoc::apply_remote` (`group/doc.rs:234-247`) and `apply_remote_inplace` (`group/doc.rs:264-276`) all start with:

```rust
LoroDoc::decode_import_blob_meta(update, true).map_err(|_| Reject::Undecodable)?;
let old_vv = self.vv(); let scratch = self.doc.fork();
let status = scratch.import(update).map_err(|_| Reject::Undecodable)?;
if status.pending.is_some() { return Err(Reject::Pending); }
let new_vv = scratch.oplog_vv();
if !new_vv.iter().any(|(p,c)| *c > old_vv.get(p).copied().unwrap_or(0)) { return Ok(empty) }
```

Extract `fn stage_import(doc: &LoroDoc, update: &[u8]) -> Result<Option<Staged{scratch, old_vv, new_vv}>, DocReject>`. The two `Reject` enums share `Undecodable`, `Pending`, `Structure`, `ForeignPeer`. A common `BatchError` base with `#[from]` into each is possible but lower value. Save about 20 lines per copy.

`Shard::load` (`chat/doc.rs:592`) and `Doc::load` (`sync/doc.rs:50`) and `GroupDoc::load_in` repeat the "import snapshot and updates, then re-`set_peer_id`" pattern.

### 1.9 Profile module (`profile/mod.rs`)

- `profile_open` (`:126-190`) and `profile_mutate` (`:238-298`) both do: clone `state`, `project_availability`, extend `profile_outbox` with allowed contacts (identical 7-line block at `:157-164` and `:262-269`), `metadata_ops`, push `put_op(PROFILE_KEY)` and `put_op("state")`, `db.apply`. Extract `fn profile_commit_ops(&self, s, db, doc, extra) -> Result<..>`; also "enqueue profile to all allowed contacts".
- `profile_receive` (`:410-462`) and `apply_profile_cache_update` (`:464-520`) share "stage cache, size check `MAX_CACHE`, metadata_ops, put CACHE_KEY, `check_write`, apply, import into live, notify/apply_deletions/download" (about 20 lines).
- `Node::set_profile_picture` / `clear_profile_picture` (`:589, 597`) are pure aliases of `replace_` / `remove_`. They are used only by `tests/profile_sync.rs`; no app calls them (grep of `android/` and `desktop`). They are also exported through UniFFI. Pick one name. (Public API, so check with the UI owners.)
- `profile/mod.rs`, `profile/model.rs`, `sync/profile*.rs` and `chat/blobs.rs` profile parts (`:158-232`, `:279-281`) are not rustfmt-style like the rest (one-line `fn` items, no blank lines between items, 3-line wrapped `.map_err`). Formatting only, but it makes diffs noisy; format only owned files per AGENTS.md.
- `profile/pictures.rs:15 fn bad`, `profile/model.rs:9 fn bad` are two more error-builder helpers.
- Architecture note: profile blobs live inside `chat::BlobHub` (`profile_claim`, `add_profile_bytes`, `keep_profile`, `recover_profile_tags`, `release_profile`, `fetch_profile`, `protected_tag(owner: Option<&str>)`). The `owner: Option<&str>` parameter threaded through `add_bytes_for`, `keep_for`, `fetch_for` is the only coupling. A `BlobHub::scoped(owner)` handle (`ProfileBlobs`) would make `profile` a clean client of the hub and remove 6 `*_for` / `*_profile` wrapper pairs. Moderate value.

### 1.10 Search (`search/api.rs`, `search/index.rs`)

- `search()` (`api.rs:462-507`) vs `search_in()` (`api.rs:509-561`): identical cursor validation (`c.split_once('/')... r.len()==16 && h.len()==16 ...`, lines 473-478 and 520-525), identical `lookups` construction, identical `'scan` loop with `looked`/`budget`. Extract `fn validate_cursor`, `fn lookups(terms)` and `fn run_scan(...)`. The loop differences are `row.src == src` and `scan.stopped()`. About 40 lines. Low risk (tests exist: `find.rs` tests, `tests/search*.rs`).
- A truncated doc comment at `api.rs:~300`: `/// In the background after open: every shard and group whose version differs from its` (sentence ends there, attached to `search_reconcile`). Fix the text.
- `search_reconcile_groups_run` (`api.rs:400-460`) is 60 lines of nested closure with 3 `return`s inside a `spawn_blocking`. Move group reconciliation into a plain `fn` taking `&SearchIndex` and `&GroupCore` for testability.
- `SearchIndex::with_store` (`index.rs:245-280`) builds `Sealed::new(store, derive_key(STORE_DOMAIN, ..))` twice (initial, and after replacing the file at `:268`). Extract `fn seal(store, p)`.
- `SearchIndex::apply` (`index.rs:376-471`, about 95 lines, including a local struct `M`) mixes four phases (name map, load cold days, merge puts, merge drops/marks, cold timers). Split into `plan_changes`, `load_cold`, `merge_into_hot`. The retry loop `for attempt in 0..4 { ... unreachable!() }` is awkward; a `loop` with `attempts` counter is clearer.
- `maintain_segments` (`index.rs:595-765`, about 170 lines) is the longest function in the scope. Natural split: `select_work` (which days/months), `build_segments`, `commit_manifest`.
- `Scan` (`index.rs:878-1141`, 265 lines) plus `Open`, `pack` and `HOT_BATCH` form a self-contained k-way merge iterator. Move to `search/scan.rs`.
- `search/bench.rs` (421 lines) is compiled into the production `p2pcore` library and exported through `lib.rs:35-44` (`search_bench`, `search_bench_live`) only for `bin/search-bench.rs`. Likewise `bin/year-bench.rs` has a private `now_ms` and `day_of`. Gate behind a `bench` Cargo feature or move into `crates/core/benches` or the bin itself, using `pub(crate)` test hooks via a feature.

---

## 2. Test duplication

### 2.1 Search tests
- `row(src,id,at,text) -> DocRow` helper, identical, `search/index.rs:1146` and `search/find.rs:93`. `T0 = 1_767_225_600_000` and `DAY/D = 86_400_000` are redefined in both (`find.rs:89-91`, `index.rs:1148-1152`); `index.rs` already has `DAY_MS` imported. Create `search/test_util.rs` (`#[cfg(test)] pub(crate)`) with `row`, `T0`, `ids`, `mark`, `temp_path`, `fill`. Also is `at_day(ix, day)` in `index.rs:1163` just `ix.set_today(Some(day))`, and `cold(ix)` in `find.rs:98` the same: both one-line wrappers; inline or share.
- `index.rs` tests: `compaction_trigger_and_same_answers`, `compaction_follows_maintenance_not_bulk`, `compaction_killed_midway_opens`, `apply_does_not_wait_for_a_compaction` (`:1568-1706`) all open a `temp_path` index, `fill`, `maintain`, compare `answers`. The setup "open + fill + set_today + maintain(Force)" repeats 5+ times; a `fn compacted_fixture(name, convs, days) -> (SearchIndex, PathBuf)` would cut about 40 lines.
- `format_2_file_is_emptied` (`find.rs:204`) and `version_mismatch_empties` (`index.rs:1289`) and `old_format_file_is_replaced` (`index.rs:1707`) test the same "other version means rebuild" invariant. Merge into one table-driven test over `(old_version, in_memory|file)`.
- `search/index.rs` has about 580 lines of tests in the same file as 1,140 lines of code. Move to `search/index_tests.rs` (as `chat/doc_tests.rs` does).
- `find_in_chat_latency_100k` and other `#[ignore]` measurement tests are labelled `KIND: measurement`; the `search/bench.rs` covers the same ground at scale. Keep one.

### 2.2 Chat tests
- `chat/doc_tests.rs` (951 lines) is already well factored (helpers `acct`, `did`, `val`, `wid`, `w`, `shard`, `recv`, `msg`, `live`/`sync`/`hist`/`own` ctx builders, `forge`, `span`). Remaining candidates:
  - `forged_batches_are_rejected_and_not_stored` (`:611`), `every_op_must_belong_to_a_did_signed_span` (`:714`), `altered_text_and_rollbacks_are_rejected` (`:798`), `forged_writer_ids_are_refused_without_any_state_change` (`:848`) all end with the same two-line assertion block "apply returns `Err(X)`, `recv.vv()` unchanged, nothing stored". Add `fn assert_refused(recv: &mut Shard, update, ctx, want: Reject)` which checks the error and the unchanged vv and snapshot. Probably 60 lines saved, and it makes the refusal reason explicit in each case (table-driven over `(label, update, ctx, Reject)`).
  - Concurrent-edit trio (`:347`, `:378`, `:407`) shares the "build two diverging edits, apply in every arrival order" loop; one `for order in permutations` helper.
  - Duplicate fixture, CROSS-FOLDER: `acct(n)` using `OnceLock<Vec<Identity>>` is chat-specific; `group/doc.rs:396 fn acct()`, `caps.rs` (7), `block.rs` (5), `node.rs` (8), `profile/tests.rs` (6) each generate identities ad hoc (`identity::generate().0`). A crate-level `crate::testkit::{acct(n), did(n), tmpdir(tag)}` would unify. Same for the temp-dir helper: `chat/blobs_tests.rs:12`, `chat/profile_blob_tests.rs:12`, `search/index.rs:1534`, `store.rs`, `voice.rs`, `account_tests.rs` each hand-roll `std::env::temp_dir().join(format!("..-{}", process::id()))` + `remove_dir_all` + `create_dir_all`. Use `tempfile::TempDir` (auto-clean, collision-free) if the dev-dependency is acceptable.
- `chat/engine.rs:2288 mod own_tests` (one test of `doc_day_of`) belongs in `doc_tests.rs` or next to `doc_day_of`. `chat/wire.rs:107-141` tests could join too.
- `chat/blobs_tests.rs` and `chat/profile_blob_tests.rs`: both open a `BlobHub` with the three no-op closures (`Arc::new(|_, _| false)`, `Arc::new(|_, _, _, _| {})`, `Arc::new(|_, _, _| {})`) at `blobs_tests.rs:44-46` and `profile_blob_tests.rs:19-24`. Add `BlobHub::open_for_test(dir)`. Also `profile_blob_tests.rs` is not rustfmt-style (narrow 80-col wrapping).
- `profile/tests.rs`, `sync/profiledoc_tests.rs`: 6 identity generations (see above); `profile/pictures.rs:400` has its own `mod tests`.

---

## 3. Long functions and files: suggested seams

`chat/engine.rs` (2303 lines, one `impl Inner` of about 2,130 lines). It already has banner comments that mark the seams:

| New file | Current lines | Contents |
|---|---|---|
| `chat/engine/core.rs` | 1-260 plus 281-420 | `ChatCore`, `Session`, `Cmd`, `chat_open`, `chat_close*`, `chat_started`, `chat_kick*`, device id |
| `chat/engine/session.rs` | 420-735 | `chat_dial`, `chat_handshake`, `handle_chat_incoming`, `chat_run` (the live session) |
| `chat/engine/sync.rs` | 736-1240 | hello, `chat_handle`, `chat_shard`, `chat_load_shard`, `chat_persist`, `chat_commit`, `chat_receive`, `chat_ack`, `chat_send_own`, `chat_owes`, `chat_push` |
| `chat/engine/local.rs` | 1240-1400 | `chat_contact`, `chat_send_message`, `chat_edit`, `chat_writer`, after-write |
| `chat/engine/read.rs` | 1400-1590 | `chat_attachment`, `chat_delivery`, `chat_api_message`, `chat_api_chat`, `chat_list`, `chat_day`, `chat_mark_read` |
| `chat/engine/blobs.rs` | 1590-1990 | hub setup, gate, download loop, fetch, want/cancel/save/send_file |
| `chat/engine/history.rs` | 1990-2290 | snapshot blobs, history requests/import/apply, compaction |

This mirrors `group/engine/{history,synth}.rs` (group already split history out). `chat/engine/synth.rs` is already a child module. Rationale: each seam has its own private state (`downloading`/`Dl`, `pulling`, `shards`), so the split is mostly mechanical `impl Inner` blocks plus `pub(super)` visibility. Do it after the dedup in 1.4, not before, otherwise the diff is unreadable.

Long functions in scope (approximate):
- `chat/engine.rs`: `chat_handshake` 128, `chat_run` 107, `chat_receive` 150, `chat_fetch_history` 55, `chat_compact` 70.
- `sync/engine.rs`: `self_loop` 180, `self_run` 60.
- `search/index.rs`: `maintain_segments` 170, `apply` 95.
- `search/api.rs`: `search_reconcile_groups_run` 60.
- `profile/mod.rs`: `profile_open` 65, `profile_receive` 55, `apply_profile_cache_update` 55.
- `chat/doc.rs`: `apply_remote` 105 (cohesive; keep, but the `containers` HashMap building at 837-845 could be a helper).
- `group/engine.rs` (CROSS-FOLDER): `group_dial` 95, `group_read` 120, `group_host_session` 85.

`chat/api.rs` and `search/api.rs` (`#[uniffi::export]` blocks): 10 of 10 chat methods use `let inner = self.inner.clone(); self.block_on(async move { inner.X(..).await })?`. Counts of that line in the crate: chat/api.rs 10, group/api.rs 7, sync/api.rs 7, node.rs 8, others 1-3. CROSS-FOLDER: a `Node::call(|inner| async move {..})` helper (`fn call<T>(&self, f: impl FnOnce(Arc<Inner>) -> Fut) -> Result<T, Error>`) would drop 2 lines per method (about 70 lines across the crate).

---

## 4. Dead code, stale items and TODOs

- No `TODO` / `FIXME` / `todo!` / `unimplemented!` / `allow(dead_code)` in scope. One marker: `chat/own.rs:157-160` "PROVISIONAL (coordinator): no immediate push or dial here..." which records a decision that has no ticket. It should be moved into docs or resolved.
- No unreferenced `pub` function in scope (checked by name across `crates/`, `android/`). Names with exactly one non-test use that are possible tidy-ups: `chat_close_now` (a copy of `chat_close` minus the hub shutdown), `chat/engine.rs:2278 merge_vv` (vs `VersionVector::merge`), `ensure_self_registered` (`sync/engine.rs`, used once, `pub(crate)`).
- `Node::set_profile_picture` and `clear_profile_picture` have no app caller (section 1.9).
- Redundant guard at `chat/engine.rs:528-531` in `handle_chat_incoming`: `if !self.is_contact_device(&remote) { /* comment only */ }` is an empty `if` (dead, only a comment). Remove it.
- `chat/engine.rs:1599-1605`: `chat_may_serve` is correct, but `did_of_device` (`:1588`) takes the `shared` lock then the `chat` lock then `sessions`: three nested locks. Note for a lock-order review.
- `search/bench.rs` compiled in the library (section 1.10).
- `sync/doc.rs::peer_id` is `fn` private and used once; `Doc.peer` is stored and used only in `load`. Fine.
- Dead-looking else-branch: `chat_receive` `:1062-1068`, a mis-indented `else { ... }` that is the only code in the file not formatted by rustfmt; indicates it was edited by hand.
- `chat/engine.rs:1127` and other `#[allow(clippy::too_many_arguments)]` (4 places: `chat_receive`, `chat_commit`, `chat_send_own`, `chat_apply_history`) signal the missing parameter object: bundle `(core, me, did, pair, day)` into `struct Conv<'a>`. That alone removes 3 args from about 12 functions.

---

## 5. File map (one line each) and doc mismatches

### chat/ (about 5.3k lines incl. 1.2k tests)
- `mod.rs` (17): module declarations. Header says "over `tinline/chat/3`": wrong, ALPN is `tinline/chat/4`.
- `api.rs` (245): UniFFI `ChatEvents`, `Chat`/`Message`/`Attachment` records and `Node` chat methods.
- `engine.rs` (2303): sessions over chat/4, hello/sync/ack, outbox, local writes, reads, blob transfers, history, compaction.
- `engine/synth.rs` (41): test-only message synthesizer for year-scale measurements.
- `doc.rs` (897): Loro day shard (`Shard`), DID-signed message versions, batch validation (`apply_remote`), day math, writer IDs.
- `doc_tests.rs` (951): invariants of the above (labelled `KIND: invariant` / `limitation`).
- `store.rs` (383): typed accessors over the sealed `chat.redb` (conv, shard rows, ack, mo/mi/out, blob, ref, snapcache).
- `wire.rs` (142): `ChatMsg` frames, `sign_batch` / `verify_batch`, `CHAT_ALPN`. Header says "chat/3" and "Version 3"; code says `/4`.
- `own.rs` (290): own-device (selfsync) support: which shards to offer, export/apply relayed batches, read cursors, acks, who may fetch blobs.
- `presence.rs` (98): ephemeral Loro presence over QUIC datagrams.
- `blobs.rs` (436) / `blobs_tests.rs` (162) / `profile_blob_tests.rs` (72): iroh-blobs hub (encrypted, gated, tagged); also owns profile-picture tag scoping.
- `crypt.rs` (206): chunked AES-GCM blob format.

### sync/ (about 2.9k)
- `mod.rs` (16): declarations; says self/3 (correct).
- `engine.rs` (876): own-device sync sessions, dial loop, account/calls/profile docs wired to `State`. Header says "`tinline/self/2`", wrong: it is `/3` (`proto::SELF_ALPN`).
- `api.rs` (380): UniFFI link/unlink/device-list API plus test hooks.
- `link.rs` (526): device linking (`tinline/link/1`, QR + code phrase transfer).
- `accdoc.rs` (365): the account Loro doc <-> `State` projection (contacts, registry, ...).
- `callsdoc.rs` (102): call-log Loro doc.
- `doc.rs` (94): shared `Doc` wrapper. Header says "`tinline/self/2`", wrong.
- `frames.rs` (89): generic frame reader/writer plus `SelfMsg`.
- `profiledoc.rs` (115) / `profiledoc_tests.rs` (107): profile fields and deletion permits doc.
- `profilecache.rs` (97): cache of contacts' profile snapshots (own-device-synced).

### search/ (about 3.9k)
- `mod.rs` (11): declarations.
- `index.rs` (1725): `SearchIndex` (hot tier + segments, apply, maintain, compaction, `Scan`), plus tests.
- `hot.rs` (129): in-memory hot tier.
- `seg.rs` (359): segment/manifest codecs and builder.
- `text.rs` (390): normalizer, tokenizer, query terms, snippets.
- `find.rs` (254): scan filter for find-in-chat, plus tests.
- `api.rs` (626): `Node::search*`, hooks `search_dm` / `search_group`, reconcile, query, test hooks.
- `bench.rs` (421): benchmark harness (should not ship in lib).

### profile/ (about 1.3k)
- `mod.rs` (648): `AccountProfile`, profile open/mutate/receive, cache update, `Node` profile API.
- `model.rs` (101): picture/field validation and bounds.
- `pictures.rs` (407): image normalization, publish, download, serving gate, deletion.
- `test_api.rs` (27) / `tests.rs` (135): test hooks and tests.

### Doc mismatches to fix
1. `chat/mod.rs:1`, `chat/wire.rs:1-6` ("Frames of `tinline/chat/3` ... Version 3"), `version.rs` CHAT comment ("`tinline/chat/3`", `old` list contains `chat/3`): current ALPN is `tinline/chat/4` (`chat/wire.rs:11`, `docs/chat.md:63`, `docs/protocol.md:90, 307, 343, 362`).
2. `sync/engine.rs:1` and `sync/doc.rs:1` say `tinline/self/2`; the ALPN is `/3`.
3. `docs/chat.md` section 7 "Code map" (`:194-198`) lists crypt, blobs, doc, wire, store, engine, api. It omits `own.rs`, `presence.rs`, `engine/synth.rs`, `*_tests.rs`, and the profile code that now lives in `blobs.rs`. Also mention `docs/chat.md` still describes "chat/3" nowhere else, so only the map needs updating.
4. `docs/search.md` has v1 and v2 implementation sections side by side ("replaced by v2 below", line 137); the code is v4 (`index.rs:28 VERSION = 4`: "z/ doc tables"). A reader needs the "Index v2" header to say "current layout is format 4".
5. `docs/iroh-blobs.md:14` names `blobs.rs:BlobHub::open`; the hub also carries per-owner profile tags (not described there).
6. `search/api.rs` truncated doc comment on `search_reconcile` (section 1.10).

---

## 6. Suggested order of work

1. Mechanical, compiler-checked, no behavior change: `From<StorageError>`, delete five `io` fns, `doc` header version strings, empty `if` in `handle_chat_incoming`, truncated doc comment, rustfmt of owned profile/sync files.
2. Shared frame module (1.2) and time/day helpers moved out of `chat::doc` (1.6).
3. `search/api.rs` query dedup (1.10), search test helpers (2.1), `index.rs` test and `Scan` extraction.
4. `self_loop` table refactor (1.3) with the existing `tests/link*.rs` and `sync` tests.
5. `chat/engine.rs` dedup (1.4), then split into `engine/*.rs`.
6. Decisions needed before touching: unify `decode_vv` empty-input behavior; stricter `day_valid` everywhere; DIAL/AUTH timeout values; whether `chat_compact` should include notice-only chats; `set_profile_picture` vs `replace_profile_picture` alias removal (public API).
