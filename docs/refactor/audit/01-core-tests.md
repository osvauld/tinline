# Audit 01: crates/core/tests (32 files, 14,526 lines + support/)

Read-only. Counts are from grep; line savings are estimates.

## 0. Headline
Every integration-test file is its own binary and re-declares the same harness. `tests/support/` holds ONE file (`group_call_capacity.rs`, a test module pulled into `group_call.rs` via `#[path]`, not a helper module). Doc comments say it outright: "helpers are copied from there, the tests share no module" (chat_shards.rs:5), "whose helpers are copied here" (group_g2.rs, group_g3.rs, group_shards.rs, group_call.rs). Nothing is shared.

Counts (files containing each item, of 32):
- `impl NodeEvents for`: 31 (all but gcall's variant); each is a 5-line no-op `Quiet`/`Tap` (`on_status`, `on_contacts_changed`, `on_incoming_call`, `on_call_state`, `on_log`)
- `impl LinkEvents for`: 15 (+2 in search.rs), each ~12 lines of no-ops + a channel
- `fn online(`: 20, byte-identical (`n.start(); poll status().online up to 10 s, 100 ms`)
- `fn eventually(`: 20 (poll interval varies 200/250 ms: call_tokens_acceptance.rs:149 uses 200, group_core.rs:118 uses 250)
- `fn peer(`: 25; `fn blank(`: 13; `fn pair(`: 10; `fn link_fresh(`: 11; `fn all_msgs(`: 13; `fn incoming(`: 10; `fn holds(`/`fn delivered(`: 5/6
- `fn restore_in_place(`: 5; `fn restart(`: 7; `fn open(`: 9; `fn now_ms(`: 5; `fn encode(`/`decode(` (hex/b64): 2 identical
- temp-dir prefix `std::env::temp_dir().join(format!("p2pcore-<x>-{name}-{pid}"))` with a hand-picked prefix: 36 sites in 31 files (listed in section 1 item 1)
- `const PASS: &str = "test-passphrase"`: 30 files
- `thread::sleep(Duration::from_secs(..))` of 1 to 10 s: 45+ sites in 29 files

Estimated saving from one shared support module: 3,500 to 4,500 lines (about 25 to 30% of the directory), nearly all mechanical.

## 1. Duplicated helpers, with proposed shared home

Proposed home: `crates/core/tests/support/mod.rs`, with each file adding `mod support; use support::*;`. Cargo does not build `tests/support/mod.rs` as its own test target. The existing `tests/support/group_call_capacity.rs` should move up a level to avoid confusion. Today each file also pays its own compile and link, since every test file is a separate binary: 32 binaries each linking the full core and iroh. Merging related files into fewer binaries (e.g. `tests/groups/main.rs`) would cut link time and make sharing trivial. This is a risk trade-off because tests run in parallel inside one binary on shared ports and process-id based dirs (see 3d).

### 1.1 Event sinks (highest payoff, lowest risk)
`struct Quiet; impl NodeEvents` appears in about 31 files at roughly 6 lines each. Examples:
- group_core.rs:19-23, group_g2.rs:19-23, group_g3.rs, group_call.rs:36-40, accounts.rs:15-19, devices.rs:19-27
- `Tap` variants that forward `on_incoming_call`/`on_call_state`/`on_log` into an `mpsc` channel: calls.rs:20-28, call_tokens_acceptance.rs:33-45, blocking_acceptance.rs:139-150, device_list_push.rs:37-48, device_list_characterization.rs:48-60, chat_shards.rs:29-40, and others
- Also in core/src: `account_tests.rs:11-16` has the same `Quiet`

Proposed: `support::Quiet` (no-op), `support::Tap { rx, logs }` (call events + log capture), `support::LinkTap` (the Code/Done/Other `LEv` channel). The `LEv` enum + `LTap` is copied in at least 15 files (group_core.rs:25-48 is typical, about 25 lines each). Savings: about 31 x 6 + 15 x 25 = 560+ lines.

### 1.2 Peer harness
`struct Peer { node, rx?, lrx, logs?, did, dir }` + `Drop` (stop node, rm -rf dir) + `online` + `blank` + `peer` + `pair` + `link_fresh` + `eventually`. Verified the group_core.rs / group_g2.rs helper block (lines 17-184) is identical except for the temp prefix (`p2pcore-g1-` vs `-g2-`, line 56 vs line 56); group_g3.rs lines 60-160 differ by only a few lines from it. `link_fresh` differs in file only by the device label ("Desk" group_core.rs:~95 vs "Pixel" call_tokens_acceptance.rs:~129) and the poll interval.

Proposed API in `support/mod.rs`:
```rust
pub const PASS: &str = "test-passphrase";
pub struct Peer { pub node: Arc<Node>, pub did: String, pub dir: PathBuf, rx, lrx, logs, phrase }
impl Peer {
    pub fn blank(tag: &str) -> Peer;            // unique dir via TempDir (see 1.3)
    pub fn new(name: &str) -> Peer;             // create_identity + online + did
    pub fn with_phrase(name: &str, ..) -> Peer; // the "peer(name, started)" variants (chat_shards.rs:131, device_list_characterization.rs:125)
    pub fn link_fresh(&self, name: &str) -> Peer;
    pub fn restart(&mut self, pass: &str);      // open(dir) + swap + unlock + online (readd.rs:73, writer_id.rs:61, old_writer_delivery.rs:231, chat_shards.rs:214, group_g2.rs:197, rotation_acceptance.rs)
    pub fn restore_in_place(&mut self) -> (String, String); // device_list_push.rs:185, device_list_characterization.rs:170, restore_ticket.rs, old_writer_delivery.rs, rotation_restore_characterization.rs
    pub fn logged(&self, needle: &str) -> bool; // 7+ copies
}
pub fn pair(tag: &str) -> (Peer, Peer);  // 10 copies
pub fn online(n: &Node);
pub fn eventually(secs: u64, what: &str, f: impl FnMut() -> bool);
```
Savings: about 100 to 150 lines per file x 25 files = 2,500 to 3,500 lines. Caveat: the five `restore_in_place` copies have different md5 (they differ in what they assert and the passphrase); unify by returning `(old, new)` and letting callers assert.

### 1.3 Temp dirs
36 inline `temp_dir().join(format!("p2pcore-<prefix>-{name}-{pid}"))` plus `remove_dir_all` first. Sites: calls.rs:47, call_tokens_acceptance.rs:100, accounts.rs:25, fanout.rs:47, device_list_characterization.rs:103, gcall.rs:70, devices.rs:44, chat_shards.rs:116, group_shards.rs:72, blocking_acceptance.rs:200, group_call.rs:84 and :363 (duplicated even within the file), restore.rs:27, lifecycle.rs:69, group_g2.rs:72, video_calls.rs:56, endpoint_close.rs:56, grant_characterization.rs:122, restore_ticket.rs:60, profile_sync.rs:27, link.rs:77 and :627, link_expiry.rs:33, group_g3.rs:82, writer_id.rs:51, tickets.rs:32, device_list_push.rs:117, readd.rs:54, rotation_restore_characterization.rs:120, rotation_acceptance.rs:264, vault.rs:20, old_writer_delivery.rs:110, group_core.rs:72, search.rs:37. Also `src/account_tests.rs:18`, `src/store.rs:730`, `src/voice.rs:631`, `src/chat/blobs_tests.rs:12`, `src/search/index.rs:1534`.
Proposed: `support::TempDir::new(tag)` (RAII, unique via pid + atomic counter, removes on drop). It would replace per-file `Dir`/`Root`/`Peer::drop` cleanups (calls.rs:42, accounts.rs:44, vault.rs, gcall.rs, video_calls.rs). The `tempfile` crate would do the same if available. A `core` `#[cfg(test)]`-gated shared helper cannot be seen from integration tests, so the src copies stay (5 copies in src/ are a separate small item).

### 1.4 Chat query helpers
- `all_msgs(p, did)`: pager over `chat_day` is byte-identical in readd.rs:93 and writer_id.rs:81 (and in 11 more files)
- `holds(p, did, id)`, `delivered(p, did, id)`, `has`, `ticks`, `find`, `state` (chat_shards.rs:195-212, device_list_characterization.rs:255-259)
- `incoming(p, secs) -> String` (10 copies), `saw_incoming` (call_tokens_acceptance.rs:182, blocking_acceptance.rs:298), `ended_after` (call_tokens_acceptance.rs:160, blocking_acceptance.rs:276, calls.rs `ended` :103, device_list_characterization.rs:206), `rings`/`call_rings_on`/`call_answered` (call_tokens_acceptance.rs:205, device_list_push.rs:255, device_list_characterization.rs:268)
Proposed: `Peer::messages(&self, did)`, `Peer::holds`, `Peer::delivered`, `Peer::wait_incoming(secs)`, `Peer::wait_ended(id, secs) -> (reason, Duration)`, `support::assert_rings(from, to)`, `support::assert_message_arrives(from, to, text)`. About 400 lines.

### 1.5 Group helpers
`join`, `all`, `posts`, `has_post`, `members`, `sorted`, `group_with(owner, members)`, `lists`: copy-pasted across group_core.rs:114-184, group_g2.rs:114-180, group_g3.rs, group_shards.rs, group_call.rs:106-127. Plus `admin_group` (group_core.rs:354). Proposed `support::group` submodule (`group_with`, `Peer::group_messages_all`, `Peer::group_members`). About 300 lines.

### 1.6 Time helpers and small utilities
- `now_ms()` x5: chat_shards.rs:156 (i64), devices.rs:110, device_list_push.rs:267, search.rs, group_shards.rs. Also `now()` in grant_characterization.rs:216, restore_ticket.rs:95, rotation_restore_characterization.rs:280. Proposed `support::now_ms() -> u64`; chat_shards' i64 variants are only used there, keep local.
- `encode`/`decode` base64/hex in device_list_push.rs:411-430 and device_list_characterization.rs:527-545 (identical): CROSS-FOLDER: check whether `crates/proto` or `identity` exposes the same codec (production crate probably already has base64; reimplementing in tests is suspicious).
- `wait(what, f)` in devices.rs:64 and profile_sync.rs: another `eventually`; `within` in device_list_*.rs:214/232 is a third.
- `count/files/contains/assert_sealed/assert_no_phrase` (accounts.rs:49-85) vs `find_files` (group_g2.rs:181) vs `snapshot`: file-walking helpers; could share `support::walk(dir)`.
- Blocking helpers `block_today`, `ignores_notices` (blocking_acceptance.rs:251-260) + `refusals` (:610) are local only, fine.

### 1.7 CROSS-FOLDER
- `crates/proto/tests/support/` (rotation_network.rs, rotation_permit_lifecycle.rs, rotation_permit_network.rs) is a separate model, not the node harness; but a shared dev-only crate (`crates/test-support`, `publish = false`) would be the right home if proto/group-sim/peer/desktop tests also need `Quiet`, `TempDir`, `eventually`. Other auditors should check `crates/peer`, `crates/desktop`, `crates/group-sim`, `crates/gcall-bench` for `eventually`/temp-dir/`online` copies. Note `Node`/`NodeEvents` live in core, so a node-level harness needs a dev-dependency on `p2pcore`; the generic bits (`TempDir`, `eventually`) can go in a crate with no deps.
- `crates/core/src/**` tests: `account_tests.rs:11` (`Quiet`) and `:18` (`tmp`), `store.rs:730` `tmp`, `voice.rs:631` `tmp`, `chat/blobs_tests.rs:12` `tmpdir`, `search/index.rs:1534` `temp_path`: 5 more temp-dir helpers, same shape. A `#[cfg(test)] pub(crate) mod testutil` in core would dedupe the in-crate ones; integration tests cannot import it unless exposed through a `test-support` cargo feature (core already has many `_for_test` public methods, see 4).

## 2. Duplicated or overlapping tests

Genuinely redundant, merge or delete (ranked):
1. **grant_characterization.rs vs call_tokens_acceptance.rs.** grant_characterization.rs was written before P6 landed; its own header (line 25) says P6 flipped its limitations. Overlaps:
   - `block_takes_both_tokens_back_and_unblock_does_not_revive_them` (grant :593) vs `block_revokes_both_tokens_and_unblock_does_not_revive_them` (ct :308): same scenario, grant uses a raw wire client, ct uses a modified-app flag. Keep both only if the raw-wire coverage matters; otherwise merge.
   - `ticket_contacts_can_message_and_call_each_other` (grant :705) vs `ticket_contacts_can_chat_and_call_both_ways` (ct :234).
   - `calls_are_refused_per_contact_while_messages_flow` (grant :728) vs `disallow_calls_stops_calls_but_not_chat` (ct :287) and `a_message_only_contact_chats_but_its_call_is_refused` (ct :253).
   Suggest merging into one `tokens.rs` with one raw-wire section and one node-level section; estimated -250 lines.
2. **device_list_characterization.rs vs device_list_push.rs.** Same helper block (lines 91-300 vs 107-275) and the same scenarios: `adding_a_device_reaches_the_contact_without_a_call` (char :332) = `linking_a_device_lets_the_contact_reach_it_without_a_call` (push :278); `unlinking_a_device_reaches_the_contact_without_a_call` exists in BOTH files under the identical name (char :349, push :300); `a_blocked_caller_receives_no_device_list` (char :426) = `a_blocked_contact_is_not_told` (push :345); `merge_refuses_stale_replayed_forged_tampered_and_wrong_did_lists` (char :444) = `stale_replayed_forged_and_far_future_pushes_are_refused` (push :366). encode/decode copied. Merge into one `device_list.rs`; -400 lines. Header in the characterization file itself says the limitations "are flipped to invariants", i.e. it is a stale predecessor.
3. **fanout.rs device-list tests (:333-:412) vs device_list_*.rs and devices.rs**: `device_list_newer_replaces_older_is_ignored`, `forged_and_foreign_device_lists_are_ignored`, `verified_resets_only_when_a_new_device_appears` (also devices.rs:75 `unfamiliar_device_in_a_call_resets_verified_and_notifies`), `own_device_list_is_signed_and_changes_with_the_registry`. These test list merging, belong with item 2. Likely 2 to 3 of them are unit-testable in `src/devlist.rs` without any network.
4. **restore family**: restore_ticket.rs (3 tests), rotation_restore_characterization.rs (10 tests, `o1_restore_reissues_the_ticket_for_the_new_device_key` :342 is a strict subset of `after_restore_my_ticket_names_the_current_device_key_and_redeems` restore_ticket.rs:130; the file itself says "redeem/chat coverage lives in tests/restore_ticket.rs"), old_writer_delivery.rs (4; `a_message_pending_at_a_restore_is_delivered_without_a_new_write` :274 overlaps `o3_pending_message_reaches_a_contact_that_holds_no_device_list` :366), writer_id.rs `a_restore_makes_a_new_writer_id` :160. All use the same `restore_in_place`. Merge into one `restore_over_existing.rs`; delete the o1 duplicate; -250 lines. Not to be confused with `restore.rs` (key-from-phrase vault re-key), which is a different subject.
5. **chat_shards.rs time-skew block (:305-:370)**: already table-like via `post_dated(tag, at_from_day_start...)`, but four tests (:328, :333, :338, :343) and two live-push tests (:351, :362) are constant-only variants. Convert to a single `#[test]` iterating a `[(offset_min, accepted)]` table (but share ONE pair of nodes; right now each case builds 2 online nodes: 6 cases = 12 nodes). -40 lines and about 5 fewer node pairs of startup.
6. **Groups**: group_core.rs (12) / group_g2.rs (8) / group_g3.rs (8) / group_shards.rs (13) / group_call.rs (3) are different features, not duplicates; only helpers overlap (1.5). `a_second_device_of_a_member_joins_sees_and_sends` (group_core :324) and `a_linked_device_learns_the_accounts_groups_without_a_token` (group_g2 :340) are adjacent, not equal. group_g2.rs `the_host_rotates_after_a_removal...` (:479) vs group_core `three_admin_flags_remove_a_member_everywhere` (:374) share the removal fixture only.
7. **calls.rs vs fanout.rs vs call_tokens vs blocking**: `glare_leaves_exactly_one_call` (calls :156) and `glare_between_a_and_two_device_b_leaves_one_call` (fanout :282) are a related pair (1 vs 2 devices); `blocked_caller_never_rings_and_ends_quickly` (blocking :353) and `blocked_caller_ends_at_once_with_the_blocked_reason` (:413) look near-duplicate (check: second likely adds the reason assert; merge). `second_call_declined_ignored_or_answered` (calls :327) already table-like.
8. **blocking_acceptance vs rotation_acceptance blocking-related**: rotation_acceptance `blocked_contact_does_not_learn_the_new_endpoint_and_loses_reach` (:584) and `unblocked_contact_gets_the_current_endpoint` (:618) are ignored stubs; no overlap until built.
9. Block-notice tests (`block_sends_one_signed_notice...` :489, `a_forged_block_notice_is_refused` :507) have unit-level twins in `src/block.rs:423` (cfg(test)); the integration copies could run without network if the notice path is exercised with `offer_block_notice_for_test` only. Verify before merging.

## 3. Dead, ignored, network/timing risk

### 3a. #[ignore]d: 18 tests, ~460 lines of stubs
- rotation_acceptance.rs: 15 ignores at :438, :454, :474, :501, :528, :547, :565, :583, :617, :633, :667, :705, :719, :748 (+ header). The file header (:24) calls them `unimplemented!("R1: ...")`. They are specification placeholders for unbuilt features (forced rotation, grace window, scheduler). Keep as spec only if endpoint rotation is on the launch list (it is, per memory "launch scope"); otherwise move to docs and delete. Note the tests still carry full helper blocks (`blank`, `open`, `pair`, `link_fresh`, etc.).
- blocking_acceptance.rs: 3 ignores at :20 (header), :465 (`blocked_contact_is_excluded_from_device_list_notes`: reason "device list travels only inside call hello/accept (no push)" is STALE: device-list push exists (device_list_push.rs, `a_blocked_contact_is_not_told`); un-ignore or delete), :740 (`personal_block_does_not_change_group_membership`: reason "needs the group chat API (none in p2pcore yet)" is STALE: group_core.rs etc. exist; implementable now, and AGENTS.md explicitly calls out that blocking and group membership are separate policies, so this test is valuable).
- The `create_group`, `add_group_member`, `group_members`, `send_group_text`, `group_texts` shims in blocking_acceptance.rs:106-122 are `unimplemented!` placeholders for a group API that exists now (dead code).
- blocking_acceptance.rs:59-100 defines a `pub mod` of wrapper fns (`block`, `unblock`, `block_state`, ..., `calls_allowed`) around Node methods: thin pass-throughs that now just re-spell the real API; check and inline.

### 3b. Dead or single-use helpers
- `support/group_call_capacity.rs` is included only by `group_call.rs:26-27` via `#[path]`; fine but misplaced under `support/`.
- `peer_with_spare` (group_call.rs:362) duplicates `peer` (:83) including the identical temp-dir format (:84 vs :363).
- `Dir`, `Root`, `Fixture`, `Tmp` structs are single-file reinventions of TempDir (calls.rs:42, accounts.rs:24, vault.rs:20, restore.rs:27, search.rs:37, gcall.rs:70, video_calls.rs:56).
- Possibly unused after merges: `within` (device_list_*), `wait` (devices.rs vs profile_sync.rs). Compiler warnings would confirm; not run (read-only).

### 3c. Network / timing exposure (flaky risk)
- 23 of 32 files say "real iroh endpoints / needs network". They need relay/discovery, not just loopback. Only `gcall.rs` and `group_call.rs` say loopback/no relay; `accounts.rs`, `vault.rs`, `restore.rs`(?), `link_expiry.rs` and the "You" conversation in `search.rs` need no network. Consider tagging network tests with one feature or `#[cfg_attr(not(feature="net-tests"), ignore)]` so offline CI has a clean run (AGENTS.md: "Separate network/device-dependent validation from deterministic offline tests").
- Fixed sleeps used as negative-assertion waits ("nothing happened after N s"): blocking_acceptance.rs (:343 2 s, :374 8 s, :477 5 s, :499 3 s, :629 2 s, :640 5 s, :654 2 s, :663 5 s, :723 1 s, :762 5 s), call_tokens_acceptance.rs:326 (8 s), group_core.rs:412 (10 s), device_list_push.rs:331 (3 s), group_call.rs:298 (3 s), link.rs:579/668/687, lifecycle.rs:125, chat_shards.rs:319, group_shards.rs:199/648. These make the suite slow (blocking_acceptance alone has about 40 s of sleeps) and are inherently racy for the "positive" counterpart. A test hook such as a "quiescence" API (`node.wait_idle()` or outbox-drained event, see 4) would replace many.
- Wall-clock and latency asserts: `chat_shards.rs:525 single_message_latency_on_loopback_is_well_under_a_second` and `:272 pct(...)` percentile math are performance assertions in a functional suite; AGENTS.md says timing-sensitive tests need a release profile. Mark `#[ignore]`/bench-only or move to `crates/gcall-bench`-style bench. Same for `calls.rs:196` (2 s sleep to age state), `video_calls.rs` (9 sleeps), `gcall.rs` 50 Hz tone feed with amplitude (`group_call.rs:198 amplitude_at`, `:212 assert_hears`: Goertzel-style audio check duplicated from gcall.rs; the audio helpers are COPIED, the header admits "the audio helpers come from gcall.rs").
- Uniform 90 s `WAIT` (group_core.rs:15) and 30 s link-code timeouts: a hang costs minutes.
- Temp dir uniqueness uses pid + test name; two tests in the same binary with the same `name` collide (group_call.rs defines the same prefix in two places); `Drop` removes the dir while node tasks may still hold redb files ("Database already open" seen in comments at endpoint_close.rs / lifecycle.rs header).
- Hard-coded port or relay: none seen, but all tests dial public iroh discovery.

## 4. Missing production abstractions the tests reveal
1. **Test hooks leaking into the public Node API**: `_for_test` methods used across tests (counts: rotation_acceptance 58, search 32, fanout 30, blocking 27, link 27, chat_shards 20): `block_notices_sent_for_test`, `forge_block_notice_for_test`, `offer_block_notice_for_test`, `ignore_block_notices_for_test`, `held_token_ids_for_test`, `add_contact_as_for_test`, `writer_id_for_test`, set-clock hooks (chat_shards header: "Time is injected per node with ..."). Because integration tests cannot see `cfg(test)` items they are `pub` in the released crate. Propose a `test-hooks` cargo feature (or a `NodeTestHandle` object returned by `Node::test_handle()` under that feature) so shipped FFI/UniFFI surface stays clean. Check also against the Android/Kotlin bindings.
2. **Waiting API**: tests poll for `status().online` (online()), `contacts()`, `chat_day` presence, "outbox drained", `writer_id_for_test().is_some()` (readd.rs:79). A real `Node::wait_online(timeout)` and an event/`flush` for outbox idle would delete `online()` x20 and many sleeps.
3. **Message paging**: `all_msgs`/`all` re-implement "read every day/page" (13 + 5 copies). Offer `Node::chat_history_iter(did)` or `chat_all(did)` (also used by the search code and export?). Similarly group `all()`.
4. **Fixture builder in the library**: `link_fresh` encodes the whole link-QR/code/approve protocol; the desktop app and Android do the same flow. A `Node::link_new_device(existing) -> Result` for in-process tests, or at least a shared `LinkEvents` channel adaptor, would help.
5. **Event adapter**: every consumer (desktop app, Android, tests) implements the no-op halves of `NodeEvents`/`LinkEvents` by hand. Provide `default` method bodies in the traits (or `NoopEvents`) so implementors override only what they care about; tests then shrink to 1 to 2 methods. CROSS-FOLDER: check desktop/peer crates for the same boilerplate.
6. `DeviceList`/`ContactTicket` base64 `encode`/`decode` helpers (1.6) and ticket claim parsing (`proto::ContactTicket::from_text(..).verify(now())`, restore_ticket.rs `claim`) hint at a missing `Node::ticket_claim()` or test-only decode helper; minor.

## 5. Ranked recommendations (payoff / risk)
1. `tests/support/mod.rs`: `Quiet`, `Tap`, `LinkTap`, `TempDir`, `Peer`, `online`, `eventually`, `pair`, `link_fresh`, `restart`, `restore_in_place`, chat/group query helpers. Saves 3,500 to 4,500 lines; risk low (mechanical; do file-by-file, run each binary). Do first.
2. Merge device_list_characterization + device_list_push (+ fanout list tests) and grant_characterization + call_tokens_acceptance and the restore family: -900 lines, risk medium (check that each unique assertion survives; the "characterization" files are historical records with KIND labels).
3. Make `NodeEvents`/`LinkEvents` default no-op (or `Noop*` structs) in core: 31+15 test impls vanish, and desktop gets the same benefit; risk low, but touches the public trait/UniFFI definition.
4. Replace fixed negative-assertion sleeps with `wait_idle`/`flush` hooks and put perf and latency checks under `#[ignore]`: runtime and flake risk, medium effort.
5. Delete or implement the stale `#[ignore]` stubs (blocking_acceptance :465, :740 and shims :106-122; rotation_acceptance 15 stubs): -400 lines; the two blocking ones are implementable today.
6. Feature-gate `_for_test` hooks (`test-hooks` feature); medium, touches FFI.
7. Cargo test-binary consolidation (`tests/net/main.rs` etc.) for compile time; medium risk (shared process: dir collisions).

## 6. File map (tests count, subject)
- accounts.rs (11): account dirs, v1/v2 migration, sealed persistence, switching, no-passphrase commit. No network.
- blocking_acceptance.rs (19, 3 ignored): visible blocking: notices, reason strings, unblock, sibling devices, forged notices.
- calls.rs (12): 1:1 call state-machine edge cases (cancel, decline, glare, busy/waiting, switching account in call).
- call_tokens_acceptance.rs (7): P6 separate message/call tokens (ticket vs group origin, allow/disallow, block revokes).
- chat_shards.rs (15): 1:1 chat day shards, UTC boundary, windows, offline gaps, cache, burst/latency.
- device_list_characterization.rs (7): how contacts learn the signed device list (post-U8 push, seq bound, merge refusals).
- device_list_push.rs (6): P4 push of DID-signed device list on link/unlink/restore; stale/forged refusal.
- devices.rs (1): second device of same person; unfamiliar device resets verified.
- endpoint_close.rs (1): stopped endpoint closes fully before restart binds the next.
- fanout.rs (13): multi-device ringing (one answer wins), plus device-list merge tests.
- gcall.rs (3): group calls on loopback with StaticDirectory, synthetic tone mic/speaker.
- grant_characterization.rs (8): what tokens a contact needs to message/call, raw-wire client; revocation by DID.
- group_call.rs (3 + capacity module): group calls wired to group chat, call-host permits, handover.
- group_core.rs (12): G1 create/invite/join/post, host endpoints, removal by 3 admin flags.
- group_g2.rs (8): G2 lost group store / writer counters, chat requests via host, host rotation.
- group_g3.rs (8): G3 direct contact invites, short invite codes.
- group_shards.rs (13): group day shards, paging, compaction, backfill.
- lifecycle.rs (6): stop/start, lock/unlock, restart, account switch with groups, chats, owed outbox.
- link_expiry.rs (1): unscanned link QR dies (own binary: shortens a process-global lifetime).
- link.rs (28): device linking, own-device sync, unlink.
- old_writer_delivery.rs (4): own ops from older writer IDs still reach contacts after restore/sibling.
- profile_sync.rs (9): profile/pictures sync API.
- readd.rs (4): block-unblock-readd and restarts never reuse (writer, counter).
- restore.rs (6): vault keyed by phrase, re-key of old stores. No network (likely).
- restore_ticket.rs (3): ticket after restore names new device key.
- rotation_acceptance.rs (15, 14+ ignored): endpoint rotation spec; most not built.
- rotation_restore_characterization.rs (10): K1 effects of restore-over-existing (O1 to O6).
- search.rs (20): local search over "You", 1:1 and group.
- tickets.rs (4): ticket/contact rules.
- vault.rs (8): passphrase vault, real Argon2id.
- video_calls.rs (3): video in 1:1 calls with synthetic camera.
- writer_id.rs (3): U16/P9c writer ID derivation.
- support/group_call_capacity.rs: module for group_call.rs (call-host capacity bounds).
