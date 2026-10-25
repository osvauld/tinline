# Several accounts reachable on one device

Status: design, 2026-10-25. Not built. Branch `feature/multi-online`.

## Model (decided with the user, 2026-10-25)

- Every account on the device is reachable. Only the **open** account keeps its endpoint up all the time.
- An account that is not open sleeps: no endpoint. Its contacts already hold wake tokens for it (each
  account registers and hands out its own while it is open, `docs/push.md`).
- A wake push for a sleeping account starts that account's endpoint **in the background** so the message
  or call gets through, then closes it again once it is quiet.
- Switching accounts opens the new account's endpoint as today. The one left goes to sleep (its endpoint
  closes), unless it is still busy with a woken exchange; then it closes when quiet.
- A call to a sleeping account rings and names the account ("Asha → Work"). **Answering switches** the app
  to that account; the call screen opens there.
- Messages to a sleeping account post a notification naming the account, and the account switcher shows an
  **unread dot** on it. Tapping the notification switches to that account.
- One 1:1 call at a time across all accounts: during a call, a call to another account gets `Busy`.

Supersedes the 2026-10-14 sketch of one endpoint per account kept up all the time.

## What wakes, and what does not

| Edition | Sleeping account reachable? |
|---|---|
| Play (FCM), iOS (APNs/PushKit) | Yes, by wake push |
| Desktop | Yes, always: every account with a remembered key keeps its endpoint up (`KeepAll`, below) |
| Liberum | No: only the open account is up; the others catch up when you switch to them (as today) |

Also not woken:

- **Passphrase-only accounts** (no key remembered on this device): the hint cannot be opened, so the
  account stays asleep until unlocked.
- **Group posts**: groups send no wake pushes; a sleeping account gets them when it is next open.

## Core (Rust, `crates/core` + `crates/node-runtime`)

The `Node` the platforms hold keeps its API: every existing method acts on the **open** account. Inside, it
holds slots:

```
Node { rt, slots: Mutex<Slots> }
Slots { open: Arc<Inner>, woken: HashMap<AccountId, Woken> }
Woken { inner: Arc<Inner>, idle_since: Option<Instant> }
```

- One tokio runtime for all slots (`Inner` gets the handle, not its own runtime).
- `self.inner` becomes `self.open()` (an `Arc<Inner>` clone); mechanical.
- Event setters (`set_chat_events`, `set_video_events`, …) are stored on `Node` and installed in every slot
  through a gate: the open slot forwards everything; a woken slot forwards only what a sleeping account may
  surface (below).

### Wake routing

`on_wake(hint)`:

1. Try the open account's hint key; if it opens, act as today.
2. Else, for each other account with a remembered key: the platform must supply it. New callback on
   `NodeEvents`: `fn account_key(&self, did: String) -> Option<Vec<u8>>` (Android Keystore / iOS Keychain,
   blocking, ≤ 1 s). The core loads the account into a woken slot, unlocks it, opens the hint.
   - Hint keys of woken accounts stay in memory while the process lives, so repeat wakes skip the unlock.
3. `DialBack` in a woken slot: start its endpoint, dial the contact, fetch.
4. `Ring` in a woken slot: start its endpoint, ring. `WakeOutcome::Ring` gains `account: AccountRef { did, name }`
   (`None` = the open account).
5. Nothing opens it: `Ignore`.

A woken slot closes (endpoint, stores, keys forgotten) after `WOKEN_IDLE_MS` (60 s, as `wire::idle_close`)
with no session, no ring and no call.

### Background mode

`Node::set_background_mode(mode)`, set by the platform before `start`:

- `OnWake` (Play, iOS): as above; other accounts open only on a wake and idle out.
- `KeepAll` (desktop): at `start`, every other account the platform has a key for (`account_key`) is
  opened as a woken slot that never idles out, so each account keeps its own endpoint. A newly
  unlocked or switched-away account stays up too.
- `OpenOnly` (Liberum, the default): only the open account runs; `on_wake` never loads another account.

### What a woken slot surfaces

- `on_incoming_call` / `on_call_state` for its calls, with `CallInfo.account: Option<AccountRef>`.
- New `AccountEvents` trait (`set_account_events`):
  - `on_background_message(account: AccountRef, from_did, from_name)`: the platform posts the notification.
  - `on_accounts_changed()`: unread counts changed; refresh the switcher.
- Nothing else (contacts, chat list, profile, video, links) reaches the screens.

`AccountSummary` gains `unread: u32`: the open account's is 0 in the switcher; a woken or sleeping account
keeps the total it had when it last closed (saved clear in the account dir, a number only).

### Calls

- Busy rule: a `CallFloor` (`Arc`, shared by all slots) is taken when a slot's `Live.call` or `waiting` is
  set and released when cleared. An incoming hello replies `Busy` if another slot holds the floor;
  `call()` fails `InCall`.
- `answer(call_id)` / `answer_wake_ring(did)` on a woken slot's call: the core first makes that slot the open
  one (the old open slot becomes woken and idles out), then answers. The platform sees the usual refresh
  after an account switch, then the call.
- `switch_account` during a call: `InCall`, as today.

### Switching

`switch_account(did)`:

- `did` is a woken slot: it becomes open (no restart); the previous open slot becomes woken and idles out.
- Otherwise: as today, load it (locked) and make it open; the previous open slot becomes woken and idles out
  instead of closing at once, so an exchange in flight finishes.

`lock()` closes every slot. `stop()` stops every endpoint. `set_push_target` applies to the open slot and,
when a woken slot opens, to it too (same device token for every account).

## Platforms

- **Android**: `NodeEvents.account_key(did)` reads `UnlockStore`. `AccountEvents`: background notification
  (message channel, title names the account), switcher refresh. The ring shows the account. A notification
  tap carries the account DID; opening switches first. `WakeHandler` needs no account logic: the core
  routes.
- **iOS**: same, with the Keychain; PushKit needs the CallKit report within seconds, so `account_key` must
  be fast.
- **Desktop**: `KeepAll`; `account_key` reads the OS keyring (`keystore.rs`). Background notification and
  ring naming the account, switcher dot. A passphrase-only account comes up once you unlock it.
- **Liberum**: `OpenOnly`; no change apart from the switcher dot (unread saved when an account closes).
- Strings: the "One account is online at a time" hint (Android `accounts_one_online_hint`, iOS
  `unlock_one_online`) changes to say that other accounts ring and notify on phones with wake-ups.

## Tests (core, deterministic)

1. Wake for a sleeping account (two accounts, A open, B asleep): message hint → B loads, dials back, the
   message lands in B's store, `on_background_message(B)`; A's screens see no event; B closes after idle.
2. Call wake for B → `Ring { account: B }`; answer → B is open, A woken; call connects.
3. B in a call (open), a call to A (woken) → `Busy`; and the reverse.
4. Hint for a passphrase-only account (no key) → `Ignore`, nothing loaded.
5. Switch to a woken account → no endpoint restart; switch away → old one idles out, then closes.
6. `lock()` with a woken slot → both closed, keys forgotten.
7. Unread: a message in woken B → `AccountSummary(B).unread == 1`, survives B closing; opening B clears it
   with the existing read cursor.
8. `KeepAll`: two accounts with keys → both endpoints up after `start`, neither idles out; a message to
   either arrives with no wake. `OpenOnly`: a hint for B → `Ignore`, nothing loaded.

## Open

1. (Decided 2026-10-25: desktop keeps every account up; Liberum only the open one.)
2. Group posts to a sleeping account: no wake for groups yet.
3. Battery: a burst of wakes to several accounts runs several endpoints at once briefly; cap at 2 woken
   slots? (default: no cap, they idle out in 60 s)
