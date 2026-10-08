# Android app

Native Kotlin + Jetpack Compose over the Rust core (`p2pcore`, UniFFI bindings in package
`uniffi.p2pcore`). The core owns identity, contacts, the iroh endpoint, the call protocol and
the codec; the app owns UI, audio devices, and staying alive.

## Core API used (generated Kotlin names)

`Node(dataDir, events)` — one per process, kept by `P2pApp`. Blocking calls run on
`Dispatchers.IO`, never the main thread.
`hasIdentity()`, `lockState()`, `createIdentity(name, passphrase) -> phrase`, `restoreIdentity(phrase, name, passphrase)`, `unlock(pass)`, `unlockWithKey(key)`, `unlockKey()`, `setPassphrase(old?, new)`, `profile()`,
`recoveryPhrase(passphrase)`, `setName()`, `accounts()`, `switchAccount(did)`, `beginNewAccount()`, `removeAccount(did)`,
`commitIdentity()`, `identityCommitted()`, `setDeviceLabel()`/`deviceLabel()`, `start()`, `stop()`, `networkChanged()`, `status()`,
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
- Call end vocabulary lives only in `endReasonText` / `classifyEnd` / `isMissedReason` (`Support.kt`) and is the
  core's token set (docs/protocol.md "End reasons"); unknown = "Call ended". `superseded` shows no ended
  screen; `unreachable` before the call was active shows "Couldn't reach".
- Core data wired (no feature flags any more): call log (`P2pApp.history`, re-read on every `Ended` and on
  contacts changes; `History.kt` words it; Home Recent + "See all" list + per-contact history), alias
  (`Contact.display()` everywhere; rename sheet, Added "Save as"), verify (`safetyNumber` in Plex Mono,
  They match -> `setVerified`), availability (`P2pApp.availability`, status pill + sheet, Settings toggle,
  banner, foreground-notification text, a coroutine timer for timed breaks, re-read on resume),
  `CallStats.reconnecting` (In call "Reconnecting" state).
- Debug builds include `GalleryActivity` (`--es screen <name>`) that renders the screens with sample data
  for screenshots.

## Accounts, passphrase + device unlock (`UnlockStore`)

The core keeps one directory per account under `filesDir/core/accounts/<id>/` (docs/vault.md); the app
only ever talks to the one selected account. Everything below is per account.

**Remembered keys.** `node.unlockKey()` (the 32-byte vault data key) is wrapped with an AES-256-GCM key in
AndroidKeyStore (alias `p2p_unlock_v1`, no user authentication) into `filesDir/unlock/<did-suffix>.bin`
(`iv(12) || ct+tag`, atomic write, read back and decrypted before `save` reports success). One blob per account
DID, one shared Keystore alias: the alias is only the wrapping key and each blob has its own IV, so one alias is
simpler and a lock-screen reset invalidates all blobs together (each then reads as gone and that account asks for
its passphrase or phrase). Upgrade: the old single `filesDir/unlock.bin` is renamed to the blob of the current
account (the one the core just migrated) on first start (`UnlockStore.migrateLegacy`); never overwrites an
existing blob. On start (`P2pApp.onCreate`, `startNode`, so also service restarts and boot) a locked node is
unlocked via `unlockWithKey` with the *current* account's blob. Unwrapping or unlocking failing for good deletes
that blob, the node stays locked, the service stays up, and "Unlock to receive calls" opens the app.
Keys and passphrases are never logged; `allowBackup=false`.

**Commit (S1).** `P2pApp.persistIdentity()` runs after create/restore: it saves the key (checking the result) and
only then calls `node.commitIdentity()`. A no-passphrase identity is only in memory until then. If the save or
the commit fails, onboarding shows "Couldn’t save your account" with Retry and "Add a passphrase instead"
(`setPassphrase(null, p)` on the unlocked identity commits it). The service and node are not started, and no
success step is shown, before the commit. For a passphrase identity the account is on disk at once; a failed key
save only means the passphrase is asked after a restart. `identityReady()` (unlock, set passphrase) does the same
persist and starts the service, but never leaves a committed account unstarted.

**Switching.** Settings account card › Switch opens the account sheet (`AccountSheet` in `Accounts.kt`): the
accounts from `node.accounts()` (current ticked and "Online on this phone"; others "Locked · not ringing here"),
Create a new account, Link an account from another device (disabled, "Coming soon"), Restore with recovery phrase.
Picking another account opens the confirmation screen (board copy; passphrase field only if that account has a
passphrase and this phone holds no key for it). `P2pApp.switchTo(did, pass?)`: `switchAccount` (the core stops the
endpoint and forgets the old session) → unlock with the typed passphrase (wrong one: switch back and show the
error) or the account's remembered key (none: the unlock screen shows) → service/`startNode`. `InCall` shows the
snackbar "Finish your call first". The back stack resets to Home. The unlock screen also has "Switch account" when
more than one exists.
Create/restore: `beginAdding()` → `beginNewAccount()`, then the onboarding flow runs in "adding" mode
(`P2pApp.adding`): starts at Name or the recovery words, skips Terms/Permissions, and backing out calls
`cancelAdding()` which selects the account that was left. `beginNewAccount` deselects in the core, so the app
remembers the last committed account (prefs `accounts/did`) and re-selects it at start if the process died
mid-add. A phrase whose account is already here gives `AccountExists`: "Already on this phone" with a button to
the switcher. "Forgot passphrase" restores over the locked account: when it is the only account its directory is
replaced (contacts and history of that account are not kept, see "needs core" below); with several accounts the
phrase cannot be matched to one, so the user is told to switch to the matching account instead.

**Onboarding.** Welcome (Get started, Link to an existing account = "coming soon", I have a recovery phrase) →
Name → Passphrase → Recovery phrase → **Name this phone** (`setDeviceLabel`; chips Personal phone / Work phone /
Tablet; works before the commit, the core keeps it in memory and writes it with the commit) → Terms →
Permissions. A restore shows the "Welcome back" screen after the device name (link button disabled for now;
"Start without them" continues). Settings › Devices › Linked devices lists "This phone — <label>" (rename) and a
disabled "Link a device".

**Needs core (not done here).** (1) A way to restore a phrase over its own locked account keeping contacts and
history (today the account directory is replaced). (2) `Error::AccountExists` carrying the DID (or a
`did_of_phrase()`), so the app can offer to switch to exactly that account instead of "Choose account".

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
