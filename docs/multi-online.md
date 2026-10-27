# Several accounts reachable on one device

Status: core built 2026-10-25 (`crates/core/src/node/slots.rs`, tests
`crates/core/tests/it/multi_online.rs`); Android, iOS and desktop not wired yet. Branch
`feature/multi-online`. See "Changed while building" at the end.

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

`lock()` closes every slot, and **locked means every account stays shut** (decided 2026-10-27, option a):
until the open account is unlocked again, no wake, `start` (`KeepAll`) or switch opens another account; a
wake is answered as on one account while locked (`Locked`: generic notice / CallKit report-and-end). A
process that starts with its account locked counts as locked too. `stop()` stops every endpoint. `set_push_target` applies to the open slot and,
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

## Changed while building (2026-10-25)

- **`account_key` is on `AccountEvents`, not `NodeEvents`.** A new method on the `NodeEvents`
  foreign trait would break every platform's implementation at once (UniFFI foreign traits have no
  default methods). `AccountEvents` is the new trait anyway; a platform that never calls
  `set_account_events` gets no background accounts (it behaves as `OpenOnly`).
- **Unread is the account's own conversations' unread total**, not a separate counter: a woken slot
  reports the sum of `Chat.unread`; when a slot closes (and when an `OpenOnly` switch leaves an
  account) that sum is written to `accounts/<id>/unread` (a clear number; removed at 0). Opening the
  account removes the file. This also gives Liberum its switcher dot, and the count never drifts from
  what the conversations show. `on_background_message` still fires once per arriving message.
- **Hint routing needs no unlock to rule an account out**: `WakeState::opens(hint)` /
  `hint_opens(key, hint)` (pure; no wire change). An account whose hint key is cached and does not
  open the hint is skipped without asking for its key; `lock` clears the cache.
- **A call wake for a background account while another account holds the call floor** returns
  `EndCall` (the ring is declined silently): the caller will get `Busy` anyway, so nothing rings.
- **`decline` / `hangup` / `decline_wake_ring`** reach a background account's call without switching;
  only `answer` / `answer_wake_ring` switch first.
- **The previous open slot keeps running only if it is unlocked and on disk**; a locked or
  never-committed one closes on the switch. Its group call is left on the switch, as today.
- **`OpenOnly` keeps today's in-place switch** (`replace_account` in the same slot), so Liberum's
  behaviour is unchanged; switching to the account that is already open also stays as today (it locks).
- `create_identity` / `restore_identity` of an account that runs in the background: `AccountExists`
  (switch to it instead), as for the unlocked open account.
- `network_changed` and `stop` act on every running slot; `start` starts the background ones in the
  background (the open one as before, blocking).

### API as built

```
Node::set_background_mode(mode: BackgroundMode)      // OpenOnly (default) | OnWake | KeepAll
Node::background_mode() -> BackgroundMode
Node::set_account_events(events: AccountEvents)
trait AccountEvents {
    fn account_key(did) -> Option<Vec<u8>>            // asked on the caller's thread of on_wake/start
    fn on_background_message(account: AccountRef, from_did, from_name)
    fn on_accounts_changed()
}
record AccountRef { did, name }
CallInfo.account: Option<AccountRef>                  // None = the open account
WakeOutcome::Ring { did, name, account: Option<AccountRef> }
AccountSummary.unread: u32
const WOKEN_IDLE_MS = 60_000 (test knob: Node::set_background_idle_ms_for_test)
```

Test hooks (`dev-tools`): `background_accounts_for_test`, `account_endpoint_for_test`,
`wake_hint_for_test`, `hint_keys_for_test`, `set_background_idle_ms_for_test`.

### Known risks for the platform waves

- `WakeOutcome.Ring` gained a field and `AccountSummary`/`CallInfo` gained fields: regenerated
  Kotlin/Swift bindings need the call sites updated (Kotlin `when` on `Ring`, Swift pattern matches).
- `account_key` is called synchronously from `on_wake` (and `start` under `KeepAll`), with no core lock
  held (the platform may call back into the node from it); on iOS it runs inside the PushKit handler's
  time budget.
- A background slot's leftover tasks end by the usual "endpoint gone / epoch changed" checks, as after
  a switch today; a slot closed by idle and reopened by a wake at once may briefly wait for its store
  files.
- Each running account is a full endpoint plus group endpoint: `KeepAll` with many accounts costs
  that many endpoints (Open item 3).

## Fixed after review (2026-10-27)

Tests 16-22 in `crates/core/tests/it/multi_online_review.rs`; floor and ring tests in
`node-runtime` (`push/ring_tests.rs`, `wake_*`); sink gating in `crates/core/src/node/gated/tests.rs`.

1. **Lock holds** (option a above). `Hub.locked`: set by `lock`, and at start when the account opens
   locked; cleared when the open slot is unlocked (`unlock`, `unlock_with_key`, create/restore).
   `on_wake` takes the open account's path while locked; `wake_slot` / `open_all` load nothing.
2. **An early answer stays with the screen.** Leaving the open slot (switch, new account) first
   turns its early-answered wake rings back to ringing (`NodeCtx::wake_rings_unanswer`): the call then
   rings in the background, naming the account, instead of connecting unheard. Race: a call that
   arrives during the switch makes it `InCall` and rings, unanswered, in the slot that stays open.
3. **Group calls count for busy.** The `CallFloor` knows every slot (`join_floor`):
   `group_call_elsewhere` makes an incoming 1:1 call `Busy` (outgoing `InCall`) while the user is in a
   group call on another account, as on one account; a call wake for a background account then is
   `EndCall`. A call wake for the open account during its own group call still rings, as today.
4. **One ring across the accounts.** A call wake while another slot holds the floor or has an open
   (not declined) wake ring is `EndCall` (`Hub::ring_refused`, decided under `ring_gate`), for the open
   account too; an incoming call while another slot rings for a wake is `Busy`.
5. **Background notices wait for read cursors.** `on_background_message` fires `NOTICE_SETTLE_MS`
   (1.5 s) after the message arrives, and only if the account's read cursor for that conversation is
   not past it (own-device sync sends messages and cursors in no fixed order).
6. **Close vs reopen.** `close_slot` marks the account closing when it is called; a load of it
   (`wake_slot`, `open_all`, `switch_to`) waits for the close first (`wait_closed`, 10 s at most;
   never on a core thread, where it skips the account and `switch_to` says `Busy`).
7. **No platform callback under `switching`.** `account_key` is asked with no lock held (the account
   is rechecked after); `make_open` no longer calls out, `announce_open` (status, contacts,
   `on_accounts_changed`) runs after the lock is released.
8. **Events of the account just left.** The open slot's sinks are gated (`gated.rs`): an event passes
   only while that slot is still open in the term the sink was installed in; the old slot's gate
   closes before the new one opens. The video sink is not gated (it follows the call).
9. `remove_account` forgets the account's hint key.
10. Not changed: no cap on background slots (Open item 3). Races noted above (2, 6).

Not covered by a core test: the group-call busy rule end to end (only the floor rule, in
`node-runtime`, with a fake group call).
