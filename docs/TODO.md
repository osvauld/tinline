# TODO

Open work after the 2026-10-16 refactor and rotation landing (dev 990c0d6). Detail on rotation is in
[rotation.md](rotation.md); the refactor plan is in [refactor/PLAN.md](refactor/PLAN.md).

## Verify what just landed
- [ ] Compile the Android app against dev. `Contact` gained `lost_touch`, and A3 changed five Kotlin
      files (`MainActivity.kt`, `P2pApp.kt`, `UnlockStore.kt`, `Unlock.kt`, `GroupsLive.kt`).
- [ ] Run the core integration tests not run on the rotation changes. Group rotation skipped
      group_call (all but the rotation test), group_g3, group_shards, video_calls, link_expiry,
      message_receipts, old_writer_delivery, profile_sync, readd, search, vault, writer_id and
      golden_core. 1:1 rotation skipped accounts, call_tokens_acceptance, chat_shards,
      endpoint_close, gcall, golden_core, grant_characterization, group_core, group_g2, group_g3,
      group_shards, lifecycle, message_receipts, old_writer_delivery, profile_sync, readd,
      restore_characterization, search, vault, video_calls and link_expiry. Skip
      `capacity_twelve_*` (fails on the old base too).

## Rotation in the apps (Android + desktop)
- [ ] "Lost touch: swap codes again" on the contact list and chat header, from `Contact.lost_touch`
      (refresh on `on_contacts_changed`), with QR share and scan.
- [ ] Settings: interval choice at or above `MIN_INTERVAL_MS` (1 day) through
      `group_rotation_policy` / `set_group_rotation_policy`. One setting drives both group and private
      endpoints. Show the next change from `private_endpoint_rotation_due()` and offer "change it now"
      (`private_rotate_now()`).
- [ ] QR screen re-renders after a rotation (`my_ticket()` returns the new address).
- [ ] Optional: neutral names for the policy API (it is still called `group_*`).

## Receipts (group receipts landed 2026-10-16, dev 12a5fec)
Detail in [design/receipt-runtime-roadmap.md](design/receipt-runtime-roadmap.md).
- [ ] Phone check on dev: group open no longer stalls (~0.75 s `Choreographer: Skipped` before the
      fix) and Privacy > Read receipts no longer changes height. If a stall remains, take a Perfetto
      trace of one group open.
- [ ] Re-run the Android unit tests and the full core integration tests on dev (only receipts/chat/
      group-receipts/tick/rotation ran after the last dev merge).
- [ ] Real two-device network test of `tinline/group/4` receipt frames (so far in-process only, plus
      three headless peers).
- [ ] Receipt size grows with group size (299 B at 3 members, 781 B at 50 per statement): find out
      why (likely the signed meta frontier) before choosing batching. Then batching, then shards (M7).
- [ ] Cache the group receipt status per (group, day) if group opens still feel slow (it verifies
      every signature per call).
- [ ] Contact device names: other people's devices show a short id ("dbebb7e4").
- [ ] Own-device receipt ledger sharing outside the group host relay.
- [ ] `p2p-peer chat-serve` group commands (gcreate/gjoin/gpost/greceipts…) sit uncommitted in the
      receipt-runtime worktree: commit as test tooling or drop.
- [ ] Decisions still open: per-device removal in groups; a proper group message details view (today a
      desktop tooltip and Android tap line); the not-delivered mark looks like a small smile at 12 px.
- [ ] Pre-existing: the chat jumps when the "X is offline" banner disappears.

## Whitepaper
- [ ] Group and 1:1 rotation are now built: change their tags from "designed" to "shipped". Add the
      verified-flag rule (it survives a rotation through old-key succession links; a new install or
      restore still resets it).

## Video (hw VP8 + adaptation landed 2026-10-16, dev 15ecc43)
Design: [design/video-hw-vp8.md](design/video-hw-vp8.md), [design/video-adaptation.md](design/video-adaptation.md).
Tested on devices: Pixel 3a -> laptop HD with no flicker; phone-started video calls no longer crash.
- [x] **Decode-cost policy no longer throttles hardware decoders** (2026-10-16): wall-clock decode latency of MediaCodec (25 ms at 720p, ~3% CPU) pushed a Pixel receiver to 15 fps. With `recv_hardware` the policy judges throughput (decoded vs received over 2 s) plus render cost; software unchanged. docs/video.md.
- [ ] **HD keeps the 4:3 shape.** HD asks the camera for 16:9 (1280x720), normal mode for 4:3
      (640x480), so a portrait picture turns narrower when HD is switched on. Ask for 4:3 in HD
      (1280x960); the fit-not-crop path keeps it. Decision taken: 4:3 (lead's recommendation; confirm).
- [ ] **Decide: does HD ignore the window size?** The desktop asks the phone for its drawn size, so
      a portrait picture in a normal window gets 540x960 even on a perfect LAN. Options: keep (sends
      what is shown), or with HD on, cap one rung higher / not at all.
- [x] **Desktop encodes VP8 (libvpx, software)** when the peer decodes VP8 in hardware (2026-10-16,
      branch feature/desktop-vp8-encode): `vpx::Vp8Encoder`, offer `hw_decode`/`hw_encode`, scoring
      `send_codec`. Pixel 3a <-> laptop on a device is still to be checked (design test 4).
- [x] **Show the codec and hardware acceleration** in the call (2026-10-16): "About this call" in
      both apps lists "Your video  VP8 · hardware" / "Their video  AV1 · software" while that
      camera is on (rows reserved on a video call, blank when off). Text from core `video_codec_label`.
      Not device-tested.
- [ ] **Video stats in the signal-strength dialog** (both apps): send/receive size, fps, bitrate,
      codec + hardware, path (direct/relay), queueing delay, the cap and its reason.
- [ ] **Old HD-chip wording blames the connection.** The desktop "sharper video paused ... their
      connection is weak" line shows for any reason (their screen, their decoder, CPU). Use the
      core label's reason (`video_send_label`) or drop the line.
- [ ] **Climb back after a real hide is slow:** after "not visible" (180p15) the sender steps up
      one step at a time (5 s holds). Restore the step held before the hint, if the estimate allows.
- [x] **Label can stay on "slow network"** after recovery (2026-10-16, fix/wifi-spike-estimator):
      "slow network" only while the send rate is below the next size's step-up threshold.
- [x] **Wi-Fi delay spikes collapse the rate to 180p** (2026-10-16, fix/wifi-spike-estimator): a
      stall followed by a burst is taken back at the second look and the rest of the spike held;
      sim `wifi_delay_spikes*` (docs/video.md, Wi-Fi delay spikes). Device check open: a 2-minute
      Pixel <-> laptop call on the 2.4 GHz router, expect no step below 540p on a spike and
      `rate` steps only with real slowdowns (logs don't show `RateTrace.spikes`/`held` yet).
- [ ] **A spike on a full link still reads as a slower link** (sim `wifi 1.6M`: 180p for 10 s): no
      burst above the old rate shows. Needs per-packet arrival timing (transport-wide feedback).
- [ ] **Starvation at 15 fps** (laptop log: 2 Mbps -> 350 kbps at 720p15, 1-2 frames per 100 ms
      window): not modelled in the sim; the spike rule may cover it, unverified.
- [ ] **Laptop `reason=cpu` step at ~32% CPU** (real call 15:44:04 UTC, 540p15 -> 360p24): look
      at the desktop encode-time samples (libvpx VP8) before tuning anything.
- [ ] Log `RateTrace.spikes` / `held` in the per-second `video send` line (pipes.rs).
- [ ] **720p15 drifts to 540p30 near link capacity** (15 fps makes the delay signal noisy); 180p has
      no fps floor; the real-AV1 loss scenario now holds 360p with p95 130 ms (was 91).
- [ ] **Other callbacks may re-enter like `on_video_state` did** (the ART "detach while still
      running code" abort): check NodeEvents calls fired from inside app calls.
- [ ] **Desktop AV1 hardware decode (VA-API)**, later: Iris Xe/Tiger Lake likely decodes AV1 on
      the GPU (check `vainfo`). Saves CPU/battery; rav1d stays the fallback; runtime probe (drivers
      differ per machine); same size caps as libvpx (the driver parses peer bytes). Decided
      2026-10-16: after the easy wins (desktop VP8 encode, hardware-aware offer, indicator).
- [ ] **Desktop hardware encode (VA-API VP9 or H.264)**, later: Tiger Lake has no AV1/VP8 encode,
      so it needs a new codec; quality varies by driver; a second encoder for the ladder. Needs a
      design note first.
- [ ] **H.264 for phones without hardware VP8** (moto g52: hw H.264/HEVC only, sends software AV1 at
      360p). Researched: hw MediaCodec on phones; desktop via Cisco's OpenH264 binary downloaded at
      run time (user toggle, attribution). Read openh264.org/BINARY_LICENSE.txt first. Deferred.
- [ ] Release packaging (deb, tarball, AppImage, CI) must build libvpx (`build_libvpx.py`, `VPX_DIR`,
      `VPX_STATIC_REQUIRED=1`); sandbox the desktop libvpx; AV1 fallback if the VP8 encoder fails.
- [ ] Device checks still open: moto on the new build; Android thermal/power-save hints; desktop
      window resize/minimise hints; the 2-minute Pixel HD call with the new ladder (bounce gone?).

## CI failures parked for the release (2026-10-21)
- [ ] `group_call::the_next_call_follows_a_rotated_call_host_and_a_revoked_call_host_hands_over`
      is `#[ignore]`d. It failed in both GitHub runs (dev 35cc6e9a and ddbf879c), never on our
      machines. Right after the chat host and then the call host rotate, a member starts a call
      with 2 candidates; neither answers its call-host request, and after about 12 s the start gives
      up with "Nobody who can host answered" and does not try again. Meanwhile a member's link to the
      chat host logs "host moved" 4 times in 14 s and only comes back about 20 s later. Likely a
      product weakness: the request is lost while the route re-forms, and the picker does not resend.
      Code: `crates/core/src/group/calls/start.rs` (`group_call_step`, `calls_originate`). Next:
      reproduce with a slow network after a rotation, fix (resend when the route returns), un-ignore.

## Decisions still open
- [ ] Delete the storage-attestation runtime? Nothing issues one. Recommendation: delete.
- [ ] Remote unlink: lock the account instead of deleting its directory? Recommendation: lock.

## Next work
- [ ] Packet C: chats onto the shared Loro ledger (last of the refactor).
- [ ] Packet B remainder: gate the remaining `*_for_test` hooks (about 39 files) behind `dev-tools`.
- [ ] Hardening H1–H6: IP exposure before app auth test, lock
      poison helper, `capacity_twelve_talking_at_once` failure (fails even alone).
- [ ] Known failure: clippy TAU error at `crates/core/src/cues.rs:89`.
- [ ] Flaky under load: `calls::hangup_while_dialing_is_immediate`,
      `blocking_acceptance::blocked_senders_messages_never_reach_the_blocker`.
- [ ] Rotation follow-ups: a dial in flight on a retired group endpoint can wait ~20 s; "lost touch"
      is one-sided and has no recovery through a shared group host yet.
- [ ] After the core: the Kotlin audit, then the iced desktop audit.
