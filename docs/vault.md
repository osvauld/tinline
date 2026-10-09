# Identity at rest (passphrase vault)

## Layout: one directory per account

The platform's data dir is a root that can hold several accounts:

```
<root>/accounts/<id>/account.json   clear: version, name, did, device_public, vault
<root>/accounts/<id>/account.redb   sealed: state, call history, device label
<root>/accounts/<id>/chat.redb, blobs/   chat store (sealed as before), moved under the account
<root>/current                      plain text: the <id> of the selected account
```

`<id>` is the DID after `did:key:`. `account.json` (formerly `profile.json`) is described below.
`account.redb` is a `storage::Sealed` store whose key is `BLAKE3-derive-key("tinline/account-store/v2", recovery phrase)`
(words lower-cased, single-spaced; see "Storage keys" below): contacts, grants, block/revoke lists, redeemed ticket nonces, the ticket we hand out, availability, the call
history and the device label are in there, and exist in memory only while the account is unlocked (`lock()` drops
them; contacts and calls read as empty while locked). Record paths (`state`, `calls`, `device_label`, `sync/account` = the own-device sync doc) are not
hidden. Not sealed, on purpose: the account **name**, DID and device public key in `account.json`, because the lock
screen and the account switcher need them while locked. Anyone who copies the files can read who the accounts are
and how many there are; nothing about contacts or calls. Ciphertext-only files do not prove secure deletion of
earlier plaintext (JSON files are zeroed before they are removed, which a copy-on-write filesystem or flash may defeat).

Migration (`Node::new`): an old root with `profile.json` (v2 or legacy) is moved into
`accounts/<id>.migrating/` with renames only (`chat.redb`, `blobs/`, `state.json`, `calls.json` and leftovers
first, `profile.json` last as `account.json`), `current` is written, then the directory is renamed to its final
name. Every step can be repeated after a crash; nothing is deleted, and an error (account already present,
unparsable profile, a file in both places) returns before anything moves. `state.json` / `calls.json` stay JSON
until the account is first unlocked; then both go into `account.redb` in one transaction and the JSON files (and
`*.corrupt`, `*.tmp`) are removed. A legacy (pre-vault) profile has no data key, so its state stays JSON until
`set_passphrase` seals it. A sealed record that fails to open is an error, never a silent reset to empty.

Switching: `accounts()` lists them (clear data only), `switch_account(did)` stops the endpoint, forgets the secrets
and state of the current one and selects another (then `unlock` / `unlock_with_key` as usual), `begin_new_account()`
leaves none selected so `create_identity` / `restore_identity` add one, `remove_account(did)` deletes a
non-current one. Switching is refused with `Error::InCall` during a ringing, waiting or active call. Creating or
restoring never overwrites the open account: its phrase gives `Error::AccountExists(did)` (see the restore table).
Everything that runs on its own (call tasks, background writes) carries the session epoch it started under and is
refused once the account in memory has changed, so a stale task cannot write into the next account.

**Remembered keys are per account.** `unlock_key()` / `unlock_with_key()` act on the current account. A platform
must store the data key under a name that includes the DID (Android: one Keystore-wrapped blob per DID; desktop:
one keyring entry per DID), and pass the right one after `switch_account`. A key of another account gives
`WrongPassphrase`.

## Storage keys come from the recovery phrase (34f)

`account.redb` and `chat.redb` are sealed under `BLAKE3-derive-key("tinline/account-store/v2" | "tinline/chat-store/v2",
normalised phrase)`, not under the data key as before (v1: `"tinline/account-store/v1"` / `"tinline chat store v1"` over the
DEK). Forgetting the passphrase (with no remembered key) therefore no longer loses contacts, call history or chats: the
24 words reopen both files. The files are still protected by the passphrase, because the phrase itself only exists on disk
inside the vault. iroh-blobs ciphertexts are keyed per blob and unchanged.

A store written under the phrase key carries an unsealed marker row `~keyver` = `2`. `rekey::open` (used for both files):
marker present: open; no rows and no marker: write the marker (fresh file); rows but no marker: the file is still v1.
On the first unlock where both keys exist (the data key from the unlock, the phrase from the vault) it re-seals EVERY row
(old key, then new key, same path binding) and the marker in ONE redb transaction. A crash leaves the old file or the
new one, never a mix, and an interrupted run simply starts over from the old file; a row that fails to open aborts the
whole migration and changes nothing. The cost is that the store passes through memory once. `chat_open` migrates
`chat.redb` the same way (a failure there is logged and leaves chat unavailable, never a partial file).

## Restore over an account that is already on the device

`restore_identity(phrase, name, passphrase)` looks at what is selected:

| selected right now | phrase's account dir exists | result |
|---|---|---|
| nothing (`begin_new_account`) | no | new account |
| nothing | yes | **restore in place** |
| the same account, locked (`Locked`) | yes | **restore in place** (the forgotten-passphrase path) |
| the same account, unlocked | yes | `Error::AccountExists(did)`: switch to it, nothing was touched |
| another account | any | `Error::HaveIdentity` |
| a legacy (clear) profile of that DID | yes | `Error::AccountExists(did)` |

Restore in place: `account.json` gets a vault with a fresh data key, the new passphrase (or none: then the S1 rule
applies and `commit_identity` writes it) and a NEW device key (the old one lived only in the old vault; the old registry
entry of this install is tombstoned and contacts learn the new device from the next DeviceList). `account.redb`,
`chat.redb` and `blobs/` stay and open from the phrase. A v1-keyed file that was never unlocked since 34f cannot be
re-keyed without the lost data key; it is renamed `*.unreadable` (kept, not deleted) and starts empty.
`did_of_phrase(phrase)` (free UniFFI function) returns the DID so a platform can say "this phrase is the account
<name> on this device" and offer to open it.

## No-passphrase identity: `commit_identity` (S1)

With an empty passphrase the data key exists only in the platform's keystore, so a profile written before the key
was saved would be unrecoverable. `create_identity` / `restore_identity` with an empty passphrase therefore write
nothing: the identity lives in memory, unlocked (`has_identity()` true, `identity_committed()` false,
`unlock_key()` returns the key). The platform saves the key and then calls `commit_identity()`, which writes the
account directory and `current`. Until then `lock()`, a failed save, or process death discards the identity and
onboarding starts again with nothing on disk. With a passphrase the account is written at once and `commit_identity`
is a no-op; adding a passphrase (`set_passphrase`) to an uncommitted identity commits it.

## `account.json`

`account.json` in the account dir holds the identity. Format (version 2):

```json
{
  "version": 2,
  "name": "alice",
  "did": "did:key:...",
  "device_public": [32 numbers],
  "vault": { "salt": "b64", "m": 65536, "t": 3, "p": 4, "wrapped_dek": "b64", "sealed": "b64" }
}
```

`name`, `did` and `device_public` are in the clear so a lock screen can show who this is.
Everything secret is in `vault` (all bytes standard base64):

- `sealed` = AES-256-GCM (`nonce || ct || tag`) of the JSON `{"mnemonic", "device_secret"}` under a random
  32-byte data key (DEK).
- `wrapped_dek` = the DEK, AES-256-GCM under KEK = Argon2id(passphrase, `salt`, m KiB, t, p) -> 32 bytes.
  Defaults are osvauld's keystore parameters: m=65536, t=3, p=4, 16-byte salt. A failed tag is reported as
  `WrongPassphrase`. Params outside m 64-256 MiB, t 3-10, p 1-8 or a salt outside 16-64 bytes (a tampered or downgraded
  file) are refused.
- Changing the passphrase rewraps the DEK (new salt) and leaves `sealed` and the DEK unchanged.
- The passphrase is optional. A vault without one has no `salt`, `m`, `t`, `p` or `wrapped_dek`, only `sealed`
  (`Vault::has_passphrase()` is false), and only the platform's copy of the DEK opens it. The version stays 2:
  every file written before this change has the full passphrase slot and opens as before. Adding a passphrase
  later (`set_passphrase(None, new)` on an unlocked node) wraps the same DEK, so nothing is re-encrypted.
  Passing an empty passphrase to `create_identity` / `restore_identity` means none; there is no minimum length.

Without a passphrase the platform copy is the only way in: Android's Keystore, on desktop the OS keyring (or,
with no keyring, a 0600 `unlock.key` in the data dir, which Settings says). If it is lost, restore with the 24 words.

The core never writes the DEK anywhere. `Node::unlock_key()` hands it to the platform, which may keep it
wrapped by hardware (Android Keystore) so the always-on service can `unlock_with_key` after a reboot without
the passphrase. If that key stops opening the vault, `unlock_with_key` returns `WrongPassphrase` and the
platform must delete its copy.

Legacy installs have the old shape (`{"mnemonic","name","device_secret"}`, no `version`). They load unlocked
so calls keep working but report `LockState::NeedsPassphrase`; `set_passphrase(None, new)` seals them and
atomically replaces `account.json` without the clear secrets, and the JSON state moves into `account.redb`.

## Threat model

Protects the recovery phrase and keys against someone who copies the app's files or a backup: they get
ciphertext, only as strong as the passphrase (Argon2id slows guessing; use a long one). It does not protect
against malware running as the app or a rooted device while unlocked. With a platform-remembered key, a stolen
phone that can unlock itself can still receive and place calls, but cannot reveal the recovery phrase
(`recovery_phrase(passphrase)` re-derives from the passphrase) nor change the passphrase without it.
Without a passphrase, anyone who can use the unlocked phone or read the keyring/file can read the secrets; the
recovery phrase then needs device authentication (Android) or a confirm (desktop), enforced by the platform,
not the core. `Error::WeakPassphrase` now only means an empty new passphrase.

## Files

The root, `accounts/` and each account dir are `0700` and every file written is `0600` (existing installs are fixed up when
opened); leftover `*.tmp` files from an interrupted write are deleted on open. `Profile` (mnemonic and
device secret in memory) is zeroized on drop.
