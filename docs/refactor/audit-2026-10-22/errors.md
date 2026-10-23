# Error-management audit (worktree rt-w8, branch refactor/rt-w8-cleanup)

Read-only. Counts are from grep over `src/` (tests included unless noted); line numbers as of this worktree.
Paths are relative to /home/abe/p2p_chat/.claude/worktrees/rt-w8.

## 0. Headline

- The error model is a deliberately flat 17-variant enum (`NoIdentity .. Audio`) copied three times
  (core `Error`, `node_runtime::Error`, and a 9-variant subset in `gcall_net::Error`). The copies agree and
  conversions are exhaustive `match`es, so drift is a compile error. That part is healthy.
- The real problems are not the enum but (a) **persist/store-write failures swallowed with `let _ =`** on paths
  where the user is told "done" (P0), (b) **store read errors turned into "absent"** via `.ok().flatten()` /
  `unwrap_or_default()` (~120 sites, two of which misreport to the UI), (c) **`Protocol` used as the catch-all for
  local validation and local faults**, and callers *do* match on `Protocol` to mean "hopeless, do not retry"
  (P1), (d) ~150 lines of mechanical conversion code duplicated between core and node-runtime and gcall (P2).
- FFI boundary is clean: all fallible `#[uniffi::export]` fns return `Result`; no `unwrap` that user input can reach.

## 1. Error type map

| Type | File | Variants | Lossy edges |
|---|---|---|---|
| `p2pcore::Error` (uniffi flat) | crates/core/src/error.rs | 17: NoIdentity, HaveIdentity, AccountExists(S), InCall, BadPhrase, WrongPassphrase, Locked, WeakPassphrase, NotStarted, NotFound, Busy, Timeout, Rejected(S), Protocol(S), Net(S), Io(S), Audio(S) | see below |
| `node_runtime::Error` | crates/node-runtime/src/error.rs | identical 17, identical messages; re-exported as `Error` by chat-, group-, sync-runtime | same |
| `p2pcore::AddError` (uniffi flat) | crates/core/src/contact_req.rs:73 | NotACode, OwnCode, Expired, Offline, Unreachable, Refused, Blocked, Failed(S) | `other => Failed(other.to_string())` |
| `gcall_net::Error` | crates/gcall-net/src/host.rs:21 | 9: NoIdentity, InCall, Locked, NotStarted, Busy, Timeout, Rejected, Protocol, Net | no Io/Audio/NotFound/... |
| lower: account_model, chat_core(1 variant `Io`), crdt(Io, MissingDeps), group_core(Io, Protocol, Rejected), search_core(Io, Protocol), transfer(Timeout, Protocol, Net, Io), call_video(NotFound, Protocol), audio(Opus, Alloc), proto (22 typed kinds), storage `StorageError` (typed, `#[from]` redb), chat_core `Reject`, group_core `Reject`/`RecordError`/`AttestationError`, `receipts_core::ReceiptError`, `identity`/`cryptography` errors |

Conversions (all in the "into the flat enum" direction unless noted):

- `core::Error` has 14 `From` impls (error.rs): io, StorageError, serde_json, proto, crdt, search_core, group_core, chat_core,
  account_model, transfer, call_video, audio, gcall_net, node_runtime; plus `Error -> gcall_net::Error`, `Error -> node_runtime::Error`.
- `node_runtime::Error` has 11 `From` impls: io, StorageError, serde_json, proto, crdt, chat_core, account_model, transfer,
  call_video, audio, group_core. **These are copy-pasted bodies of the core ones** (about 85 identical lines).
- `group-runtime/src/calls.rs:352-381` defines `from_gcall` / `gcall_err` for `node_runtime::Error` <-> `gcall_net::Error`; core/error.rs:145-185 defines the same pair for `core::Error`. Same `other => Protocol(other.to_string())` wildcard twice.
- `AddError`: `From<Error>` (contact_req.rs:97) and `From<node_runtime::Error>` (:242, via `Error::from`).
- Free helpers that are conversions in disguise: `rio()` x2 (chat-runtime receipts.rs:70, group-runtime member_receipts.rs:53, both `ReceiptError -> Io("receipts: ..")`), `bad()` x2 (chat-runtime profile/pictures.rs:15, group-runtime images.rs:42, both `Display -> Protocol("..picture: ..")`), `Error::net` in both enums (+ `gcall_net::Error::net`).

### Where information is lost

1. **Typed errors to `String`.** `StorageError` (Unseal vs Closed vs redb kinds), `proto::Error` (BadSignature, Expired, Replayed, Revoked, NotContact, ... 22 kinds), `Reject` (BadSignature, ForeignPeer, NotYours...), `ReceiptError`, iroh connect errors all become `Io(String)` / `Protocol(String)` / `Net(String)`. Nothing downstream can match on them. Today **no caller does**: the only kind-matching I found on the flat enum is
   - chat-runtime blobs.rs:141 `retryable = !matches!(err, NotFound | Protocol(_))`
   - group-runtime engine/images.rs:438 `hopeless = matches!(err, Protocol(_))` (and `row.dropped |= hopeless && am_host`)
   - sync-runtime engine/group_history.rs:288, :322 `Err(Protocol(_)) => deferred`
   - group-runtime code.rs:296 `Timeout | Net(_)` => retry; chat-runtime session.rs:380 (log if Protocol); contact_req.rs:100 `AddError` mapping.
   So the places that need a kind are exactly the ones that use **`Protocol` as "permanent / peer's fault"** and `Net|Timeout` as "transient". That is a de-facto taxonomy the enum does not state; see 3.
2. **`Protocol` swallows `Reject`/`proto::Error` and Io swallows `Reject`.** `chat_core::Reject` has no `From`, so 25 sites write `.map_err(|e| Error::Io(e.to_string()))` (history.rs, local.rs, receipts.rs, sync.rs, synth.rs, read.rs, core/search/api.rs, core/sync/api.rs). A `Reject` on a message the *peer* sent is a protocol violation, a `Reject` loading our own shard is corruption; both are labelled `storage: ...`.
3. **gcall wildcard** (core/error.rs:185, group-runtime calls.rs:377): `Io`, `Audio`, `NotFound`, `HaveIdentity`, `BadPhrase`, `AccountExists`... collapse to `Protocol(text)`. In practice the host closure (`with_no_call`) can only produce `InCall/Locked/NotStarted/..` so it is dormant, but the round trip core -> gcall -> core would turn an `Io("storage: x")` into `Protocol("storage: x")` and then `images.rs:438` style code would treat it as hopeless. Low risk today (P2) but the comment ("the rest are text") is the only guard.
4. **`AddError::Failed(other.to_string())`** (contact_req.rs:104): every other error (storage, protocol, `Locked`, `Busy`...) shown verbatim, including `"storage: Database error: ..."`. Kotlin AddContactSheet.kt:146-148 matches only `Refused/Blocked/Offline`; the rest fall through to a default reach state. Acceptable, but the string is user-visible and unmapped (P2).
5. **`audio::Error -> Audio(e.to_string())`**: Opus code kept in text only; fine.
6. **JoinError** (task panicked/cancelled) mapped four different ways: `Io("persist task failed")` (persist.rs:133), `Io("contact task failed")` (accept.rs:172), `Io("accept task failed")` (core/contact_req.rs:221), `Io(e.to_string())` (core/search/api.rs:282,302, history.rs:196), **`Protocol("kdf task failed")`** (core/node/mod.rs:164), `Protocol(e.to_string())`-ish for `rx.recv` "core runtime stopped" (node/mod.rs). A panic inside the pool is a bug, not storage and not protocol; a panic message is dropped (`|_|`). Pick one helper (see plan item 6).

Kotlin side: `uniffi.p2pcore.AddException` is matched by variant in AddContactSheet.kt; I found no Kotlin code matching the flat `Error` variants (they come across as `P2pException`/flat messages), so the user-visible text of the flat error **is the Display string**. That raises the stakes of sections 2 and 3.

## 2. Stringly-typed `Protocol` errors

Counts of `Error::Protocol(` (constructor or pattern): core 37, node-runtime 26, chat-runtime 48, group-runtime 52, sync-runtime 46 (209 total, incl. ~20 test/pattern uses). `Error::Io(` 73. `Error::Net(` literal 6.

Three populations are mixed under the one variant:

A. **Peer misbehaviour on the wire** (the "right" use): "bad day" x9 (read/receipt/history paths), "bad receipt day" x3, "too many receipt days", "own history batch bounds", "profile cache bounds", "receipt response scope mismatch", "blob size does not match its descriptor" (transfer/hub.rs:302,309,327), "bad batch signature" x2 (sync-runtime group_history.rs:412, sessions.rs:349; note profile one says "bad profile batch signature", chat sync.rs:123), "{label}frame too large" x2. Not user-visible in normal use (they end a session; logged). Wording is internal-jargon but harmless.
B. **Local user-input validation** that reaches the UI verbatim (p2pcore Display):
   - "message too long" (chat-runtime local.rs:46,146) vs **"message must be 1-20000 bytes"** (core/group/api.rs:292): same rule, two wordings, two homes.
   - "a device name is 1-64 characters without control characters" duplicated in core/node/api_identity.rs:116 and sync-runtime engine/account.rs:182.
   - "invalid profile name", "profile field too large", "group name must be 1-100 bytes", "files are limited to 2 GB", "not a file", "use the profile picture API", "required profile field", "the current account cannot be removed", "no passphrase is set yet", "that is your own ticket", "that ticket names a relay we won't use", "that code has expired" (x2).
   - **"you blocked them; unblock first"** (chat-runtime local.rs:29, node-runtime outgoing.rs:97) as `Protocol`, while the other direction is `Rejected(BLOCKED)` ("they refused: blocked", local.rs:26, session.rs:426...). Same domain, two shapes; `AddError` maps only the `Rejected(BLOCKED)` form to `Blocked`. A user who blocked someone and tries to message them sees a protocol-flavoured string. These do not follow docs/design/VOICE.md ("I/my buttons, you/your text") consistently ("they refused: blocked" reads as if the *peer* refused).
   - `contact_req.rs:15`: `Err(Error::Protocol(PENDING.into()))` is used as a *success-ish outcome carrier* (Requested) via an error; callers must string-compare `PENDING`. Replace with a typed outcome (P1).
C. **Local faults / internals**: "blocking call made from a NodeEvents callback" (core/node/mod.rs:127), "kdf task failed" (:164), "core runtime stopped", "persist task failed" (Io), "a link is already in progress" (sync link.rs:93), `format!("notice not taken: {other:?}")`, `"welcome not taken: {other:?}"`, `"device list not taken: {other:?}"`, `"tokens not taken: {other:?}"` (node-runtime block.rs:287, contact_req.rs:260, devlist.rs:239, ... ). The `{other:?}` ones put the **Debug of a wire `Msg`** (possibly containing tickets/DIDs/keys) into an error that is logged and could be surfaced; leaks internals and may carry identifiers. P1 (cheap): replace with `std::mem::discriminant`-style name or a `msg.kind()` label.

Prefix inconsistency: `Io` Display is "storage: x", but `crdt::Error::Io` carries *Loro decode* text and `Reject` shard-corruption text; `search_core`'s docs call its Protocol "bad request". "Storage" is also printed for a JoinError and for `rio()` ("storage: receipts: ...").

## 3. Wrong / misleading variant use

1. **`Protocol` = "do not retry" vs `Protocol` = "local oddity".** `images.rs:438` (`hopeless = Protocol(_)`) and `blobs.rs:141` treat any `Protocol` from a fetch as permanent. The fetch path (`transfer` + `version::connect`) can yield `Protocol("the other device runs an older app: ..")` (node-runtime version.rs:92) and `"peer closed"`-style `Protocol` at handshake (chat session.rs:418/440, accept.rs:159, contacts.rs:118, sync sessions.rs:164). Effects:
   - group image: an older peer or a connection that closes before auth marks `row.failed = true` and, **if this device is the host, `row.dropped = true`** ("A host refuses for good what cannot match its post") with no retry. A flaky first connection to an older client can permanently drop a host's image. Needs verification of which error reaches `group_image_done` from `group_fetch_image` (version mismatch is checked at connect). P1, small fix: introduce an explicit `NotRetryable`/size-mismatch signal (e.g. have transfer return `Error::Protocol` only for the three "blob ..." mismatch cases and map "peer closed"/version to `Net`).
   - chat download: same, but there it ends in "failed" state with a user retry; lower impact.
2. **"peer closed" / "peer closed before auth|hello" / "stream ended mid-frame" are `Protocol`** (x5 + frames.rs x2) but are transport conditions (EOF). Callers that retry on `Net|Timeout` (code.rs:296 invite fetch) will not retry these. `ok_or_else(|| Error::Protocol("peer closed".into()))` repeated 5x: one helper `eof()` returning `Net("peer closed")`.
3. **Timeouts:** `tokio::time::timeout(..).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?` is correct (6 sites). Elsewhere iroh/quinn timeouts surface as `Net(text)`: `AddError` handles it (`Timeout | Net -> Unreachable`), fine. Not a bug. `history.rs:96-97,169-170`: a dropped oneshot (`rx` closed) is `Net("session closed")` - acceptable.
4. **`Io` used for non-IO**: `Io("injected write failure")` (test hook, fine), `Io(JoinError)`, `Io(Reject)` (2.2), `Io(e.to_string())` for `Loro` errors (`crdt::Error::Io` is a misnomer: it is "corrupt document bytes" = data error). Display prefix "storage:" misleads in logs.
5. **`NotFound` vs `Protocol`:** `Protocol("unknown own history group")`, `"unknown own group document"` (remote asks for something we do not have) vs `NotFound` used for missing local contact/message. Fine as-is; call_video's `NotFound` mapping is right.
6. **`Rejected(String)` is overloaded:** also used for non-refusal outcomes ("not accepted", "profile not accepted", "picture not accepted", group `Rejected(why)` for no-host reason (calls/start.rs:80), group-core `Rejected` for "invalid rotation policy" which is *local validation*, displayed as "they refused: ..."). Display prefix "they refused:" is wrong for the last two. Message-only fix (no new variant) is impossible since Display is on the shared enum; options in 7.

## 4. Swallowed errors

Totals (src only, excl. benches/tests): `let _ =` core 41 / node-runtime 69 / chat-runtime 25 / group-runtime 109 / sync-runtime 16. Roughly 60% are `tokio::time::timeout(.., conn.closed())`, `writer.0.finish()`, `take_notice`, `release_owned`, `remove_file`, logger init: **deliberate best-effort and fine** (a few comments exist; only 5 "best effort" comments in all five crates, so intent is mostly implicit - add one-liners or a `best_effort!`/`ignore(...)` helper that logs at debug).

### P0: accidental, user told "done" or data lost

| # | Where | What is swallowed | Why it matters | Fix (size/risk) |
|---|---|---|---|---|
| P0-1 | sync-runtime/src/link.rs:412 `let _ = self.persist_for(Some(me.epoch));` after adding the newly linked device to `registry` | the durable write of the link result | Linking succeeds in the UI/logs ("linked device ..") but the registry entry is only in memory; a restart (or disk full / Locked epoch) loses the link silently. Must be `?`/failure event. | Return the error (the fn already returns Result? if not, fail the link via `this.fail(id, ..)`). S, low |
| P0-2 | node-runtime/src/rotation.rs:419 (and :167, :231, :254, :382, :414) `let _ = self.persist_async(me.epoch).await;` | persist after `complete_rotation` swaps the live endpoint | the comment says "a crash first is completed on the next start", but a *failed* write (not a crash) leaves the process running on the new endpoint with a state that was never saved; the log says nothing. The profile file was saved (step 3) but `State` (contacts' notification outbox, overlap) was not. | At least `if let Err(e) = .. { self.log(..) }`; for :419 retry/escalate. S, low |
| P0-3 | sync-runtime/src/engine/account.rs:253 `let _ = self.persist_for(..)` (new own device added); :110 `let _ = save_label(db,l)`; :174 `let _ = self.history().save()` | device-registry, device label, call history | device list merged from a peer is lost on restart; label and call-history save failures are invisible. | log + (for :253) retry on next tick. S, low |
| P0-4 | core/src/sync/api.rs:213 `let _ = self.inner.persist_for(Some(me.epoch));` in remove-own-device, then `wait_pushed` and removal proceed | marking `removed = true` | if the write fails, the other devices are told but we reload with the device un-removed. Return `Err`. S, low |
| P0-5 | chat-runtime/src/chat/engine/blobs.rs:117-119 `if let Ok(op) = put_blob_op(..) { let _ = c2.store.db.apply(&[op]); }` and `if let Ok(Some(mut info)) = c2.store.blob(..)` | writes `ready`/`failed` for a finished download, then `chat_emit_changed` fires regardless | UI is told "changed" while the row was not updated; on restart the blob re-downloads or shows stuck. Log + do not emit on failure. S, low |
| P0-6 | chat-runtime/src/chat/engine/read.rs:38 `chat_delivery`: `let Some((peer,c)) = core.store.mo(..).ok().flatten() else { return Delivered }` | **read error = "Delivered"** | A transient vault read error shows two ticks on an undelivered message (and `ack` read error -> `unwrap_or(0)` -> Pending, inconsistent). `Ok(None)` (message from another device) really is Delivered; `Err` should be `Pending` or logged. S, low. Same family: history.rs:286-287. |
| P0-7 | group-runtime/src/engine/images.rs:782 `let _ = std::fs::write(&path, &p.jpeg);` (+ :780 create_dir_all), returns `Ok(img)` | our own image's cache file | the publish returns success and the blob is registered, but if the cache write fails our own image never displays (`image_state` reads the file; `Remote` forever). Return `Err(Io)` (before the blob is hubbed) or at least fail the post. S, low |
| P0-8 | group-runtime/src/engine/images.rs:464 `let _ = core.store.put_image(gid, &row)` right before `group_image_event` (`row.failed = true`) | failure state not durable | image keeps retrying forever after restart. P1 really. |

### P1: accidental, behaviour degrades quietly

- **~120 `store.xxx().ok().flatten()` / `.unwrap_or_default()` read sites turn a vault error into "no data".** Highest consequence:
  - group-runtime/invitations.rs:327,437 (`store.invs().unwrap_or_default()`): a read failure yields an empty invitation list for own-device sync (`group_own_invitations`) and for the "owed" scan; sync then sends an empty set, and the peer may treat that as authoritative (verify before treating as safe).
  - chat-runtime/blobs.rs:312 `wanted_blobs().unwrap_or_default()`: downloads silently not resumed after reconnect.
  - group-runtime/engine/admit.rs:116 `acks_day().unwrap_or_default()`: missing acks => posts considered un-acked => may re-flag `owed`/refuse posts.
  - group-runtime/api.rs:295 `acks(gid).unwrap_or_default()` => "not told" computed from empty acks.
  - group-runtime/engine/changes.rs:254 `outbox().unwrap_or_default()`; engine/rotation.rs:257 `dir().unwrap_or_default()`; engine/history.rs:134,198 `days()`.
  - group-runtime/requests.rs:138,155,170 `req(..).ok().flatten()`: a read error on an incoming request id lets a duplicate request be re-created.
  - chat-runtime/receipts.rs:334, blobs.rs:166,176,326 `s.messages().ok()`: a corrupt in-memory shard looks like "no such message".
  Proposal: add `trait ResultExt { fn or_log(self, what: &str) -> Option<T>; fn or_log_default(...) }` (tracing::warn once per site) rather than `.ok()`; mechanically replace the ~120. M, low (behaviour identical + a log line).
- **Invitation/request/meta store writes `let _ = core.store.put_inv|put_meta|put_req|put_dir|put_host|note_removal_seen|drop_*`** (invitations.rs:123,179,232,492; dialing.rs:237,327-347,377; code.rs:254-277,355-360; requests.rs:184; own.rs:84; accepting.rs:179,303). Mostly "will be re-derived / will be retried" and the dialing.rs:370-380 restore is explicitly designed, but `invitations.rs:123` (Joined state), `requests.rs:184` (`next_try` for retry; failure = tight retry or no retry) and `own.rs:84` (`put_host`) have no comment. Not data-losing individually; consolidated logging would reveal a failing vault immediately (today a full disk produces zero log lines in group-runtime).
- **`let _ = self.persist_async(epoch)` x18 across node-runtime** (contacts.rs:100,105,122; contact_req.rs:244; caps.rs:273; devlist.rs:200; rotation.rs ...; persist.rs:140 `persist_in_background` documented as cache-only). The "undo then persist" ones (contacts.rs:100-105) are fine to ignore *only* if they log; none do. Wrap in `persist_logged(epoch).await` (one fn, `warn!` on Err). S, none.
- **Signature/verification paths:** I found no `let _ = verify(..)`. Verification failures are turned into drops via `filter_map(.. .open().ok())` (group-runtime code.rs:128,135,168,189,217; images.rs:145,552; engine.rs:409 `open_record(..).ok()`; node-runtime contacts.rs:233, devices.rs:149, lifecycle.rs:325). These are deliberate "ignore forged/garbled records", but invisible; engine.rs:409 drops a record that fails to open *in our own store* with no log (could be local corruption). Add a debug counter/log.
- `search/api.rs:228,232,298`: reconcile + `maintain` results `let _ =`: search index maintenance failures invisible (Join result and inner `Result` both dropped). P2.
- `group_call_step` result `let _ =` (calls/start.rs:146,210): ok, failures emit `StartFailed` events internally.

### Deliberate best-effort (no action beyond a one-line comment)
`conn.closed()` lingers (~30), `finish()` (~12), `take_notice` after a blocking notice (6; contacts.rs:128, contact_req.rs:257, outgoing.rs:268, devlist.rs:234, run.rs:201, session.rs:425, invitations.rs:316), `remove_file/remove_dir_all` cleanup (persist.rs:188, images.rs:635,798), `release_owned` (pictures.rs:132,372, images.rs:469,628,794), `send_datagram` presence (presence.rs:84), `logging.rs` double init, `hooks.set` (lib.rs:278), `Refused` reply sends (accepting.rs:81,238,285).

## 5. Plumbing noise

Counts (core, node-runtime, chat-runtime, group-runtime, sync-runtime, search-ix src):
- `.map_err(|e| Error::Io(e.to_string()))`: **25** (23 of them on `chat_core::Reject`).
- `.map_err(|e| Error::Protocol(e.to_string()))`: **8** (mostly `Signed::sign`, `open`).
- `.map_err(Error::net)`: **32** (fine, idiomatic; keep).
- `.map_err(|e| Error::Net(e.to_string()))`: 2 (use `Error::net`).
- `.map_err(Error::from)`: **38**, almost all in core, converting `node_runtime::Error` to `p2pcore::Error`; e.g. api_identity.rs:88,97 `self.block_on(async move { .. })?.map_err(Error::from)` could be `self.block_on(..)??`; api_call.rs:9,18,22; lifecycle.rs:11,48; rotation.rs:20; chat/api.rs:291,304,315; block.rs:81,96; many in gcall/net/node.rs. 
- `Ok(x?)` with identical error type or a `From`: **45** in core (synth.rs 7, group/api.rs 6, sync/api.rs 5, ...) - clippy `needless_question_mark` is evidently allowed; for the ones whose inner already returns `core::Error` (`Ok(self.inner.group.synth_group_flush(g)?)` where `group` returns `node_runtime::Error`) the `?` *is* needed for the conversion, so these are not noise: they are the cost of having two flat enums. If core re-exported `node_runtime::Error` (7), ~80 of the 83 `.map_err(Error::from)` / `Ok(..?)` sites vanish.
- `|_| Error::Protocol("..")` / `|_| Error::..`: 57; EOF helper candidates listed in 3.2.
- Helper fns: `rio` x2, `bad` x2, `net` x3 definitions.

Consolidations (ordered by value):
1. `impl From<chat_core::Reject> for node_runtime::Error` in node-runtime (it already depends on chat-core): replaces 23 closures; choose `Protocol` or `Io` once. (Also lets `s.messages()?` work.) Orphan-rule OK (Error is local).
2. `ReceiptError -> Error` once, in node-runtime if it depends on receipts-core (check), removing both `rio()`.
3. `sign`/`open` failures: `From<proto::Error>` already exists; the 8 `map_err(|e| Error::Protocol(e.to_string()))` become `?` if the source error is `proto::Error`/`cryptography::CodecError`; add `From<CodecError>` (check types first).
4. One `join(e: JoinError) -> Error` helper for all spawn_blocking results (see 1.6).
5. `persist_logged()`, `.or_log()` as in section 4.
6. Collapse `.map_err(Error::from)` in core by making core's `Error` the node-runtime one (7).

## 6. FFI boundary: panics vs errors

- 63 `#[uniffi::export]` blocks in core. All fallible API fns return `Result<_, Error>`; the non-Result ones are pure labels/accessors (`core_version`, `avatar_art`, `call_cue_pcm`, `tick_*`, `video_*_label`, `set_*_events`, `video_live`, `video_tuning`). No exported fn that touches storage/network is infallible.
- Production `unwrap/expect` reachable from FFI (non-test, non-bench): 
  - core/voice.rs:111 `try_into().unwrap()` on a slice just sized `FRAME`: safe by construction.
  - core/voice/ogg.rs:121-124 `h[a..b].try_into().unwrap()` on a header whose length was checked above (it decodes a *user-supplied voice file*; confirm the `h.len() >= 27` check precedes it - `bad_ogg` errors above suggest yes). Safe.
  - core/video_call/ffi_bytes.rs:58 `expect("bytes over 2 GiB")` in the UniFFI `write` for `Bytes`: a >2 GiB video/voice buffer would panic inside lowering; UniFFI converts panics in lowering to an abort/`InternalError` rather than a clean `Err`. Practically unreachable (frames are KB). Replace with `unwrap_or(i32::MAX)`-style clamp or `try_into` -> panic-free error. S.
  - ffi_bytes.rs:80 `expect("4 bytes")`: after `check_remaining(buf,4)`: safe.
  - core/group/requests.rs:97 `req_bytes(&req).expect("16 bytes")` where `req = new_req_id()` just generated: safe but needless; return `Err(Error::Protocol)`.
  - core/group/invitations/node.rs:74, group-runtime invitations.rs:86, own.rs:40, api.rs:339, code.rs:140,444 `.to_bytes(..).expect("serialises")`: serialising our own structs; safe unless `MAX_ROW`-style limits are added to the encoder; consider `?` since all callers return `Result` (code.rs is in `Result` fns).
  - node-runtime/run.rs:15, accept.rs:167, group-runtime invitations.rs:366, chat session.rs:323 `unreachable!()` after matching `Msg` in a caller that pre-checked the variant: these are **on network-input paths**. They hold today but a refactor of the dispatcher turns a remote message into a panic of a runtime task (not across FFI, tasks are spawned, but it kills that connection task silently). Convert to `let .. else { return Err(Error::Protocol("unexpected message".into())) }`. S, low. 
  - sync-runtime engine/group_history.rs:177,221 `self.current.as_ref().unwrap()` / `.take().unwrap()`: state-machine invariants on network input; prefer `ok_or(Protocol)`. S.
  - node-runtime call.rs:391 `t.as_mut().unwrap()` right after assigning `Some`: safe.
  - `synth.rs:38` std `Mutex::lock().unwrap()`: test-support only (feature-gated? it is compiled into the lib; check `cfg`), poisoned mutex would panic.
- Panics across FFI: `catch_unwind` is only in `video_jni.rs` (JNI entry points, correct). UniFFI itself wraps each exported call in `catch_unwind` and reports `InternalError` for non-`Result`-returning fns / panics for `Result` ones as an `UnexpectedUniFFICallbackError`-style exception on Kotlin, so a panic does not unwind into the JVM. OK.
- **Foreign callbacks** (`on_message_added`, `on_status`, `on_*` traits, ~20): they return `()`; if Kotlin throws, UniFFI aborts the *rust task* with a logged "unexpected callback error". Check that core invokes them in a way that a throw cannot poison `shared` (parking_lot, no poisoning: good) - no finding.
- `*_for_test` fns are `#[uniffi::export]`ed in the production library: `group_skip_request_checks_for_test` (group/requests.rs:147) disables request checks and is callable from the shipped Kotlin/Swift bindings; `group_invitation_hosts_for_test`, `group_fetch_token_for_test`. Outside the error dimension, but worth `#[cfg(feature="dev-tools")]` (note: this would change generated bindings, which you want to avoid; flag only).

## 7. `thiserror` `#[from]`/`#[source]`, and sharing one Error definition

Facts:
- `thiserror` is already used everywhere. `#[from]` would only replace the trivial `impl From<io|StorageError|serde_json|proto>` blocks (about 4 per enum x 8 enums = ~100 lines); but the variants carry `String`, so `#[from]` is impossible without changing payloads (`Io(#[from] io::Error)`) - and that would change the UniFFI surface (flat errors only need `Display`, so internal payload changes are *binding-neutral*, but `Clone/PartialEq` derives on lower enums would break because `io::Error`/`StorageError` are not `Clone/Eq`). **Not worth it.**
- `#[source]` buys error chains (`anyhow`-style logs with causes). The flat design intentionally drops them; adding it needs non-String payloads. A cheaper way to get cause text in logs is the existing `{e}` formatting (they already log with `%e`).
- **Sharing one definition.** node-runtime already depends on transfer, audio, call-video, group-core, chat-core, account-model, crdt: so `node_runtime::Error` *already* has every `From` core has, **except two**: `search_core::Error` and `gcall_net::Error`. The premise in the brief ("node-runtime doesn't depend on transfer/call_video/audio") is not true here; the orphan problem is limited to those two.
  Options:
  - **A. Keep as is.** Cost: ~190 lines of mirrored enum + 2x 17-arm `From`, ~83 `map_err(Error::from)`/`Ok(..?)` crossings in core, and the 2 gcall mappings. Benefit: zero binding risk, exhaustive-match drift detection, no uniffi remote machinery. Honest verdict: acceptable; the duplication is boring rather than risky.
  - **B. `#[uniffi::remote(Error)]` in core over `node_runtime::Error`** (UniFFI 0.32 supports `#[uniffi::remote(Error)]` + `#[uniffi(flat_error)]` for foreign enums; the macro needs the variant list re-declared, which keeps drift protection by compile error). Then `pub use node_runtime::Error as Error;` in core, delete core `error.rs` conversions (~170 lines) and ~80 crossing sites. Required work: (1) move `From<search_core::Error>` into search-core? impossible (cycle) -> either add `search-core` as a node-runtime dependency (it only needs `storage`; search-ix already links both) or convert at the 3-4 call sites in core with a local `fn search(e) -> Error`; (2) `gcall_net::Error`: replace `core::Error<->gcall` mapping by making `gcall_net` depend on `node-runtime` and use `node_runtime::Error` directly (gcall-net is "no UniFFI", already pulls iroh), which also deletes group-runtime `from_gcall/gcall_err` and the lossy wildcard; or keep gcall's own enum and put the two small mapping fns in group-runtime only (core would call group-runtime's `from_gcall`) - the latter adds no dependency edges. (3) `AddError` impls stay in core (core owns AddError; orphan fine). (4) **Must verify bindings byte-identical**: run uniffi-bindgen for Kotlin+Swift before/after and diff (the Swift/Kotlin class names `P2pException`/`Error` and variant order come from the declaration, so the re-declared list must keep order). Risks: a `remote` flat error must have `Display`; thiserror gives that. Size: M (1-2 days incl. binding diff). Risk: medium-low, entirely caught by build + binding diff + `tests/` that `matches!(.., Err(Error::..))` (about 20 sites in core tests use `p2pcore::Error`).
  - **C. Move the enum into a tiny leaf crate** (thiserror only), re-exported by node-runtime, gcall-net and core. Lower-crate `From` impls would have to live in each lower crate (needing a dependency on the leaf), touching ~10 crates that today are deliberately independent of any shared error type. Not recommended now.
  Recommendation: **A for now with items 1-4 of the fix plan, then B as a time-boxed spike** (copy of the spike: one-hour check that `#[uniffi::remote(Error)]` + flat_error generates the same Kotlin/Swift as today; if yes, do it; if the generated names change even slightly, stay on A). C: no.

## 8. Fix plan (ordered)

1. **P0 batch "persist failures are loud"** (S, ~1 h, no behaviour change except fail-fast):
   `persist_logged()` helper in node-runtime/persist.rs; replace the 18 `let _ = ..persist*`; make P0-1 (link.rs:412), P0-4 (sync/api.rs:213), P0-3 (account.rs:253) return the error; P0-2 log at warn and surface one `on_status`-style signal if the post-rotation write fails.
2. **P0 batch "read errors are not absence"** (S): `chat_delivery` (read.rs:38) and blobs.rs:119 write+emit; `image_state`/images.rs:782 cache-write fail -> `Err`; introduce `OrLog` ext trait and convert the group invitation/ack/wanted-blob reads (invitations.rs:327,437, admit.rs:116, api.rs:295, blobs.rs:312, requests.rs:138-170).
3. **Variant hygiene without touching bindings** (S-M): decide and document the taxonomy in error.rs docs: `Protocol` = peer broke the rules or request is invalid (do not retry); `Net/Timeout` = transient; `Io` = our storage. Move EOF/"peer closed"/"stream ended mid-frame" to `Net`; stop `kdf task failed` / "core runtime stopped" / JoinError being `Protocol`; one `join()` helper -> `Io("task failed: ..")`. Re-check `images.rs:438`/`blobs.rs:141` after (the hopeless check then only sees the 3 genuine blob-mismatch cases).
4. **`From<chat_core::Reject>` (+ ReceiptError, CodecError) in node-runtime**; replace 23+8 closures; delete `rio()` x2, `bad()` consolidate. (S)
5. **Copy**: one place for user-visible validation texts (`MSG_TOO_LONG`, `DEVICE_NAME_RULE` shared constants in node-runtime; unify "message too long" vs "message must be 1-20000 bytes"; make "you blocked them" text consistent with VOICE.md); replace `{other:?}` Debug dumps with a short message kind. Replace `Protocol(PENDING)` in contact_req.rs:15 by a typed outcome. (S)
6. **Unreachable/unwrap on network input** -> `let else Err(Protocol)` at run.rs:15, accept.rs:167, invitations.rs:366, session.rs:323, group_history.rs:177,221; clamp ffi_bytes.rs:58. (S)
7. **Make core re-export `node_runtime::Error`** (option B) as a spike with a Kotlin/Swift binding diff; if clean, delete core/error.rs mappings and the ~80 crossing sites; fold gcall conversions into one place (group-runtime) and drop the `other => Protocol` wildcard by listing `Io/Audio/NotFound/..` explicitly as `Protocol(text)` so adding a variant forces a decision. (M, medium risk, gated on the binding diff.)
8. **Optional**: comment or `best_effort(...)` wrapper for the ~60 deliberate ignores so the next audit can grep instead of read.
