# Logging discipline audit (worktree rt-w8)

Scope: crates/core, node-runtime, chat-runtime, group-runtime, sync-runtime, search-ix. Read-only; evidence from grep plus reading the cited code. Paths are relative to `crates/`.

## 1. Mechanisms and sinks

| Mechanism | Sink | Use (non-test) |
|---|---|---|
| `Node::log(line)` (`node-runtime/src/lifecycle.rs:59`): does `tracing::info!("{line}")` AND `sink.on_log(line)` | tracing subscriber (below) plus the app callback `NodeEvents::on_log` | ~46 sites node-runtime, ~10 chat, ~28 group, ~6 sync (about 90) |
| `tracing::warn!/info!/debug!` direct | tracing subscriber only | core 5, node 22, chat 13, group 37, sync 14, search-ix 1 (about 92) |
| `eprintln!/println!` | stderr | none in library code; core hits are `bin/year-bench.rs`, tests, benches. One in `group-runtime/.../group_receipts_sim.rs:926` (sim test). `peer/src/main.rs` prints to stdout/stderr (dev tool). |
| `log::` crate / `dbg!` | none | zero |

Subscriber (`core/src/logging.rs`, called from `core/src/node/api_identity.rs:9`): Android = `paranoid_android` layer (logcat tag `p2pcore`), filter `info,iroh=warn,...`, fixed, no env override. Other platforms = fmt to stderr, `RUST_LOG` honoured, same default. No `tracing` `release_max_level_*` feature is set, so info/warn reach logcat in release builds.

App sinks of `on_log`: Android `P2pApp.kt:543` logs at Log.d only when `BuildConfig.DEBUG`; desktop `crates/desktop/src/main.rs:261` drops it; `crates/peer` prints in verbose mode. So `on_log` is a debug firehose; in release the only reachable copy is the `tracing::info!` inside `Node::log`, i.e. the callback mechanism adds nothing in production except FFI crossings.

Key observation: the two mechanisms are not two audiences. `Node::log` is "tracing::info + callback", direct tracing is "tracing only". The split between them is by author habit, not by level or purpose. Chat/group/sync runtimes reach `Node::log` through `self.log` on their Ctx/Rt wrappers (same function).

## 2. Inconsistency
- Same event class, different mechanism: refusals. `self.log("refused a group letter: blocked")` (group-runtime/src/invitations.rs:355) vs `tracing::debug!("refusing an old version: {e}")` (node-runtime/src/lifecycle.rs:244) vs `tracing::warn!("welcome from {did}: {e}")` (node-runtime/src/contact_req.rs:167) which is paired with `self.log("refused a welcome: {e}")` at :173, so one refusal is logged twice, at warn and at info.
- Levels: `Node::log` is always info, including pure failures ("link failed", "connection lost", "blob download err", "rejected call"). Direct tracing has levels but a fairly arbitrary split: store write failures are warn in ownsync.rs:34 but swallowed elsewhere (section 5); identical "peer sends garbage" cases are debug in lifecycle.rs:274 and info (via log) in session.rs:381.
- `short()` is defined three times with different behaviour: `group-core/src/lib.rs:30` (8 chars, panic-free), a re-wrapper `group-runtime/src/calls.rs:128`, and `desktop/src/group_live.rs:643 short_dev`. Chat-runtime, node-runtime and sync-runtime have none. There is also `proto::device_to_text` used for full device keys (sync-runtime). Group-runtime applies `short()` to gids in most lines but not all (section 3).
- Message style: mix of "group {gid}: ..." / "group {} ..." (short vs full) / unprefixed ("tokens taken", "declined: ...", "connection lost: ..."). Lines like `declined:` (run.rs:190, outgoing.rs:249) and `connection lost:` (run.rs:213,226,263,272,284) carry no call id or peer.
- Mixed context syntax: format strings are interpolated into the message; no structured fields (`did=`, `gid=`) anywhere, so nothing can be grepped/filtered uniformly.
- `year-bench`/tests use eprintln, fine; no library code does.

## 3. Sensitive data (what is logged, at info, into logcat in release)
Nothing was found that logs a secret (root secret, mnemonic, passphrase, DEK, invite token, message bodies, file names or raw tickets). No P0 secret/plaintext leak. Grep for ticket/token/secret/phrase/body/text/`{:?}` in log lines found only fixed strings ("tokens taken", "no valid message token"). `{:?}` is used only for enum states and CancelReason. The following are identifier and social-graph leaks (info level, logcat or desktop stderr):

P1 (account DIDs, contact display names, full endpoint/device IDs at info; in a release APK these reach logcat via `tracing::info` even though `onLog` is debug-gated):
- node-runtime/src/accept.rs:180 `contact {name} added us`; :194 `contact request from {name}` (peer-chosen free text, unclipped, also a log-injection vector, see below).
- node-runtime/src/contacts.rs:84 `adding contact {name}: dialing {addr.id}` (name plus full endpoint id); :138, :148 contact names.
- node-runtime/src/contact_req.rs:183 contact name; :245 `{what}: {did}` full DID.
- node-runtime/src/block.rs:193 `{claim.from} blocked us` (full DID plus who blocked you), :261 `notice delivered to {did}`.
- node-runtime/src/caps.rs:274, devlist.rs:129/131/202/265 full DIDs ("device list taken from {did}").
- node-runtime/src/rotation.rs:173,233,434 full endpoint ids (`ep.id()`), :310 `{did} reached our previous address`. Log lines linking old and new endpoint ids of one device defeat the point of endpoint rotation if a logcat dump is shared in a bug report.
- node-runtime/src/lifecycle.rs:127,283 `endpoint {id} bound/closed` full id; group-runtime/src/engine/lifecycle.rs:215, rotation.rs:138 same for group endpoints.
- chat-runtime/src/chat/engine/session.rs:343,381,490,630 and history.rs:120,178, receipts.rs:296, sync.rs:342, blobs.rs:140 full peer DID (`sess.did`), blob hash.
- group-runtime: sessions.rs:43,165,173; calls/recv.rs:183 (`starter_did`, `holder_did`); calls/start.rs:145; requests.rs:143 (`gid` full plus blocked member), :194; rotation.rs:311; accepting.rs:235,282 and requests.rs:143,161 use FULL `{gid}` while sibling lines use `short(gid)`. A group id is an invite-grade capability-ish identifier, treat as sensitive; invitations.rs:393,415 log `{from}` DID and `row.group_name` (user-visible group name).
- group-runtime/src/engine/member_receipts.rs:376,413 and dialing.rs:229 / accepting.rs:307 use `{gid}` or `{did}` unshortened in tracing lines (`tracing::warn!("group {gid}: removal time: {e}")`, `"group {gid} day {day}: {e}"` documents.rs:382, lifecycle.rs:72).
- sync-runtime/src/link.rs:413 `linked device {device_to_text}`, :453 `linked as {did}` (this logs the NEW account DID at info during device linking); sessions.rs:206,299 own-device ids.
- tracing lines with `did`: node caps.rs:260, block.rs:248, contact_req.rs:167,228, devlist.rs:187; chat session.rs:303; group invitations.rs:266.
- Errors rendered with `{e}` can embed addresses (`Error::net(...)`, IP:port, relay URLs) from iroh; not audited per variant. Worth a check of `Error::Display` for `Net`/`Io` (Io paths include `path.display()` at store.rs:46, which contains the account dir).

Peer-controlled text in logs (log injection, size, no clipping; display names are free-text per the demo cast):
- node-runtime/src/outgoing.rs:262 `LegEvent::Failed(why)`: `{why}` is formatted unclipped (compare `clip_reason` used at :249 and run.rs:190,196). Check what `why` contains; if it carries the peer's Reject text it is unbounded.
- run.rs:66 `{reason}` (local?), accept.rs:194 `request.name`, contacts.rs:84/138/148 names: control characters and newlines pass through to logcat and the app.
- group-runtime/src/engine/sessions.rs:165 `{r}` (peer-supplied refusal reason text formatted into log).

Verdict on `short()`: not consistently used. Group-runtime partly; chat/node/sync never. DIDs are never shortened anywhere in the log lines above.

## 4. Noise / hot paths
- No logging found inside per-packet or per-frame audio/video paths in these crates (media lives in other crates; not in scope). No `format!` in tight loops found; `tracing::debug!` args are lazily formatted. `self.log(format!(...))` always allocates and crosses FFI even when the app discards it (release Android), but call sites are per-session / per-call-state, not per-message. Not hot.
- Possibly chatty at info: `Node::log` per chat session start and end (session.rs:490,630) and per group session (sessions.rs:43). With always-on background service and reconnect loops (idle-peer links, 60 s idle close), session lines recur per peer per minute; no rate limit. Same for sync-runtime sessions.rs:206/299 ("own-device sync ... closed: idle") and `devlist` deliveries. These should be debug.
- Retry loops: chat blob retry `blobs.rs:140` logs every attempt with the full error (bounded by RETRY_BACKOFF, acceptable); `group-runtime/src/code.rs:300` logs every fetch retry at info with backoff (bounded by FETCH_DEADLINE, OK); dial retry loop in chat session.rs:~380 logs only Protocol errors (see silence).
- Flood: node-runtime/src/lifecycle.rs:224-228 `incoming.refuse()` when both semaphores are exhausted is silent. Right call to not log per refusal, but a counter or a rate-limited warn ("N incoming refused in the last minute") is valuable for diagnosing "my phone stopped answering".

## 5. Silence (important failures with no log)
P1 (store/persistence failures that corrupt or lose state and can't be diagnosed):
- node-runtime: `let _ = self.persist_async(...)` at contact_req.rs:244; contacts.rs:100,105,122; rotation.rs:167,231,254,382,414,419; block.rs:260; devlist.rs:200; caps.rs:273; plus `persist_in_background` (persist.rs:140) `let _ = this.persist_for(...)` and sync-runtime link.rs:412, account.rs:253, core/src/sync/api.rs:213. Each is the write of the account-state file (contacts, devices, rotation state). Failure means a contact/rotation silently vanishes on restart. Only persist.rs:303 and ownsync.rs:34/51/55 warn. Proposed: one helper `persist_logged(epoch, what)` that does `if let Err(e) = ... { warn!(what, %e) }` and use it at every site (mechanical, ~25 sites).
- group-runtime: `let _ = core.store.put_meta/put_inv/put_dir/put_req/put_image/put_host/drop_*` at dialing.rs:237,327,334,341,347,377; invitations.rs:123,179,232,492; requests.rs:184; accepting.rs:179,303; own.rs:84; images.rs:299,464,634,782 (jpeg write); code.rs:254,274,277,355,360,452. dialing.rs:326-327 even comments "put back if the write fails (so it is tried again)" and then ignores the result of that write, so the retry the comment promises cannot happen on failure. images.rs:782 `let _ = std::fs::write(&path, &p.jpeg)` loses a cached image silently.
- chat-runtime/src/chat/engine/blobs.rs:119 `let _ = c2.store.db.apply(&[op])` after a blob download completes: if this fails, the blob stays not-ready/wanted and the UI event is still emitted. Also `put_blob_op` Err dropped by `if let Ok`.
- group-runtime/src/store.rs:27, node-runtime/src/store.rs:66: open retry loops (up to 50 / 20 tries x 100 ms) drop the intermediate error; only the last is returned (fine), but `store.rs:46` warns only on success-after-wait. Acceptable; note that the first open error (often the real cause: lock owner, disk full) is lost when the last attempt differs.
P1 (connection and sync rejections):
- node-runtime/src/lifecycle.rs:228-236 `incoming.accept()` Err and `accepting.alpn()` Err both `return` silently (handshake failures from contacts are indistinguishable from scanners; add debug with `alpn` when known).
- node-runtime/src/accept.rs:148 (`busy` close; the returned `Error::Busy` is only debug-logged at lifecycle.rs:279).
- chat-runtime/src/chat/engine/session.rs:386 dial `Err(_)` timeout path and the non-Protocol branch at :380 only log `Error::Protocol`; plain network dial failures are silently turned into `last`, then returned to the caller. session.rs:303 logs the final result at debug. Acceptable if the caller logs; at least include attempt count.
- chat-runtime/src/chat/engine/session.rs:580 `Err(_) => break 'run "connection lost"`: the underlying error is dropped, the session end log (`ended: connection lost`) loses the cause.
- node-runtime/src/call.rs:213 `Err(_) => break` (control channel error mid-call, cause dropped), run.rs:219/95 (`me()` failure ends call with "hangup_local").
- sync-runtime/src/link.rs:246,250,251 map `Err(_)` to `fail(id,"rejected"/"net"/"timeout")` dropping the actual error from the device-link handshake; this is the path a user hits when linking fails. P1 for debuggability.
- core/src/sync/engine/group_receipts/faults.rs:95 `Err(_)`.
- `.ok()` pattern on store reads that mean "treat error as absent": chat-runtime read.rs:9,38,39,75; receipts.rs:160 (`shard_meta(...).ok().flatten()`) and history.rs:286-287 / blobs.rs:145,335,391 turn a store error into "no such message / not wanted / not closed"; a corrupt store then behaves like an empty one with no trace. Recommend `warn_once` helper or at least a `debug!` on error.
- group-runtime: 109 `let _ =` (many are rightful `writer.send(..).await` / `finish()` best-effort replies, ~40 of them); 69 `.ok()`.
- Counts (non-test): `let _ =` core 29, node 65, chat 25, group 109, sync 16; `.ok()` 34/17/33/69/9.

## 6. Missing context
- run.rs:213,226,263,272,284 `connection lost: {e}`; run.rs:190,196 `declined/rejected: ...`; outgoing.rs:249 `declined: ...`: no call id/peer in the line (adjacent state line has call id, but interleaving of two calls loses the link).
- node-runtime/src/ownsync.rs:34 `saving {key}: {e}` fine; :51/:55 `account doc flush` ok (no account).
- chat-runtime/src/chat/engine/receipts.rs:182 `chat receipts: {e}`; session.rs:559 `hello: {e}` with no peer; read.rs:170 no peer/day.
- group-runtime: invitations.rs:151 `group invitation: {e}`, requests.rs:97 `chat request: {e}`, changes.rs:102/131 `group key/mail: {e}`, dialing.rs:34/368 `group endpoint rotation/join`, history.rs:218, images.rs:46/305/329 `group image row: {e}` (no gid or hash), calls.rs:165, accepting.rs:25,36,41 (`group incoming: {e}`, no peer), lifecycle.rs:174 : none say which group.
- sync-runtime sessions.rs:218 `own-device sync ended: {e}` (no remote device), :406/426/435 name doc but not remote.
- node call.rs:396 `encode: {e}` (what message?).
- search: core/src/search/api.rs:224,241 and search-ix/src/lib.rs:64 `search index: {e}` (no day/group); fine except not saying which index.

## 7. Prioritised findings
P0: none (no secrets/plaintext found in log lines). Confirm by a one-time review of `Error::Display` impls for `Net/Io/Protocol` payloads (could embed peer addresses or paths).
P1:
1. `Node::log` mirrors every line to `tracing::info!`, so contact names, full DIDs, full endpoint ids and old/new rotation ids go to logcat/stderr in release builds (section 3). The callback is gated by DEBUG on Android but the tracing mirror is not.
2. Store/persist failures swallowed with `let _ =` in ~60 places (section 5); some defeat their own retry logic (dialing.rs:326).
3. Link-handshake and call errors collapse to opaque strings, losing the cause (link.rs:246-251, session.rs:580, call.rs:213).
4. Peer-supplied strings unclipped in logs (outgoing.rs:262, accept.rs:194, sessions.rs:165).
5. No context (group/peer/call) on ~25 error lines.
P2:
6. Two mechanisms plus inconsistent levels (all `Node::log` at info including failures); duplicate log of same refusal (contact_req.rs:167+173).
7. `short()` duplicated, absent from chat/node/sync; group lines mix full and short gid.
8. Session start/end logs at info with reconnect churn.

## 8. Proposed convention
- One mechanism: `tracing` only. `Node::log` becomes `tracing::info!` only for the few user-meaningful events, or is removed from the runtimes; `NodeEvents::on_log` is kept as an optional UI/debug line source fed by a tracing layer (a small `Layer` in core that forwards INFO+ events whose target starts with `p2pcore`/runtime crates to `on_log`), so there is a single source of truth and the app callback stops being a second path. Android release: set filter to `warn` (or `info` for own crates only after shortening); debug: `debug`.
- Levels: error = invariant broken/data loss (store write failed, state not persisted); warn = a peer or subsystem failure we recovered from but a developer would want (dial failed after retries, sync rejected, persist failed once); info = lifecycle (endpoint bound/rotated, session opened/closed once per connection, call state) with no identifiers beyond short ids; debug = per-attempt/per-peer detail, refused strangers, retries; trace = per-message/frame.
- Use structured fields, not string interpolation: `warn!(gid = %short(gid), peer = %short_did(did), error = %e, "sync rejected")`. Put `short_id(&str)` (first 8 chars, panic-free) and `short_did()` in one shared crate (group-core `short` is already there; move it to a common crate used by node/chat/group/sync) and add a newtype `Redacted`/`Short` for fields so `{did}` cannot be used by accident.
- Never log: root secret, mnemonic/phrase, passphrase, DEK/keys, invite tokens/short codes, tickets, message bodies, file names, display names/group names, full DIDs/endpoint ids/device keys, IPs and paths, raw peer-supplied strings (clip via `clip_reason` and strip control chars), account dir paths.
- Never swallow store/persist errors: `fn log_err<T>(r: Result<T,E>, what: &'static str, ctx...)` helper (warn level) replaces `let _ =` on fallible store writes; best-effort network replies (`writer.send`) may stay `let _ =`.
- Hot paths: use `tracing::trace!`/`debug!` (lazy); no `format!` before the level check; rate-limit repeated warnings (a `warn_every(key, 60 s)` helper) for flood/refusal and retry loops.
- Add a CI grep guard: forbid `eprintln!/println!` in library crates and `{did}`/`{gid}` unshortened in log macros (simple regex lint script).

## 9. Fix plan (ordered)
1. (S, ~half day) Make `Node::log` not mirror to `tracing::info` in release, or shorten the identifiers at the ~30 info sites listed in section 3 via a shared `short`. Quick win; closes P1-1. Add `release_max_level_info` or android filter `warn,p2pcore=info`.
2. (M, 1 day) Introduce `persist_logged` + `log_err` helpers and convert the ~60 swallowed store/persist sites (section 5). Mechanical; start with group dialing.rs:326-377 (retry logic broken) and chat blobs.rs:119.
3. (S) Keep the real error in `link.rs:246-251`, `session.rs:580`, `call.rs:213`; log at warn with peer/call id.
4. (S) Clip/strip peer-supplied strings in `outgoing.rs:262`, `accept.rs:194`, `sessions.rs:165`, `invitations.rs:415`.
5. (M) Add context (short gid/peer/call id) to the ~25 context-free error lines (section 6).
6. (M, 1 day) Unify mechanism: single `short_id` in a common crate, convert `self.log` sites to `tracing` with fields, downgrade session churn lines to debug, add the tracing-to-`on_log` forwarding layer, remove duplicate contact_req.rs:167+173.
7. (S) Add a rate-limited warn for flood-refused incoming connections (lifecycle.rs:224-228) and grep-based lint in CI.
