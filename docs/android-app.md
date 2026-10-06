# Android app

Native Kotlin + Jetpack Compose over the Rust core (`p2pcore`, UniFFI bindings in package
`uniffi.p2pcore`). The core owns identity, contacts, the iroh endpoint, the call protocol and
the codec; the app owns UI, audio devices, and staying alive.

## Core API used (generated Kotlin names)

`Node(dataDir, events)` — one per process, kept by `P2pApp`. Blocking calls run on
`Dispatchers.IO`, never the main thread.
`hasIdentity()`, `createIdentity(name) -> phrase`, `restoreIdentity(phrase, name)`, `profile()`,
`recoveryPhrase()`, `setName()`, `start()`, `stop()`, `networkChanged()`, `status()`,
`myTicket()`, `addContact(ticket)`, `contacts()`, `removeContact(did)`, `call(did) -> CallInfo`,
`answer(id)`, `decline(id)`, `hangup(id)`, `currentCall()`, `pushMic(ShortArray-ish List<Short>)`,
`pullSpeaker() -> List<Short>` (960 samples = 20 ms @ 48 kHz mono), `callStats()`,
`setTestTone(hz?)`.
`NodeEvents` callback (called on core threads — hop to main/flows): `onStatus`, `onContactsChanged`,
`onIncomingCall(CallInfo)`, `onCallState(callId, CallState)`, `onLog(line)`.

## Staying reachable ("always on")

- `CoreService`, a foreground service started at app launch (once an identity exists), on boot
  (`BOOT_COMPLETED`, `MY_PACKAGE_REPLACED`) and from incoming-call handling. Idle it runs as
  `foregroundServiceType="specialUse"` (subtype property: "peer-to-peer call listener") with a
  low-importance ongoing notification "Ready for calls"; during a call it re-calls
  `startForeground` with `specialUse|microphone`. `START_STICKY`.
- It owns `node.start()`, a `ConnectivityManager` default-network callback that calls
  `node.networkChanged()`, and a partial wake lock + `WIFI_MODE_FULL_LOW_LATENCY` lock **only
  while a call is active**.
- First run asks for: notifications, microphone, ignore-battery-optimizations
  (`ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`), and on API 34+ full-screen intents if
  `canUseFullScreenIntent()` is false.

## Incoming call

`onIncomingCall` → high-importance "calls" channel notification with `CATEGORY_CALL`,
`setFullScreenIntent(IncomingCallActivity)`, Answer/Decline actions (`Notification.CallStyle` on
API 31+), ringtone + vibration until answered/ended. `IncomingCallActivity` uses
`setShowWhenLocked(true)`/`setTurnScreenOn(true)`. Answer → `node.answer`, service goes
microphone-type, open `CallActivity`. Ended/missed → cancel ringing, post a "Missed call" note.

## Audio during a call (`AudioEngine`)

Starts on `CallState.Active`, stops on `Ended`. `AudioManager.MODE_IN_COMMUNICATION`.
Mic: `AudioRecord(VOICE_COMMUNICATION, 48000, MONO, PCM_16BIT)` with `AcousticEchoCanceler`,
`NoiseSuppressor`, `AutomaticGainControl` enabled when available; a thread reads 960-sample
frames → `pushMic`. Speaker: `AudioTrack` (`USAGE_VOICE_COMMUNICATION`,
`CONTENT_TYPE_SPEECH`, 48 kHz mono PCM16, stream mode, low-latency perf mode); a thread loops
`pullSpeaker` → blocking `write` (the write paces the loop). Earpiece by default; speaker toggle
via `setCommunicationDevice` (API 31+) / `isSpeakerphoneOn` below. Mute pushes zeros. Bluetooth
is out of scope for v1.

## UI (Compose, Material 3)

- Onboarding: name → create (show the 24-word recovery phrase, confirm saved) or restore.
- Home: status chip (online via relay / offline), "My contact card" (QR of `myTicket()` + copy +
  share), contacts list (tap → call; long-press → remove), "Add contact" (paste ticket; scan QR
  with the camera).
- Call screen: name, state (Calling…/Ringing…/timer), mute, speaker, hang up, small stats line
  (direct/relay, rtt, loss).
- Settings: name, show recovery phrase, test tone toggle.

## Test hooks (debug builds only)

Exported receiver `DebugReceiver` in `src/debug/AndroidManifest.xml`, action
`com.osvauld.p2p.DEBUG`, extra `cmd`; every result and every call event logs one line with tag
`P2PTEST` so scripts can `adb logcat -s P2PTEST`:

| cmd | extras | logs |
|---|---|---|
| `create` | `name` | `P2PTEST created did=…` |
| `ticket` | | `P2PTEST ticket=osvc1.…` |
| `add` | `ticket` | `P2PTEST added name=… did=…` or `P2PTEST error=…` |
| `contacts` | | one `P2PTEST contact name=… did=…` each |
| `call` | `who` (name or did) | call states |
| `answer` / `decline` / `hangup` | | |
| `tone` | `hz` (0 = off) | |
| `status` | | `P2PTEST status online=… relay=… id=…` |
| `stats` | | `P2PTEST stats …` (also logged every second during a call) |

Always logged: `P2PTEST incoming id=… from=…`, `P2PTEST state id=… state=…`.
