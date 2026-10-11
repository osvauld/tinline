# Handoff — start here (2026-10-11)

Everything from the overnight launch push is merged into `dev` (nothing pushed). Three sessions
were retired: rotation/groups coordinator, group-audio, video. A fresh session starts from `dev`.

## What is in `dev` now

| Area | What works | Docs |
|---|---|---|
| 1:1 chat | DID-signed messages (writer ID = hash(DID ‖ device ID), stable per install); history accepted per message; edits ranked (deleted, edited_at, writer ID); our undelivered messages survive restore / lost store / failed write; wire `tinline/chat/3`, `tinline/self/3` | `docs/chat.md`, `docs/protocol.md` |
| Blocking | Consent-based, visible: one signed notice, both apps enforce, deletion request removes the contact entry only, chats never deleted, re-add after unblock (U5/U19/U20/U24). Apps: Block replaces "Remove contact" | `crates/core/src/block.rs` |
| Call permissions | Separate message / call tokens; "Allow calls"; "calls not allowed" reason; per-DID cut-off revocation; calls ALPN `osvauld/p2p/1` | `docs/protocol.md` "Tokens", `crates/core/src/caps.rs` |
| Device lists | Signed device list pushed to unblocked contacts on link / unlink / restore (seq bound now + 1 day) | `crates/core/src/devlist.rs` |
| Group chat | Second (group) endpoint per install, hosts (multi-host), invite by short link `tinline://g/…` (75 chars, host-held code) or directly to contacts, multi-device + own-device group sync, 3 admin flags remove, chat requests via host (sealed), host endpoint rotation 2 days after removal, store lifecycle on stop/lock/switch | `docs/group-core.md` |
| Group calls (core + audio) | One call host per call, call hosting permit separate from chat hosting, app audio threads + host conditions (battery/thermal/metered); call shutdown on stop/lock/account switch. **No call UI yet** | `docs/design/group-call-spec.md` §10–§11, `docs/design/group-call-bench.md` |
| Video calls (1:1) | AV1 (libaom-RT encode via `scripts/build_libaom.py`, rav1d decode), Android + desktop (Linux V4L2) UI. User tested phone ↔ laptop: works | `docs/video.md` |
| Apps | Groups tab (Android bottom bar, desktop nav), group chat/info/invites/requests/host prompts, blocking, allow calls, video | design boards `docs/design/groups/` (Claude Design canvas https://claude.ai/artifact/KS9HUDfQzRFX9tpayTt76a) |

Decisions U1–U24: `docs/design/rotation-open-decisions.md` §0. Packet history and provisional
choices: `docs/design/production-phase1-plan.md`.

## Next work (priority order)

1. **Device test of the combined build** (phone ZD22257P6G + desktop). Needs an app-data wipe
   (formats changed; U18 allows it) — ask the user first. Install: see memory "Local install restart".
2. **Group call UI** (Android + desktop) on top of the group screens. API, events and MUST-NOTs:
   `docs/design/group-call-spec.md` §11 and the notes below.
3. **User decisions still open:**
   - Should a flagged member see flags before removal (lean: no; removal shows signed flags)?
   - A chat request from someone you blocked: requester sees "X blocked you" (current) or "declined"?
   - One group endpoint per install (a removed member in another group hosted by the same device
     learns the rotated endpoint) — accept for launch (lean) or per-group endpoints?
   - PROVISIONAL choices listed in `docs/group-core.md` (items 1–16) and the phase-2 notes in
     `production-phase1-plan.md` (delete beats edit; "calls not allowed" shown to any contact;
     message-token renewal 30 days; creator's device first host; owner can't be flagged; removal
     final; 7-day bearer invites; links name one host).
4. **Endpoint rotation in the app** (scheduler, contact/group announcements; models in
   `crates/group-sim`, `crates/proto/tests/rotation_*`; acceptance tests ignored in
   `crates/core/tests/rotation_acceptance.rs`).
5. Smaller gaps: media deletion (P12, U24); invite link fallback to other hosts; group call
   `CallHostStatus.load` always 0, hosting pause not persisted, `metered_allowed` setting; calls
   whose host vanished stay listed 4 h; "blocked person in this call" warning (no data);
   desktop camera on Windows/macOS; phone video stays 360p; Opus licence notice; release APK not
   yet tested with a video call; verified flag after a pushed device list (§12 Q5).

### Group call UI notes (from the group-audio lead)
- Functions (UniFFI on Node): `group_call_start(group_id) -> call_id` (failure → `StartFailed`),
  `group_call_join(group_id, call_id)`, `group_call_leave()`, `group_call_set_muted(bool)`,
  `group_calls(group_id)`, `group_call_here()` (source of truth; re-read on every event),
  `group_appoint_call_host` / `group_revoke_call_host` / `group_accept_call_host`,
  `GroupDetail.{am_call_host, call_hosts, call_host_requests, call_host_requested_here}`,
  `group_call_hosting_enable`, `group_call_set_conditions` (already fed by HostConditions.kt /
  desktop hostcond.rs; UI only adds a `metered_allowed` setting).
- Events `GroupCallEvent{kind, group_id, call_id, participants, detail}`: Announced, Ended,
  StartFailed, Joined, RosterChanged, Reconnecting (don't end the screen), Left, HostingStarted,
  HostingEnded, HostingLimited/Unlimited, MeteredData.
- MUST NOT: call `set_group_call_events` from screens (one sink: Android `GroupCallAudio`, desktop
  `GroupCallBridge` → use `app.groupCalls.events/here` and `Ev::GroupCall` in desktop app.rs);
  start/stop audio engines from UI; trust event order. Android: check RECORD_AUDIO and call
  `app.groupCalls.prepare()` in the foreground before start/join (`unprepare()` on failure).
  Show "Hosted by <name>'s <device> — the host can hear this call"; no ringing (non-ringing
  "call started" notice); 1:1 and group calls exclude each other (surface InCall/Busy). The
  ongoing-notification tap must open the group call screen during a group call.

## How to work here

- Build queue: `python3 /home/abe/p2p_chat/scripts/buildlock.py --who <name> -- <cmd>` (absolute
  path; `cargo test` auto-splits compile/run; `--run` for prebuilt binaries/benches; `--device
  <serial>` for phone/emulator; never `touch` sources; `--status`).
- `/home/abe/build-cache` fills up: clear build output only of merged, idle worktrees.
- Test-first with real nodes (`crates/core/tests`); full `cargo test -p proto -p p2pcore
  --no-fail-fast`, plus `-p video --features net`, `-p desktop`, Android
  `scripts/build_android.py --abis x86_64,arm64-v8a`.
- Video tests are timing/throughput sensitive: run `-p video --features net` and p2pcore
  `--test video_calls` in **release** (`--release`); in debug `video_both_ways` sends too few
  frames and the video lib tests abort.
- Video needs libaom: dev hosts use the system libaom; release/Android use
  `scripts/build_libaom.py` (needs nasm, cmake).
- Never push; merging into `dev` / `main` and installing on the user's devices need the user's OK.
