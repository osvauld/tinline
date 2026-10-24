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
  no `CallHello` for that contact arrives within a short window after the node is up
  (10–15 s, PROVISIONAL). No cancel push is needed.
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
- Custom domain `push.osvauld.com` once the probe passes; until then `*.workers.dev`.

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

1. Relay: `wake-relay-core` + tests, the Worker deployed at `tinline-wake.wake-relay-worker.workers.dev` (2026-10-24; secrets pending).
2. Core: tokens, `WakeTokens` on the wire, wake decision, revoke, `on_wake`; in-process tests.
3. Android flavours: `play` (FCM) and `liberum` (no Google SDK, always-on); the no-Google
   build check; both in the release workflow. Then UnifiedPush in `liberum`.
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
   window (10–15 s) need a device check; on a slow network the first `CallHello` may come late.
4. ~~Default on Tinline (Play) and Liberum app ID~~ — decided 2026-10-24: Play wakes by push
   by default, always-on is an option in Settings. Liberum has its own application ID
   `com.osvauld.tinline.liberum`, so both can be installed side by side; moving between them
   means moving the account (phrase or device link). Name and icon variant still to design.
5. **Relay operations:** who runs it, uptime, what happens to wakes while it is down (senders
   fall back to normal retries; nothing breaks, sleeping devices just miss the wake).
6. All PROVISIONAL numbers (token lifetime, rate limits, stale-call window).
7. **iOS banner from a blocked contact before the revoke lands.** Accepted for now (about a minute,
   no name, no text). Later: ask Apple for the notification-filtering entitlement
   (`com.apple.developer.usernotifications.filtering`), so a Notification Service Extension can
   open the hint and hide wakes from blocked contacts.
