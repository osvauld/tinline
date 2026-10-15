# Audit 05: crates/core/src/group/ and crates/core/src/gcall/

Read-only audit. Line counts: group 8.9k (engine.rs 1871, calls.rs 1054, api.rs 904, state.rs 835, invitations 692, code 618, doc 603, store 444, requests 447, record 420, shard 173, mail 152, wire 147, calldir 123, own 90, engine/history 170, engine/synth 160). gcall 5k (net 1393, tests 1103 + tests/ 245, host 634, rank 558, client 466, select 283, mod 142, board 144, policy 93).
Paths below are relative to crates/core/src unless noted. Line numbers are from the current dev checkout.

## 1. Production duplication (ranked by payoff/risk)

### P1. Three byte-identical u32-BE + bincode stream codecs (CROSS-FOLDER: chat, sync)
- group/wire.rs:102-147 `Writer`/`Reader`, chat/wire.rs:70-114 `ChatWriter`/`ChatReader`, sync/frames.rs:10-60 `FrameWriter`/`FrameReader`.
- The logic is the same in all three: 64 KiB chunk, `len > MAX_FRAME` check, `bincode::deserialize`, `drain(..4+len)`, `None if buf.is_empty()`. Only the error text and the message type differ. `sync/frames.rs` is already generic (`recv<T: DeserializeOwned>`, `send<T: Serialize>`).
- Proposal: use `sync::frames::{FrameReader, FrameWriter}` everywhere, or move them to `crates/core/src/frames.rs` (or `net::frames`). Make the reader generic over `M`: `FrameReader<M>` (PhantomData) or keep the generic method. Replace `Writer`/`Reader`/`ChatWriter`/`ChatReader` with `type GroupWriter = FrameWriter` aliases, or just delete them and update the call sites. `MAX_FRAME` is also defined three times (16 MiB).
- Saves about 90 lines. Risk: low. Mechanical; error strings change ("group frame too large" becomes "frame too large"), so check whether any test matches the text.
- gcall/net.rs:929-967 (`writer`, `read_frame`) is a fourth, different framing: `proto::gcall::decode_control` over a QUIC stream with a 2048-byte read buffer. It is not the same protocol, so leave it.

### P2. group/calls.rs `group_call_msg` (416-499): five arms with the same skeleton
- Each arm does: `if big(&blob) {return}`, `w::open_x(&blob)`, a validity predicate (`gid == gid && dir.is_member(..) && dir.host_permit_ok(..) && signer_ok(..)`), `calls_first_sight(sig, ttl)`, local delivery (`calls_keep_status` / `group_call_started` / `group_call_ended` / `group_call_requested` / `group_call_refused`), then `calls_send(.. route, fid)`.
- Proposal: a private `struct Frame { blob: SignedBlob, msg: GroupMsg, ttl_ms, route: Route }`. A helper `fn call_gate(&self, sig: &str, blob_len: usize, ttl: u64, ok: bool) -> bool` would replace the repeated `big()` and `first_sight` lines. Do not build a trait-object table; the validity predicates differ and are the security-relevant part. A modest cleanup saves about 25 lines.
- Related: `calls_send` (350-382) and `group_mail_intake` (engine.rs:1047-1105) both implement "host relays to the right sessions". The mail version routes to the recipient's sessions plus other hosts. The calls version uses `Route::{All,Device,Did}` plus other hosts. Shared helper: `fn relay_targets(core:&GroupCore, gid, from:Option<u64>, route:&Route) -> Vec<Arc<Sess>>` in engine.rs. It would take `Route::Did(m.to)` from mail and give a single copy of the "peer_host || (s.hosting && member && !removed && route)" rule. Saves about 25 lines and makes it harder for the two to drift (this is the removal/joining enforcement rule, so drift matters). Risk: medium; needs the `group_g2`/`group_call` integration tests.

### P3. Host and call-host are the same feature twice
- state.rs:352-422 (`derive_meta`): the permit/accept fold for `HostPermit`/`HostAccept` (352-392) and `CallHostPermit`/`CallHostAccept` (394-423) are about 35 lines each and differ only in the Body variants, the info type (`HostInfo` vs `CallHostInfo`, which has `permit`), and the output vectors. Proposal: a generic `fn fold_permits<'a>(entries, admin_as_of, permit_of: fn(&Body)->Option<(&str,[u8;32],bool)>, accept_of: fn(&Body)->Option<bool>) -> (BTreeMap<..,&Entry>, BTreeMap<..,&Entry>)` that returns the winning permit and accept per device. The callers then do the classification (the two classifications differ slightly: hosts also compute `host_offers`). Saves about 25 lines. Risk: medium. State derivation must stay order-independent, and state.rs tests cover it (`hosts_need_permit_and_acceptance`, `call_hosts_need_their_own_permit_and_acceptance`).
- api.rs:671-730: `group_appoint_host` / `group_revoke_host` / `group_accept_host` and `group_appoint_call_host` / `group_revoke_call_host` / `group_accept_call_host` are six near-copies. Proposal: private `fn host_permit(&self, gid, did, dev, active, kind: HostKind)` and `fn host_accept(&self, gid, accept, kind)`, with `HostKind::{Chat,Call}` choosing the `Body`. The uniffi methods stay one-liners. Saves about 35 lines. Risk: low.
- State accessors `is_host`/`is_call_host`/`call_host` and the `core.groups.lock().get(gid).is_some_and(|l| l.state.is_host(&core.did,&core.dev))` pattern appear 16 times (api 4, calls 2, code 2, engine 8). Add `GroupCore::am_host(&self, gid) -> bool` and `GroupCore::with_state(gid, f)` (calldir.rs:56 already has a private `state()` with this shape). Saves about 30 lines.

### P4. Membership predicate repeated
- `is_member(d) && !is_removed(d)` appears in 12 places (calldir.rs:73, calls.rs x2, code.rs, history.rs, engine.rs x3, api.rs, invitations.rs, own.rs, state.rs). The api.rs:388 `group_member_check` variant also adds `ignore_removal` (a test hook).
- Add `GroupState::is_current_member(&self, did)`, which is `is_member && !is_removed`. Check whether `is_member` already excludes removed members (state.rs:194); if it does, the extra `!is_removed` calls are redundant and some are only defensive. Verify before changing.
- Also `core.groups.lock().get(gid)` appears 37 times; `with_live(gid, |live| ..)` would shorten many of them.

### P5. Session notification and logging boilerplate in engine.rs
- `if let Some(ev) = self.group_ev() { let g = gid.clone(); self.handle.spawn_blocking(move || ev.on_group_changed(g)); }` appears at engine.rs:~1259, 1480, 1550, 1566 and 1656, plus the api.rs and invitations/requests variants. Proposal: `Inner::group_changed(&self, gid: &str)`. Saves about 25 lines.
- `&gid[..8.min(gid.len())]` appears in engine.rs 3 times (1527, 1596, 1771) and as `short()` in calls.rs:299. Make `short` `pub(crate)` in group/mod.rs and reuse it.
- Forgetting a group is done in two places: api.rs:831 `group_forget` and the inline copy at engine.rs:1505-1510 (`groups.remove`, `store.forget`, `search_forget`). Make `group_forget` `pub(crate)` and call it from `group_dial`.

### P6. Dial/accept plumbing
- engine.rs `group_dial` (1426-1519), `group_tell_moved` (1792-1805), group/code.rs (2x `ep.connect`) and group/invitations.rs (1x) all do `timeout(DIAL_TIMEOUT, ep.connect(addr, ALPN))`, then `open_bi`, then wrap in `Writer`/`Reader`. gcall/net.rs:763 does the same with `GCALL_ALPN`.
- Proposal: `async fn open_group_stream(ep:&Endpoint, addr:EndpointAddr, alpn:&[u8], to:Duration) -> Result<(Connection, Writer, Reader), Error>` in group/wire.rs. Saves about 20 lines and makes the timeout consistent (DIAL_TIMEOUT 20 s in group vs `ANSWER_TIMEOUT_MS` 5 s in gcall). Also `let _ = timeout(2s, conn.closed()).await;` appears 4 times in `group_host_session`; a `finish_and_wait(conn, writer)` helper would remove those.

### P7. gcall/net.rs is the largest IO seam with a mirrored loop
- `Session::dial` (763-846) and `Session::serve` (848-893) each run `loop { select! { read_frame => .. , conn.read_datagram() => .. } }`, then a cleanup. The arms differ only in which engine they feed (client vs host). Extract `async fn pump(conn, recv, buf, on_ctl: impl FnMut(GroupCallMsg), on_dgram: impl FnMut(Bytes))`. Saves about 25 lines. Risk: low-medium; the state-lock closures are fiddly.
- `host_act` (581-637) and `client_act` (639-757, 120 lines) both have a `ConnId::LOCAL` shortcut that feeds the other engine in-process. Splitting `client_act` into `act_dial`, `act_control`, `act_datagram` and `act_ended` (the Dial and Ended arms are 30-40 lines each) is the natural seam.

### P8. Two different `tie_hash` functions (CORRECTNESS RISK, possible spec drift)
- gcall/board.rs:12 `tie_hash` = BLAKE3(call_id || JSON(HostRef)), used for the §5 split order.
- gcall/rank.rs:113 `tie_hash` = BLAKE3(call_id || 0 || holder_did || 0 || device_slot_be || permit_digest), used for the §2.3 tie-break. Its doc comment is only "Tie-break 3."
- board.rs says "§2.3 tie-break and §5 split order", which describes the rank.rs one. If both specs mean the same function, this is a latent bug (board.rs hashes serde_json bytes, which is also fragile to field-order changes). If they are meant to differ, rename them (`split_hash` vs `rank_tie_hash`) and fix the comments. Needs a human or spec decision; I did not resolve it.

### P9. Stub authorities duplicated
- gcall/mod.rs:121-141 `StaticAuthority` and gcall/net.rs:104-129 `StaticDirectory` (which just wraps a Mutex<StaticAuthority> and forwards every call). Production users are zero: `StaticDirectory` is used only by tests (12 refs: core/tests/gcall.rs, gcall tests, gcall-bench). Move both behind `#[cfg(any(test, feature="testing"))]` or into a `gcall/testing.rs`. Saves about 40 lines of production surface.
- Doc mismatch: gcall/mod.rs:12-13 says "real group membership is not in production yet" and net.rs:4-8 says "the group endpoint replaces it when group chat lands". Both are stale: group/calldir.rs `GroupCallDir` and group/calls.rs already wire it.

### P10. Smaller items
- `recent_days(now,n)` group/shard.rs:171 vs chat/engine.rs:743 `recent_days(&self)` (same `(0..N).map(|i| day_of(t - i*DAY_MS))`). CROSS-FOLDER: chat. Move into chat/doc.rs next to `day_of`, as `recent_days(now, n)`, and use it from both.
- Backoff tables: group/engine.rs:45 `BACKOFF: [1,2,5,15,30,60]` and chat/engine.rs:42 `RETRY_BACKOFF: [2,5,15,30,60]`. Both are used as "index = min(n, len-1)" (engine.rs:1356). A tiny shared `Backoff` type (`next(&mut self) -> Duration`) would fit.
- group/api.rs:540-560 `group_messages` is a 55-line function with an inline cursor parser (`"at:id"`). Extract `parse_cursor` and `page_items`.
- group/api.rs `group_post_at_for_test` (api.rs ~855) duplicates the item-lookup half of `group_write` (368-385). Have `group_write` take an optional `at`.
- api.rs `group_write` (368-385) builds a fallback `Item` with 10 fields to avoid a missing-item case. `Changed.new_items` should always contain the written post, so make that fallback an `expect` or `Error::NotFound` and drop about 12 lines. Check that a post hidden by the visibility rules can't land here.

## 2. Test duplication

- **gcall/rank.rs tests (260-558)** have `ident(seed)`, `gid()` and `host_ref(id, slot)` fixtures that duplicate gcall/tests.rs:32-40 and `World::href`. Also in group-sim: group-sim/src/{flags,multi_host,host_only,removal_rotation}.rs each define their own `fn ident(seed)` (the same Mnemonic::from_entropy -> recover). CROSS-FOLDER. Proposal: a `#[cfg(test)] pub(crate) mod fixtures` in gcall (`ident`, `gid`, `host_ref`, `T0`) and, for group-sim, a `crates/identity` `testing::ident(seed)` helper behind a feature.
- **gcall/tests.rs (1103 lines)**: the `World` harness (struct 56-135 + routing 137-455) is about 400 lines and is reasonable. But:
  - Tests `s06` (598), `s08` (748), `s16` (889), `s13` (825) are 77-87 lines each, with repeated "tick host -> host_actions -> pump" triples: `let actions = s.hosts.get_mut(&H1).unwrap().tick(s.now,&s.auth); s.host_actions(H1, actions); s.pump();`. This appears at least 6 times in tests.rs and tests/stall.rs:11-14, 31-34, 58+. Add `World::tick_host(h)` (and `World::tick_all`). Saves about 30 lines.
  - `base()` + `for m in [A1,BOB,CAROL] { s.join(m).unwrap(); }` repeats in about 8 tests. Add `base_with(&[A1,BOB,CAROL])`.
  - s01 (465-523) is already table-driven for bad joins; keep that style. The JSON-tamper block (475-482) could move into a `tamper_payload(blob, |v| v["issued_ms"]=..)` helper in the test support.
  - tests/stall.rs `steady()` plus tick triples are the same pattern; fold into World helpers.
- **tests/mix_alloc.rs**: a thread-local counting `#[global_allocator]` in a unit-test module of the whole core crate. This replaces the allocator for every core test binary that includes it. It is a global side effect and should be isolated in its own integration test binary (`crates/core/tests/gcall_mix_alloc.rs`) or behind a feature. Not duplication, but a risk.
- **Integration tests CROSS-FOLDER**: core/tests/gcall.rs (390) and core/tests/group_call.rs (426) each define `drive(.., dur, rec, until)`, `amplitude_at(x, freq)` (identical modulo one doc comment), `assert_hears`, `joined`. Also `eventually` exists in group_call.rs:95 and likely in group_core/g2/g3 tests (check core/tests/support/). Move to `core/tests/support/audio.rs` and `support/eventually.rs`. About 70 lines. core/tests/support/group_call_capacity.rs (145) is the only support file here.
- **group/calls.rs tests (1005-1054)** are fine; two tests, one per behavior. `conditions(...)` could be a `GroupCallConditions::for_test`.
- **group/state.rs tests (478-835)**: `hosts_need_permit_and_acceptance` and `call_hosts_need_their_own_permit_and_acceptance` repeat "permit, accept, assert" sequences; a `Dev::make_host(&mut o, &mut m, kind)` helper would shrink them by about 20 lines. Other test helpers (`Dev`, `sync`, `group(n)`) are sound.
- group/doc.rs tests (391-603), mail/record/store/code tests were not examined in depth.

## 3. Long functions and files, natural seams

| Where | Size | Suggested seam |
|---|---|---|
| group/state.rs `derive_meta` 225-495 | 271 lines | split per rule: `derive_membership`, `derive_flags_removals`, `derive_hosts`, `derive_call_hosts`, `derive_items`. State is a pure function, so splitting is safe and is the best-tested part. |
| group/engine.rs (1871) | one `impl Inner` | already has engine/history.rs and synth.rs. Further seams: `engine/session.rs` (`group_run`, `group_read`, `group_host_session`, `group_dial`, `group_removed_upload`), `engine/rotation.rs` (rotation_due, rotation_tick, rotate, tell_moved, moved_in; ~170 lines), `engine/mail.rs` (send_mail, mail_intake, mail_on_session, ~110 lines; mail.rs holds only the types). `GroupCore` data-plane methods (write/apply/merge_hosts, 265-700) could move to `engine/core.rs`. |
| engine.rs `group_core` 740-830 | 90 lines | open-store/load-groups (`load_live_groups`) vs the GroupCore struct literal. |
| engine.rs `group_read` 1591-1693 | 103 lines | per-frame handlers `on_update`, `on_ack`, `on_days`. |
| engine.rs `group_dial` 1426-1519 | 94 lines | the `Removed` branch (1451-1493, 40 lines) -> `group_dial_removed`; the `Refused` branch -> `on_refused`. |
| engine.rs `group_host_session` 1202-1284 | 83 lines | `Verdict` enum and its computation (1230-1247) -> `fn judge_hello(&self, core, hello) -> Verdict`. |
| group/api.rs (904) | uniffi types + Inner helpers + exported fns + 14 `_for_test` hooks | move test hooks (api.rs ~740-840) to `api_test.rs` or a `testing` cfg; the ones in the pub `#[uniffi::export]` surface ship into every binding (Kotlin included). Check how Android treats them. |
| group/calls.rs (1054) | types, helpers, `Inner` logic, `Node` API | `group_calls_tick` (731-799) and `group_call_step` (649-717) form the starter state machine; `calls/start.rs` split is natural. |
| gcall/net.rs (1393) | 6 distinct jobs | split: `net/config.rs` (CallDirectory, Static*, GcallConfig, status types), `net/session.rs` (Session, `with`, act fns), `net/link.rs` (dial/serve/writer/read_frame), `net/node.rs` (Inner/Node hooks). |
| gcall/tests.rs (1103) | World + 11 tests | `tests/world.rs` for the harness, one file per scenario family (already started: stall.rs, mix_alloc.rs). |

## 4. Dead code, unused pub items, TODOs

- No `TODO`/`FIXME`/`allow(dead_code)` found in either folder. "PROVISIONAL" markers are used instead (about 15: invitations.rs:46,165; code.rs:59; mail.rs:26,28; requests.rs:5,12,29,42,333; state.rs:7,16,607; engine.rs:707,1764; api.rs:507,624). These are open product decisions; per AGENTS.md do not resolve silently. They would make a good checklist for docs (docs/group-core.md says to check these).
- Unreferenced or test-only public items (verified by grep across crates/core, desktop, gcall-bench, group-sim):
  - `HostEngine::is_limited` (host.rs:214): 0 uses anywhere.
  - `HostEngine::lower_priority` (host.rs:271): 0 uses anywhere.
  - `CallBoard::previous` (board.rs:121): 0 uses anywhere.
  - `CallBoard::hosted_by` (board.rs:134): only gcall/tests.rs:263.
  - `rank::pick` (rank.rs:176): only a rank.rs test (381); `pick_from` is used by it.
  - `HostEngine::dropped(did)` (host.rs:232): test only. `ClientEngine::heard` (client.rs:154): single use; verify.
  - `GroupDoc::raw_other` (doc.rs:347): only the doc.rs test; already `pub(crate)`, so mark `#[cfg(test)]`.
  - `gcall::net::StaticDirectory`, `StaticAuthority`: test-only (see P9).
  - `GroupCore::peek_mut` / `Shards::peek_mut` (shard.rs:128): 2 hits, check.
  - `MAX_HELLO_DAYS` is used only 2 times (declared + 1 use); fine.
- `group/engine.rs` method naming: `merge_hosts_pub`, `group_owes_pub`, `kick_pub`: `_pub` shims exist because the private versions share a name; use `pub(crate)` on the originals (about 4 lines each).
- 14 `*_for_test` methods in `#[uniffi::export] impl Node` (api.rs ~742-840) ship in the production API and the generated Kotlin bindings. Gate with `#[cfg(feature = "test-hooks")]` if the Android build doesn't use them; AGENTS.md says no hidden behavior changes, so this needs coordination.

## 5. File map (for docs)

### group/
- mod.rs: module list, doc pointer to docs/group-core.md.
- api.rs: UniFFI surface: DTOs (GroupMessage, GroupSummary, GroupDetail...), `GroupEvents`, member/admin checks, `group_write`, exported `Node::group_*` (create, invite, join, post, flag, admin, leave, host/call-host appoint/revoke/accept) and test hooks; `group_join_token` join wait loop.
- engine.rs: runtime: `GroupRt` knobs, `Live` (meta doc + derived state + day shards), `GroupCore` (write/apply/push/merge hosts), group endpoint lifecycle, accept loop, host session, dial loop and dial, session `group_run`/`group_read`, mail relay, removal enforcement, endpoint rotation (U12), moved notices, auto-join.
- engine/history.rs: day-doc inventory sync, backfill, acks, compaction. engine/synth.rs: synthetic groups for tests (SynthGroup/Remote).
- state.rs: pure `derive_meta` (owner, admins, members, flags->removal, hosts, call hosts, enc keys, items) and `post_visible`.
- record.rs: DID-signed `Record`/`Body` variants, `SignedInvite`, `InviteToken`, `EndpointAuth`, `SignedHostAddr`, `GroupMeta`, id/writer-id derivation.
- doc.rs: `GroupDoc` Loro wrapper: sign/verify, apply_remote with admission, export since/until, `DocId` (meta or day).
- shard.rs: bounded cache of day docs with visibility judgments, `recent_days`, note_posts.
- store.rs: `group.redb` sealed store: meta, day docs, hosts, directory, mail, codes, outbox, acks.
- wire.rs: `tinline/group/2` GroupMsg enum and Reader/Writer framing.
- code.rs: short invite codes: parse/format `tinline://g/...`, host-side code store, fetch/issue/revoke protocol.
- invitations.rs: direct invitations over the 1:1 channel (GroupLetter), outbox rows, events.
- requests.rs: U3 1:1 chat requests between members via sealed mail; accept mints a contact ticket.
- mail.rs: sealed signed mail envelopes between two members through a host.
- own.rs: own-device sync of the group list (SelfMsg::Groups).
- calls.rs: group calls over group chat: call-host status/start/request/refusal relay, ranking+picker integration, Node call API.
- calldir.rs: `GroupCallDir`: `CallDirectory`/`CallAuthority` over GroupState, host addresses, device slots.

### gcall/
- mod.rs: sans-IO group-call engines, constants, `CallAuthority`, `Refusal`, `StaticAuthority`.
- host.rs: `HostEngine` (join/leave/roster/heartbeat/rate limits/select/active/takeover/end).
- client.rs: `ClientEngine` (verifies host control, roster, jitter buffers, mix, silence gate, redial/failover).
- board.rs: `CallBoard` (generations, glare, split, takeover eligibility).
- rank.rs: host status admit, tiering, deterministic ranking, `Picker` (ask-next on refusal or 5 s); unit tests 260-558.
- select.rs: `Forwarder`: speaker selection / K-of-N forwarding with hysteresis and provisional slot.
- policy.rs: `Cond`, `Class`, `Thermal`, caps, phone degrade policy (pure).
- net.rs: iroh wiring: `Session`, `Service`, dial/serve, close codes, `Node::gcall_*` unstable API, `CallDirectory`.
- tests.rs (+tests/stall.rs, tests/mix_alloc.rs): GC-S scenario world over the engines.

### Mismatches with docs / comments
- gcall/mod.rs:12-13, net.rs:4-8: "group membership not in production yet" / "when group chat lands" are stale (group/calldir.rs exists).
- gcall/mod.rs:15 points to `crates/group-sim/src/group_call.rs` (3477 lines, a model of the same engines). Keep the gap-to-code mapping, but note the sim and the real engines are two independent implementations (CROSS-FOLDER: any change to host/client/rank/select must be mirrored or the sim retired). group-sim is 23.5k lines total (flags 3.1k, multi_host 2.8k, removal_rotation 2.6k, host_only 2.3k...). Whether the sim still has value alongside core/tests/group_*.rs needs a lead decision.
- group/wire.rs header documents `Code/RevokeCode/Fetch` and calls but the `Hosts`/`Days` frames are only described in comments, not the diagram; fine but incomplete.
- Doc names to cross-check in the update pass: docs/group-core.md, group-shards.md, group-persistence-contract.md (store.rs key layout comment at the top of group/store.rs is the code-side source of truth), docs/design/group-call-spec.md (§ numbers referenced throughout gcall).
- api.rs docs say "any admin may flag (3 flags remove)" and `state.rs` says "the same admin may flag 3 times": consistent with each other, but check docs/group-flag-removal-model.md.

## 6. Cross-folder summary (CROSS-FOLDER)
1. Framing codec x3 (group/chat/sync) plus no shared dial helper (P1, P6). Highest payoff, lowest risk.
2. `recent_days` and backoff tables shared with chat (P10).
3. group-sim re-implements gcall (group_call.rs 3.5k) and state rules (flags.rs, removal_rotation.rs...), each with its own `ident(seed)` fixture; core/tests/gcall.rs and group_call.rs share ~70 lines of helpers.
4. Not examined: desktop/src/group_live*.rs, groups_fake.rs, Android group code. Expect them to re-derive role/host summaries from the UniFFI DTOs (GroupDetail); check for duplicated "am I admin/host" logic against `GroupRole` in api.rs:216.

## Suggested order of work
1. P1 (shared frames) and P5/P6 helpers: about 200 lines, low risk, `cargo test -p core group` plus an integration run.
2. P3 api.rs host/call-host collapse and `GroupCore::am_host`/`with_state`: about 100 lines.
3. Test helper extraction (World::tick_host, fixtures, support/audio.rs): about 150 lines.
4. Split state.rs `derive_meta`, engine.rs and net.rs into submodules (no logic change).
5. Resolve P8 (tie_hash) with the spec owner before touching rank/board.
6. Gate `*_for_test` and `Static*` behind cfg; delete confirmed-dead items (`is_limited`, `lower_priority`, `previous`).
Estimated net saving: about 450-550 lines of production code and about 250 lines of tests, without behavior change.
