# Handoff: video codec negotiation and call setup, explored again

Status: **exploration, nothing decided**. Written 2026-10-17 at the end of the hw VP8 / rate
control session. It is for a fresh session: read this, then the docs it links, before
changing anything.

Worktree: `.claude/worktrees/codec-negotiation`, branch `explore/codec-negotiation`, from
`dev` 5af6b37 (0.1.33). This file is not committed; commit it when the user OKs it.

## The question

The user's observation: on one call, **AV1 in software on both sides gave a better picture than
a call that used the Pixel's VP8 hardware encoder**. Negotiation today prefers hardware, on the
assumption that hardware means better. That may be wrong for picture quality. The user wants
negotiation and call setup re-examined, so each direction picks the codec that gives the best
picture the two devices and the link can sustain.

## What we know (with how sure we are)

### Devices
| device | VP8 encode | VP8 decode | AV1 | notes |
|---|---|---|---|---|
| Pixel 3a `9BWAY1LSDC` (Snapdragon 670) | **hw** `c2.qti.vp8.encoder` | **hw** | sw (libaom / rav1d) | sw AV1 tops out ~360p30 encode; decodes the laptop's AV1 at a 15 fps cap |
| moto g52 `ZD22257P6G` (Snapdragon 680), **the user's own phone: ask before every use** | none (no hw) | sw (`c2.android.vp8.decoder`) | sw | sends AV1 sw ~360p (memory `test-phones-codecs.md`) |
| laptop (desktop app) | sw libvpx | sw libvpx | sw | encode 5-6 ms/frame at 720p VP8 |
| Malta friend's phone | ? | ? | sw both ways (their screen) | model and Tinline version unknown |

### Calls observed 2026-10-17
1. **Pixel <-> laptop, LAN, 0.1.33 (good).** VP8 both ways, 720p30 at 3 Mbps in 2-8 s.
   One ~10 s patch of keyframe-fed stutter Pixel->laptop. Logs:
   `/tmp/claude-1000/-home-abe-p2p-chat/9abbd06c-f531-48e3-9ae2-be3466a64206/scratchpad/call6/`
   (the `/tmp` scratchpad may be gone after a reboot).
2. **Pixel <-> moto, 17:43 and 17:44 (Pixel dialled), 0.1.33.** `video codecs: send vp8 recv av1`.
   The Pixel received AV1 at 180x240, then 360x480 at ~15 fps, decoding in 3-16 ms. Its own
   `video send` stayed `0x0@0 0kbps` for the **whole** of both calls. Android logged that call
   audio was on the **earpiece**, that the proximity sensor was enabled, then `Screen: 0`, and the
   camera closing at 17:43:10. Later the Pixel sent `video hint max=0` (17:43:23, 17:44:42), so
   the moto stopped sending.
   **This conflicts with the user's account** ("the screen was not off, we could see each
   other"). Don't build on it until it's reproduced. Pixel log:
   `.../scratchpad/call7/pixel-logcat.log`. The app's lines only start at 17:42, so earlier
   calls are gone.
3. **Malta friend calls (earlier, no logs):** good with the moto, bad clarity with the Pixel.
   The Pixel showed "your video VP8 · hardware, their video AV1 · software".

**Not known:** whether "software AV1 looks better" holds at the same bitrate and resolution on
the same link. Calls 2 and 3 differ in network, time and who called, and have no logs from the
far end.

## How negotiation works today (verified in code)
- Offer, per device: `crates/core/src/video_call/codecs.rs` `local_offer_codecs()`.
  - `decode` is always `[Av1, (Vp8 if any VP8 decoder)]`.
  - `encode` is `[Vp8, Av1]` on a phone with a hw VP8 encoder, `[Av1, Vp8]` on the desktop
    (libvpx), else `[Av1]`.
  - `hw_decode` / `hw_encode` come from the MediaCodec probes (`mediacodec::hardware_vp8_*`).
- Choice per direction: `crates/proto/src/msg.rs:167` `send_codec(sender, receiver)`. Over
  the sender's `encode` that the receiver decodes, the score is (sender hw-encodes it) +
  (receiver hw-decodes it). The highest score wins, ties go to the sender's order, and no
  candidate means AV1. Both ends compute both directions; there's no extra message.
- Rationale and table: `docs/design/video-hw-vp8.md` ("Hardware-aware offer"). It assumes
  hardware VP8 is "sharp at 1 Mbps" (lab `HwVp8BenchActivity`). **No quality-per-bit
  comparison against AV1 was ever measured.**
- After the choice, quality is set by the sender's ladder (`crates/video/src/media.rs`:
  rungs 180/360/540/720p, cap 3 Mbps since e17d29c), its encode-time check (CPU), the
  receiver's hint (`Policy::wanted()` in `crates/core/src/video_call/adapt.rs`: drawn size
  one rung up, decode-fps budget, thermal), and `VideoOffer::max_pixels/max_fps`.
- So "codec" and "how high the ladder can climb" are tangled. AV1 sw on a Pixel can't climb
  above ~360p, and VP8 hw can reach 720p. Any comparison has to hold resolution and
  bitrate fixed.

### What the score ignores
1. Quality per bit. AV1 is roughly 30-50% more efficient than VP8, and hw VP8 encoders often
   do worse than libvpx at low bitrates.
2. How much CPU headroom each side has for software coding (a fast phone vs a Pixel 3a).
3. The available bitrate: at 0.5-1 Mbps AV1's efficiency matters most, at 3 Mbps less.
4. Heat and battery.
5. The receiver's real decode cost for each codec (e.g. a hw AV1 decoder on newer phones;
   Android hw AV1 decode isn't offered yet).

## Also worth checking during the exploration (call setup)
- Calls register with telecom as `CALL_TYPE_AUDIO_CALL` in
  `android/app/src/main/kotlin/com/osvauld/p2p/CallSession.kt:85` and `answer(...)` at `:148`.
  So a video call starts on the earpiece, which turns the proximity sensor on.
  `speakerIfEarpiece()` (same file, ~:181) is meant to move video calls to the speaker.
  Check that it runs for outgoing and incoming video calls; observation 2 suggests not.
- When the screen stops, `RemoteVideo` sends `setView(..., false)` (`CallScreens.kt:592-594`),
  which makes the peer stop sending. Check that this can't fire while the picture is still on
  screen.
- The desktop camera takes 10-15 s to deliver its first frame (call 1).

## Suggested plan
1. **Measure, don't guess.** In the sim or a bench, encode the same clips (talking head, motion)
   with Pixel hw VP8, libvpx and libaom-RT at 0.5 / 1 / 2 / 3 Mbps and 360p/540p/720p. Compare
   the quality (PSNR/SSIM, VMAF if available offline) and the encode time on each device. The
   lab code for the Pixel encoder is on branch `hwvp8-lab`.
2. **Tuning controls (user request 2026-10-17).** The user wants to *see* the codec and *switch*
   between codecs by hand while tuning. Build a developer-only "Video tuning" panel in both apps.
   It must be hidden in friends' builds: either debug builds only, or a hidden developer toggle
   that the user decides on.
   - **Show, live in the call:** the send and receive codec, hw/sw, resolution, fps, bitrate,
     encode/decode ms and the ladder rung. Most of this already exists in the `video send` stats
     line and the codec labels; surface it on screen.
   - **Force the send codec:** Auto / VP8 / AV1. Phase 1 applies it at the next call: the
     offer is filtered locally and the peer still validates it. Phase 2 is an optional mid-call
     switch. That needs a protocol message (new encoder, keyframe, the receiver swaps decoders)
     and docs in `docs/protocol.md` / `docs/video.md`.
   - **Also pin, for fair A/B:** a max bitrate (1 / 2 / 3 Mbps) and a fixed rung (360/540/720p),
     so codec comparisons run at the same rate and size.
   - The rule logic stays in the Rust core (shared); the apps only show it and forward choices.
3. **Recorded A/B call:** Pixel <-> moto in the same room. Run VP8 hw then AV1 sw, 2 min each,
   with logs from both phones (moto only with the user's OK that day) and screenshots or a
   screen recording of each side.
4. **A rule proposal** in place of the hw score, for example: pick the codec that reaches
   the highest predicted rung at the link's rate within both devices' CPU/decode budgets,
   with AV1 preferred when both can sustain the same rung. Include a table of outcomes for
   Pixel/moto/laptop/AV1-only peers and acceptance tests in `crates/proto`. Note that
   negotiation happens once at call start, so consider whether a mid-call switch is worth it
   (keyframe cost).
5. Then decide with the user: hw AV1 decode on Android, and H.264 for phones without hw VP8.

## Goal: pick the codec automatically (user, 2026-10-17)
**The only objective is picture quality** (sharpness and smoothness as seen by the person
watching). Battery, heat and data use are **not** goals. They enter only as limits on what is
feasible:
- a codec/size a device can't encode or decode at full frame rate stutters, so it's out;
- heat matters only when the phone throttles and the fps drops.

Rule: the codec x resolution with the best predicted picture at the link's current rate, among
those both devices sustain at full fps. Re-evaluate during the call.

There is no single value to tune. Which codec looks best depends on the two devices, the rate
the link allows, and heat/CPU. The user wants it chosen automatically and adjusted live. Phases:
1. **A quality table** from the tuning panel and bench (step 1-3 above): quality and encode/decode
   cost per codec x bitrate x resolution x device.
2. **A better start choice.** A short one-time device benchmark (per app version) records the
   highest rung each codec can encode and decode at 30 fps. It goes in `VideoOffer` (extends
   the hw flags). `send_codec` picks the codec with the best predicted quality at the start
   bitrate within both devices' limits. Ties go to AV1 (quality per bit). Deterministic tests
   in `crates/proto`.
3. **A live switch during the call**, driven by the per-second stats the sender already has
   (estimate, encode ms, peer decode ms/fps, thermal). Switch when another codec is clearly
   better (e.g. 3 Mbps -> 0.8 Mbps: VP8 720p -> AV1 360p; overheating: AV1 sw -> VP8 hw).
   - Hysteresis: at most one switch per ~30 s, and only when the predicted gain clears a margin.
   - It costs one keyframe.
   - Needs a protocol message ("codec X from frame N"), with both ends swapping the
     encoder/decoder together.
   - **A live quality measure (idea, 2026-10-17):** the sender keeps the original frame, so
     about once a second it decodes its own encoded frame and computes SSIM (or PSNR)
     against the original, downscaled to save CPU. That gives a codec-neutral sharpness score.
     The receiver supplies smoothness (shown fps, freezes, decode ms). Picture score =
     sharpness x smoothness.
   - **Trial switching:** start with the predicted best. When the link is steady, try the
     other codec for ~5 s and keep the better score. Retry after a big change (rate, thermal),
     otherwise at most every few minutes.
   - QP and bits per pixel are weaker proxies (QP isn't comparable across codecs).
   - **Manual option (user wants it):** an Auto / VP8 / AV1 control in the call that switches
     at once (same switch message), showing the codecs, the stats and the live score. Open: for
     everyone (in the call's info sheet) or developer-only.
   - Sim-test it like the ladder (no flapping, bounded freeze per switch).

## Rules for the session
- Follow `AGENTS.md`: heavy work through `tasks` (offload to the LAN worker
  `abe@192.168.1.7`; commands in memory `lan-worker-offload.md`); one offload per worktree,
  direct commands only.
- No installs, merges, pushes or app-data wipes without the user's OK. The moto is the user's
  phone: ask every time.
- Sonnet subagents for well-scoped tasks, Opus for the hard ones; a fresh agent per batch.
- Replies to the user: next action first, short numbered steps (memory `output-format-adhd.md`).

## Repo state at handoff
- `dev` = 5af6b37 (0.1.33, versionCode 581): hw VP8 desktop, stall-aware rate control,
  fast start, hint one rung up, 3 Mbps cap. Not pushed.
- Builds: `~/tinline-builds/tinline-0.1.33-5af6b37-android.apk` (release, debug-signed) and
  `~/tinline-builds/tinline-desktop-0.1.33-5af6b37` (portable).
- In flight in the previous session: agent `hd-suggest` on `feature/hd-suggest` (subtle "your
  line can carry sharper video" / "X would like sharper video" hints, for 0.1.34). Check its
  worktree `.claude/worktrees/hd-suggest` before touching `adapt.rs`, hints or the call UI.
- Deferred follow-ups: the headroom rule (bursty Wi-Fi p95 rose 205 -> 333 ms with the 3 Mbps
  cap), the keyframe storm after stalls, the desktop camera start delay, and the
  `package_linux_tarball.sh` still building with libaom only (no VP8/libvpx).
