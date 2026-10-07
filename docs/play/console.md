# Play Console — what to fill in

App: **Tinline** · package `com.osvauld.tinline` · free · no ads.
Build: `scripts/build_android.py --bundle` → `~/tinline-builds/tinline-<versionCode>-<commit>.aab`
(upload key `~/keys/tinline-upload.jks`, alias `upload`; Play App Signing holds the app key).

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

- Does your app collect or share any of the required user data types? **No.**
  (Everything is stored only on the device; calls are end-to-end encrypted; nothing reaches our
  servers. Google's definition of "collect" is data transmitted off the device to the developer or a
  third party. Relays only forward end-to-end encrypted traffic, which Google exempts.)
- Is all data encrypted in transit? **Yes.**
- Do you provide a way to request deletion? **Data is never sent to us; uninstalling deletes it.**

### Foreground service

Types used: `microphone` and `specialUse`. The form asks per permission: FOREGROUND_SERVICE_MICROPHONE →
**Background audio input**; FOREGROUND_SERVICE_SPECIAL_USE → **Other** (then the texts below).

- **microphone** — "Active voice call. The service records the microphone only while a call the
  user started or answered is in progress, with an ongoing call notification."
- **specialUse** — "Tinline is a peer-to-peer calling app with no push server. To receive incoming
  calls it keeps one encrypted network connection open in a foreground service, shown by a
  persistent notification. There is no FCM or server-side wake-up, so without this service calls cannot ring."
  User impact if deferred: "Incoming calls would be missed."
  Video: record the phone showing the persistent notification, then a second device calling and the
  locked phone ringing full-screen.

### Full-screen intent (`USE_FULL_SCREEN_INTENT`)

Declare: **Calling app** — "Shows the incoming-call screen over the lock screen when another user
calls."

### Battery optimisation exemption (`REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`)

Acceptable use: **VoIP / calling app whose core function is broken by Doze, and that cannot use FCM
high-priority messages**. "Tinline has no server and no push messages; peers connect directly. Doze
would cut the connection and incoming calls would not ring. The user is asked in onboarding and can
skip."

### Camera

Only for scanning contact QR codes (no declaration form; mentioned in the privacy policy).

## Store listing (needed for closed/open testing and production, optional for internal)

- Short description (80): `Call people directly. End-to-end encrypted. No phone number, no ads.`
- Full description: draft from the onboarding copy and `docs/design/Flow.dc.html`.
- Icon 512×512, feature graphic 1024×500, at least 2 phone screenshots.
- Category: Communication. Contact email: osvauld@gmail.com. Website: https://tinline.osvauld.com
