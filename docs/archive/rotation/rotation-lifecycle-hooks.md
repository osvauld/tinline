# Rotation A2 — endpoint lifecycle hook map

Status: proposal — not approved

Packet A2 is a read-only audit. Nothing outside this file was changed, and no builds or tests
were run because the packet requires none. This document answers one question: where would a
rotation scheduler start and stop endpoints, and where must overdue retirement run? It
proposes no protocol and decides no policy gate.

The A1 report (`docs/design/rotation-baseline-report.md`) already covers which state is keyed by
the device key, and where it persists. This file does not repeat that table. It points to A1
rows as "A1 §x.y".

Sections 1–5 are **verified in source**: each claim cites `path:line` at the commit below.
Section 6 lists **inferences** drawn from reading the code. None of them is covered by a test.
Section 7 is **proposal only**.

| Item | Value |
|---|---|
| Worktree / branch | `/home/abe/p2p_chat/.claude/worktrees/rot-a2`, `rot/a2` |
| HEAD at audit | `a50ed7f` (contains `35564bc`) |
| `git status --short` before writing | clean |
| Paths | `node.rs` = `crates/core/src/node.rs`. `engine.rs` = `crates/core/src/chat/engine.rs`. `sync/…` = `crates/core/src/sync/…`. `kt/…` = `android/app/src/main/kotlin/com/osvauld/p2p/…`. `desktop/…` = `crates/desktop/src/…` |

---

## 1. Endpoint startup → accept → shutdown (verified)

### 1.1 Startup: `Node::start` → `Inner::start`

| Step | path:line | What happens |
|---|---|---|
| UniFFI entry | `node.rs:871-875` | `Node::start` blocks on `Inner::start`. It is documented as idempotent |
| Lifecycle lock | `node.rs:1770` | Takes `Inner.lifecycle`, a tokio mutex (`node.rs:450-451`). That mutex serialises start, stop, lock, set_passphrase and replace_account |
| Requires unlocked | `node.rs:1771` | `self.me()?` fails with `Locked` or `NoIdentity` (`node.rs:1719-1726`) |
| Idempotence | `node.rs:1772-1774` | Returns `Ok` if `Shared.endpoint` is already `Some` |
| Transport config | `node.rs:1775-1779` | keep-alive 5 s, idle timeout 20 s |
| Builder | `node.rs:1780-1789` | `SecretKey::from_bytes(&me.profile.device_secret)`, five ALPNs: call `proto::ALPN`, `CHAT_ALPN`, `iroh_blobs::ALPN`, `SELF_ALPN`, `LINK_ALPN` |
| Test knob | `node.rs:1792-1795` | `P2P_RELAY_ONLY` clears IP transports |
| Bind | `node.rs:1796` | `builder.bind().await` |
| Lock-race guard | `node.rs:1797-1802` | If `me()` vanished during the bind, the endpoint is closed. Comment: "an endpoint must never outlive the secrets it was made from" |
| Install | `node.rs:1804-1815` | Stores `Shared.endpoint = Some(ep.clone())`. If another start won the race, closes this endpoint |
| Post-start hooks | `node.rs:1816-1820` | `log`, `emit_status`, `chat_started()` (`engine.rs:228-252`), `selfsync_started()` (`sync/engine.rs:636-650`), `sync_kick()` (`sync/engine.rs:77-80`) |
| Address watcher task | `node.rs:1822-1831` | Streams `watch_addr()` and emits status on each change |
| Accept loop task | `node.rs:1832-1887` | See 1.2 |

`chat_started` and `selfsync_started` are called only from `node.rs:1818-1819`. These two
calls are the only places that start endpoint-scoped background loops.

### 1.2 Accept loop (`node.rs:1833-1886`)

- `while let Some(incoming) = ep.accept().await` (`node.rs:1835`). The loop ends when the endpoint
  closes, then logs `endpoint … closed` (`node.rs:1885`).
- Admission: a permit from `pending` (`MAX_PENDING_HELLOS`), else from `reserved`, else `refuse()`
  (`node.rs:1839-1849`).
- ALPN dispatch, one task per connection (`node.rs:1850-1882`):
  - `SELF_ALPN` → `handle_self_incoming` (`sync/engine.rs:286-293`)
  - `LINK_ALPN` → `handle_link_incoming`
  - `CHAT_ALPN` / blobs → `handle_chat_incoming` (`engine.rs:453-482`)
  - anything else → `handle_incoming`: contact hello / call hello (`node.rs:2059`), which leads to
    `incoming_call` (`node.rs:2141`) and `run_call` (`node.rs:2691`)

### 1.3 Shutdown paths

All of them close the endpoint through `Inner::close_endpoint` (`node.rs:1712-1717`). It
`take()`s `Shared.endpoint` and then runs `ep.close()` with a 2 s timeout.

| Path | path:line | Order of operations |
|---|---|---|
| `Node::stop` | `node.rs:877-889` | lifecycle lock → `chat_sessions_close()` (`engine.rs:205-211`) → `close_endpoint()` → `emit_status`. Secrets stay in memory, so the node is still unlocked |
| `Node::lock` | `node.rs:819-836` | No-op unless `Disk::V2` (`node.rs:820-822`). Order: `link_cancel()` (`sync/link.rs:107-117`) → lifecycle lock → `close_endpoint()` → `forget_session()` (`node.rs:1467-1486`) → `chat_close()` (`engine.rs:193-203`) → `emit_status` |
| `replace_account` (switch / deselect / self-unlink) | `node.rs:1643-1667` | lifecycle lock → `not_in_call()?` → select/deselect pointer → `chat_sessions_close()` → `close_endpoint()` → `chat_close()` → `forget_session()` and clear disk/acct → optional `install()` (`node.rs:1448-1465`, which leaves the new account **locked**) → `emit_status`, `on_contacts_changed` |
| `Node` drop | `node.rs:469-485` | Takes the endpoint and closes it with a 1 s timeout when not on a core thread, then `rt.shutdown_background()` |

Nothing restarts the endpoint inside core. Every restart comes from a platform calling
`Node::start` again (§4).

---

## 2. Endpoint ownership and account switching (verified)

- **One `Node` per process.**
  - Android: `P2pApp` "owns the one [Node]" (`kt/P2pApp.kt:26-29`), created once in
    `onCreate` (`kt/P2pApp.kt:68`, `:212`).
  - Desktop: one `Node::new` in `main` (`desktop/main.rs:309`).
  - Test CLI: `crates/peer/src/main.rs:177`.
- **At most one long-lived endpoint per `Node`.** It belongs to the currently selected,
  unlocked account: `Shared.endpoint: Option<Endpoint>` (`node.rs:396`), next to
  `Shared.me` (`node.rs:391`) and `Shared.dek` (`node.rs:393`). There is no per-account map.
  Only the selected account is ever online, which matches the "accounts online model" memory
  note.
- **Session epoch.** `Shared.epoch` (`node.rs:397-400`) changes on install, forget, unlock and
  identity creation (`node.rs:1450`, `:1468`, `:1231`, `:1317`, `sync/link.rs` create-linked).
  `Me.epoch` (`node.rs:378-379`) records the epoch a session started under. Persist and log
  writes compare against it (`node.rs:1494-1515`, `:1680-1696`). **The endpoint has no epoch
  of its own.** Background loops tell "this endpoint" from "no endpoint" only by
  `Shared.endpoint.is_none()` (§5, §6 L1).
- **Account switch.** `Node::switch_account` (`node.rs:572-579`) → `replace_account(Some(loaded))`.
  It refuses with `InCall` during a call (`node.rs:574`, `:1645`). It closes the old endpoint and
  installs the new account locked. The platform must then unlock it and call `start` again
  (§4). `begin_new_account` (`node.rs:582-586`) does the same with `None`.
- **Second, temporary endpoint (linking, new device N only).** `fresh_link_endpoint`
  (`sync/api.rs:65-76`) binds a separate endpoint with a fresh secret and only `LINK_ALPN`.
  This happens only while no account is selected (`require_no_account`, `sync/api.rs:99`,
  `:106`). After the link run it is closed (`sync/link.rs:299-301`). N then keeps that same
  secret as its account device key (`sync/link.rs:439-441`). This is the **only existing code
  where an endpoint lives outside `Shared.endpoint`**. It never coexists with a main endpoint.
- **Every connection holds a clone of the endpoint.** `Leg.ep` (`node.rs:287-288`),
  `run_outgoing(ep, …)` (`node.rs:2464`, `:2480`, `:2502-2537`) and `add_contact`
  (`node.rs:1951`) each hold a clone. Chat/self/blob dials fetch `self.endpoint()` per attempt
  (`engine.rs:343`, `:1483`, `sync/engine.rs:296`, `:688`, `:709`).

---

## 3. Vault lock/unlock and where secrets become available (verified)

`device_secret` exists in clear only inside `Me.profile` (`node.rs:377-383`, `store.rs:41-46`).
`Me` exists only while unlocked. `account.json` holds the clear `device_public` and the sealed
vault (A1 §3.1).

### 3.1 Secrets enter memory

| Entry | path:line | Notes |
|---|---|---|
| `unlock(passphrase)` | `node.rs:676-685` | Argon2id `vault::open` (`vault.rs:117-128`) → `finish_unlock` |
| `unlock_with_key(dek)` | `node.rs:687-706` | Uses a platform-remembered DEK (`vault.rs:131-138`) → `finish_unlock` |
| `finish_unlock` | `node.rs:1205-1246` | `Me::load` (`node.rs:1083-1088`, makes a fresh attestation) → loads the sealed state → `epoch += 1` → sets `me` and `dek` → `chat_open()`, `acc_open()`. **Does not start the endpoint** |
| `create_identity` / `restore_identity` → `set_identity` | `node.rs:647-658`, `:1248-1330` | Fresh `new_device_secret()` (`:1250`). Restore-over-existing tombstones the old self key (`:1287-1292`, A1 §3.1) |
| Link (N) `create_linked_account` | `sync/link.rs:452-510` | Uses the temp endpoint's secret |
| Legacy profile | `node.rs:1453-1458` | `install` unlocks a legacy JSON profile at once |

### 3.2 Secrets leave memory

- `forget_session` (`node.rs:1467-1486`) clears `me`, `dek`, `state`, history, `acc`, backing.
  It is called by `lock` (`node.rs:831`), by `replace_account` (`node.rs:1655`) and by
  `install` (`node.rs:1449`).
- Endpoint closure is ordered **before** `forget_session` in both `lock` and `replace_account`.
  `start` re-checks `me()` after binding (`node.rs:1797-1802`).

### 3.3 Vault primitives relevant to rotation

- `vault::seal` (`vault.rs:101-115`) always makes a **fresh DEK**. With a passphrase it also runs
  Argon2id.
- `vault::rewrap` (`vault.rs:140-142`) keeps the sealed blob and the DEK but changes the passphrase.
- No function re-seals new `Secrets` under the **existing** DEK while keeping `wrapped_dek` and
  `salt`. See §6 L5.
- Platforms remember the DEK:
  - Android: `rememberKey` → `UnlockStore.save` (`kt/P2pApp.kt:192-196`).
  - Desktop: `keystore::auto_unlock` (`desktop/keystore.rs:207-224`).

---

## 4. Platform hooks and the core calls they make (verified)

### 4.1 Android

| Hook | path:line | Core calls |
|---|---|---|
| `Application.onCreate` | `kt/P2pApp.kt:62-81` | `Node(...)`, `setLinkEvents`, `selectLastAccountIfNone` → `switchAccount` (`:151-159`), `tryAutoUnlock` → `unlockWithKey` (`:169-186`). **No `start` here** |
| Boot / package replaced | `kt/BootReceiver.kt:8-12`, manifest `AndroidManifest.xml:86-93` | `hasIdentity()`, then `CoreService.ensureRunning` |
| `MainActivity.onCreate` | `kt/MainActivity.kt:47` | `CoreService.ensureRunning` if an identity exists |
| Foreground service `onStartCommand` (every start/poke) | `kt/CoreService.kt:39-53` | call actions → `enterForeground` → `updateLocks` → **`app.startNode()`**. `START_STICKY` |
| `startNode` (single-thread lifecycle executor) | `kt/P2pApp.kt:326-344` | `tryAutoUnlock()`. If still locked: "locked" notification and **no start**. Else `node.start()` |
| Network change | `kt/CoreService.kt:23-30`, registered at `:36`, unregistered at `:95` | `node.networkChanged()` on `onAvailable`/`onLost`. Core: `node.rs:891-898` (no-op without an endpoint) → `ep.network_change()`, `chat_kick_pending()`, `sync_kick()` |
| Activity `ON_RESUME` | `kt/MainActivity.kt:115-122` | `app.refresh()` only: reads `lockState`, `contacts`, `status`, history (`kt/P2pApp.kt:104-115`). **No core lifecycle call** |
| Unlock screen | `kt/Unlock.kt:75`, `:84-87`, `:260-265` | `unlock` / `setPassphrase` → `identityReady` (`kt/P2pApp.kt:306-315`) → `persistIdentity` → `startServices` → `ensureRunning` + `startNode` |
| Account switch | `kt/P2pApp.kt:248-268` (callers `kt/Accounts.kt:100`, `kt/Unlock.kt:66`, `kt/Onboarding.kt:245`) | `switchAccount` → `tryAutoUnlock` or `unlock` → `ensureRunning`, `startNode` |
| Add account | `kt/P2pApp.kt:222-241` | `beginNewAccount` (endpoint closed, offline until return) |
| Unlinked / account gone | `kt/Link.kt:141-143` → `kt/P2pApp.kt:133-141` | Core already ran `replace_account(None)` (`sync/engine.rs:226-251`). App selects another account → `startNode` |
| Call state | `kt/CallController.kt:99`, `:167`, `:174` | `ensureRunning` with IN_CALL/IDLE (wake/wifi locks, `kt/CoreService.kt:77-92`) |
| Device "Sync now" | `kt/Devices.kt:129` | `networkChanged()` |

Verified negatives (grep over `android/app/src`):
- Android never calls `node.lock()` or `node.stop()`.
- Android has no `onTrimMemory`, Doze, `AlarmManager` or `WorkManager` hook.

### 4.2 Desktop (iced)

| Hook | path:line | Core calls |
|---|---|---|
| Process start | `desktop/main.rs:309-318` | `Node::new`, `keystore::auto_unlock` |
| App init | `desktop/app.rs:714-721` | `unlock` (test) or `start_node` when it has an identity and is not locked |
| `start_node` | `desktop/app.rs:772-775` | `node.start()` → `Msg::Started` (`:1398-1405`) |
| Unlock | `desktop/app.rs:1209-1221` | `unlock` → `start_node` |
| Setup / link finished | `desktop/app.rs:1105-1122`, `:1392-1396` | `start_node` |
| Switch account | `desktop/app.rs:1789-1800`, `desktop/flow.rs:68-71` | `switch_account` + `auto_unlock` → `start_node` if unlocked |
| Quit | `desktop/app.rs:1006-1018` | `node.stop()` with a 3 s cap |
| 1 s UI tick | `desktop/app.rs:2015`, `:939-955` | Polls `status`, `lock_state`, `availability` |

Verified negatives:
- Desktop never calls `network_changed`.
- Desktop never calls `node.lock()`.
- Desktop has no OS sleep/resume hook.

### 4.3 Core-side timers that exist today

| Timer | path:line | Clock |
|---|---|---|
| Chat tick, 30 s | `engine.rs:234-249` | tokio `interval` |
| Self-sync dial loop, 15 s (`P2P_SELF_SYNC_SECS`) | `sync/engine.rs:63-65`, `:636-650` | tokio `sleep` |
| Chat dial backoff 2→60 s | `engine.rs:280-309` | tokio `sleep` |
| Blob retry `[2,5,15,30,60]` s | `engine.rs:40`, `:1394-1425` | tokio `sleep` |
| Self-sync per-device backoff up to 300 s | `sync/engine.rs:675-681` | `Instant` |
| Availability "until" | `node.rs:1669-1677` | Wall clock `now()` (`node.rs:3218-3220`), evaluated **when read**, with no timer |

The availability check is the existing pattern of "evaluate a deadline against wall-clock on
read". No timer and no field in core today relates to rotation.

---

## 5. In-flight work tied to the endpoint and how it ends today (verified)

"Closed by endpoint close" means the task ends because its connection or `accept` ends after
`close_endpoint`. No explicit cancel exists.

| Work | Started at | Holds | On `stop` | On `lock` | On `replace_account` |
|---|---|---|---|---|---|
| Accept loop | `node.rs:1833` | `ep` clone | ends (`accept()` → None) | same | same |
| Address watcher | `node.rs:1824-1831` | `ep` clone | ends with the endpoint's watch stream | same | same |
| Outgoing call legs | `node.rs:2517-2537`, `Leg::run` `node.rs:298-374` | `ep` clone + conn | closed by endpoint close (`conn.closed()` branch `node.rs:369-371`) | same | refused while in call (`node.rs:1645`) |
| Active call `run_call` | `node.rs:2691-2812` | conn | closed by endpoint close → `lost(active)` (`node.rs:2784-2791`) → `end_call` | same. **`lock` does not check `not_in_call`** | refused (`InCall`) |
| Media datagram loop | `node.rs:2814-2833` | conn | ends on read error | same | — |
| Waiting call | `Live.waiting` `node.rs:426-428` | conn | same as active | same | refused |
| Incoming hello / contact add | `node.rs:2059-2135` | conn | closed by endpoint close | same | same |
| `add_contact` (outgoing) | `node.rs:1938-1989` | `ep` clone | dial fails or conn lost | same | same |
| Chat sessions `chat_run` | `engine.rs:420-450`, `:484-560` | conn | `Cmd::Close` via `chat_sessions_close` (`engine.rs:205-211`), then endpoint close | `Cmd::Close` via `chat_close` (`engine.rs:193-203`), **after** endpoint close | `chat_sessions_close` + `chat_close` |
| Chat periodic tick | `engine.rs:228-252` | none | breaks on its next tick when `endpoint.is_none()` (`engine.rs:239-241`) | same | same |
| Chat dial/backoff per DID | `engine.rs:276-311` | none | breaks when `endpoint.is_none()` (`engine.rs:290`) or `me()` fails | same | same |
| Blob download + retries | `engine.rs:1347-1425` | `AbortHandle` in `Dl` | **not cancelled**. Each attempt fails (`NotStarted`, retryable) until the 5 retries run out, then the blob is marked `failed` | same | same |
| Blob serving | `engine.rs:469-481` | conn | closed by endpoint close | same; hub shut down in `chat_close` (`engine.rs:199-201`) | same |
| Presence | per session (`engine.rs:504-508`) | session | ends with the session | same | same |
| Self-sync sessions | `sync/engine.rs:286-306`, `self_run` `:362+` | conn in `SelfSync.sessions` | closed by endpoint close. Map entry removed by `drop_session` (`sync/engine.rs:355-360`) | same | same |
| Self-sync dial loop | `sync/engine.rs:636-650` | none | breaks on its next wake when `endpoint.is_none()` (`:640-642`) | same | same |
| `send_unlinked` | `sync/engine.rs:707-731` | `ep` clone | closed by endpoint close | same | same |
| Link run (E) | `sync/link.rs:133-160` | uses main `ep` | **not cancelled** by `stop` | `link_cancel()` (`node.rs:824`) | **not cancelled** |
| Link run (N, temp endpoint) | `sync/link.rs:165-200`, `:285-302` | own endpoint | unaffected | `link_cancel` | n/a (needs no account) |
| Background persist | `node.rs:1526-1539` | none | runs on; epoch-checked | refused by epoch | refused by epoch |

---

## 6. Observations (inferred from source, not tested)

Each of these matters for rotation. Fixing any of them is out of scope.

- **L1 — Loops cannot tell one endpoint from another.** The chat tick, the chat dial and the
  self-sync dial loop stop only on `Shared.endpoint.is_none()` (`engine.rs:239-241`, `:290`,
  `sync/engine.rs:640-642`). Suppose rotation replaced `Some(old)` by `Some(new)` in place, then
  called `chat_started`/`selfsync_started` again. The old loops would keep running alongside the
  new ones. The same can already happen with a quick `stop`→`start` inside the 15 s / 30 s sleep
  window, because the old loop wakes, sees `Some`, and continues. I have not seen this
  happen; it is inferred from the code.
- **L2 — Tasks holding an endpoint clone pin the old endpoint.** Call legs, `run_outgoing`,
  `add_contact`, `send_unlinked` and the accept loop each hold an `Endpoint` clone (§2). After
  an in-place swap that does not `close()` the old endpoint, they would keep using it. Overlap
  therefore needs explicit ownership of the old endpoint. That ownership is a G-RETIREMENT
  decision. Whether `close()` on one clone ends all clones depends on iroh's semantics. It is
  not verified in this repo, but today's shutdown relies on it (`node.rs:1712-1717`).
- **L3 — Core has no "resume" or "foreground" entry point.** Android `ON_RESUME` only refreshes
  UI state (`kt/MainActivity.kt:117-118`). Core learns about a wake-up in only two ways:
  `networkChanged` (`kt/CoreService.kt:23-30`) or `onStartCommand` → `startNode` → `start`.
  The second is a no-op when an endpoint already exists (`node.rs:1772-1774`). On Linux/Android,
  tokio timers are based on a monotonic clock. As far as I know, that clock does not advance
  while the device is suspended. A deadline implemented as `tokio::time::sleep(interval)` could
  therefore fire late by the suspended time. This is not measured, and the handoff already
  requires a measurement (`endpoint-rotation-implementation-plan.md:157-159`).
- **L4 — Android never locks or stops.** The service is always on (`kt/CoreService.kt:16`,
  `START_STICKY`), and no Kotlin code calls `lock()` or `stop()`. Once unlocked, a process keeps
  its endpoint until it dies or switches account. A rotation trigger tied to lock/unlock alone
  would therefore rarely fire on Android.
- **L5 — Re-sealing a new `device_secret` has no primitive.** `vault::seal` makes a fresh DEK
  (`vault.rs:101-115`). That would invalidate the DEK remembered by Android `UnlockStore`
  (`kt/P2pApp.kt:192-196`) and by the desktop keyring (`desktop/keystore.rs:207-224`). For a
  passphrase vault it would also need the passphrase, because Argon2id wraps the new DEK.
  Rotating while unlocked without a user prompt therefore needs a new "re-seal under the
  existing DEK, keep `wrapped_dek`/`salt`" function, plus an `account.json` rewrite of
  `device_public` (A1 §3.1). The DEK is in memory only while unlocked (`node.rs:393`), so a
  rotation cannot be persisted while locked. Crash ordering is out of scope here (G-STORAGE / C2).
- **L6 — `lock` and `replace_account` treat calls differently.** `replace_account` refuses
  during a call (`node.rs:1645`). `lock` does not, and ends any call by closing the endpoint.
  Rotation retirement of a live call is undecided (G-RETIREMENT). The handoff's initial
  proposal is "close old connections at retirement" (`endpoint-rotation-implementation-plan.md:153-155`).
- **L7 — Blob downloads survive endpoint loss as failing retries.** `Error::NotStarted` counts as
  retryable (`engine.rs:1402`). A retirement gap longer than about 112 s (2+5+15+30+60) marks
  wanted blobs `failed` (`engine.rs:1403-1405`, `:1374-1384`).
- **L8 — Link cancellation is lock-only.** `stop` and `replace_account` do not call
  `link_cancel`. An E-side link run in flight across a rotation would break, because the
  transcript binds `e_dev` (A1 §3.2, `sync/link.rs` row).
- **L9 — `network_changed` does nothing while stopped** (`node.rs:892-893`). It is the only
  OS-driven core call on Android that does not depend on the endpoint being down.
- **L10 — Status and UI follow `ep.id()`.** `NodeStatus.endpoint_id` (`node.rs:1738-1762`) and
  the `watch_addr` stream (`node.rs:1824-1831`) are per endpoint. A swap needs a fresh watcher
  and an `emit_status`.

---

## 7. Candidate insertion points (PROPOSAL — not approved)

These are labelled proposals only. Every option touches an open gate: G-TIME, G-RETIREMENT,
G-STORAGE, G-KEYS, G-DISCOVERY. They must not be read as decisions. No derivation scheme is
assumed: "new secret" below means "whatever G-DISCOVERY selects", and today that would be
`proto::new_device_secret()`.

### P1 — "rotate now" (core)

- **P1a (proposal).** Add a new `Inner::rotate_endpoint` that runs under `Inner.lifecycle`
  (`node.rs:450-451`), like `start`/`stop`/`lock`. That gives it the same serialisation against
  account switch and lock (`node.rs:1643-1645`, `:827-829`). Sequence to evaluate, with ordering
  owned by G-STORAGE/C2:
  1. Require `me()` and `dek`.
  2. Make the new secret.
  3. Persist it: re-seal under the existing DEK (needs a new primitive, L5), and rewrite
     `device_public`.
  4. Rebuild `Me` with a new attestation and epoch handling (`Me::load` `node.rs:1083-1088`).
  5. Bind the new endpoint using the `start` body (`node.rs:1775-1820`), factored out.
  6. Retire the old endpoint (P2).
  7. Re-run `chat_started`/`selfsync_started` only after L1 is addressed.
- **P1b (proposal).** Accept that the only safe restart is a stop→start cycle: `stop`
  (`node.rs:877-889`), swap secrets, then `start`. This is simpler, gives no overlap, and makes
  the whole install unreachable while it runs. Its downtime is measurable.
- **Trigger sources (proposal, each needs a G-TIME rule):**
  - A core timer started in `start` next to `chat_started` (`node.rs:1818`).
  - A deadline check on every `start` call (`node.rs:1769-1774`) and every `network_changed`
    (`node.rs:891-898`).
  - An explicit UniFFI `rotate_now()` for tests and debug builds.

  Not lock/unlock alone, because of L4.

### P2 — "retire old" (core)

- **P2a (proposal, no overlap).** Reuse `close_endpoint` (`node.rs:1712-1717`) on the old
  endpoint immediately after the new one is installed. Before that, run `chat_sessions_close`
  (`engine.rs:205-211`) and `link_cancel` (`sync/link.rs:107-117`). Calls end as they do on
  `lock` (L6). Matches the handoff's "close old connections at retirement".
- **P2b (proposal, bounded overlap).** Keep the old endpoint in a new `Shared` slot, for example
  `retiring: Option<(Endpoint, deadline)>`. It would have no accept loop for new work, or only
  for named ALPNs, and would be closed at the deadline. This needs:
  - ownership of the clones (L2),
  - loops keyed by an endpoint generation instead of `is_none()` (L1),
  - a G-RETIREMENT rule for calls, transfers and chat sessions.

  `fresh_link_endpoint` / `link_session` (`sync/api.rs:65-76`, `sync/link.rs:299-301`) is the
  only existing pattern for a non-primary endpoint with its own close.
- **Loop hygiene, needed by both options (proposal).**
  - Give loops an endpoint generation, or a `CancellationToken` per endpoint, checked where
    they test `is_none()` today (`engine.rs:239-241`, `:290`, `sync/engine.rs:640-642`).
  - Decide whether blob retries (`engine.rs:1394-1425`) pause, abort or keep counting across
    a retirement gap (L7).

### P3 — "enforce overdue on resume / start" (core + platforms)

- **P3a (proposal, core).** Evaluate an overdue deadline against wall-clock on entry. Two
  candidate places:
  - in `Inner::start` before bind (`node.rs:1771-1775`), so an overdue key is never bound after
    unlock, boot or switch;
  - in `network_changed` (`node.rs:891-898`).

  This copies the availability pattern (`node.rs:1669-1677`). One successor on overdue resume
  is the current candidate model policy (`rotation-agent-handoff.md:107-108`), not a decision.
- **P3b (proposal, core API).** Add a `Node::resumed()` (name TBD). It would do the deadline check
  and nothing else, because `start` is a no-op while bound (L3).
- **P3c (proposal, Android).** Call P3b from:
  - `CoreService.onStartCommand` (`kt/CoreService.kt:39-53`), which already runs on boot, on
    every `ensureRunning` and on call state changes;
  - the network callback (`kt/CoreService.kt:23-30`);
  - optionally `MainActivity` `ON_RESUME` (`kt/MainActivity.kt:117`).

  Whether a wake-up alarm such as `AlarmManager` is used is a battery/G-TIME question. None
  exists today.
- **P3d (proposal, desktop).** Desktop has no sleep/resume hook and does not call
  `network_changed`. The 1 s UI tick (`desktop/app.rs:939-955`) could call P3b. A core timer
  check alone may also be enough if desktop tokio timers behave across sleep. Not measured.
- **Fail-closed point (proposal, G-TIME/G-RETIREMENT).** If the new endpoint cannot bind at an
  overdue check, the options are:
  - refuse to bind the old key, staying offline with a visible status;
  - bind it with a visible degraded state.

  The handoff forbids silent unlimited extension (`endpoint-rotation-implementation-plan.md:156-158`).

### Suggested deterministic tests, before production code (proposal)

These belong to C1/C2 or a later packet, not to A2:
- A loop-generation test: an old loop stops after a swap (L1).
- stop→start inside the sleep window leaves exactly one chat loop and one self-sync loop.
- A re-seal-under-same-DEK round trip keeps `unlock_key` valid (L5).
- An overdue check on start never binds an overdue key under the chosen G-TIME option.
- `lock` during a call versus `rotate` during a call, under each G-RETIREMENT candidate.

---

## 8. Policy questions surfaced (not decided here)

1. G-RETIREMENT: should rotation be no-overlap (P2a) or bounded overlap (P2b)? What happens to
   a live call, a waiting call, a running blob transfer, an E-side link run and chat sessions?
2. G-TIME: where does the deadline check run (start / network change / explicit resume /
   alarm)? Is it measured against wall-clock or monotonic time? What does "overdue" mean after
   suspend (L3)?
3. G-STORAGE: what is the commit order between the vault re-seal, the `account.json`
   `device_public` rewrite, sealed-state registry updates (A1 §3.2) and the endpoint bind? What
   is the recovery from a crash between them?
4. G-KEYS: a new attestation is minted per `Me::load` (`node.rs:1085`). Is that acceptable at
   rotation, or does rotation need an ordered record (A1 O8)?
5. Should Android ever call `lock()`/`stop()` (L4)? That is a product decision that affects
   which triggers are available.

## 9. Not in scope / not done

- No edits outside this file. No builds and no tests run (the packet requires none).
- No measurement of Android/desktop suspend behaviour, iroh `close()` semantics across clones,
  or bind latency.
- Persistence and migration of key-dependent state are covered by A1 and not repeated here.
