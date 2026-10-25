# Play Console — what to fill in

App: **Tinline** · package `com.osvauld.tinline` · free · no ads.
Build: `:app:bundlePlayRelease` (the `play` flavour; the `liberum` flavour is the GitHub/site APK and is never uploaded
here), run under `tasks2`; CI's release workflow produces `tinline-play-v<version>.aab`
(upload key `~/keys/tinline-upload.jks`, alias `upload`; Play App Signing holds the app key).
The Firebase settings (`FIREBASE_APP_ID`, `FIREBASE_API_KEY`, `FIREBASE_GCM_SENDER_ID`; see
`android/firebase.properties.example`) must be set for the upload, or the bundle has no push and is always-on only.

## Internal testing release

Testing → Internal testing → Create new release → upload the `.aab`.

Release notes:
```
<en-US>
First test build: direct, end-to-end encrypted calls between Tinline users. Add a contact by scanning their code, then call.
</en-US>
```

Testers tab → email list → copy the opt-in link and send it to testers.

## App content

**Privacy policy:** https://tinline.osvauld.com/privacy (text in `docs/play/privacy.md`; must be live
before the first rollout).

**Ads:** No.

**App access:** All functionality is available without special access (no login). Explanation if asked:
"No account or login. On first launch the app creates a local identity. To test a call, install on
two devices, add each other by scanning the code in Add contact, then call."

**Content rating:** questionnaire → category "Communication". Users can interact/communicate: Yes.
Shares location: No. Digital purchases: No.

**Target audience:** 18 and over (avoids the Families policy).

**News app:** No. **Government app:** No. **Financial features:** none. **Health:** No.

### Data safety

- Does your app collect or share any of the required user data types? **Yes**: one type.
  - **Device or other IDs** (the Firebase push token): collected, transmitted off the device to our wake
    service (`push.osvauld.com`) inside a sealed wake token. Purpose: **App functionality** (wake the phone
    for a call or message). **Not shared** (Google's delivery of the push is a service provider acting for
    us). **Optional:** the user can turn on "Stay connected all the time". Processed ephemerally: **Yes**
    (the wake service keeps no record of wakes).
  - Nothing else: messages and calls are end-to-end encrypted and never reach our servers; relays only
    forward encrypted traffic, which Google exempts.
- Is all data encrypted in transit? **Yes.**
- Do you provide a way to request deletion? **Uninstalling deletes everything; the wake service holds no
  per-user data to delete.**

### Foreground service

Types used: `microphone` and `specialUse`. The form asks per permission: FOREGROUND_SERVICE_MICROPHONE →
**Background audio input**; FOREGROUND_SERVICE_SPECIAL_USE → **Other** (then the texts below).

- **microphone** — "Active voice call. The service records the microphone only while a call the
  user started or answered is in progress, with an ongoing call notification."
- **specialUse** — "Tinline is a peer-to-peer calling and messaging app. By default it rests and is
  woken by a content-free FCM message; the service then runs briefly to connect to the contact and
  receive the call or message, shown by a notification, and stops by itself. If the user turns on
  'Stay connected all the time', the service keeps one encrypted connection open, shown by a
  persistent notification."
  User impact if deferred: "Incoming calls and messages would be missed."
  Video: record the phone showing the persistent notification, then a second device calling and the
  locked phone ringing full-screen.

### Full-screen intent (`USE_FULL_SCREEN_INTENT`)

Declare: **Calling app** — "Shows the incoming-call screen over the lock screen when another user
calls."

### Battery optimisation exemption (`REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`)

Acceptable use: **VoIP / calling app whose core function is broken by Doze**. "By default Tinline is
woken by FCM high-priority messages. Users who turn on 'Stay connected all the time' (or whose phone
has no Google Play services) keep a direct connection instead; Doze would cut it and incoming calls
would not ring. The user is asked in onboarding and can skip."

### Camera

Only for scanning contact QR codes (no declaration form; mentioned in the privacy policy).

## Store listing (needed for closed/open testing and production, optional for internal)

- Short description (80): `Call people directly. End-to-end encrypted. No phone number, no ads.`
- Full description: draft from the onboarding copy and `docs/design/Flow.dc.html`.
- Icon 512×512, feature graphic 1024×500, at least 2 phone screenshots.
- Category: Communication. Contact email: osvauld@gmail.com. Website: https://tinline.osvauld.com

## Push wake-ups (docs/push.md)

The `play` edition ships `firebase-messaging` only (no analytics or crashlytics, ad id off, token
auto-init off until the user is in wake-by-push mode). Done on 25 October 2026: foreground service,
battery exemption, data safety (above) and the privacy policy (`docs/play/privacy.md`) now describe it.
Still to check when the store listing is written: no line may say "no push", "no servers", "nothing
leaves your phone" or "no Google services" for `play` (that stays true only for Liberum).
