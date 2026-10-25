# Push wake-up (Android + iOS)

**Status: proposed 2026-10-24, not built.** Nothing here is implemented. Open decisions are in
"Open" at the end; nothing in them is decided by this doc.

Today a phone receives messages and calls only while the app keeps a background connection open
(Android: the always-on foreground service, `docs/play/console.md`; iOS: nothing, a suspended app
misses calls, `ios/Tinline/Screens/Shell/ShellCore.swift`). This doc adds a **wake-up push**: a
sender that cannot reach a sleeping device asks a small relay to wake it, the device starts its
node and the normal p2p protocol does the rest.

## Decisions (user, 2026-10-24)

- **The push carries no content.** Only "wake up", the kind (`msg` or `call`) and a sealed
  contact hint (below). Message text, names and call details never go through the relay,
  Google or Apple. Anything shown to the user that names a contact is looked up locally.
- **Two Android editions.** **Tinline** (Play Store) ships the Firebase messaging SDK and
  wakes by FCM. **Tinline Liberum** ships no Google SDK, keeps the always-on background service
  (as today) and is released on GitHub Releases and the Osvauld site. See "Editions".
- **Push only, no mailbox.** A message is still delivered only while both devices are online at
  the same time (`docs/chat.md` "Delivery and offline"). The push makes that window happen; it
  does not store anything.

## Why a relay is needed

Apple (APNs) and Google (FCM) accept a push only from whoever holds the app's credentials (APNs
`.p8` key, FCM service account). A peer cannot push another phone directly, and the credentials
cannot ship inside an open-source app. So Osvauld runs a **wake relay**: a small stateless
service that holds those two credentials and turns a wake token into an empty push.

What the relay learns: which push token is woken, when, how often, and the sender's IP. It
does not learn DIDs, contact names or content. Google/Apple learn that a device got an empty
push at a time. This fits "no anonymity promise" (`AGENTS.md`); the privacy policy must say it.

## Transports

| Platform | Build | Wake transport | Fallback |
|---|---|---|---|
| Android | Play | FCM high-priority data message (`firebase-messaging` only, no analytics, auto-init off) | Always-on service as a setting |
| Android | Tinline Liberum (GitHub + site) | UnifiedPush when the user has a distributor app (ntfy, NextPush, Sunup, …) | Always-on service (as today) without one |
| iOS | App Store / TestFlight | APNs: PushKit VoIP push for calls, alert push for messages | none |
| Desktop | — | none (runs while open, as today) | — |

- FCM cannot deliver PushKit VoIP pushes, so iOS talks to APNs directly; no Firebase on iOS.
- Liberum has no Google code, so it could also go to F-Droid later (which refuses the closed
  Play Services messaging library); the stale `release/fdroid-prep` branch has metadata for that.
- If Liberum gains UnifiedPush, it goes through the same relay too (the relay POSTs to the endpoint URL), so rate
  limits and revocation stay in one place and contacts never see the endpoint URL.

## Flow (A sends to B, B's device asleep)

```text
A (sender)                 Relay            Google/Apple          B (asleep)
 |-- dial B (iroh) ---X  fails
 |-- POST /v1/wake ----->|
 |                       |-- empty push --->|
 |                       |                  |-- wake-up --------->|
 |                                             B opens the hint → contact A
 |                                             B starts node, dials A
 |<================= iroh session ================================|
 |== normal sync (ChatHello / outbox) ============================|
 |                                             B posts local notification
 ... idle close after 60 s (wire::IDLE_CLOSE), B sleeps again
```

1. **Direct first.** A dials B as today. If B answers, no push is sent.
2. **Wake.** The dial fails (or a call is ringing with no session) and A holds a wake token for
   that device of B: A POSTs `{wake_token, kind}` to the relay.
3. **Push.** The relay opens the token, checks expiry, revocation and rate limits, and sends an
   empty push whose only data is the sealed hint.
4. **B comes online.** B's app opens the hint with its own key, finds contact A and starts the
   node (which registers with its iroh relay, so it is reachable again).
   - `msg`: B dials A (`chat_kick`). A also keeps retrying, faster for ~30 s after a wake
     (today's backoff reaches 60 s); whichever dial lands first wins. Direction does not matter:
     `ChatHello` reconciles both ways on any session.
   - `call`: **B does not dial.** The call keeps its normal direction: A redials B about every
     1 s while it is ringing, and the first dial that gets through sends the usual `CallHello`
     A→B. No protocol change. B rings locally as soon as the wake arrives (see "Calls" below).
5. **Sync and sleep.** Normal protocol; the link closes on the existing idle rule.

**Multi-device.** Every device of B registers and issues its own wake tokens; A wakes each
device it dialled and failed to reach (same as calls to several devices, `docs/protocol.md`).

**If A goes offline before B wakes**, B's dial-back finds nobody; the message waits until both
are online together. Only a mailbox would fix this (not in scope).

### Calls

```text
A (caller)                 Relay / push                     B (asleep)
 |-- CallHello dial ---X
 |-- wake {call} -------------------------------------------->| rings now (local name)
 |-- redial every ~1 s while ringing                          | node starts, reachable
 |-- CallHello ---------------------------------------------->| (normal call protocol)
 |<-------------------------------------------- Accept -------| user answers
```

- B rings before the call is connected, so answering may wait a moment for A's next redial;
  the UI shows "Connecting…" on answer until `CallHello` arrives.
- **A hangs up first:** its redials stop and no `CallHello` ever reaches B. B ends the ring when
  no `CallHello` for that contact arrives within `WAKE_RING_WINDOW_MS` = 15 s (PROVISIONAL),
  **measured from the wake's arrival** (the caller may still be redialling for ~36 s after
  pressing Call, and repeat call wakes arrive about every 10 s). No cancel push is needed.
- **Repeat wakes:** one ring per contact. A call wake for a contact that is already ringing moves
  the end out to 15 s from the new wake, capped at `WAKE_RING_CAP_MS` = 40 s from the first. It
  never starts a second ring.
- **The real call arrives:** the normal `CallHello` path runs (`on_incoming_call`) and the wake
  ring is consumed silently; the app matches the call to the ring by DID.
- **Answered early:** `answer_wake_ring(did)` before the `CallHello` arrives is remembered; the
  incoming call is accepted the moment it arrives, only if that is within the window (the UI shows
  "Connecting…"). Otherwise the ring ends and a late call rings as an ordinary one.
  `decline_wake_ring(did)` likewise declines the call unrung when it arrives.
- **For app authors:** a wake ring's `call_id` is always `"wake:" + did`. Its end is reported
  through the existing `on_call_state("wake:<did>", Ended { reason: "cancelled" })`; no other
  state is sent for it, and nothing at all when the user declined it themselves.
- **Caller states stay honest** (`CallState::Dialing` → `Ringing`, `crates/core/src/node/types.rs`):
  A shows "Calling…" (`Dialing`) while the wake is out and it redials, and switches to "Ringing"
  only when B's device answers `Ringing` to the `CallHello` (`docs/protocol.md` "Call"), exactly as
  today. B's phone may already be ringing a few seconds before A sees "Ringing"; A never claims
  ringing it has not been told about.
- The `Dialing` phase gets longer (wake + node start). How long A stays in `Dialing` before
  giving up is a device-tuned number (about 30 s, PROVISIONAL); the ring timeout after `Ringing`
  is unchanged.

## Wake tokens

Per receiving **device** and per **contact**, so one contact's token can be taken back alone.

```text
register (device → relay, once per platform-token change, batched):
  POST /v1/register { platform, push_token, voip_token?, hint_key[32], tags: [{tag[16], kinds, exp}] }
  → [{ tag, wake_token }]

wake_token = AEAD(relay_key[kid], { platform, push_token, voip_token?, hint_key, tag, kinds, exp })
```

- `tag` is random per (device, contact), chosen by the receiver and kept with the contact in
  its sealed state. `kinds` ⊆ {msg, call}: a contact without call permission gets `msg` only.
- `exp`: 30 days (PROVISIONAL), renewed like the message token: when a chat session finds it
  within 7 days of expiry the receiver issues a fresh one.
- The relay stores nothing per token. `kid` lets the relay rotate its key.
- **Hint in the push:** `AEAD(hint_key, fresh nonce, {tag, kind, sent_at})`. A fresh nonce each
  time, so Google/Apple cannot link two pushes from the same contact. `sent_at` lets the
  receiver drop a stale call wake (older than 30 s, PROVISIONAL).

**Handing tokens to contacts.** A new message on the calls ALPN, `WakeTokens { device, token,
exp }`, answered `WakeTokensTaken`, queued per contact with the same outbox and backoff as
`Tokens { grants }` (`docs/protocol.md` "Tokens"). Sent after a contact is added, after a
renewal, and to every contact after the push token changes (FCM `onNewToken`, a new APNs token,
a new UnifiedPush endpoint). The sender keeps at most one token per (contact DID, device).

**Taking a token back** (personal policy only; does not touch group or device policy):

- The receiver keeps a copy of every token it issued (sealed state, per contact), so it can
  hand one back.
- Block, or `allow_calls(did, false)` for the `call` kind: the device POSTs `/v1/revoke
  {wake_token}`. Queued in a local revoke outbox and retried like block notices (2, 5, 15, 30,
  then every 60 s while online, and at start) until the relay answers 204. The relay keeps the
  tag in its revoke list (KV) until the token would have expired; a later wake gets 410 and the
  sender drops the token. A learns nothing new: a block is already visible to the blocked person.
- **Gap before the revoke takes effect** (KV spreads in about a minute; longer if B blocked while
  offline). The device drops the wake itself: the hint names the tag, the tag maps to a blocked
  contact. Android: nothing shown, nothing dialled. iOS call: reported to CallKit and ended at
  once (Apple requires the report), a brief flash at most. iOS message: the generic "New message"
  banner still shows, since iOS displays it before the app runs (accepted 2026-10-24; Open 7).
- Unblock: the old token stays revoked. The re-add after an unblock carries a new token with a
  new tag, like the fresh grants of the existing re-add flow.
- Unlinking a device: its tokens die with its push token; the remaining devices are unaffected.
- Endpoint rotation (`docs/rotation.md`) does not change wake tokens: they are bound to the push
  token and the contact, not the iroh endpoint.

## Relay

`POST /v1/register`, `POST /v1/wake {wake_token, kind}` → 202, `POST /v1/revoke {wake_token}`.

- Rate limits per tag (PROVISIONAL): `msg` 2 per minute, `call` 3 per minute; `register` 20
  per minute per IP. A limited request gets 429 and the sender falls back to its normal retry.
  (Cloudflare's limiter counts over 10 or 60 s windows only, so no daily cap for now.)
  **Measured 2026-10-24:** the limiter is approximate and per Cloudflare location: a burst of 12
  `msg` wakes on one token got 8 through before 429s. So the relay limit is a coarse abuse cap;
  the exact limit is the sender's own: the core sends at most one `msg` wake per device per 30 s
  and only after a failed dial.
- **User-Agent:** `osvauld.com`'s Cloudflare bot protection answers `403 error code: 1010` to
  generic client agents (e.g. `Python-urllib`). The apps send `Tinline/<version>`.
- Answers: 202 sent; 410 drop the token (expired, revoked, push token gone, relay key retired);
  429 / 503 retry later; 403 kind not allowed; 400 malformed.
- No request logging of tokens or IPs; errors only.
- Secrets (`.p8`, FCM service account, relay keys) live only in the relay's secret store, never
  in git.

### Relay on Cloudflare (decided 2026-10-24)

- `crates/wake-relay-core`: all logic, no I/O, tested natively (`cargo test -p wake-relay-core
  --lib`). The app core will use its token and hint formats.
- `crates/wake-relay-worker`: the Worker (`workers-rs`, wasm32; outside the main workspace).
  Deploy guide in its `README.md`. Storage: KV `WAKE_KV` (revoke list, shared provider bearers);
  Cloudflare rate-limit bindings; Workers secrets.
- Signing uses the runtime's WebCrypto (ECDSA P-256 for APNs, RSASSA-PKCS1-v1_5 for the FCM
  service account), so no RSA crate is compiled to wasm.
- APNs needs HTTP/2. Deployed Workers reach it through Cloudflare's proxy, which upgrades to
  HTTP/2 (workerd issue #4841, not in Cloudflare's docs); `wrangler dev` cannot. The Worker's
  `GET /v1/probe/apns` checks it once deployed. **Confirmed 2026-10-24:** the deployed Worker got
  `403 InvalidProviderToken` from the APNs sandbox (fake key), so HTTP/2 to Apple works; probe
  turned off (`PROBE = "0"`). Fly.io Mumbai stays the fallback if that ever changes.
- Served only at `https://push.osvauld.com` (Cloudflare custom domain; workers.dev off).

## Platforms

### Android (Play)

- `FirebaseMessagingService.onMessageReceived`: open the hint (via the Rust core), then start
  the node. High-priority FCM lets the app start a foreground service from the background.
- `msg`: after sync, post the normal `MessagingStyle` notification (text came over p2p).
- `call`: existing full-screen + core-telecom ring path; the caller name is local.
- FCM deprioritises high-priority messages that lead to no visible notification. A wake whose
  sync finds nothing (A gone, already delivered) shows nothing; watch for deprioritisation.
- Battery: the always-on service becomes a setting ("Keep my line open"); default is push.
  `docs/play/console.md` foreground-service and battery-exemption texts say "no FCM" and must be
  rewritten before release.

### Android (Tinline Liberum)

No Google SDK. UnifiedPush (decided 2026-10-24): the UnifiedPush connector library (open
source, no Google code) finds the user's distributor app; when there is one, Liberum registers
its endpoint with the relay, wakes on `onMessage` like Play does on FCM, and can drop the
always-on service. With no distributor it keeps the always-on service as today (battery
exemption prompt in onboarding) and Settings says which mode is active. Never the UnifiedPush
"embedded FCM distributor": that would bring Google code back.

The distributor app itself holds one background connection; it saves battery when several apps
share it, and lets the user pick a self-hosted server.

## Editions

| | Tinline | Tinline Liberum |
|---|---|---|
| Where | Google Play | GitHub Releases + Osvauld site |
| Google SDK | `firebase-messaging` only (no analytics, auto-init off) | none |
| Waking | FCM wake-up push; always-on as an option | UnifiedPush with a distributor; always-on without |
| Relay needed | yes | only for UnifiedPush wakes |

- One codebase, two Gradle product flavours (`play`, `liberum`). The FCM service class, the
  Firebase dependency and `google-services.json` exist only in `play`; a build check fails if
  any `com.google.firebase` / `com.google.android.gms` class ends up in `liberum`.
- The Rust core is identical. Wake tokens are simply never registered by a Liberum install
  (its contacts reach it directly, as today).
- Release flow: the release workflow builds both APKs; the GitHub release carries Liberum
  (Play gets the `play` bundle).
- Application ID: `com.osvauld.tinline` (Play), `com.osvauld.tinline.liberum` (Liberum,
  signed with the Osvauld release key).

### iOS

- **Calls:** PushKit VoIP push. iOS requires the app to report a call to CallKit for **every**
  VoIP push, or it stops delivering them. So: open the hint, look up the contact name locally,
  report the call to CallKit at once, start the node and wait for A's `CallHello` (B does not
  dial). If the hint is stale or the contact is blocked, report and end the call immediately; if
  no `CallHello` arrives within the window, end it. Never send `call`
  wakes for anything but a real ringing call.
- **Messages:** alert push with a fixed generic text ("New message") plus `content-available`.
  iOS may give the app background time to sync (throttled, not guaranteed); otherwise it syncs
  when opened. Hiding the notification needs Apple's filtering entitlement, which we don't have.
- A Notification Service Extension that runs the node (to show real text) is a later option:
  about 24 MB memory and ~30 s; unknown whether iroh fits.
- Needs: Push Notifications capability, `UIBackgroundModes` `voip` + `remote-notification`, a
  real iPhone for testing (VoIP pushes do not arrive in the simulator).

## Rust-first split

Core (shared by all apps): wake-token store and outbox, `WakeTokens` messages, the
"should I wake this device" decision, the relay HTTP client, hint open/verify, revoke on block
and on call-permission change. Apps only: get the platform token, hand it to the core, receive
the push and call one core entry point (`on_wake(hint) -> WakeAction`).

## Costs

| Item | Cost |
|---|---|
| Apple Developer Program (organization, D-U-N-S) | $99 / year (needed for TestFlight anyway) |
| APNs, FCM, UnifiedPush | free |
| Google Play account | $25 once (likely paid) |
| Relay (Cloudflare Workers) | free tier; about $5 / month on the paid plan if needed |

## Setup (user, needs the accounts)

1. Apple: create an APNs auth key (`.p8`); enable Push Notifications on the app ID.
2. Firebase: create a project, add `com.osvauld.tinline`, download `google-services.json`,
   create a service account key for the relay.
3. Cloudflare: deploy the Worker (`crates/wake-relay-worker/README.md`), then the secrets.

## Build order

1. Relay: `wake-relay-core` + tests, the Worker deployed at `https://push.osvauld.com` (2026-10-24; `RELAY_KEYS` set; FCM / APNs secrets pending). End-to-end check passed: register, wake, kind refused, tampered token, revoke then 410.
2. Core: tokens, `WakeTokens` on the wire, wake decision, revoke, `on_wake`; in-process tests.
   - 3a done (2026-10-24, `crates/node-runtime/src/push.rs`): `wake::WakeState` sealed in `State::wake`;
     relay client behind `WakeRelay` (`HttpRelay` on reqwest); `set_push_target`; one wake task
     (register, hand `WakeTokens` to contacts, revoke outbox with retries); `WakeTokens` intake in
     `accept.rs`; in-process tests with a fake relay (`cargo test -p node-runtime --lib wake`).
   - 3b done (2026-10-24): the relay client builds its own rustls config (ring + webpki roots;
     live check `P2P_WAKE_LIVE=1 cargo test -p node-runtime --lib wake_live`); every device of a
     contact gets its own `WakeTokens` copy (`State::wake_outbox`: DID -> devices that hold it);
     `NodeCtx::wake_device` / `wake_unreached` / `fast_redial` (`push.rs`) are called by
     `chat_kick`'s dial loop (a `msg` wake per unreached device when something is pending, then
     fast redials at +2, 4, 7, 10, 15, 20, 30 s); `call.rs` `connect_leg` wakes a callee device
     (`call`) after a first dial of 6 s and redials every 1 s for 30 s, state stays `Dialing`.
     Tests: `dial_tests.rs` (W1, W2, W4/4b/4c, W8, W9, 410).
   - Slice 5 done (2026-10-24): `NodeCtx::on_wake` and the wake ring (`crates/node-runtime/src/push/ring.rs`;
     tests `ring_tests.rs`), the app API `crates/core/src/node/api_push.rs` (`PushTarget`, `set_push_target`,
     `push_target`, `on_wake -> WakeOutcome { Ignore, EndCall, DialedBack, Ring { did, name } }`,
     `answer_wake_ring`, `decline_wake_ring`). The callback trait is unchanged. With no open account
     `on_wake` returns `Locked` / `NoIdentity`: the app shows a generic notice (iOS call: report to
     CallKit and end at once). Android `play` calls `set_push_target(Fcm)` from `onNewToken` (and at
     start) and `on_wake(data["h"])` from `onMessageReceived`.
3. Android flavours: `play` (FCM) and `liberum` (no Google SDK, always-on); the no-Google
   build check; both in the release workflow. Then UnifiedPush in `liberum`.
   - Slice 6 done (2026-10-24, `android/`): flavour dimension `edition` (`play`, `liberum`, applicationId
     `com.osvauld.tinline.liberum`). Seam: `PushProvider` (`PushMode.kt`); `src/play` = FCM
     (`PlayPush.kt`, `WakeMessagingService`, manifest with analytics/ad-id off and auto-init off), `src/liberum` =
     `NoPush`. Firebase is started by hand (`FirebaseOptions`, no google-services plugin or json) from
     `FIREBASE_*` values in `android/firebase.properties` (gitignored), Gradle properties or env; without them
     `play` is always-on. `WakeHandler`: `onWake(hint)` unlocks with the remembered key, calls `Node.on_wake`, then
     `Ring` -> `WakeRing` (CallStyle notification, same id as the real call, so it is replaced in place; Answer ->
     `answer_wake_ring`, "Connecting…"; Decline -> `decline_wake_ring`; ends on `on_call_state("wake:<did>", Ended)`),
     `DialedBack` -> keep the node up, `Locked`/`NoIdentity` -> generic "You may have a new message". The real
     `on_incoming_call` takes the ring over by DID (an early answer carries into the call). Push mode: no service at
     boot or app open; a wake or call raises `CoreService`, which stops itself 90 s after the last activity.
     "Stay connected all the time" (Settings > Background & battery, play only) keeps the old always-on service and
     sets `set_push_target(None)`. `scripts/check_no_google.py` is the W13 check (CI runs it on the Liberum APK).
     Not device-tested; FCM secrets and the relay's FCM credential are still pending.
4. iOS PushKit + CallKit, then message alert push.
5. Device tests (Pixel 3a; a real iPhone), Play console texts, privacy policy.

## Acceptance tests

| ID | Scenario | Expected |
|---|---|---|
| W1 | B online; A sends | No wake request is made |
| W2 | B's app killed; A sends | One wake; B dials A; message delivered; B notifies; link closes after idle |
| W3 | B asleep; A calls | B rings (Android full-screen / iOS CallKit) with A's local name; answer connects |
| W4 | A hangs up before B is reachable | B's ringing ends within the no-`CallHello` window; no call connects |
| W4b | Call wake: connection direction | The call session is dialled A→B with `CallHello`; B never dials for a call |
| W4c | Call wake: caller UI | A shows "Calling…" until B's `Ringing` arrives, then "Ringing"; never "Ringing" before |
| W5 | Call wake older than 30 s arrives | Android: nothing; iOS: reported and ended at once |
| W6 | B blocks A, then A sends | After the revoke: relay answers 410 and A drops the token. Before it: Android shows nothing and dials nothing; iOS call reported and ended at once; never a dial to A |
| W6b | B blocks A while offline | Revoke waits in the outbox and reaches the relay on B's next start |
| W6c | B unblocks, A re-adds | A holds a new token (new tag); the old one still gets 410 |
| W7 | `allow_calls(A, false)` | A's `call` wakes refused; `msg` wakes still work |
| W8 | A sends 10 messages in 5 s | At most 2 `msg` wakes per minute per device |
| W9 | B has two devices, both asleep | Both woken; both sync |
| W10 | B's FCM/APNs token changes | Contacts receive new tokens; old ones stop working |
| W11 | A offline before B wakes | B's dial-back fails quietly; message arrives at the next overlap |
| W12 | Inspect relay and push payload | No DID, name, text or call detail anywhere; two pushes from the same contact are not linkable |
| W13 | Tinline Liberum APK | No `com.google.firebase` / `com.google.android.gms` classes; always-on service works as today |
| W13b | Liberum contact of a Play user | Reaches the Liberum phone directly when it is always-on, or by a UnifiedPush wake when it has a distributor |
| W13c | Liberum with ntfy installed, app asleep | UnifiedPush wake; same as W2/W3; ntfy uninstalled → back to always-on |
| W14 | Endpoint rotation on B | Wake tokens keep working |

## Open

1. **Dial-back and consent (messages only; calls never dial back).** Today a contact never
   learns we are online without our action (`docs/protocol.md` "Session lifetime"). Dial-back
   on a `msg` wake tells A that B's device is awake. It is no more than A's direct dial would show if B were always-on, but it is a
   change to the stated rule. Proposal: dial-back only for contacts we neither block nor muted.
2. ~~FCM SDK vs "no tracking SDKs"~~ — decided 2026-10-24: the SDK ships only in the Play
   edition; Liberum has none.
3. **Call wake timing:** A's redial interval while ringing (~1 s) and B's no-`CallHello`
   window (15 s from each wake, 40 s cap) need a device check; on a slow network the first `CallHello` may come late.
4. ~~Default on Tinline (Play) and Liberum app ID~~ — decided 2026-10-24: Play wakes by push
   by default, always-on is an option in Settings. Liberum has its own application ID
   `com.osvauld.tinline.liberum`, so both can be installed side by side; moving between them
   means moving the account (phrase or device link). Name and icon variant still to design.
5. **Relay operations:** who runs it, uptime, what happens to wakes while it is down (senders
   fall back to normal retries; nothing breaks, sleeping devices just miss the wake).
6. All PROVISIONAL numbers (token lifetime, rate limits, stale-call window). In code (3b,
   `push.rs`): `MSG_REDIAL_AT_MS` (2, 4, 7, 10, 15, 20, 30 s after a msg wake), `CALL_REDIAL_MS` 1 s,
   `CALL_WAKE_DIAL_MS` 30 s (from the wake), `CALL_FIRST_DIAL_MS` 6 s (first dial of a call that can
   be woken, instead of 20 s), `CALL_EARLY_WAKE_MS` 2 s (the `call` wake goes out while that first
   dial keeps running; the caller then shows "Waking their phone…", `CallStats::waking`), `CALL_ATTEMPT_MS` 3 s. Slice 5 (`ring.rs`): `WAKE_RING_WINDOW_MS` 15 s from the wake's arrival,
   `WAKE_RING_CAP_MS` 40 s in total. Slice 7 (`push.rs`): `MSG_EARLY_WAKE_MS` 3 s: a chat dial with
   something to deliver that has not connected after 3 s sends the `msg` wake in parallel with the
   still-running dial (`NodeCtx::early_wake`, called by `chat_kick`; only for devices we hold a live
   `msg` token for; the wake throttle still decides, and the post-failure `wake_unreached` stays as
   the fallback, throttled away when the early wake went out). A dial that connects sooner sends
   nothing. Call timing is unchanged.
7. **iOS banner from a blocked contact before the revoke lands.** Accepted for now (about a minute,
   no name, no text). Later: ask Apple for the notification-filtering entitlement
   (`com.apple.developer.usernotifications.filtering`), so a Notification Service Extension can
   open the hint and hide wakes from blocked contacts.
8. **Token handover at contact setup.** A new contact cannot wake us until our first wake round has
   run while the app is up (register at the relay, then `WakeTokens` to each device of the contact).
   If the app is closed right after adding the contact, the token never reaches them and they cannot
   wake us. Options (not implemented; the user decides):
   - (a) carry the wake token in `ContactWelcome` / the contact request, so it arrives with the contact;
   - (b) Android keeps the service up while `wake_outbox` is non-empty, bounded (for example 60 s);
   - (c) both.
9. **After a wake the process lingers.** Seen on device (2026-10-24): after `CoreService` stops, the
   process and its node keep running. Check that the node is stopped with the service in push mode.
10. **Call audio after a push wake needs a device check.** When a call wake started `CoreService`,
    Android logged "Foreground service started from background can not have
    location/camera/microphone access". The microphone type may be refused for a service started by
    a push; the existing fallback starts it as `specialUse`, but whether call audio then works is
    unverified.

## Device test, 2026-10-24 (Pixel 3a, play edition, desktop sender, relay `push.osvauld.com`)

First real run. Passed end to end, with these numbers and findings:

- **Message to a fully stopped phone:** the sender's chat dial ran its full `DIAL_TIMEOUT` (20 s)
  before `wake_unreached` sent the `msg` wake; FCM delivered 2.2 s later; the phone connected 3 s
  after the wake. Total about 24 s, 20 s of it waiting. Fix: `MSG_EARLY_WAKE_MS` (slice 7, Open 6).
- **Call to a stopped phone:** the first dial gave up at about 6.8 s (`CALL_FIRST_DIAL_MS`), wake,
  the phone rang 6.3 s later. 2026-10-25 (v0.2.1 build): wake sent 7.5 s after dialing, the phone
  rang at once, and the caller said "Calling…" for another ~7 s until its redial reached the phone.
  Fix: `CALL_EARLY_WAKE_MS` 2 s and the caller's "Waking their phone…" state.
- **The phone failed silently for a long time:** its push errors were `tracing::debug!`, which
  Android does not show, and a successful register / handover / receipt was not logged. Slice 7 logs
  these at info (no tokens, hints, full DIDs or endpoints): `wake register ok/failed`, `wake token
  handed to / refused by / not delivered to a device of <short did>`, `wake token received and
  accepted / refused: <reason>`, `wake <Kind> sent for a device of <short did>: relay status N`,
  `wake received: outcome <Kind>`.
- **Notification text:** during a wake the foreground-service notification read "Available for
  calls" (the always-on text). In push mode it now reads "Getting your messages…"; always-on mode
  keeps "Available for calls".
- Open 8, 9 and 10 record what else was seen.
