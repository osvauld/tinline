# Audit 06: crates/core top-level + small crates (read-only; paths under /home/abe/p2p_chat/crates)

Depth note: I skimmed structure, signatures and selected bodies (node.rs dispatch, store.rs, voice.rs, video_call.rs, wire.rs, the small crates). I did not read all of node.rs line by line; line estimates are rough.

## 0. App-facing API surface (priority section)

### How each frontend calls core today
- **One shared Rust API, exposed two ways.**
  - Android: UniFFI-generated Kotlin (`android/app/build/generated/uniffi/kotlin/uniffi`). `uniffi::setup_scaffolding!()` is at `core/src/lib.rs:4`. `#[uniffi::export]`/`Record`/`Enum`/`Object`/callback interfaces are spread over about 15 core files: node.rs (2 exports, 11 types), chat/api.rs, group/{api,calls,requests,invitations,code}.rs, search/api.rs, sync/api.rs, profile/mod.rs, block.rs, caps.rs, voice.rs, video_call.rs, store.rs.
  - Desktop: `crates/desktop` links `p2pcore` directly as a Rust lib (`desktop/Cargo.toml:12`) and calls the same `Node`, `Chat`, `Group*` objects. About 60 `p2pcore::` references in desktop/src.
  - So there is NOT two parallel facades at the Rust level. The `Node` object plus the sub-objects (`Chat`, group host, etc.) are the single facade.
- **Hand-written JNI is the exception.** Only `core/src/video_jni.rs` has `#[no_mangle]` JNI (`Java_com_osvauld_p2p_VideoJni_pushYuv` at :43-80, `pullRgba` at :85-110). It exists for direct ByteBuffer/zero-copy video planes. Kotlin side: `VideoIo.kt:65,135`. There is also a handle registry (`frame_port`, `video_call.rs:474`, `Node::video_frame_handle` :907). It is gated `#[cfg(target_os="android")]` (lib.rs:28).
  - Related UniFFI workaround: `Bytes` newtype with a hand-written `FfiConverter` (`video_call.rs:68-135`) so Kotlin gets a ByteArray, not `List<UByte>`. PCM has the same hack as `push_mic_pcm16`/`pull_speaker_pcm16` (`node.rs:1143-1150`).
- **Where the platform-neutral line actually breaks (logic that lives in each frontend, not core):**
  1. **View-model/state layer is duplicated per frontend and not in core.**
     - Desktop: `desktop/src/chat.rs` (1048), `groups.rs` (327), `group_live.rs` (1139), `blocks.rs` (470), `link.rs` (554), `flow.rs` (241), `compose_state.rs`, `keystore.rs`.
     - Android: `ChatSource.kt` (122, `ChatSource` interface plus `CoreChatSource` adapter at :68, sealed `ChatEvent` :17), `GroupsSource.kt` (294), `Blocking.kt`, `Devices.kt`, `Accounts.kt`, `History.kt`, `Support.kt`.
     - Both re-derive "chat list state, unread, pending delivery, event -> state reduction" independently. Example: `desktop/src/app.rs:1046` filters `m.outgoing && delivery == Pending`, and Kotlin has its own `ChatEvent.Delivery` reducer.
  2. **End-reason text.** `desktop/src/reason.rs` ("the one place a core end-reason token becomes words") is re-implemented in Kotlin (`Support.kt`, `HomeLists.kt`, `History.kt`, `CallController.kt` all match `no_answer`/`declined_local`). CROSS-FOLDER. Strings should live in core, or core should return a typed `EndPresentation`, per VOICE.md. Same for BLOCKED/CALLS_NOT_ALLOWED reason mapping (`app.rs:925-928`, `group_live.rs:158`).
  3. **Fakes/demo duplicated.** `desktop/src/chat_fake.rs` (399), `groups_fake.rs` (1761), `demo.rs` vs Kotlin `FakeGroupsPrivacy.kt` (510), fake groups in `GroupsSource.kt` (its header says "None of this exists in the core yet", which is stale now that `group/api.rs` exists). Same fixture data written twice. A core `demo`/`fake` feature that implements the same traits would serve both.
  4. **Group API shapes are mirrored by hand.** Kotlin `GroupsSource.kt` re-declares `GroupRole`, `GroupFlag`, `GroupMember`, `GroupHost`, `GroupDelivery`, `Outcome` (:20-61) that also exist as UniFFI records in `core/src/group/api.rs`. Ignoring name clashes with generated types, this is a hand-copied model. Delete it in favour of the generated types.
  5. **Platform call plumbing.** Node exposes `push_mic`/`pull_speaker` (pull model, `node.rs:1132-1150`) and desktop `audio.rs:545` / Android CoreService each drive it with their own timers and call-state hooks (`audio.rs on_state` matches `CallState::Active/Ended` to start and stop streams; Android does the same via Telecom).
- **Why this matters for the "one implementation" goal.** The core already contains the protocol, state and persistence. What is missing is a **presentation-model layer** (what to show, in what order, with which copy) and a **single event stream**. Today there are 5 callback traits on `NodeEvents` (`node.rs:77-81`) plus `ChatEvents`, `GroupEvents`, `GroupRequestEvents`, `GroupInvitationEvents`, `GroupCallEvents`, `LinkEvents`, `ProfileEvents`, `VideoEvents`, `ProfileEvents`. That is about 9-10 separate callback interfaces, each registered separately and each hand-adapted per frontend (73 `impl ...Events for` across core/desktop/peer).

### Proposed platform-neutral layer
Name: `core/src/app/` (public module `p2pcore::app`; keep `Node` internal-ish or as the engine).
```
app::Session (uniffi::Object)       // one per process; owns Node + Chat + Groups + Search
  fn dispatch(&self, cmd: Command) -> CommandResult     // all user intents
  fn snapshot(&self, view: ViewId) -> ViewModel         // pull the current model for a screen
  fn subscribe(&self, sink: Arc<dyn AppSink>)           // ONE callback interface
trait AppSink { fn on_event(&self, ev: AppEvent); }     // coarse invalidation: AppEvent::{Changed(ViewId), Call(CallEvent), Toast(Notice), ...}
enum Command { CreateIdentity{..}, Unlock{..}, SwitchAccount{did}, AddContact{ticket}, StartCall{did}, Answer{id}, SendMessage{peer,text}, BlockContact{..}, ... }
enum ViewModel { Home(HomeVm), Chat(ChatVm), Call(CallVm), Contacts(..), GroupList(..), GroupDetail(..), Settings(..), Devices(..), Blocked(..) }
```
- View models carry already-presented data: display names (alias vs name), sorted/grouped lists, unread counts, delivery state, and presented copy via `app::copy::end_reason(reason, name, ctx) -> End` (move `desktop/src/reason.rs` here and use it from Kotlin too). Presentation-only details (spacing, colour, animation) stay in the UI.
- **Reduction logic moves to Rust**: `chat.rs`/`ChatSource.kt` event reducers, pending-send queues, optimistic message insertion, group request state machine (`ChatRequestState`), lock/gate flow (`flow.rs`, `gate_view.rs`), compose state. Each frontend keeps only widgets.
- **Binding technology.** UniFFI fits and is already in use, so keep it. A single `dispatch(Command)`/`on_event(AppEvent)` pair of enums is a better UniFFI fit than 200 exported methods (fewer generated symbols, one sink, trivially mockable in tests and fakes). Do not hand-write more JNI; the only legitimate JNI is zero-copy media (`video_jni.rs`), and that could be replaced by UniFFI `Vec<u8>` for planes at 30 fps only if profiling allows. Keep `video_jni.rs` and wrap it in `app::media` so Kotlin and desktop share the same frame-port semantics. For desktop, `Session` is called directly in-process (no generated layer), implementing `AppSink` with an Iced `Subscription`/channel.
- **Migration order (payoff/risk):** (1) move `reason.rs` copy into core and expose via UniFFI (small, certain, kills Kotlin copy); (2) delete Kotlin hand-copied group models in `GroupsSource.kt` in favour of generated ones; (3) unify the ~9 event traits into one `AppSink` (additive: keep old traits as adapters); (4) introduce `ChatVm`/`HomeVm`; (5) shared fakes behind a `fake` feature. Risk: medium-high for 4 (large UI rewiring), the rest low.
- **Acceptance test idea:** a headless core-level test drives `Session` with scripted `Command`s and asserts the `ViewModel` sequence; both UIs then only need render tests.

## 1. Production duplication in core top-level (ranked)

| # | Finding | Where | Proposal | Lines saved | Risk |
|---|---|---|---|---|---|
| 1 | `io(e: StorageError) -> Error` copied 5 times | store.rs:385, rekey.rs:56, chat/store.rs:24, group/store.rs:67, search/index.rs:76 | `impl From<storage::StorageError> for Error` in error.rs (core/src/error.rs already has From for io/serde/proto/audio, :49-67) and delete the 5 fns plus `.map_err(io)` | ~30 | very low |
| 2 | Wall-clock helpers: `now()`, `now_ms()`, `random_id()` in node.rs:3658-3666; `now_ms` again in chat/engine.rs:157; desktop view.rs:154, chat_fake.rs:20; at least 9 test files define their own (`core/tests/*`, year-bench.rs:53) | see grep | `crates/core/src/clock.rs` (`now_secs()`, `now_ms()`), plus a `#[doc(hidden)] pub mod testkit` so tests share it | ~60 | low |
| 3 | After-reply "linger then close" tail repeated in `handle_incoming`: `tokio::time::timeout(LINGER, conn.closed())` 20 times in node.rs; each branch in 2321-2430 is `drop(permit); handler(ctrl,..).await?; linger; Ok(())` | node.rs:2392-2425 | `async fn linger(conn)`; `Ctrl::reply_and_close(msg, conn)`; table of `Msg` kinds -> handler fn that drops the permit up front | ~50 | low |
| 4 | Opus `Encoder`/`Decoder` unsafe wrappers duplicated | core/src/voice.rs:231-320 (`Encoder::new/ctl/lookahead/encode`, `Decoder`, `audio_err` :24) vs audio/src/lib.rs:147-` (`Encoder(*mut sys::OpusEncoder)`) | Expose `audio::opus::{Encoder,Decoder}` (configurable rate/bitrate) and have voice.rs use it, dropping the `opusic-sys` dep from core | ~120 and removes a direct unsafe/FFI dep | medium (voice message bitstream must stay identical; there are round-trip tests at voice.rs:631+) |
| 5 | `Node` public methods are thin wrappers: `inner.clone()` + `self.block_on(...)` + `?` flatten, e.g. node.rs:1027-1029, 1008-1010, 654-656, 663-665, 974-976; `Result<Result<..>>` flatten each time | node.rs:1224-1280 (`block_on`), 626-1160 | `fn run<T>(&self, f: impl FnOnce(Arc<Inner>) -> impl Future<Output=Result<T>>) -> Result<T,Error>` that does clone + block_on + flatten | ~40 | low |
| 6 | `update_contact`-style closures and `self.inner.shared.lock().state.contacts.iter().any(...)` repeated for contact lookup | node.rs:1023, 1032-1045, 1181 and other modules | `State::contact(&self, did) -> Option<&StoredContact>`, `contact_mut` | ~30 | low |
| 7 | `Node::push_video_frame*` family (ffi, yuv, rgba, yuyv, planes at video_call.rs:763-905) and `push_mic/_pcm16` are adapter variants that differ only in input conversion | video_call.rs:758-925 | `enum FrameSrc` already exists (`Source`); collapse to one `push_frame(FrameSrc)` internal plus thin exports; UniFFI exports must stay | ~60 | low |
| 8 | Test-only hooks sit in the production impl: 11 `*_for_test` fns in node.rs:1422-1520, synth.rs (173 lines) | node.rs | Move to `node/testkit.rs` (`#[cfg(any(test, feature="test-hooks"))]`); today they are `pub` UniFFI-visible only if exported (they are `#[doc(hidden)] pub`) so they ship in the cdylib | ~100 moved | low |
| 9 | Disk/legacy-profile enum matching: `Disk::V2` etc. at 34 sites in node.rs; legacy JSON profile + `Backing::Json` path (store.rs:321-330, 389-405, 528-700) coexists with sealed redb | store.rs, node.rs | Decide a sunset for the JSON legacy path (open decision, do not delete silently); until then, hide behind `Account` trait with `Json`/`Sealed` impls | up to ~400 if sunset | high (data migration) |
| 10 | `docs` cross-reference: `desktop` and `peer` re-declare `NodeEvents` impls (no-ops, logging) | desktop/src/keystore_tests.rs:5, flow.rs:87, account_tests.rs:10 (`Quiet`) | `NodeEvents` default no-op impl (`struct NoEvents`) in core, exported `#[doc(hidden)]`; 57 `Quiet` hits in core alone | ~150 | low |

voice.rs vs video_call.rs: not call-setup overlap. voice.rs is Ogg/Opus **voice messages** (recorder/decoder, CRC, page framing, ~600 lines of code plus ~400 tests); video_call.rs is the video pipeline + UniFFI exports. Call setup lives in node.rs (`start_call` :2805, `run_outgoing` :2863, `run_call` :3074, `incoming_call` :2453). Real overlap between them: the sizing/validation helpers (`plane_fits`, `size_ok`, ...) are video-only. The Ogg CRC (`voice.rs:39-115`) is self-contained: could live in an `ogg.rs` submodule.

## 2. node.rs split (3770 lines)
Rough seams by line range (struct + impl blocks already separate them):
```
node/mod.rs        Node, Inner, Shared, Me, NodeEvents + Node::new/start/stop            (1-600, 1536-1700, 1905-2090)
node/types.rs      NodeStatus, ProfileInfo, AccountSummary, LockState, Contact, CardPeek, Availability, CallInfo, CallState, CallStats (77-200, 1181)
node/api_identity.rs   Node pub fns: accounts, passphrase, unlock/lock, recovery_phrase, set_name (600-970, 1277-1420)
node/api_contacts.rs   Node pub fns: contacts/rename/verify/safety_number/availability (1003-1095, 2166-2263, 3490-3585)
node/api_call.rs   Node pub fns: call/answer/decline/hangup/mic/speaker/stats (1095-1160)
node/persist.rs    persist*, write_state, create/restore_account_files, commit_locked, replace_account (1617-1830)
node/lifecycle.rs  start, endpoint open/close, ticket, relay helpers (1855-2165, 3590-3615)
node/accept.rs     handle_incoming + accept_hello + notice/tokens/devices dispatch (2264-2433)
node/call.rs       Call, Leg, Cmd, SlotGuard (180-510), incoming_call, begin_call, run_outgoing, run_call, set_state, end_call, record_ended (2434-3470)
node/devices.rs    note_device, accept_device_list, my_device_list, merge_device_list (2594-2720, 3541)
node/testkit.rs    all *_for_test (1422-1520)
node/util.rs       now/now_ms/random_id/core-thread detection/msg_name (3616-3670)
node/tests.rs      3674-3770
```
Call state machine (Leg::run 319-505 plus run_call 3074-3220, about 650 lines together) is the heaviest unit; extracting it as `call/` with its own `CallRuntime` struct reduces Inner's lock surface.

## 3. Test duplication
- account_tests.rs (119 lines): has its own `Quiet` NodeEvents (:10-16), `tmp()` (:18), `new_contact()` (:24), `record()` (:35); only one test (`stale_session_cannot_write_into_next_account`, :50). `tmp` pattern repeats in store.rs:730, voice.rs:631, chat/blobs_tests.rs:12, chat/blobs.rs:115 and many core/tests files. Proposal: `core/src/testkit.rs` (`pub mod testkit` under `cfg(any(test, feature="testkit"))`): `TempDir` (Drop-removing, replaces `process::id()` names that leak and can collide), `NoEvents`, `new_contact_fixture`, `fixed_clock`. Saves ~200 lines across 15 files. Note `new_contact` builds with synthetic identities: consistent with AGENTS.md.
- voice.rs tests (~400 lines, :631-1036) include hand-built Ogg byte fixtures; fine, but would be neater in `voice/tests.rs`.
- node.rs tests (3674-3770): small unit tests; fine, move to node/tests.rs.

## 4. Small crates
Workspace members: identity, cryptography, proto, audio, video, storage, group-sim, gcall-bench, core, peer, desktop, tin-icons.
- **cryptography (334 lines: aead 36, ecies 60, kdf 35, signature 22, error 11)**: depended on by identity (Cargo.toml:8) and used by core `vault.rs:13` (`use cryptography::{aead, kdf}`). Core also depends directly on `aes-gcm = "0.10"`, `sha2`, `blake3` (core/Cargo.toml). Need to check `chat/blobs` and `rekey.rs` for primitives that bypass the crate (rekey uses blake3 derive-key directly). Low value as a separate crate but cheap; merge into `identity` as `identity::crypto` (cryptography has no other consumers besides identity and vault.rs) or keep and move core's direct AEAD/KDF uses (vault.rs, rekey.rs, storage `Sealed` which also uses AES-GCM "via cryptography") into it. Recommendation: keep, make it the only place with `aes-gcm`/`blake3` calls, and drop those deps from core if possible.
- **identity (581 lines)**: used by core (lib.rs:`identity::recover`, tests) and proto presumably. Self-contained, I/O free. Keep. Its `lib.rs` `verify`/`encrypt_for` are trivial pass-throughs to cryptography (:36-47), fine as the public facade.
- **storage (525 lines)**: header says "Copied from osvauld2 ... keep in sync by hand" (storage/src/lib.rs:4-6). Used only by core (store.rs, rekey.rs, chat/group/search stores). store.rs in core (896 lines) is a different layer (profile JSON + State + CallRecord + `Backing` enum), so no direct duplication, but the `Backing::Json` path duplicates "sealed vs plain" persistence that storage::Sealed already models. Keep as separate crate; consider whether the "keep in sync by hand" claim is still valuable (diverged: apply, prefix scans). Add `From<StorageError> for Error` in core (see #1).
- **peer (933 lines, single main.rs)**: binary `p2p-peer` depending on `p2pcore` only; headless test peer ("far end for end-to-end tests against the Android app"). Not depended on by anything. Keep, but 933-line main.rs should be split by subcommand; it also duplicates `NodeEvents` impl patterns and reimplements chat serve loops that a core `Session` (section 0) would give for free.
- Core also builds `[[bin]]` year-bench (637 lines) and search-bench: bench tools inside the production crate; year-bench defines its own `now_ms` (:53). Move to `crates/core-bench` or `examples/`.
- build.rs (8 lines): only `-Bsymbolic` for rav1d on android/linux; fine.

## 5. Dead code / TODOs
- grep for TODO/FIXME/todo!/unimplemented in core/src/*.rs, bin, video_call: none found. Only two `allow(dead_code)`: video_call.rs:276 and :473 (both `cfg_attr(not(target_os="android"), allow(dead_code))`: android-only items, consistent with `video_jni` being android-gated; fine but they mean desktop never uses `frame_port`/`push_yuv_planes`, which are exactly the JNI zero-copy path).
- Stale comments: GroupsSource.kt:8 "None of this exists in the core yet" (core/src/group/api.rs exists). core/src/lib.rs exports `search_bench*` hooks used only by a bench binary.
- `synth.rs` plus 11 `_for_test` fns are only for year-scale measurements; check whether `core/tests/*` and year-bench still use all of them.
- Not verified: unused `pub` items across the crate (no tool run; read-only constraint). Suggest `cargo +nightly udeps`/`cargo-machete` and `-W unreachable_pub` by the lead later.

## 6. File map (core/src top level)
- lib.rs: module list, re-exports, `core_version`, `did_of_phrase`, `video_bench` UniFFI exports.
- node.rs: the `Node` UniFFI object: accounts, identity/passphrase, contacts, tickets, 1:1 call state machine, accept loop, device lists, persistence orchestration (3.8k lines).
- store.rs: on-disk models (`State`, `StoredContact`, `CallRecord`, `Profile*`), JSON/sealed `Backing`, `History`, account.redb save/load.
- accounts.rs: per-account directory layout and `current` pointer (`AccountDirs`).
- account_tests.rs: stale-session-write regression test.
- vault.rs: envelope encryption (DEK sealed with Argon2id-derived KEK) for mnemonic + device secret.
- rekey.rs: new store keys derived from recovery phrase and the one-time rekey from the old DEK-derived keys.
- block.rs: visible consent-based blocking (signed notices, state, UniFFI types).
- caps.rs: message/call tokens ("caps"), contact origins, call-allow flow.
- devlist.rs: pushing DID-signed device lists to contacts (outbox).
- version.rs: ALPN/protocol version constants and mismatch refusal.
- wire.rs: `Ctrl` framed control stream over iroh bi-streams (`proto::Msg`).
- voice.rs: voice messages: Ogg/Opus recorder + decoder (`VoiceRecorder`, `VoiceDecoder`) and Ogg framing validation.
- video_call.rs: video pipeline for calls (encode/decode threads, frame ports, stats, UniFFI video API).
- video_call/traffic.rs: video traffic accounting helper (41 lines).
- video_jni.rs: Android-only JNI zero-copy `pushYuv`/`pullRgba`.
- synth.rs: test/bench hooks writing synthetic year-scale data.
- logging.rs: tracing init (logcat on Android, stderr elsewhere).
- error.rs: `Error` enum (UniFFI error) and From impls.
- bin/uniffi-bindgen.rs: bindgen entry; bin/search-bench.rs, bin/year-bench.rs: benchmarks.
- build.rs: `-Bsymbolic` link arg.
- crates: storage = redb KV + sealed records; identity = BIP39 -> keys/DID/keystore; cryptography = AEAD/KDF/ECIES/signature primitives; peer = headless test peer CLI.
