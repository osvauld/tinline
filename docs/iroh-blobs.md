# iroh-blobs: what it gives us, what we use

Version in use: `iroh-blobs 0.103.1` (`fs-store` only), on `iroh 1.3`. Source read from the cargo
registry on 2026-10-08. Scope (user, 2026-10-08): reliable transfer of everyday files (photos,
PDFs, voice notes). Big files, folders, LAN discovery and preview-before-download are out of
scope. DashBeam (github.com/tonyantony300/dashbeam, AGPL-3.0) is a reference for ideas only;
copy no code.

## 1. Feature map

| Area | iroh-blobs feature | API | Tinline today | Plan |
|---|---|---|---|---|
| Content addressing | BLAKE3 hash of the bytes, verified while streaming (bao tree, 16 KiB chunk groups); a corrupt or wrong byte fails the transfer | `Hash`, `HashAndFormat` | Used: hash of the **ciphertext** (`crypt.rs` encrypts first) | Keep |
| Store | Persistent store: redb metadata + data/outboard files; small blobs inlined in the db | `store::fs::FsStore`, `Options { path, inline, batch, gc }` | Used, `<data>/blobs` (`blobs.rs:BlobHub::open`) | Keep defaults |
| Import | Add bytes, a path, or a stream; progress per import | `blobs().add_bytes`, `add_path`, `add_stream` | `add_bytes` (voice, snapshots), `add_path` of the encrypted temp file | Keep |
| Export | Whole blob to a path, a byte range, a single chunk, `AsyncRead + AsyncSeek` reader | `export`, `export_ranges`, `export_chunk`, `reader()` | `export` then decrypt (`export_plain`), `get_bytes` for small blobs | Keep. `reader()` later if we want to decrypt without a temp copy |
| Lifetime / GC | Tags protect blobs from GC; temp tags protect during an operation; `batch()` scopes temp tags | `tags().set/delete/list_prefix`, `temp_tag`, `blobs().batch()` | Tag `tl/<hash>` per kept blob, GC every 30 s | Keep. Cancel = drop the tag, GC frees partial data |
| Partial data | Store keeps a bitfield of verified chunks per blob; a later fetch only asks for what is missing | `remote().local(hash)` → `LocalInfo { local_bytes(), is_complete(), missing() }` | Resume works implicitly through `fetch` | **Use `local()`** to report real bytes-on-disk after restart, and to tell "partial" from "nothing" |
| Get (pull) | Fetch a blob or hash-seq over one connection, skipping ranges we hold; progress stream of payload bytes | `remote().fetch(conn, content).stream()` → `GetProgressItem::{Progress, Done, Error}` | Used (`blobs.rs:fetch`) with **no stall timeout** | **Add watchdog** (see §2) |
| Range get | Ask for byte ranges of a blob, or ranges per child of a hash-seq | `GetRequest::blob_ranges`, `ChunkRangesSeq` | Not used | Not needed (small files) |
| Get many | Several hashes in one request without building a hash-seq | `GetManyRequest`, provider `get_many` | Not used (gated off by mask = intercepted) | Not needed now |
| Hash sequences / collections | A blob that lists child hashes; `Collection` adds names (multi-file, folders) | `hashseq::HashSeq`, `format::collection::Collection`, `BlobFormat::HashSeq` | Not used: one message = one raw blob | Out of scope (folders) |
| Push | Sender writes into the receiver's store | `remote().execute_push`, `Request::Push` | **Disabled** in the event mask | Keep disabled: receiver pulls, so the gate stays on the serving side |
| Observe | Watch another node's (or our own) bitfield of a blob live | `remote().observe`, `blobs().observe` | Remote observe intercepted/refused | Keep refused remotely; local `observe` usable for UI if needed |
| Downloader | Multi-provider download: tries providers in turn (`TryProvider`/`ProviderFailed` events), optional split across providers, own connection pool | `api::downloader::Downloader`, `DownloadRequest`, `SplitStrategy`, `Shuffled` | Not used; we loop over the contact's devices ourselves (`engine.rs:chat_blob_conn`) | Optional later, see §3 |
| Connection pool | Reuses connections per endpoint id, idle timeout 5 s, connect timeout **1 s** by default | `iroh_util::connection_pool::Options` | Not used | Only with Downloader; 1 s connect is too short over relay, must raise |
| Provider events | Per connection/request hooks: allow/deny connect, allow/deny get, transfer progress, completed, aborted; deny reasons `Permission`, `RateLimited` | `EventMask`, `EventSender::channel`, `ProviderMessage`, `AbortReason` | Used for the gate and upload progress (`blobs.rs:gate_loop`) | **Use `transfer_aborted`** so the sender's bubble leaves "Sending" when the peer drops |
| Throttle | Provider can delay each chunk send (rate limiting) | `ThrottleMode::Intercept`, `ProviderMessage::Throttle` | Off | Not needed |
| Tickets | `BlobTicket` = address + hash + format, as a string | `ticket::BlobTicket` | Not used: hashes travel inside signed chat messages | Keep not using |
| Metrics | Prometheus counters for store and protocol | `metrics` feature | Off | Off (no telemetry) |

## 2. Reliability work (task 32)

Built on the features above, nothing new from the crate:

1. **Stall watchdog.** In `BlobHub::fetch`, wrap each `stream.next()` in `tokio::time::timeout(STALL)`
   (20 s). On timeout, drop the stream (to verify in a test: dropping it must close the request) and return `Error::Timeout`.
   The `downloading` entry is cleared in `chat_download`, so a retry can start.
2. **Retry with backoff while connected.** After a failure: 2 s, 5 s, 15 s, 30 s, 60 s, then give up
   and mark `failed` ("Tap to retry"). Reconnect still restarts `wanted && !ready` blobs. Every
   retry resumes from the bitfield, so nothing is downloaded twice.
3. **Cancel (receiver).** Abort the fetch task, set `wanted = false`, drop the `tl/<hash>` tag so
   GC removes the partial bytes; the bubble returns to "Tap to download".
4. **True progress after restart.** Seed the progress from `remote().local(hash).local_bytes()`.
5. **Sender side.** Use the provider's transfer-aborted event (arrives on the per-request channel under `RequestMode::InterceptLog`; verify) to end the "Sending n%" state
   instead of leaving it stuck.
6. **Size check stays.** `fetch` already rejects a blob whose size does not match the message.

## 3. Later, if needed

- **Downloader with the contact's devices as providers**: replaces our manual device loop and
  gives failover between a contact's phone and laptop. Needs `connect_timeout` raised (relay
  dials take seconds) and our relay/address hints available to the endpoint's address lookup,
  because the downloader dials by endpoint id only.
- **`reader()` + streaming decrypt** to open a photo without exporting a ciphertext temp copy.
