# Panic-path audit (non-test code), worktree rt-w8

Scope: crates/core, node-runtime, chat-runtime, group-runtime, sync-runtime, search-ix.
Method: scripted scan of 169 non-test .rs files (tests.rs, *_tests.rs, faults/guards, durability,
testkit, synth, src/bin excluded; inline `#[cfg(test)] mod x {}` blocks cut), then manual reading of
every hit and of the data's origin. Raw hit list: scratchpad/sites.txt (scan script nt.py).

## Headline

- P0 (reachable from a peer): **0 found.**
- P1 (reachable locally): **1 real but near-theoretical** (blobs.rs:273), plus 3 hardening items.
- UNCLEAR: 1 (peer-controlled i64 `at` plus/minus HOUR_MS; only panics with overflow checks, i.e. never in release).
- SAFE-BY-CONSTRUCTION: 26 `unwrap/expect/unreachable` sites in non-bin src (list below). Another 112
  hits are in `src/bin/year-bench.rs` and `src/bin/search-bench/` (`required-features = ["bench"]`, never in app builds; ignore).
- The 514/434/52/40/2 counts you quoted are essentially all test code. Non-test per crate:
  core 9 (+112 bench bins), node-runtime 6, group-runtime 8, chat-runtime 1, sync-runtime 2, search-ix 0.

Mitigating facts verified:
1. **No std Mutex/RwLock anywhere in scope.** All locks are `parking_lot` (no poisoning). The single `.lock().unwrap_or(..)`
   (node-runtime/src/rotation.rs:110) is on a `parking_lot::Mutex<Option<u64>>` guard-then-Option; it is a plain `Option::unwrap_or`, fine.
2. Release profile (`Cargo.toml:55`) sets no `overflow-checks` and no `panic = "abort"`. So integer overflow wraps (logic bug, not a crash)
   and panics unwind. UniFFI-exported functions convert a panic to an error at the FFI boundary, and the JNI entry points in
   core/src/video_jni.rs wrap the body in `catch_unwind`. Tokio tasks that panic die alone (see "residual risk").
3. Wall clock: every `SystemTime::now().duration_since(UNIX_EPOCH)` uses `map_or(0..)`/`unwrap_or(0)` (crdt/src/time.rs:8, node-runtime/src/util.rs:75,
   chat-runtime/src/chat/engine/mod.rs:221, core/src/cues.rs:46). `Instant::duration_since` is saturating (Rust >= 1.60); the
   sites at sync-runtime group_history.rs:138/154, group_receipts.rs:98/116, chat engine mod.rs:110/115, group-runtime code.rs:80 are safe.
4. The untrusted-bytes parsers are bounds-checked: `FrameBuf::recv` (node-runtime/src/frames.rs:24-33; `len > limit` is rejected before `4 + len`),
   `read_ogg` (voice/ogg.rs), `ffi_bytes::read_len` (uses `check_remaining`), hex ids (`hex::decode(..).ok()?.try_into().ok()`, group-runtime api.rs:36,
   invitations.rs:98, requests.rs:71, core/group/code.rs:32), recovery_key.rs:60-72 (length checked before `split_at`/`[1..]`), `letter_of` (length cap first).

## SAFE-BY-CONSTRUCTION (all 26 verified)

| file:line | site | why safe |
|---|---|---|
| core/src/voice.rs:111 | `pending[used..used+FRAME].try_into().unwrap()` | loop condition `len - used >= FRAME` right above; recorder is local mic input |
| core/src/voice/ogg.rs:121-124 (4) | `h[6..14].try_into().unwrap()` etc. | `h` is `data.get(pos..pos+27)?`, fixed 27 bytes; **this parses peer-sent voice notes**, and the rest of the parser is fully bounds-checked (lacing/body via `.get`, packet size caps, granule checks, CRC) |
| core/src/video_call/ffi_bytes.rs:58 | `i32::try_from(len).expect("bytes over 2 GiB")` | `Bytes` is only used for camera frames (video_call/api.rs:74-95); a frame cannot reach 2 GiB |
| core/src/video_call/ffi_bytes.rs:80 | `n.try_into().expect("4 bytes")` | `split_at(4)` after `check_remaining(buf,4)` |
| core/src/group/requests.rs:97 | `req_bytes(&req).expect("16 bytes")` | `req = new_req_id()` = hex of 16 random bytes, two lines above |
| core/src/group/invitations/node.rs:74, group-runtime api.rs:339, code.rs:140, code.rs:444, invitations.rs:86, own.rs:40 (6) | `codec::to_bytes(..).expect("serialises")` | postcard `to_allocvec` of our own derive-Serialize structs; fails only for unsized `serialize_seq(None)` (codec.rs:24 documents "none of ours") |
| node-runtime/src/frames.rs:26 | `buf[..4].try_into().unwrap()` | guarded by `buf.len() >= 4` |
| node-runtime/src/lifecycle.rs:165, group-runtime/src/engine/lifecycle.rs:294 | `Duration::from_secs(20).try_into().expect("small timeout")` | constant, quinn IdleTimeout limit is ~2^62 ms |
| node-runtime/src/outgoing.rs:297 | `ring_deadline.expect("guarded")` | inside `async {}` block of a `select!` branch with `if ring_deadline.is_some()`; a disabled branch's future is built but never polled, so the body never runs |
| node-runtime/src/call.rs:391 | `t.as_mut().unwrap()` | `*t = Some(..)` just set under the same guard on the lines above |
| group-runtime/src/calldir.rs:32 | `dev[..8].try_into().expect("8 bytes")` | `dev: &[u8;32]` |
| sync-runtime/src/engine/group_history.rs:177, 221 | `self.current.as_ref()/take().unwrap()` | `current` is set in the `is_none()` branch of the same `loop` immediately above; 221 is inside an `else if` that read `scope` from `current` (single-threaded `&mut self`) |
| node-runtime/src/accept.rs:167, run.rs:15, group-runtime/invitations.rs:366, chat-runtime/session.rs:323 | `let Msg::X{..} = &m else { unreachable!() }` | re-destructure of a value matched/constructed as that variant (accept.rs:223 dispatches `hello @ Msg::CallHello`, accept.rs:252 passes the GroupLetter; session.rs:323 matches the output of `proto::call_hello`). Safe today, but the invariant spans crates (accept -> `Hooks::group_letter` -> `group_letter_incoming`); see fix plan 5 |

Other panicky-looking arithmetic I verified safe: `avatar::params` (`kmax-1 >= 1` since n >= 5), `cues::pluck/norm` (local constants),
`voice::waveform` and `seek` (`starts[0]==0`, `want >= 0`), `voice::read` (`total - delivered`, delivered only grows by <= `total-delivered`),
`group/api.rs:80,90` (`days[idx]` after `idx -= 1` with `idx <= len`; `items[start..]` with `start <= len`), `member_receipts::covers`
(index from `binary_search` Ok), `devices.rs:34` (`has_device` true and `device_list.is_none()` means it is in `devices`, so `remove(position)` is in range),
`dialing.rs:58` (`.min(BACKOFF.len()-1)`), images.rs:448 (`tries` >= 1 and <= len), `images.rs scale` (decoded image is never 0x0), hex slices `[..12]/[..16]` of hash digests.

## P0 reachable from a peer

None. I specifically tried: frame length/overflow (frames.rs, wire.rs, link.rs readers all use `limit`), Ogg voice notes, hex ids, device ids,
presence/receipt timestamp subtraction (all `saturating_sub`: group-runtime presence.rs:133,136,201,303,433), group image rows, chat sync
`decode_vv`, request ids. Nothing panics on malformed peer input in release.

## P1 reachable locally

1. **chat-runtime/src/chat/engine/blobs.rs:273** in `fetch` setup:
   `Arc::new(Mutex::new(std::time::Instant::now() - Duration::from_secs(1)))`.
   `Instant - Duration` panics on underflow. Triggers only if the monotonic clock is under 1 s (Android's CLOCK_BOOTTIME-based clock cannot be that
   low when the app runs; Linux likewise), so near-zero risk, but it is the only unchecked `Instant -` in scope.
   Fix: `Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(Instant::now)` (or store `Option<Instant>`).
2. **Hardening, not a current crash**: `crates/core/src/lib.rs:4` has `#![deny(clippy::string_slice)]`, but none of the runtime crates
   (node-runtime, chat-runtime, group-runtime, sync-runtime) carry it. I found no str slicing by index in them today (only hex digest `[..12]`/`[..16]`),
   so nothing to fix; add the lint so it stays that way.
3. **Hardening**: `core/src/video_jni.rs:31` `bytes(p, n)` builds a slice from JVM-reported direct-buffer pointer/capacity; wrapped in `catch_unwind`,
   but an invalid pointer is UB, not a panic. Strides (`ys/uvs/ps`) come from Java and are validated inside `push_yuv_planes` (video crate, out of scope).
4. **Test hooks compiled into release**: `p2pcore` exposes ~91 `*_for_test` methods; nearly all carry `#[cfg(any(test, feature="dev-tools"))]`, but a few do not
   (e.g. core/src/block.rs:145 `block_notices_sent_for_test`, caps.rs:82, group-runtime engine.rs:65/74 `receipt_cores_for_test`, node-runtime
   rotation.rs:49 `overlap_ms_for_test`, `fail_writes` at node-runtime lib.rs:221/persist.rs:121; `profile/test_api.rs` is `mod test_api;` ungated, profile/mod.rs:4).
   None panics; they are dead surface, and env knobs (`P2P_CHAT_CLOCK_MS_OFFSET` in chat engine mod.rs:220, `P2P_IDLE_CLOSE_MS`, `P2P_WAITING_RING_SECS`,
   `P2P_CLOSE_WAIT_MS`, `P2P_SELF_SYNC_SECS`, `P2P_RELAY_ONLY`) are read in release builds. `P2P_CHAT_CLOCK_MS_OFFSET` shifts message timestamps; only settable by whoever
   controls the process env, so low risk. Gate them if you want release to be minimal.

## UNCLEAR

- group-runtime/src/engine/images.rs:656 `[r.at - crdt::HOUR_MS, r.at, r.at + crdt::HOUR_MS]` where `r.at` is the item's author-supplied `i64` post time (images.rs:320 `at: i.at`).
  If a member can publish a post with `at` near `i64::MAX/MIN`, this overflows: wraps in release (then `day_of` of a wrapped value is harmless: `div_euclid`, no panic),
  panics in debug/test builds. Need to know: whether the group-doc ingest rejects posts whose `at` is outside its day shard +/- 1 h (the doc comment at core/src/group/api.rs:65 suggests day placement is checked).
  Cheap to remove regardless: `saturating_sub/add`.

## Residual risk worth knowing (not a site, a property)

- There is no `catch_unwind` around spawned tokio tasks or `std::thread`s. A panic in an accept loop / dial loop / session task kills that task silently;
  the app keeps running but that service (e.g. the incoming-connection loop) stays dead. Today no found site can panic, so this is defence in depth.
- UniFFI-exported sync calls: a panic becomes an `InternalException` on Kotlin (not a process abort) provided the build keeps `panic = "unwind"` (the default; do not add `panic = "abort"` to release profile).

## Mechanical fix plan (ordered)

1. (5 min) blobs.rs:273 `checked_sub`. Only real P1.
2. (10 min) `images.rs:656` use `saturating_sub/saturating_add`.
3. (30 min) Lint guard so the audit stays true: in each runtime crate `lib.rs` add
   `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::string_slice))]`
   then `#[allow(clippy::expect_used)]` with a one-line reason on the ~26 SAFE sites above (or fix them per 4-5). `indexing_slicing` will flag
   ~40 more sites (ogg.rs, voice.rs, avatar.rs, cues.rs, outgoing.rs `state[idx]`, node-runtime frames.rs) that are bounds-checked; allow per function.
   Cannot run clippy under the read-only constraints, so the exact count is unverified.
4. (30 min) Replace the 8 `codec::to_bytes(..).expect("serialises")` with one helper, e.g. `codec::to_bytes_infallible<T: Serialize>(v) -> Vec<u8>` that
   internally does the single `expect`, so six crates stop carrying their own. Same for `[u8;N]` slice conversions: `cryptography::bytes::array::<N>(&[u8]) -> Option<[u8;N]>`
   or use `slice.first_chunk::<N>()` / `split_first_chunk` / `as_chunks` (std, stable) at voice.rs:111, ogg.rs:121-124, frames.rs:26, calldir.rs:32, ffi_bytes.rs:80. Removes ~9 unwraps with no behaviour change.
5. (30 min) Remove the cross-crate `unreachable!()` pairs by passing the destructured fields: have `incoming_call`/`group_letter_incoming` take `(call_id, relay, devices, ...)` or an already-matched struct instead of a `Msg` they re-match (accept.rs:167, run.rs:15, invitations.rs:366); session.rs:323 can return `ChatMsg::Auth` from a `proto::call_hello_parts()` helper.
6. (15 min) One `now_ms()`: there are at least 4 copies (crdt/src/time.rs:7 returns u64, node-runtime util.rs:75 secs, chat-runtime chat/engine/mod.rs:218 i64 with env offset, core/src/cues.rs:46 nanos). They are all panic-free; consolidating in `crdt::time` removes drift (one has the test offset knob, the others don't) rather than a crash.
7. (optional) Add `tokio::spawn` wrapper `spawn_supervised(name, fut)` that logs a panic (via `JoinHandle` result) and restarts the accept/dial loops; needs a decision on which loops are restartable.
8. (optional) Gate remaining `*_for_test` / `test_api` modules behind `any(test, feature="dev-tools")` and the `P2P_*` env knobs behind the same or `debug_assertions`.
