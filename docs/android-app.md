# Android app

Native Kotlin + Jetpack Compose over the Rust core (`p2pcore`, UniFFI bindings in package
`uniffi.p2pcore`). The core owns identity, contacts, the iroh endpoint, the call protocol and
the codec; the app owns UI, audio devices, and staying alive.

## Core API used (generated Kotlin names)

`Node(dataDir, events)` — one per process, kept by `P2pApp`. Blocking calls run on
`Dispatchers.IO`, never the main thread.
`hasIdentity()`, `lockState()`, `createIdentity(name, passphrase) -> phrase`, `restoreIdentity(phrase, name, passphrase)`, `unlock(pass)`, `unlockWithKey(key)`, `unlockKey()`, `setPassphrase(old?, new)`, `profile()`,
`recoveryPhrase(passphrase)`, `setName()`, `start()`, `stop()`, `networkChanged()`, `status()`,
`myTicket()`, `addContact(ticket)`, `contacts()`, `removeContact(did)`, `call(did) -> CallInfo`,
`answer(id)`, `decline(id)`, `hangup(id)`, `currentCall()`, `pushMicPcm16(ByteArray)`,
`pullSpeakerPcm16() -> ByteArray` (little-endian PCM16; 960 samples = 1920 bytes = 20 ms @
48 kHz mono — use these, not the `List<Short>` variants), `callStats()`,
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

- Onboarding: name + passphrase (min 8, confirmed) → create (show the 24-word recovery phrase, confirm saved) or restore.
- Home: status chip (online via relay / offline), "My contact card" (QR of `myTicket()` + copy +
  share), contacts list (tap → call; long-press → remove), "Add contact" (paste ticket; scan QR
  with the camera).
- Call screen: name, state (Calling…/Ringing…/timer), mute, speaker, hang up, small stats line
  (direct/relay, rtt, loss).
- Settings: name, show recovery phrase (asks the passphrase), change passphrase, test tone toggle.
- Unlock screen when `lockState == LOCKED`; "Forgot passphrase" restores the same identity from its
  phrase (sealed profile.json is moved aside, state.json/contacts kept). Legacy installs
  (`NEEDS_PASSPHRASE`) get a blocking "Set a passphrase" screen; calls keep working meanwhile.

## Passphrase + device unlock (`UnlockStore`)

After create/restore/unlock/set-passphrase the app calls `node.unlockKey()` (the 32-byte vault data
key) and wraps it with an AES-256-GCM key in AndroidKeyStore (alias `p2p_unlock_v1`, no user
authentication required) into `filesDir/unlock.bin` (`iv(12) || ct+tag`, atomic write). On process
start (`P2pApp.onCreate`, `startNode`, so also service restarts and boot) a locked node is unlocked
via `unlockWithKey`. If unwrapping or unlocking fails, `unlock.bin` is deleted, the node stays
locked, the service stays up, and a "Unlock to receive calls" notification opens the app. Keys and
passphrases are never logged; `allowBackup=false`.

## Test hooks (debug builds only)

Exported receiver `DebugReceiver` in `src/debug/AndroidManifest.xml`, action
`com.osvauld.tinline.DEBUG`, extra `cmd`; every result and every call event logs one line with tag
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
