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

## UI (Compose, Material 3, Tinline design)

Spec: `docs/design/*.dc.html` (tokens in `Main.dc.html`). Code map (package `com.osvauld.p2p`):

- `Theme.kt` `TinlineTheme` (exact light/dark tokens, dynamic colour off, `Tin.c.*` custom tokens: thread
  amber, relay, warn, call accept/end; `TinType` scale in Figtree + IBM Plex Mono from `res/font`, OFL
  texts in `assets/licenses/`). Dark mode follows the system.
- `Components.kt`, `Glyphs.kt`: buttons, fields, avatars (initials, colour by hashing the DID), progress
  dots, banners, status pill, list rows, sheets/dialogs, custom Direct/Relayed glyphs. In-app icons are
  Material Symbols Rounded via `material-icons-extended` (R8 strips the unused ones).
- Screens: `Onboarding.kt` (welcome, name, passphrase + strength meter, recovery phrase, quick check,
  terms, permissions), `Unlock.kt` (unlock, restore with BIP-39 suggestions from `assets/bip39_english.txt`,
  legacy set-passphrase), `Home.kt`, `AddContact.kt` (My code / Scan / Paste / Adding / Added / Couldn't
  add), `ContactDetail.kt`, `Settings.kt` (settings, background & battery, recovery phrase gate/shown,
  change passphrase, about, licences, debug diagnostics), `CallScreens.kt` (incoming, calling, in call +
  audio route sheet, call ended with 4 s auto-close, couldn't reach, microphone needed).
- Navigation: `MainActivity.Root` with a small sealed `Route` back stack (not saved across process death;
  the recovery phrase may sit in it).
- Terms & privacy: `LegalStore` persists the accepted version (`LegalStore.CURRENT`); required once, also
  for existing installs.
- Call end vocabulary lives only in `endReasonText` / `classifyEnd` (`Support.kt`); unknown = "Call ended".
- `Features` (`Features.kt`) hides entry points whose data the core does not have yet: rename, verify
  (safety number), availability, history. Flip the flag when wired; the screens already exist.
- Debug builds include `GalleryActivity` (`--es screen <name>`) that renders the screens with sample data
  for screenshots.

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
